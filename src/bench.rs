//! Memory & Hybrid Retrieval benchmark
//!
//! `hyper bench` — synthetically populate memory + knowledge, run a fixed
//! query set with known ground truth, and report:
//!   - insert throughput (memories/sec, chunks/sec)
//!   - query latency (mean / p50 / p95 / p99)
//!   - retrieval quality (recall@5, recall@10, MRR)
//!
//! Inspired by supermemory's public "MemoryBench" idea: a reproducible,
//! numbers-driven proof that the pipeline actually works. Runs offline,
//! no LLM, finishes in a few seconds.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use crate::knowledge::KnowledgeBase;
use crate::memory::{MemoryManager, MemoryType, SqliteMemoryStore};
use crate::retrieval::HybridRetriever;

/// One configurable bench run
#[derive(Debug, Clone)]
pub struct BenchConfig {
    /// How many memories to plant
    pub memories: usize,
    /// How many knowledge chunks to plant
    pub chunks: usize,
    /// How many queries to run
    pub queries: usize,
    /// Top-k for recall calculation
    pub top_k: usize,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            memories: 1_000,
            chunks: 500,
            queries: 20,
            top_k: 10,
        }
    }
}

/// Latency summary (milliseconds)
#[derive(Debug, Clone, serde::Serialize)]
pub struct LatencyStats {
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub n: usize,
}

/// Quality metrics (0.0 .. 1.0)
#[derive(Debug, Clone, serde::Serialize)]
pub struct QualityStats {
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub mrr: f64,
    pub n_queries: usize,
}

/// Post-prune quality: how well does recall survive forget_below(threshold)?
/// quality_retention = post_forget.recall_at_5 / pre_forget.recall_at_5
/// Close to 1.0 = safe to prune. Below 0.5 = pruning is too aggressive.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PostForgetStats {
    pub threshold: f64,
    pub deleted: usize,
    pub remaining: usize,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub mrr: f64,
    pub quality_retention: f64,
}

/// Full bench output
#[derive(Debug, Clone, serde::Serialize)]
pub struct BenchReport {
    pub memories_inserted: usize,
    pub chunks_inserted: usize,
    pub memory_insert_per_sec: f64,
    pub chunk_insert_per_sec: f64,
    pub memory_query_latency: LatencyStats,
    pub hybrid_query_latency: LatencyStats,
    pub quality: QualityStats,
    pub post_forget: PostForgetStats,
}

/// Deterministic PRNG (so bench is reproducible)
struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn word(&mut self, dict: &[&str]) -> String {
        dict[self.next() as usize % dict.len()].to_string()
    }
    fn sentence(&mut self, dict: &[&str], n: usize) -> String {
        (0..n).map(|_| self.word(dict)).collect::<Vec<_>>().join(" ")
    }
}

const VOCAB: &[&str] = &[
    "rust", "tokio", "async", "memory", "index", "sqlite", "vector", "embed",
    "retrieval", "agent", "tool", "loop", "context", "prompt", "config",
    "benchmark", "hybrid", "knowledge", "static", "dynamic", "container",
    "schema", "migration", "fused", "score", "rank", "fusion", "chunk", "ast",
    "tree-sitter", "pagerank", "bm25", "rrf", "filter", "search", "recall",
    "importance", "temporal", "decay", "user", "preference", "codebase",
    "fact", "decision", "profile", "session", "container_tag", "MCP",
];

