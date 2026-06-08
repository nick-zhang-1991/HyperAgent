//! RAG Knowledge Base — document indexing and retrieval
//!
//! Stores knowledge chunks in the unified MemoryManager under the
//! `_knowledge` container tag. This gives all chunks BM25 scoring,
//! entity extraction, temporal decay, and container isolation — the
//! same pipeline as memory — instead of the separate LIKE-based DB
//! the original used.
//!
//! Each chunk is stored as a MemoryEntry with content prefixed by
//! `[path/to/file#chunk_index]` so provenance is preserved without
//! schema changes.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::memory::{MemoryManager, MemoryType};

/// A document chunk with metadata
#[derive(Debug, Clone)]
pub struct DocChunk {
    pub file: PathBuf,
    pub content: String,
    pub chunk_index: usize,
    pub score: f64,
}

/// RAG knowledge base backed by the unified MemoryManager.
///
/// After `build()` completes, chunks are searchable via the same
/// `recall_fused()` pipeline that drives memory retrieval. The
/// `search()` method returns `DocChunk` for backward compatibility
/// with the HybridRetriever.
pub struct KnowledgeBase {
    root: PathBuf,
    mgr: MemoryManager,
}

impl KnowledgeBase {
    pub fn new(root: &Path, mgr: MemoryManager) -> Self {
        Self {
            root: root.to_path_buf(),
            mgr,
        }
    }

    /// Build or rebuild the knowledge base from project files.
    /// Each chunk is stored as a memory entry under container `_knowledge`.
    pub fn build(&self) -> Result<usize> {
        let walker = ignore::WalkBuilder::new(&self.root)
            .standard_filters(true)
            .build();

        let mut total = 0usize;
        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let path = entry.path();
            if !Self::is_indexable(path) {
                continue;
            }

            let data = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if data.len() > 100_000 {
                continue;
            }

            let chunks = Self::chunk_text(&data, 1000);
            let rel_path = path.strip_prefix(&self.root).unwrap_or(path)
                .to_string_lossy().to_string();

            for (i, chunk) in chunks.iter().enumerate() {
                // Prepend provenance marker so recall_fused can return it
                let tagged = format!("[{}#{}]\n{}", rel_path, i, chunk);
                self.mgr.remember(&tagged, MemoryType::Learned)?;
                total += 1;
            }
        }
        Ok(total)
    }

    /// Search the knowledge base using the same fused-search pipeline
    /// as memory. Returns `DocChunk` results for backward compat.
    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<DocChunk>> {
        // Scope to the _knowledge container tag
        let tagged = self.mgr.recall_fused(query, max_results * 2)?;

        let mut results: Vec<DocChunk> = tagged
            .iter()
            .filter_map(|sm| {
                let content = &sm.entry.content;
                // Strip provenance prefix [path/file#N]
                if let Some(rest) = content.strip_prefix('[') {
                    if let Some(end_bracket) = rest.find(']') {
                        let meta = &rest[..end_bracket];
                        let body = &rest[end_bracket + 2..]; // skip "]\n"
                        if let Some(hash_pos) = meta.rfind('#') {
                            let file = meta[..hash_pos].to_string();
                            let idx: usize = meta[hash_pos + 1..].parse().unwrap_or(0);
                            return Some(DocChunk {
                                file: PathBuf::from(file),
                                content: body.to_string(),
                                chunk_index: idx,
                                score: sm.total_score,
                            });
                        }
                    }
                }
                None
            })
            .collect();

        results.truncate(max_results);
        Ok(results)
    }

    /// Display search results
    pub fn display_results(results: &[DocChunk]) {
        if results.is_empty() {
            println!("   No relevant documents found.");
            return;
        }
        println!("\n\u{1f4da} Knowledge Base Results:");
        for chunk in results {
            let preview = if chunk.content.len() > 150 {
                format!("{}...", &chunk.content[..147])
            } else {
                chunk.content.clone()
            };
            println!(
                "  \u{1f4c4} {} (score: {:.2})",
                chunk.file.display(),
                chunk.score
            );
            println!("     {}", preview.replace('\n', " "));
            println!();
        }
    }

    fn is_indexable(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md" | "txt" | "rs" | "py" | "ts" | "js" | "toml" | "yaml" | "yml" | "json")
        )
    }

    fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut current = String::new();
        for line in text.lines() {
            if current.len() + line.len() > max_chars && !current.is_empty() {
                chunks.push(current);
                current = String::new();
            }
            current.push_str(line);
            current.push('\n');
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        chunks
    }
}