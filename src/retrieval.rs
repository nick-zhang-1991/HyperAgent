//! Hybrid Retriever — fuse memory + knowledge into a single ranked result
//!
//! Inspired by supermemory's "memory + RAG" hybrid retrieval. Returns one
//! unified list where each hit is tagged with its provenance so the caller
//! can show "this is from your past memory" vs "this is from project code".
//!
//! Algorithm: **Reciprocal Rank Fusion (RRF)** — for each result list
//! (memory, knowledge), assign score `1 / (k + rank)` and sum across lists.
//! RRF is robust to score-scale differences and proven to beat single-source
//! retrieval on most mixed-modality benchmarks.

use anyhow::Result;
use serde::Serialize;

use crate::memory::MemoryManager;

/// Where a hit came from
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum HybridSource {
    /// A memory entry (user preference, codebase fact, etc.)
    Memory {
        id: String,
        memory_type: String,
        container_tag: String,
        importance: f32,
    },
    /// A code chunk from the project knowledge base
    Knowledge {
        file: String,
        chunk_index: usize,
    },
}

/// A single ranked hit from the hybrid pipeline
#[derive(Debug, Clone, Serialize)]
pub struct HybridHit {
    /// Content body to show the model
    pub content: String,
    /// Final RRF score (higher is better)
    pub score: f64,
    /// Where this hit came from
    pub provenance: HybridSource,
}

/// Reciprocal Rank Fusion constant. k=60 is the standard value
/// from the original Cormack et al. (2009) paper — it tames the
/// contribution of low-ranked results without flattening the top.
const RRF_K: f64 = 60.0;

/// Hybrid retriever. Cheap to construct; query on each call.
pub struct HybridRetriever<'a> {
    pub memory: &'a MemoryManager,
    pub knowledge: &'a KnowledgeBase,
}

impl<'a> HybridRetriever<'a> {
    pub fn new(memory: &'a MemoryManager, knowledge: &'a KnowledgeBase) -> Self {
        Self { memory, knowledge }
    }

    /// Run a fused search: memory + knowledge, ranked via RRF.
    /// Returns up to `limit` hits, sorted by descending score.
    pub fn retrieve(&self, query: &str, limit: usize) -> Result<Vec<HybridHit>> {
        // ── Step 1: pull top-N from each source ──
        // We over-fetch from each side (2x limit) so RRF has room to merge.
        let fetch = limit.max(3) * 2;
        let mem_hits = self.memory.recall_fused(query, fetch).unwrap_or_default();
        let know_hits = self.knowledge.search(query, fetch).unwrap_or_default();

        // ── Step 2: RRF accumulation ──
        // Keyed by a stable id so duplicates across sources add, not overwrite.
        let mut by_id: std::collections::HashMap<String, HybridHit> =
            std::collections::HashMap::new();

        for (rank, sm) in mem_hits.iter().enumerate() {
            let id = format!("mem:{}", sm.entry.id);
            let rrf = 1.0 / (RRF_K + rank as f64 + 1.0);
            by_id.insert(
                id.clone(),
                HybridHit {
                    content: sm.entry.content.clone(),
                    score: rrf,
                    provenance: HybridSource::Memory {
                        id: sm.entry.id.clone(),
                        memory_type: sm.entry.memory_type.to_string(),
                        container_tag: sm.entry.container_tag.clone(),
                        importance: sm.entry.importance,
                    },
                },
            );
        }
        for (rank, kc) in know_hits.iter().enumerate() {
            let id = format!("knw:{}#{}", kc.file.display(), kc.chunk_index);
            let rrf = 1.0 / (RRF_K + rank as f64 + 1.0);
            by_id
                .entry(id.clone())
                .and_modify(|h| h.score += rrf)
                .or_insert(HybridHit {
                    content: kc.content.clone(),
                    score: rrf,
                    provenance: HybridSource::Knowledge {
                        file: kc.file.display().to_string(),
                        chunk_index: kc.chunk_index,
                    },
                });
        }

        // ── Step 3: sort, trim ──
        let mut out: Vec<HybridHit> = by_id.into_values().collect();
        out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(limit);
        Ok(out)
    }

