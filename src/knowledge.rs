//! RAG Knowledge Base — document indexing and retrieval
//!
//! Scans project files, chunks them, and stores in SQLite
//! with keyword-based retrieval (BM25-like scoring).
//! No external API needed.

use anyhow::Result;
use std::path::{Path, PathBuf};

/// A document chunk with metadata
#[derive(Debug, Clone)]
pub struct DocChunk {
    pub file: PathBuf,
    pub content: String,
    #[allow(dead_code)]
    pub chunk_index: usize,
    pub score: f64,
}

/// RAG knowledge base
pub struct KnowledgeBase {
    db_path: PathBuf,
}

impl KnowledgeBase {
    pub fn new(root: &Path) -> Self {
        let db_path = root.join(".hyper").join("knowledge.db");
        Self { db_path }
    }

    /// Build or rebuild the knowledge base from project files
    pub fn build(&self, root: &Path) -> Result<usize> {
        let conn = rusqlite::Connection::open(&self.db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY,
                file TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                content TEXT NOT NULL,
                words TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_chunks_content ON chunks(content);
            DELETE FROM chunks;"
        )?;

        let mut total = 0usize;
        let walker = ignore::WalkBuilder::new(root)
            .standard_filters(true)
            .build();

        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let path = entry.path();
            if !Self::is_indexable(path) {
                continue;
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if content.len() > 100_000 {
                continue; // Skip files over 100KB
            }

            let chunks = Self::chunk_text(&content, 1000);
            for (i, chunk) in chunks.iter().enumerate() {
                let words = Self::extract_keywords(chunk, 20).join(" ");
                conn.execute(
                    "INSERT INTO chunks (file, chunk_index, content, words) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        path.strip_prefix(root).unwrap_or(path).to_string_lossy().as_ref(),
                        i,
                        chunk,
                        words,
                    ],
                )?;
                total += 1;
            }
        }

        Ok(total)
    }

    /// Search the knowledge base for relevant chunks
    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<DocChunk>> {
        let conn = rusqlite::Connection::open(&self.db_path)?;

        let query_words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(|w| format!("%{w}%"))
            .collect();

        if query_words.is_empty() {
            return Ok(Vec::new());
        }

        // Build SQL: search by keywords in content
        let mut sql = String::from(
            "SELECT file, chunk_index, content, 0.0 as score FROM chunks WHERE "
        );
        for (i, _word) in query_words.iter().enumerate() {
            if i > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str(&format!("content LIKE ?{}", i + 1));
        }
        sql.push_str(" LIMIT ?");
        sql.push_str(&format!("{}", query_words.len() + 1));

        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for w in &query_words {
            params.push(Box::new(w.clone()));
        }
        params.push(Box::new(max_results as i64));

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let file: String = row.get(0)?;
            let idx: i32 = row.get(1)?;
            let content: String = row.get(2)?;
            Ok(DocChunk {
                file: PathBuf::from(file),
                content,
                chunk_index: idx as usize,
                score: 0.0,
            })
        })?;

        let mut results: Vec<DocChunk> = rows.filter_map(|r| r.ok()).collect();

        // Score by keyword frequency
        for chunk in &mut results {
            let lower = chunk.content.to_lowercase();
            let score: f64 = query_words.iter()
                .filter(|w| lower.contains(&w[1..w.len()-1]))
                .count() as f64 / query_words.len() as f64;
            chunk.score = score;
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);

        Ok(results)
    }

    /// Display search results
    pub fn display_results(results: &[DocChunk]) {
        if results.is_empty() {
            println!("   No relevant documents found.");
            return;
        }
        println!("\n📚 Knowledge Base Results:");
        for chunk in results {
            let preview = if chunk.content.len() > 150 {
                format!("{}...", &chunk.content[..147])
            } else {
                chunk.content.clone()
            };
            println!("  📄 {} (score: {:.2})", chunk.file.display(), chunk.score);
            println!("     {}", preview.replace('\n', " "));
            println!();
        }
    }

    fn is_indexable(path: &Path) -> bool {
        matches!(path.extension().and_then(|e| e.to_str()), Some("md" | "txt" | "rs" | "py" | "ts" | "js" | "toml" | "yaml" | "yml" | "json"))
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

    fn extract_keywords(text: &str, max: usize) -> Vec<String> {
        use std::collections::HashMap;
        let stop_words = ["the", "and", "for", "was", "are", "but", "not", "you", "all",
                          "can", "had", "her", "his", "its", "out", "see", "she", "too",
                          "use", "get", "put", "set", "let", "var", "pub", "fn", "mut",
                          "let", "use", "mod", "new", "self", "impl", "trait", "enum",
                          "struct", "type", "const", "static", "async", "await", "move"];

        let mut freq: HashMap<&str, usize> = HashMap::new();
        for word in text.split_whitespace() {
            let word = word.trim_matches(|c: char| !c.is_alphanumeric());
            if word.len() > 2 && !stop_words.contains(&word) {
                *freq.entry(word).or_insert(0) += 1;
            }
        }

        let mut words: Vec<(&str, usize)> = freq.into_iter().collect();
        words.sort_by(|a, b| b.1.cmp(&a.1));
        words.into_iter().take(max).map(|(w, _)| w.to_string()).collect()
    }
}
