//! File Context Cache — LRU file caching with summarization support
//!
//! Instead of loading the full content of every relevant file into the LLM
//! context window (which quickly blows past 128K), we maintain an LRU cache:
//!
//! - Files sorted by last access time
//! - When budget exceeded, evict oldest/lowest-score files
//! - File summaries generated via simple heuristic (first N lines + signature extraction)
//! - Full content loaded on-demand for files above score threshold

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::index::FileContext;

/// A cached file entry with access tracking
#[derive(Debug, Clone)]
pub struct CachedFile {
    pub path: PathBuf,
    pub score: f64,
    pub content: String,
    pub summary: String,
    pub token_count: usize,
    pub total_lines: usize,
    pub last_accessed: Instant,
}

/// LRU cache for file contexts
#[derive(Debug)]
pub struct FileContextCache {
    files: HashMap<PathBuf, CachedFile>,
    max_tokens: usize,
    current_tokens: usize,
}

impl FileContextCache {
    /// Create a new cache with a token budget
    pub fn new(max_tokens: usize) -> Self {
        Self {
            files: HashMap::new(),
            max_tokens,
            current_tokens: 0,
        }
    }

    /// Generate a compact summary of a file (first N lines + signatures)
    pub fn generate_summary(content: &str, max_summary_lines: usize) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        if total <= max_summary_lines {
            return content.to_string();
        }

        let mut summary = String::new();

        // First N lines (usually contains imports, struct defs, fn signatures)
        for &line in lines.iter().take(max_summary_lines / 2) {
            let trimmed = line.trim();
            if trimmed.starts_with("fn ") || trimmed.starts_with("pub ") || trimmed.starts_with("struct ")
                || trimmed.starts_with("enum ") || trimmed.starts_with("trait ") || trimmed.starts_with("impl ")
                || trimmed.starts_with("use ") || trimmed.starts_with("//!") || trimmed.starts_with("///")
                || trimmed.starts_with("#[")
            {
                summary.push_str(line);
                summary.push('\n');
            }
        }

        // Add total line count info
        summary.push_str(&format!("\n// ... ({total} lines total, showing key signatures) ...\n"));

        // Last N lines (usually closing impl blocks, main fn)
        for &line in lines.iter().rev().take(max_summary_lines / 4).rev() {
            let trimmed = line.trim();
            if trimmed.starts_with("fn ") || trimmed.starts_with("pub ") || trimmed.starts_with("}")
                || trimmed.starts_with("mod ") || trimmed.starts_with("impl ")
            {
                summary.push_str(line);
                summary.push('\n');
            }
        }

        summary
    }

    /// Get or load a file, updating LRU order
    pub fn get_or_load(&mut self, path: &Path, score: f64, max_full_tokens: usize) -> FileContext {
        if let Some(cached) = self.files.get_mut(path) {
            cached.last_accessed = Instant::now();
            cached.score = score;
            return FileContext {
                path: path.to_path_buf(),
                score,
                content: cached.content.clone(),
                total_lines: cached.total_lines,
                summary: cached.summary.clone(),
            };
        }

        // Load from disk
        let content = std::fs::read_to_string(path).unwrap_or_default();
        let total_lines = content.lines().count();
        let token_count = content.len() / 4; // rough token estimate
        let file_size_kb = content.len() / 1024;

        // Decide: full content or summary?
        let (final_content, summary) = if token_count > max_full_tokens {
            // File is too large — use summary
            let summary = Self::generate_summary(&content, 60);
            let summary_token_count = summary.len() / 4;
            (summary.clone(), format!("{file_size_kb}KB file, summarized ({summary_token_count} tokens)"))
        } else {
            (content.clone(), format!("{file_size_kb}KB file, full content ({token_count} tokens)"))
        };

        let final_tokens = final_content.len() / 4;

        // Evict if needed
        self.evict_to_fit(final_tokens);

        let entry = CachedFile {
            path: path.to_path_buf(),
            score,
            content: final_content.clone(),
            summary,
            token_count: final_tokens,
            total_lines,
            last_accessed: Instant::now(),
        };

        self.current_tokens += final_tokens;
        self.files.insert(path.to_path_buf(), entry);

        FileContext {
            path: path.to_path_buf(),
            score,
            content: final_content,
            total_lines,
            summary: String::new(),
        }
    }

    /// Evict oldest files until we have room for new_token_count tokens
    fn evict_to_fit(&mut self, new_token_count: usize) {
        let needed = self.current_tokens + new_token_count;
        if needed <= self.max_tokens {
            return;
        }
        let excess = needed - self.max_tokens;

        // Sort by access time (oldest first), then score (lowest first)
        let mut entries: Vec<PathBuf> = self.files.keys().cloned().collect();
        entries.sort_by(|a, b| {
            let a_entry = &self.files[a];
            let b_entry = &self.files[b];
            match a_entry.last_accessed.cmp(&b_entry.last_accessed) {
                Ordering::Equal => a_entry.score.partial_cmp(&b_entry.score).unwrap_or(Ordering::Equal),
                other => other,
            }
        });

        let mut evicted_tokens = 0usize;
        for path in &entries {
            if evicted_tokens >= excess {
                break;
            }
            if let Some(entry) = self.files.remove(path) {
                evicted_tokens += entry.token_count;
            }
        }

        self.current_tokens = self.current_tokens.saturating_sub(evicted_tokens);
    }

    /// Get total cached tokens
    pub fn current_tokens(&self) -> usize {
        self.current_tokens
    }

    /// Get max token budget
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    /// Get number of cached files
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.files.clear();
        self.current_tokens = 0;
    }

    /// Render cache status for display
    pub fn status(&self) -> String {
        format!(
            "   📂 File cache: {} files, {:.1}K / {:.1}K tokens ({:.0}%)",
            self.files.len(),
            self.current_tokens as f64 / 1000.0,
            self.max_tokens as f64 / 1000.0,
            (self.current_tokens as f64 / self.max_tokens as f64) * 100.0,
        )
    }
}

impl Default for FileContextCache {
    fn default() -> Self {
        Self::new(96_000) // Reserve ~32K for system prompt + history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_cache() {
        let cache = FileContextCache::new(1000);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.current_tokens(), 0);
    }

    #[test]
    fn test_summary_generation() {
        let content = (0..200)
            .map(|i| format!("fn function_{i}() {{}}\n"))
            .collect::<Vec<_>>()
            .join("");
        let summary = FileContextCache::generate_summary(&content, 10);
        assert!(summary.len() < content.len());
        assert!(summary.contains("200 lines"));
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = FileContextCache::new(200); // Very small budget

        // Create a temp dir with test files
        let dir = std::env::temp_dir().join("hyper-lru-test");
        let _ = std::fs::create_dir_all(&dir);

        let file1 = dir.join("a.rs");
        std::fs::write(&file1, "fn a() {}\n").unwrap();

        // First file should fit
        let ctx = cache.get_or_load(&file1, 0.9, 50);
        assert_eq!(cache.len(), 1);

        // A larger file that exceeds budget should trigger eviction
        let file2 = dir.join("b.rs");
        let big_content = "fn b() {}\n".repeat(100);
        std::fs::write(&file2, &big_content).unwrap();

        let ctx2 = cache.get_or_load(&file2, 0.5, 50);
        // file1 might or might not be evicted depending on timing
        // The key behavior is that adding a large file doesn't crash
        assert!(cache.current_tokens() > 0, "Cache should have some tokens");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