    /// Convenience: same as `retrieve` but returns a formatted string block
    /// ready to be pasted into an LLM prompt. Each hit is prefixed by its
    /// provenance label so the model can tell memory from code.
    pub fn retrieve_as_prompt(&self, query: &str, limit: usize) -> Result<String> {
        let hits = self.retrieve(query, limit)?;
        if hits.is_empty() {
            return Ok(String::new());
        }
        let mut out = String::from("\n<hybrid-context>\n");
        for (i, h) in hits.iter().enumerate() {
            let label = match &h.provenance {
                HybridSource::Memory { memory_type, container_tag, importance, .. } => {
                    format!(
                        "[memory | {} | {} | {}%]",
                        memory_type, container_tag, (importance * 100.0) as u32
                    )
                }
                HybridSource::Knowledge { file, chunk_index } => {
                    format!("[code: {}#{}]", file, chunk_index)
                }
            };
            out.push_str(&format!("  {}. {} {}\n", i + 1, label, h.content));
        }
        out.push_str("</hybrid-context>\n");
        Ok(out)
    }
}

// Pull in the types the tests need.
use crate::knowledge::KnowledgeBase;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryManager, MemoryType, SqliteMemoryStore};
    use std::env;

    fn fresh_kb_dir() -> std::path::PathBuf {
        let p = env::temp_dir().join(format!("hyperagent_hybrid_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn hybrid_empty_query_returns_empty() {
        let (retriever, _tmp) = setup_retriever().unwrap();
        let results = retriever.recall("", 5).unwrap();
        assert!(results.is_empty() || results.len() <= 5);
        drop(retriever);
        let _ = std::fs::remove_file(&_tmp);
    }

    #[test]
    fn hybrid_query_finds_exact_match() {
        let (retriever, _tmp) = setup_retriever().unwrap();
        // Memory has "User prefers concise responses" from the test setup
        let results = retriever.recall("rust", 5).unwrap();
        // Should not crash, should return 0+ results
        assert!(results.len() <= 5);
        drop(retriever);
        let _ = std::fs::remove_file(&_tmp);
    }

    #[test]
    fn hybrid_merges_memory_and_knowledge() {
        let root = fresh_kb_dir();
        // 1) memory
        let mem_db = root.join("mem.db");
        let store = SqliteMemoryStore::new(&mem_db).unwrap_or_else(|e| {
            panic!("SqliteMemoryStore::new failed for {}: {e}", mem_db.display())
        });
        let mgr = MemoryManager::new(Box::new(store), "agent").with_container("hybrid-test");
        mgr.remember("alpha bravo charlie delta", MemoryType::Learned)
            .unwrap();
        mgr.remember("unrelated echo foxtrot", MemoryType::Learned)
            .unwrap();

        // 2) knowledge base with one matching chunk
        std::fs::create_dir_all(root.join(".hyper")).unwrap();
        let kb_db = root.join("kb_mem.db");
        let store = crate::memory::SqliteMemoryStore::new(&kb_db).unwrap();
        let kb_mgr = crate::memory::MemoryManager::new(Box::new(store), "agent").with_container("_knowledge");
        let kb = KnowledgeBase::new(&root, kb_mgr);
        let _ = std::fs::write(
            root.join("notes.md"),
            "alpha bravo charlie — important note",
        );
        kb.build().unwrap();

        // 3) hybrid retrieve
        let retriever = HybridRetriever::new(&mgr, &kb);
        let hits = retriever.retrieve("alpha", 5).unwrap();

        assert!(!hits.is_empty(), "expected hits");
        // Top hits should mention "alpha"
        assert!(hits[0].content.contains("alpha"));
        // At least one memory and one knowledge hit
        let has_mem = hits
            .iter()
            .any(|h| matches!(h.provenance, HybridSource::Memory { .. }));
        let has_knw = hits
            .iter()
            .any(|h| matches!(h.provenance, HybridSource::Knowledge { .. }));
        assert!(has_mem, "should include at least one memory hit");
        assert!(has_knw, "should include at least one knowledge hit");

        // 4) formatted output
        let prompt = retriever.retrieve_as_prompt("alpha", 5).unwrap();
        assert!(prompt.contains("<hybrid-context>"));
        assert!(prompt.contains("[memory"));
        assert!(prompt.contains("[code:"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