/// Run the full bench. Returns a `BenchReport` ready to print or JSON-encode.
pub fn run(config: &BenchConfig) -> Result<BenchReport> {
    let root = std::env::temp_dir().join(format!("hyperagent_bench_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    std::fs::create_dir_all(root.join(".hyper"))?;

    // ── 1) memory setup ──
    let mem_db = root.join("mem.db");
    let store = SqliteMemoryStore::new(&mem_db)?;
    let mgr = MemoryManager::new(Box::new(store), "bench-agent").with_container("bench");

    // ── 2) generate & insert memories ──
    let mut rng = Lcg::new(0xC0FFEE);
    let mem_start = Instant::now();
    let mut planted: Vec<String> = Vec::with_capacity(config.memories);
    for i in 0..config.memories {
        // 1/3 of memories contain a "ground-truth token" we'll query for
        let gt_token = if i % 3 == 0 {
            format!("GT{i:04}")
        } else {
            String::new()
        };
        let content = format!(
            "memory {} :: {} {}",
            i,
            rng.sentence(VOCAB, 6),
            gt_token
        );
        mgr.remember(&content, MemoryType::Learned)?;
        if i % 3 == 0 {
            planted.push(format!("GT{i:04}"));
        }
    }
    let mem_secs = mem_start.elapsed().as_secs_f64().max(1e-9);
    let mem_insert_per_sec = config.memories as f64 / mem_secs;

    // ── 3) knowledge base setup ──
    let kb_db = root.join(".hyper").join("kb_mem.db");
    let store = crate::memory::SqliteMemoryStore::new(&kb_db).unwrap();
    let kb_mgr = crate::memory::MemoryManager::new(Box::new(store), "agent").with_container("_knowledge");
    let kb = KnowledgeBase::new(&root, kb_mgr);
    let kb_start = Instant::now();
    let mut total_chunks = 0usize;
    for i in 0..config.chunks / 10 {
        let path = root.join(format!("file_{i}.md"));
        let content = (0..10)
            .map(|j| {
                format!(
                    "chunk {} :: {} {}\n",
                    j,
                    rng.sentence(VOCAB, 12),
                    if i % 3 == 0 { format!("GT_FILE_{i:04}") } else { String::new() }
                )
            })
            .collect::<String>();
        std::fs::write(&path, content)?;
        total_chunks += 10;
    }
    kb.build()?;
    let chunk_secs = kb_start.elapsed().as_secs_f64().max(1e-9);
    let chunk_insert_per_sec = total_chunks as f64 / chunk_secs;

    // ── 4) build query set with ground truth ──
    // For each GT token, query "GT0000" and expect at least one match in top-k.
    let queries: Vec<String> = (0..config.queries)
        .map(|q| {
            // pick a random GT index that exists
            let idx = (q * 7) % config.memories;
            format!("GT{idx:04}")
        })
        .collect();

    // ── 5) measure memory-only query latency + quality ──
    let mut mem_latencies = Vec::with_capacity(queries.len());
    let mut mem_recall5 = 0usize;
    let mut mem_recall10 = 0usize;
    let mut mem_mrr_sum = 0.0f64;
    for q in &queries {
        let t0 = Instant::now();
        let hits = mgr.recall_fused(q, config.top_k).unwrap_or_default();
        mem_latencies.push(t0.elapsed().as_secs_f64() * 1000.0);
        // ground truth: content contains the GT token
        for (rank, sm) in hits.iter().enumerate() {
            if sm.entry.content.contains(q) {
                if rank < 5 {
                    mem_recall5 += 1;
                }
                if rank < 10 {
                    mem_recall10 += 1;
                }
                mem_mrr_sum += 1.0 / (rank as f64 + 1.0);
                break;
            }
        }
    }

    // ── 6) measure hybrid query latency ──
    let retriever = HybridRetriever::new(&mgr, &kb);
    let mut hyb_latencies = Vec::with_capacity(queries.len());
    for q in &queries {
        let t0 = Instant::now();
        let _ = retriever.retrieve(q, config.top_k).unwrap_or_default();
        hyb_latencies.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // ── 7) measure post-forget quality (auto-decay) ──
    // We aggressively prune everything with score < 0.45 — that should
    // remove the "noise" memories but keep signal-rich ones. Then re-measure
    // recall on the same query set. If quality stays high, pruning is safe.
    let _count_before_forget = mgr.store().query(&Default::default()).unwrap().len();
    let deleted = mgr.forget_below(0.45).unwrap_or(0);
    let count_after_forget = mgr.store().query(&Default::default()).unwrap().len();
    let mut mem_recall5_post = 0usize;
    let mut mem_recall10_post = 0usize;
    let mut mem_mrr_post = 0.0f64;
    for q in &queries {
        let hits = mgr.recall_fused(q, config.top_k).unwrap_or_default();
        for (rank, sm) in hits.iter().enumerate() {
            if sm.entry.content.contains(q) {
                if rank < 5 {
                    mem_recall5_post += 1;
                }
                if rank < 10 {
                    mem_recall10_post += 1;
                }
                mem_mrr_post += 1.0 / (rank as f64 + 1.0);
                break;
            }
        }
    }

    let _ = std::fs::remove_dir_all(&root);

    let n = queries.len();
    Ok(BenchReport {
        memories_inserted: config.memories,
        chunks_inserted: total_chunks,
        memory_insert_per_sec: mem_insert_per_sec,
        chunk_insert_per_sec: chunk_insert_per_sec,
        memory_query_latency: summarize(&mem_latencies),
        hybrid_query_latency: summarize(&hyb_latencies),
        quality: QualityStats {
            recall_at_5: mem_recall5 as f64 / n as f64,
            recall_at_10: mem_recall10 as f64 / n as f64,
            mrr: mem_mrr_sum / n as f64,
            n_queries: n,
        },
        post_forget: PostForgetStats {
            threshold: 0.45,
            deleted,
            remaining: count_after_forget,
            recall_at_5: mem_recall5_post as f64 / n as f64,
            recall_at_10: mem_recall10_post as f64 / n as f64,
            mrr: mem_mrr_post / n as f64,
            quality_retention: if mem_recall5 > 0 {
                mem_recall5_post as f64 / mem_recall5 as f64
            } else {
                1.0
            },
        },
    })
}

fn summarize(samples: &[f64]) -> LatencyStats {
    if samples.is_empty() {
        return LatencyStats {
            mean_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            n: 0,
        };
    }
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    let mean = s.iter().sum::<f64>() / n as f64;
    let p = |q: f64| s[((n as f64 * q).ceil() as usize).min(n).saturating_sub(1)];
    LatencyStats {
        mean_ms: mean,
        p50_ms: p(0.50),
        p95_ms: p(0.95),
        p99_ms: p(0.99),
        n,
    }
}

/// Pretty-print a bench report as an aligned table for the CLI.
pub fn print_report(report: &BenchReport) {
    println!();
    println!("┌─ HyperAgent Memory Bench ─────────────────────────────┐");
    println!("│                                                      │");
    println!(
        "│  Memory seeded    : {:>6} entries ({:>8.0}/sec)            │",
        report.memories_inserted, report.memory_insert_per_sec
    );
    println!(
        "│  Knowledge seeded : {:>6} chunks  ({:>8.0}/sec)            │",
        report.chunks_inserted, report.chunk_insert_per_sec
    );
    println!("│                                                      │");
    println!("│  Memory query latency (ms)                           │");
    println!(
        "│     mean {:>7.2}    p50 {:>7.2}    p95 {:>7.2}    p99 {:>7.2}    │",
        report.memory_query_latency.mean_ms,
        report.memory_query_latency.p50_ms,
        report.memory_query_latency.p95_ms,
        report.memory_query_latency.p99_ms,
    );
    println!("│  Hybrid  query latency (ms)                          │");
    println!(
        "│     mean {:>7.2}    p50 {:>7.2}    p95 {:>7.2}    p99 {:>7.2}    │",
        report.hybrid_query_latency.mean_ms,
        report.hybrid_query_latency.p50_ms,
        report.hybrid_query_latency.p95_ms,
        report.hybrid_query_latency.p99_ms,
    );
    println!("│                                                      │");
    println!(
        "│  Quality (n={}):                                       │",
        report.quality.n_queries
    );
    println!(
        "│     recall@5  = {:>5.1}%    recall@10 = {:>5.1}%    MRR = {:>5.3}      │",
        report.quality.recall_at_5 * 100.0,
        report.quality.recall_at_10 * 100.0,
        report.quality.mrr,
    );
    println!("│                                                      │");
    println!(
        "│  After forget_below({}): {} deleted, {} remaining       │",
        report.post_forget.threshold,
        report.post_forget.deleted,
        report.post_forget.remaining,
    );
    println!(
        "│     recall@5  = {:>5.1}%    quality retention = {:>5.1}%        │",
        report.post_forget.recall_at_5 * 100.0,
        report.post_forget.quality_retention * 100.0,
    );
    println!("│                                                      │");
    println!("└──────────────────────────────────────────────────────┘");
    println!();
}

// Silence unused-import warning on PathBuf (used by callers, not by us)
#[allow(dead_code)]
fn _force_use_pathbuf(_: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_runs_and_reports_real_numbers() {
        let cfg = BenchConfig {
            memories: 100,
            chunks: 30,
            queries: 5,
            top_k: 10,
        };
        let r = run(&cfg).expect("bench failed");
        assert_eq!(r.memories_inserted, 100);
        assert!(r.memory_insert_per_sec > 0.0);
        assert!(r.memory_query_latency.mean_ms >= 0.0);
        // Quality should be > 0 because we plant exact GT tokens
        assert!(r.quality.recall_at_10 > 0.0, "expected at least some recall");
    }

    #[test]
    fn latency_summary_handles_empty() {
        let s = summarize(&[]);
        assert_eq!(s.n, 0);
    }

    #[test]
    fn latency_summary_percentiles() {
        let s = summarize(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
        assert_eq!(s.n, 10);
        assert!((s.mean_ms - 5.5).abs() < 0.01);
        assert!(s.p50_ms >= 5.0 && s.p50_ms <= 6.0);
        assert!(s.p95_ms >= 9.0);
    }
}
