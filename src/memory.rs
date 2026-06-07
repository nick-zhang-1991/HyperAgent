#![allow(unused)]
//! Smart Memory System — inspired by mem0 v3 + EverOS
//!
//! Design:
//! - **Add-only extraction** — memories accumulate, never overwritten (mem0 v3 style)
//! - **Entity linking with co-occurrence graph** — entities extracted, linked,
//!   and their co-occurrence tracked for boosted retrieval
//! - **Multi-signal fusion ranking** — BM25 keyword + entity boost + temporal decay
//!   + importance + access frequency, fused into a unified score
//! - **Temporal reasoning** — exponential time-decay; current state > recent > old
//! - **SQLite-backed persistence** — using rusqlite with entity co-occurrence tables
//! - **Two-stage pipeline** (codex inspired):
//!   Stage 1: Extract facts from agent conversation
//!   Stage 2: Consolidate into global memory (periodic merge)

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Symbolic Short-term Memory — inspired by TencentDB Agent Memory.
///
/// Compresses tool execution logs and conversation noise into compact
/// symbolic notation instead of raw verbatim text, reducing token usage
/// while preserving semantic structure.
pub mod symbolic {
    use super::MemoryType;

    /// Compress a memory entry into symbolic notation.
    /// Returns (compressed_text, compression_ratio).
    pub fn compress(content: &str, memory_type: &MemoryType) -> (String, f64) {
        match memory_type {
            MemoryType::ActionOutcome => compress_action_outcome(content),
            MemoryType::Ephemeral => (extract_key_signal(content), 0.0),
            _ => (lenient_compress(content), 0.0),
        }
    }

    fn compress_action_outcome(content: &str) -> (String, f64) {
        let original = content.len() as f64;
        let c = content
            .replace("Applied: modified ", "[EDIT] ")
            .replace("files for", "\u{2192}")
            .replace("No changes needed for ", "[OK] ")
            .replace("Changes rejected for ", "[REJECT] ")
            .replace("Bug fix: ", "[FIX] ")
            .replace("Relevant files for ", "[FILES] ")
            .replace("Plan for ", "[PLAN] ");
        let c = if c.len() > 200 {
            let mut s: String = c.chars().take(120).collect();
            s.push_str("...");
            s
        } else {
            c
        };
        let ratio = if original > 0.0 { c.len() as f64 / original } else { 1.0 };
        (c, ratio)
    }

    fn extract_key_signal(content: &str) -> String {
        let lower = content.to_lowercase();
        let re = regex::Regex::new(r"[a-zA-Z0-9_/.-]+\.[a-z]{2,}").unwrap();
        let files: Vec<&str> = re.find_iter(content).map(|m| m.as_str()).collect();
        let signals = ["error", "success", "fail", "timeout", "complete", "done", "crash", "warn"];
        let hits: Vec<&str> = signals.iter().filter(|t| lower.contains(*t)).copied().collect();
        if !files.is_empty() && !hits.is_empty() {
            format!("[{}] {}", hits.join("|"), files.join(", "))
        } else if !hits.is_empty() {
            format!("[{}]", hits.join("|"))
        } else if !files.is_empty() {
            format!("[NOTE] {}", files.join(", "))
        } else {
            content.chars().take(80).collect::<String>()
        }
    }

    fn lenient_compress(content: &str) -> String {
        if content.len() <= 150 {
            return content.to_string();
        }
        let mut s: String = content.chars().take(150).collect();
        s.push_str("...");
        s
    }
}

/// Memory Track — Dual-track memory system inspired by EverOS.
///
/// - **UserTrack**: user preferences, profile, habits, personal context.
///   Persists across all sessions; slower decay; pruned last.
/// - **AgentTrack**: codebase facts, decisions, bug fixes, skills, outcomes.
///   Session-scoped; faster decay; pruned first.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MemoryTrack {
    User,
    Agent,
}

impl std::fmt::Display for MemoryTrack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryTrack::User => write!(f, "user"),
            MemoryTrack::Agent => write!(f, "agent"),
        }
    }
}

impl MemoryTrack {
    pub fn infer(memory_type: &MemoryType) -> Self {
        match memory_type {
            MemoryType::UserPreference => MemoryTrack::User,
            _ => MemoryTrack::Agent,
        }
    }
}

/// Default half-life for temporal decay (in days).
/// A memory loses half its temporal score after this many days.
const TEMPORAL_HALF_LIFE_DAYS: f64 = 14.0;

/// Number of terms to keep per document for BM25 index (max).
const BM25_MAX_TERMS_PER_DOC: usize = 50;

/// Entity co-occurrence boost multiplier.
/// When a query term matches entity X, memories linked to entities
/// that co-occur with X get this multiplier on their entity score.
const ENTITY_CO_OCCUR_BOOST: f64 = 1.5;

/// Minimum access count before auto-promotion to next layer
const LAYER_PROMOTION_THRESHOLD: u32 = 5;

/// Layer-based score boost: higher layers get more weight in fusion
const LAYER_SCORE_MULTIPLIER_TRACE: f64 = 1.0;
const LAYER_SCORE_MULTIPLIER_POLICY: f64 = 1.3;
const LAYER_SCORE_MULTIPLIER_SKILL: f64 = 1.6;

/// Memory Layer — inspired by MemOS L1/L2/L3 self-evolving memory
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MemoryLayer {
    /// L1 — Trace: raw observations, unprocessed facts, ephemeral signals.
    /// Auto-cleaned; lowest retrieval priority.
    Trace,
    /// L2 — Policy: consolidated patterns, decisions, bug fixes, action outcomes.
    /// Medium retention; promoted from Trace on repeated access.
    Policy,
    /// L3 — Skill: crystallized knowledge, frequently used workflows.
    /// Highest retention; promoted from Policy on high access count.
    Skill,
}

impl std::fmt::Display for MemoryLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryLayer::Trace => write!(f, "trace"),
            MemoryLayer::Policy => write!(f, "policy"),
            MemoryLayer::Skill => write!(f, "skill"),
        }
    }
}

impl MemoryLayer {
    pub fn score_multiplier(&self) -> f64 {
        match self {
            MemoryLayer::Trace => LAYER_SCORE_MULTIPLIER_TRACE,
            MemoryLayer::Policy => LAYER_SCORE_MULTIPLIER_POLICY,
            MemoryLayer::Skill => LAYER_SCORE_MULTIPLIER_SKILL,
        }
    }

    /// Determine default layer based on memory type
    pub fn default_for(memory_type: &MemoryType) -> Self {
        match memory_type {
            MemoryType::Ephemeral => MemoryLayer::Trace,
            MemoryType::UserPreference => MemoryLayer::Policy,
            MemoryType::CodebaseFact => MemoryLayer::Policy,
            MemoryType::ActionOutcome => MemoryLayer::Trace,
            MemoryType::Decision => MemoryLayer::Policy,
            MemoryType::BugFix => MemoryLayer::Policy,
            MemoryType::Learned => MemoryLayer::Trace,
        }
    }
}

/// A single memory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub content: String,
    pub memory_type: MemoryType,
    pub layer: MemoryLayer,
    pub track: MemoryTrack,
    pub entities: Vec<String>,
    pub importance: f32,  // 0.0 - 1.0
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub consolidated: bool,
    /// Optional embedding vector (semantic search). Stored as BLOB.
    pub embedding: Option<Vec<f32>>,
    /// Container tag for memory isolation (per-project, per-user, per-customer).
    /// Inspired by supermemory's containerTag — separates memory namespaces so
    /// different projects/users don't pollute each other's recall.
    /// Default: "_default" (legacy single-namespace mode).
    pub container_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MemoryType {
    /// User preference or fact about the user
    UserPreference,
    /// Fact about the codebase (API, pattern, quirk)
    CodebaseFact,
    /// An action that was taken and its result
    ActionOutcome,
    /// A decision made and why
    Decision,
    /// A bug that was found and how it was fixed
    BugFix,
    /// General knowledge learned during session
    Learned,
    /// Ephemeral — only relevant to current session
    Ephemeral,
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryType::UserPreference => write!(f, "user_preference"),
            MemoryType::CodebaseFact => write!(f, "codebase_fact"),
            MemoryType::ActionOutcome => write!(f, "action_outcome"),
            MemoryType::Decision => write!(f, "decision"),
            MemoryType::BugFix => write!(f, "bug_fix"),
            MemoryType::Learned => write!(f, "learned"),
            MemoryType::Ephemeral => write!(f, "ephemeral"),
        }
    }
}

/// Query for memory retrieval
#[derive(Debug)]
pub struct MemoryQuery {
    pub text: String,
    pub memory_type: Option<MemoryType>,
    pub entity: Option<String>,
    pub max_age: Option<chrono::Duration>,
    pub limit: usize,
    /// Weight for temporal decay signal (0.0–1.0)
    pub temporal_weight: f64,
    /// Weight for entity boost signal (0.0–1.0)
    pub entity_weight: f64,
    /// Weight for keyword/BM25 signal (0.0–1.0)
    pub keyword_weight: f64,
    /// Weight for importance signal (0.0–1.0)
    pub importance_weight: f64,
    /// Weight for vector/semantic signal (0.0–1.0)
    pub vector_weight: f64,
    /// Pre-computed query embedding for semantic search
    pub query_embedding: Option<Vec<f32>>,
    /// Optional container tag filter — if set, only memories in this
    /// container are returned. When None, no filter is applied (all tags).
    /// MemoryManager::recall_fused sets this from self.container_tag automatically.
    pub container_tag: Option<String>,
}

impl Default for MemoryQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            memory_type: None,
            entity: None,
            max_age: None,
            limit: 10,
            temporal_weight: 0.15,
            entity_weight: 0.20,
            keyword_weight: 0.25,
            importance_weight: 0.10,
            vector_weight: 0.30,
            query_embedding: None,
            container_tag: None,
        }
    }
}

/// Scored memory entry returned by fused search
#[derive(Debug, Clone)]
pub struct ScoredMemory {
    pub entry: MemoryEntry,
    pub total_score: f64,
    pub keyword_score: f64,
    pub entity_score: f64,
    pub temporal_score: f64,
    pub importance_score: f64,
    pub vector_score: f64,
}

#[allow(dead_code)]
/// Memory Store trait — abstract over storage backend
pub trait MemoryStore: Send + Sync {
    /// Insert a new memory (add-only, never overwrites)
    fn insert(&self, entry: MemoryEntry) -> anyhow::Result<()>;

    /// Retrieve memories matching the query (basic mode)
    fn query(&self, query: &MemoryQuery) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Fused multi-signal search with scored results
    fn fused_search(&self, query: &MemoryQuery) -> anyhow::Result<Vec<ScoredMemory>>;

    /// Retrieve memories by entity
    fn query_by_entity(&self, entity: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Get all entities with their memory count
    fn list_entities(&self) -> anyhow::Result<Vec<(String, usize)>>;

    /// Mark memories as consolidated (stage 2 pipeline)
    fn mark_consolidated(&self, ids: &[String]) -> anyhow::Result<()>;

    /// Get memories ready for consolidation (stage 2 pipeline)
    fn get_unconsolidated(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Delete a memory entry
    fn delete(&self, id: &str) -> anyhow::Result<()>;

    /// Total memory count
    fn count(&self) -> anyhow::Result<usize>;
}

/// SQLite-backed memory store with multi-signal fusion ranking
#[derive(Clone)]
pub struct SqliteMemoryStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl SqliteMemoryStore {
    pub fn new(db_path: &Path) -> anyhow::Result<Self> {
        let conn = rusqlite::Connection::open(db_path)?;
        Self::build_schema(&conn)?;
        // Apply additive migrations for pre-existing databases.
        Self::migrate_add_container_tag(&conn);
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Build the schema for a fresh memory database.
    fn build_schema(conn: &rusqlite::Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA cache_size=-64000;

             CREATE TABLE IF NOT EXISTS memories (
                id               TEXT PRIMARY KEY,
                agent_id         TEXT NOT NULL,
                session_id       TEXT,
                content          TEXT NOT NULL,
                memory_type      TEXT NOT NULL,
                memory_layer     TEXT NOT NULL DEFAULT 'trace',
                entities         TEXT NOT NULL DEFAULT '[]',
                importance       REAL NOT NULL DEFAULT 0.5,
                embedding        BLOB,
                created_at       TEXT NOT NULL,
                last_accessed    TEXT NOT NULL,
                access_count     INTEGER NOT NULL DEFAULT 0,
                consolidated     INTEGER NOT NULL DEFAULT 0,
                container_tag    TEXT NOT NULL DEFAULT '_default'
             );

            CREATE TABLE IF NOT EXISTS memory_entities (
                entity       TEXT NOT NULL,
                memory_id    TEXT NOT NULL,
                PRIMARY KEY (entity, memory_id),
                FOREIGN KEY (memory_id) REFERENCES memories(id)
            );

            -- Entity co-occurrence graph: entity_a co-occurs with entity_b
            -- in count memories. Both directions stored for fast lookup.
            CREATE TABLE IF NOT EXISTS entity_cooccurrence (
                entity_a     TEXT NOT NULL,
                entity_b     TEXT NOT NULL,
                count        INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (entity_a, entity_b)
            );

            -- Per-term document frequency for BM25
            CREATE TABLE IF NOT EXISTS term_df (
                term         TEXT NOT NULL,
                doc_count    INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (term)
            );

            -- Per-document term frequency for BM25
            CREATE TABLE IF NOT EXISTS doc_terms (
                term         TEXT NOT NULL,
                doc_id       TEXT NOT NULL,
                freq         INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (term, doc_id),
                FOREIGN KEY (doc_id) REFERENCES memories(id)
            );

            -- Consolidated memory summaries (for future Dreaming feature)
            CREATE TABLE IF NOT EXISTS memory_summaries (
                id               TEXT PRIMARY KEY,
                memory_ids       TEXT NOT NULL,
                summary          TEXT NOT NULL,
                memory_type      TEXT NOT NULL,
                entities         TEXT NOT NULL DEFAULT '[]',
                created_at       TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
            CREATE INDEX IF NOT EXISTS idx_memories_consolidated ON memories(consolidated);
            CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at);
            CREATE INDEX IF NOT EXISTS idx_entity_cooccurrence_b ON entity_cooccurrence(entity_b);
            CREATE INDEX IF NOT EXISTS idx_doc_terms_doc ON doc_terms(doc_id);",
        )?;
        Ok(())
    }

    /// In-place schema migration: add container_tag to pre-existing tables.
    /// Safe to call repeatedly (duplicate-column error is swallowed).
    /// No-op for new tables created with the column in their CREATE TABLE.
    fn migrate_add_container_tag(conn: &rusqlite::Connection) {
        let _ = conn.execute(
            "ALTER TABLE memories ADD COLUMN container_tag TEXT NOT NULL DEFAULT '_default'",
            [],
        );
        // Backfill any pre-existing NULL-like rows (shouldn't happen with NOT NULL DEFAULT,
        // but defensive for databases created before the column existed).
        let _ = conn.execute(
            "UPDATE memories SET container_tag = '_default' WHERE container_tag IS NULL OR container_tag = ''",
            [],
        );
        // Index for fast per-container recall.
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_container_tag ON memories(container_tag)",
            [],
        );
    }

    // ═══════════════════════════════════════════
    // Entity Extraction (enhanced)
    // ═══════════════════════════════════════════

    /// Extract entities from content using multi-pattern heuristics.
    ///
    /// Patterns:
    /// - PascalCase / CamelCase identifiers (e.g. `HyperAgent`, `SqliteMemoryStore`)
    /// - UPPER_CASE acronyms (e.g. `SQLite`, `API`, `LLM`)
    /// - snake_case identifiers (e.g. `memory_manager`, `build_context`)
    /// - kebab-case identifiers (e.g. `long-term-memory`, `agent-id`)
    /// - Qualified names (e.g. `crate::memory::MemoryManager`)
    /// - Natural-language proper names (e.g. `User prefers`)
    /// - File paths (e.g. `/src/memory.rs`, `Cargo.toml`)
    /// - Version numbers (e.g. `v2.0.0`, `1.0.0`)
    /// - Hyphenated compound words (e.g. `state-of-the-art`)
    /// - URL origins (e.g. `github.com`, `api.example.com`)
    /// - Quoted terms as single entities
    pub fn extract_entities(content: &str) -> Vec<String> {
        let mut entities = Vec::new();
        let mut seen = HashSet::new();

        // Pattern 1: PascalCase / CamelCase (e.g. HyperAgent, SqliteMemoryStore)
        let re1 = regex::Regex::new(r"[A-Z][a-z]+[A-Z][a-zA-Z0-9]*").unwrap();
        for cap in re1.find_iter(content) {
            let e = cap.as_str().to_string();
            if seen.insert(e.clone()) {
                entities.push(e);
            }
        }

        // Pattern 2: UPPER_CASE acronyms (2+ chars, e.g. SQL, API, LLM, HTML)
        let re2 = regex::Regex::new(r"\b[A-Z]{2,}(?:[A-Z][a-z]+)?\b").unwrap();
        for cap in re2.find_iter(content) {
            let e = cap.as_str().to_string();
            if seen.insert(e.clone()) {
                entities.push(e);
            }
        }

        // Pattern 3: Qualified Rust/TS names (e.g. crate::memory::MemoryManager)
        let re3 = regex::Regex::new(r"\b[a-z_][a-z0-9_]*::[a-zA-Z_][a-zA-Z0-9_:]*").unwrap();
        for cap in re3.find_iter(content) {
            let e = cap.as_str().to_string();
            if seen.insert(e.clone()) {
                entities.push(e);
            }
        }

        // Pattern 4: snake_case identifiers (2+ words)
        let re4 = regex::Regex::new(r"\b[a-z]+_[a-z][a-z0-9_]*(?:_[a-z][a-z0-9_]*)*\b").unwrap();
        for cap in re4.find_iter(content) {
            let e = cap.as_str().to_string();
            if seen.insert(e.clone()) {
                entities.push(e);
            }
        }

        // Pattern 5: kebab-case identifiers
        let re5 = regex::Regex::new(r"\b[a-z]+-[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*\b").unwrap();
        for cap in re5.find_iter(content) {
            let e = cap.as_str().to_string();
            if seen.insert(e.clone()) {
                entities.push(e);
            }
        }

        // Pattern 6: File paths (.ext or leading /)
        let re6 = regex::Regex::new(r"(?:/[a-zA-Z0-9_./-]+|\b[a-zA-Z0-9_]+\.(?:rs|ts|js|py|toml|json|yaml|md|css))").unwrap();
        for cap in re6.find_iter(content) {
            let e = cap.as_str().to_string();
            if seen.insert(e.clone()) {
                entities.push(e);
            }
        }

        // Pattern 7: Quoted multi-word terms as single entity
        let re7 = regex::Regex::new(r#"[""'']([A-Za-z][A-Za-z0-9]+(?:\s+[A-Za-z][A-Za-z0-9]+)+)[""'']"#).unwrap();
        for cap in re7.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                let e = m.as_str().to_string();
                if seen.insert(e.clone()) {
                    entities.push(e);
                }
            }
        }

        // Pattern 8: URL origins
        let re8 = regex::Regex::new(r"https?://([a-zA-Z0-9_-]+(?:\.[a-zA-Z0-9_-]+)+)").unwrap();
        for cap in re8.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                let e = m.as_str().to_string();
                if seen.insert(e.clone()) {
                    entities.push(e);
                }
            }
        }

        entities
    }

    /// Tokenize content into normalized terms for BM25 indexing.
    fn tokenize(content: &str) -> Vec<String> {
        content
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .filter(|t| t.len() >= 3 && !Self::is_stopword(t))
            .map(|t| t.to_string())
            .collect()
    }

    fn is_stopword(term: &str) -> bool {
        matches!(term, "the" | "and" | "for" | "was" | "are" | "but" | "not"
                      | "this" | "that" | "with" | "from" | "have" | "has"
                      | "been" | "were" | "will" | "can" | "all" | "its"
                      | "also" | "than" | "very" | "just" | "about" | "been"
                      | "said" | "some" | "them" | "then" | "they" | "what"
                      | "when" | "where" | "which" | "who" | "how" | "into"
                      | "more" | "most" | "much" | "over" | "such" | "like")
    }

    // ═══════════════════════════════════════════
    // Importance Calculation (enhanced)
    // ═══════════════════════════════════════════

    /// Calculate importance based on content signals.
    /// Enhanced with more pattern signals and entity density bonus.
    pub fn calculate_importance(content: &str) -> f32 {
        let mut score: f32 = 0.5;

        // High-impact signal words (strong indicator of importance)
        let high_impact = [
            "always", "never", "must", "critical", "bug", "fix", "important",
            "prefers", "projects", "config", "API", "architecture", "requires",
            "mandatory", "blocking", "security", "vulnerability", "break",
            "deprecated", "migration", "production", "deploy",
        ];

        // Medium-impact signal words
        let medium_impact = [
            "usually", "often", "recommend", "pattern", "convention",
            "style", "prefer", "common", "typical", "should", "avoid",
            "instead", "preferred", "standard", "default",
        ];

        let lower = content.to_lowercase();
        for word in &high_impact {
            if lower.contains(word) {
                score += 0.08;
            }
        }
        for word in &medium_impact {
            if lower.contains(word) {
                score += 0.04;
            }
        }

        // Entity density bonus: more extracted entities = richer content
        let entities = Self::extract_entities(content);
        let entity_density = entities.len() as f32 / (content.len().max(1) as f32) * 100.0;
        if entity_density > 2.0 {
            score += 0.05;
        }
        if entity_density > 5.0 {
            score += 0.08;
        }

        // Recency signal: mentions of "yesterday", "today", "now" are contextual
        if lower.contains("today") || lower.contains("currently") || lower.contains("now") {
            score += 0.05;
        }

        score.min(1.0_f32).max(0.1_f32)
    }

    // ═══════════════════════════════════════════
    // Temporal Decay
    // ═══════════════════════════════════════════

    /// Exponential temporal decay score.
    /// Returns 1.0 for now, 0.5 after `half_life_days`, 0.25 after 2×, etc.
    fn temporal_decay(created_at: &DateTime<Utc>, half_life_days: f64) -> f64 {
        let now = Utc::now();
        let age = (now - *created_at).num_hours() as f64 / 24.0; // age in days
        if age <= 0.0 {
            return 1.0;
        }
        (-age / half_life_days).exp()
    }

    // ═══════════════════════════════════════════
    // Vector / Embedding Scoring
    // ═══════════════════════════════════════════

    /// Compute cosine similarity between two embedding vectors.
    /// Returns value in [0.0, 1.0] where 1.0 = identical direction.
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| (*x as f64) * (*y as f64)).sum();
        let norm_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
        let norm_b: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            (dot / (norm_a * norm_b)).clamp(0.0, 1.0)
        }
    }

    // ═══════════════════════════════════════════
    // BM25 Scoring
    // ═══════════════════════════════════════════

    /// Calculate BM25 score for a single document against query terms.
    /// Uses average document length from the database for length normalization.
    fn bm25_score(
        conn: &rusqlite::Connection,
        doc_id: &str,
        query_terms: &HashSet<String>,
        total_docs: f64,
        avg_doc_len: f64,
    ) -> f64 {
        let k1 = 1.2;
        let b = 0.75;

        let doc_len: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(freq), 0) FROM doc_terms WHERE doc_id = ?1",
                params![doc_id],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        let mut score = 0.0;
        for term in query_terms {
            // Document frequency of this term
            let df: f64 = conn
                .query_row(
                    "SELECT COALESCE(doc_count, 0) FROM term_df WHERE term = ?1",
                    params![term],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            if df == 0.0 {
                continue;
            }

            let idf = ((total_docs - df + 0.5) / (df + 0.5) + 1.0).ln();

            // Term frequency in this document
            let tf: f64 = conn
                .query_row(
                    "SELECT COALESCE(freq, 0) FROM doc_terms WHERE term = ?1 AND doc_id = ?2",
                    params![term, doc_id],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            if tf == 0.0 {
                continue;
            }

            let normalized_tf = (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * doc_len / avg_doc_len.max(1.0)));
            score += idf * normalized_tf;
        }

        score
    }

    // ═══════════════════════════════════════════
    // Entity Co-occurrence Boost
    // ═══════════════════════════════════════════

    /// Get co-occurring entities for a given entity, sorted by count.
    fn get_co_occurring_entities(
        conn: &rusqlite::Connection,
        entity: &str,
        limit: usize,
    ) -> Vec<(String, i64)> {
        let mut stmt = conn
            .prepare(
                "SELECT entity_b, count FROM entity_cooccurrence
                 WHERE entity_a = ?1 ORDER BY count DESC LIMIT ?2",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![entity, limit as i64], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    // ═══════════════════════════════════════════
    // Internal helpers
    // ═══════════════════════════════════════════

    /// Parse entities JSON from the DB row
    fn parse_entities(entities_str: &str) -> Vec<String> {
        serde_json::from_str(entities_str).unwrap_or_default()
    }

    /// Parse a memory row from query result
    fn row_to_memory(
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<MemoryEntry> {
        let mem_type_str: String = row.get(4)?;
        let mem_type = match mem_type_str.as_str() {
            "user_preference" => MemoryType::UserPreference,
            "codebase_fact" => MemoryType::CodebaseFact,
            "action_outcome" => MemoryType::ActionOutcome,
            "decision" => MemoryType::Decision,
            "bug_fix" => MemoryType::BugFix,
            "learned" => MemoryType::Learned,
            "ephemeral" => MemoryType::Ephemeral,
            _ => MemoryType::Learned,
        };
        let layer_str: String = row.get(5)?;
        let layer = match layer_str.as_str() {
            "policy" => MemoryLayer::Policy,
            "skill" => MemoryLayer::Skill,
            _ => MemoryLayer::Trace,
        };
        let entities_str: String = row.get(6)?;
        let entities: Vec<String> = serde_json::from_str(&entities_str).unwrap_or_default();
        let created: String = row.get(9)?;
        let accessed: String = row.get(10)?;

        let track = MemoryTrack::infer(&mem_type);

        // Read optional embedding BLOB (stored as f32 little-endian bytes)
        let embedding: Option<Vec<f32>> = row.get::<_, Option<Vec<u8>>>(8)?.map(|bytes| {
            bytes.chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        });

        Ok(MemoryEntry {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            session_id: row.get(2)?,
            content: row.get(3)?,
            memory_type: mem_type,
            layer,
            track,
            entities,
            importance: row.get(7)?,
            embedding,
            created_at: DateTime::parse_from_rfc3339(&created)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_accessed: DateTime::parse_from_rfc3339(&accessed)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            access_count: row.get(11)?,
            consolidated: row.get::<_, u32>(12)? != 0,
            container_tag: row.get::<_, String>(13).unwrap_or_else(|_| "_default".to_string()),
        })
    }

    /// Update BM25 indexes for a new memory entry.
    fn update_bm25_index(
        conn: &rusqlite::Connection,
        doc_id: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let terms = Self::tokenize(content);
        let mut term_freqs: HashMap<String, usize> = HashMap::new();
        for term in &terms {
            *term_freqs.entry(term.clone()).or_default() += 1;
        }

        for (term, freq) in &term_freqs {
            // Insert or update term frequency in document
            conn.execute(
                "INSERT OR REPLACE INTO doc_terms (term, doc_id, freq) VALUES (?1, ?2, ?3)",
                params![term, doc_id, *freq],
            )?;

            // Update global document frequency for the term
            conn.execute(
                "INSERT INTO term_df (term, doc_count) VALUES (?1, 1)
                 ON CONFLICT(term) DO UPDATE SET doc_count = doc_count + 1",
                params![term],
            )?;
        }

        Ok(())
    }

    /// Update entity co-occurrence graph for a new memory entry.
    fn update_entity_cooccurrence(
        conn: &rusqlite::Connection,
        entities: &[String],
    ) -> anyhow::Result<()> {
        for i in 0..entities.len() {
            for j in (i + 1)..entities.len() {
                let a = &entities[i];
                let b = &entities[j];

                // Both directions for fast lookup
                conn.execute(
                    "INSERT INTO entity_cooccurrence (entity_a, entity_b, count) VALUES (?1, ?2, 1)
                     ON CONFLICT(entity_a, entity_b) DO UPDATE SET count = count + 1",
                    params![a, b],
                )?;
                conn.execute(
                    "INSERT INTO entity_cooccurrence (entity_a, entity_b, count) VALUES (?1, ?2, 1)
                     ON CONFLICT(entity_a, entity_b) DO UPDATE SET count = count + 1",
                    params![b, a],
                )?;
            }
        }
        Ok(())
    }

    /// Update the memory_entities table for a new memory entry.
    /// Removes old entries first if updating, then inserts new ones.
    fn update_memory_entities(
        conn: &rusqlite::Connection,
        doc_id: &str,
        entities: &[String],
    ) -> anyhow::Result<()> {
        // Remove old entity links
        conn.execute(
            "DELETE FROM memory_entities WHERE memory_id = ?1",
            params![doc_id],
        )?;

        // Insert new entity links
        for entity in entities {
            conn.execute(
                "INSERT OR IGNORE INTO memory_entities (entity, memory_id) VALUES (?1, ?2)",
                params![entity, doc_id],
            )?;
        }

        Ok(())
    }
}

// ═══════════════════════════════════════════════
// MemoryStore trait implementation
// ═══════════════════════════════════════════════

impl MemoryStore for SqliteMemoryStore {
    fn insert(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let entities_json = serde_json::to_string(&entry.entities)?;

        // Convert embedding to bytes for SQLite storage
        let embedding_bytes: Option<Vec<u8>> = entry.embedding.as_ref().map(|v| {
            v.iter().flat_map(|f| f.to_le_bytes()).collect()
        });

        conn.execute(
            "INSERT INTO memories (id, agent_id, session_id, content, memory_type, memory_layer, entities, importance, embedding, created_at, last_accessed, access_count, consolidated, container_tag)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                entry.id,
                entry.agent_id,
                entry.session_id,
                entry.content,
                entry.memory_type.to_string(),
                entry.layer.to_string(),
                entities_json,
                entry.importance,
                embedding_bytes,
                entry.created_at.to_rfc3339(),
                entry.last_accessed.to_rfc3339(),
                entry.access_count,
                entry.consolidated as u32,
                entry.container_tag,
            ],
        )?;

        // Update entity co-occurrence graph
        Self::update_entity_cooccurrence(&conn, &entry.entities)?;

        // Update entity links table
        Self::update_memory_entities(&conn, &entry.id, &entry.entities)?;

        // Update BM25 term indexes
        Self::update_bm25_index(&conn, &entry.id, &entry.content)?;

        Ok(())
    }

    fn query(&self, query: &MemoryQuery) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, agent_id, session_id, content, memory_type, memory_layer, entities, importance, embedding, created_at, last_accessed, access_count, consolidated, container_tag
             FROM memories WHERE 1=1"
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        // Type filter
        if let Some(ref mem_type) = query.memory_type {
            sql.push_str(&format!(" AND memory_type = ?{}", param_values.len() + 1));
            param_values.push(Box::new(mem_type.to_string()));
        }

        // Container-tag filter
        if let Some(ref tag) = query.container_tag {
            sql.push_str(&format!(" AND container_tag = ?{}", param_values.len() + 1));
            param_values.push(Box::new(tag.clone()));
        }

        // Entity filter
        if let Some(ref entity) = query.entity {
            sql.push_str(&format!(
                " AND id IN (SELECT memory_id FROM memory_entities WHERE entity = ?{})",
                param_values.len() + 1
            ));
            param_values.push(Box::new(entity.clone()));
        }

        // Age filter
        if let Some(ref max_age) = query.max_age {
            let cutoff = (Utc::now() - *max_age).to_rfc3339();
            sql.push_str(&format!(" AND created_at >= ?{}", param_values.len() + 1));
            param_values.push(Box::new(cutoff));
        }

        // Keyword search in content (basic LIKE)
        let search_terms: Vec<&str> = query.text.split_whitespace()
            .filter(|w| w.len() > 2 && !["the", "and", "for", "was", "are", "but", "not"].contains(w))
            .collect();

        if !search_terms.is_empty() {
            sql.push_str(" AND (");
            for (i, term) in search_terms.iter().enumerate() {
                if i > 0 {
                    sql.push_str(" OR ");
                }
                sql.push_str(&format!("content LIKE ?{}", param_values.len() + 1));
                param_values.push(Box::new(format!("%{}%", term)));
            }
            sql.push(')');
        }

        // Order by relevance (importance × recency × access_count boost)
        sql.push_str(" ORDER BY importance * (1.0 + access_count * 0.1) * CASE WHEN consolidated THEN 1.2 ELSE 1.0 END DESC");

        // Limit
        sql.push_str(&format!(" LIMIT ?{}", param_values.len() + 1));
        param_values.push(Box::new(query.limit as i64));

        let mut stmt = conn.prepare(&sql)?;

        let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter()
            .map(|p| p.as_ref())
            .collect();

        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Self::row_to_memory(row)
        })?;

        let results: Vec<MemoryEntry> = rows.filter_map(|r| r.ok()).collect();

        // Update last_accessed for retrieved memories
        if !results.is_empty() {
            let now = Utc::now().to_rfc3339();
            for mem in &results {
                let _ = conn.execute(
                    "UPDATE memories SET last_accessed = ?1, access_count = access_count + 1 WHERE id = ?2",
                    params![now, mem.id],
                );
            }
        }

        Ok(results)
    }

    fn fused_search(&self, query: &MemoryQuery) -> anyhow::Result<Vec<ScoredMemory>> {
        let conn = self.conn.lock().unwrap();

        // ── Step 1: Fetch total doc count and avg doc length for BM25 ──
        let total_docs: f64 = conn
            .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
            .unwrap_or(1) as f64;

        let avg_doc_len: f64 = conn
            .query_row(
                "SELECT COALESCE(AVG(doc_len), 40.0) FROM (
                    SELECT COALESCE(SUM(freq), 0) AS doc_len FROM doc_terms GROUP BY doc_id
                )",
                [],
                |row| row.get(0),
            )
            .unwrap_or(40.0);

        // ── Step 2: Prepare query terms ──
        let query_terms: HashSet<String> = query.text
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .flat_map(|w| {
                let lower = w.to_lowercase();
                if Self::is_stopword(&lower) { None } else { Some(lower) }
            })
            .collect();

        // ── Step 3: Entity expansion ──
        // Find entities in the query text
        let query_entities = Self::extract_entities(&query.text);
        // Also expand: find co-occurring entities for each query entity
        let mut expanded_entities: HashSet<String> = HashSet::new();
        for entity in &query_entities {
            expanded_entities.insert(entity.clone());
            let co_occurring = Self::get_co_occurring_entities(&conn, entity, 10);
            for (co_entity, _count) in &co_occurring {
                expanded_entities.insert(co_entity.clone());
            }
        }

        // ── Step 4: Build SQL to fetch all candidate memories ──
        // Start with all memories, apply type/age/entity filters
        let mut sql = String::from(
            "SELECT m.id, m.agent_id, m.session_id, m.content, m.memory_type,
                     m.memory_layer, m.entities, m.importance, m.embedding, m.created_at, m.last_accessed,
                     m.access_count, m.consolidated, m.container_tag
             FROM memories m WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref mem_type) = query.memory_type {
            sql.push_str(&format!(" AND m.memory_type = ?{}", param_values.len() + 1));
            param_values.push(Box::new(mem_type.to_string()));
        }

        if let Some(ref tag) = query.container_tag {
            sql.push_str(&format!(" AND m.container_tag = ?{}", param_values.len() + 1));
            param_values.push(Box::new(tag.clone()));
        }

        if let Some(ref max_age) = query.max_age {
            let cutoff = (Utc::now() - *max_age).to_rfc3339();
            sql.push_str(&format!(" AND m.created_at >= ?{}", param_values.len() + 1));
            param_values.push(Box::new(cutoff));
        }

        // If we have entity expansion, boost candidates via entity match
        if !expanded_entities.is_empty() {
            // Entity match: get memory IDs linked to any expanded entity
            sql.push_str(" AND (");
            let mut first = true;
            for entity in &expanded_entities {
                if !first {
                    sql.push_str(" OR ");
                }
                sql.push_str(&format!(
                    "m.id IN (SELECT memory_id FROM memory_entities WHERE entity = ?{})",
                    param_values.len() + 1
                ));
                param_values.push(Box::new(entity.clone()));
                first = false;
            }
            // Always include content keyword matches too
            sql.push(')');
        }

        // ── Step 5: Fetch all candidates ──
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter()
            .map(|p| p.as_ref())
            .collect();

        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Self::row_to_memory(row)
        })?;

        let candidates: Vec<MemoryEntry> = rows.filter_map(|r| r.ok()).collect();
        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        // ── Step 6: Score each candidate ──
        let half_life = TEMPORAL_HALF_LIFE_DAYS;

        // Pre-compute entity match sets for faster scoring
        let candidate_entity_sets: HashMap<String, HashSet<String>> = candidates
            .iter()
            .map(|m| (m.id.clone(), m.entities.iter().cloned().collect()))
            .collect();

        let query_embedding_ref = query.query_embedding.as_ref();

        let mut scored: Vec<ScoredMemory> = candidates
            .into_iter()
            .map(|entry| {
                // ── Keyword / BM25 score ──
                let bm25 = if query_terms.is_empty() {
                    0.0
                } else {
                    Self::bm25_score(&conn, &entry.id, &query_terms, total_docs, avg_doc_len)
                };

                // ── Entity score ──
                let entry_entities = candidate_entity_sets.get(entry.id.as_str()).cloned().unwrap_or_default();
                let entity_score = if expanded_entities.is_empty() || entry_entities.is_empty() {
                    0.0
                } else {
                    // Direct entity match
                    let direct_matches: usize = entry_entities
                        .intersection(&expanded_entities)
                        .count();
                    // Co-occurrence bonus (entity not in query but co-occurs with query entity)
                    let mut score = direct_matches as f64;
                    if score > 0.0 {
                        score *= ENTITY_CO_OCCUR_BOOST;
                    }
                    score
                };

                // ── Temporal score ──
                let temporal = Self::temporal_decay(&entry.created_at, half_life);

                // ── Importance score (with layer multiplier) ──
                let imp = entry.importance as f64;
                let layer_mult = entry.layer.score_multiplier();
                let imp_with_layer = imp * layer_mult;
                let vector_score = if let (Some(qe), Some(ee)) = (query_embedding_ref, entry.embedding.as_ref()) {
                    Self::cosine_similarity(qe, ee)
                } else {
                    0.0
                };

                // ── Fused total score ──
                let total = query.keyword_weight * bm25
                    + query.entity_weight * entity_score
                    + query.temporal_weight * temporal
                    + query.importance_weight * imp_with_layer
                    + query.vector_weight * vector_score;

                ScoredMemory {
                    total_score: total,
                    keyword_score: bm25,
                    entity_score,
                    temporal_score: temporal,
                    importance_score: imp,
                    vector_score,
                    entry,
                }
            })
            .collect();

        // ── Step 7: Sort by total_score descending ──
        scored.sort_by(|a, b| b.total_score.partial_cmp(&a.total_score).unwrap_or(std::cmp::Ordering::Equal));

        // ── Step 8: Update last_accessed for top results ──
        let now = Utc::now().to_rfc3339();
        for sm in &scored[..scored.len().min(query.limit)] {
            let _ = conn.execute(
                "UPDATE memories SET last_accessed = ?1, access_count = access_count + 1 WHERE id = ?2",
                params![now, sm.entry.id],
            );
        }

        // Trim to limit
        scored.truncate(query.limit);

        Ok(scored)
    }

    fn query_by_entity(&self, entity: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let q = MemoryQuery { entity: Some(entity.to_string()), limit, ..Default::default() };
        self.query(&q)
    }

    fn list_entities(&self) -> anyhow::Result<Vec<(String, usize)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT entity, COUNT(*) as cnt FROM memory_entities GROUP BY entity ORDER BY cnt DESC LIMIT 200"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn mark_consolidated(&self, ids: &[String]) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        for id in ids {
            conn.execute(
                "UPDATE memories SET consolidated = 1 WHERE id = ?1",
                params![id],
            )?;
        }
        Ok(())
    }

    fn get_unconsolidated(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, session_id, content, memory_type, memory_layer, entities, importance, created_at, last_accessed, access_count, consolidated
             FROM memories WHERE consolidated = 0 ORDER BY created_at ASC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Self::row_to_memory(row)
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn delete(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        // Remove from all auxiliary tables
        conn.execute("DELETE FROM memory_entities WHERE memory_id = ?1", params![id])?;
        conn.execute("DELETE FROM doc_terms WHERE doc_id = ?1", params![id])?;
        conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        // Note: entity_cooccurrence and term_df are left intact (could be cleaned by periodic maintenance)
        Ok(())
    }

    fn count(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

// ═══════════════════════════════════════════════
// Query Cache — background prefetch support
// ═══════════════════════════════════════════════

const QUERY_CACHE_MAX: usize = 5;

/// Bounded in-memory cache for memory query results.
/// Used for background prefetch: warm the cache on one turn,
/// serve from cache on the next (if query overlaps).
struct QueryCache {
    entries: VecDeque<(String, Vec<ScoredMemory>, Instant)>,
}

impl QueryCache {
    fn new() -> Self {
        Self { entries: VecDeque::with_capacity(QUERY_CACHE_MAX) }
    }

    /// Check if a query can be served from cache.
    /// Returns cached results if the query text shares a significant prefix.
    fn get(&mut self, query: &str) -> Option<Vec<ScoredMemory>> {
        let lower = query.to_lowercase();
        for i in 0..self.entries.len() {
            let (cached_q, results, _ts) = &self.entries[i];
            let cached_lower = cached_q.to_lowercase();
            // Prefix match: either query starts with cached or cached starts with query
            if lower.len() >= 3 && cached_lower.len() >= 3 {
                if lower.starts_with(&cached_lower) || cached_lower.starts_with(&lower) {
                    return Some(results.clone());
                }
            }
        }
        None
    }

    fn put(&mut self, query: &str, results: Vec<ScoredMemory>) {
        if self.entries.len() >= QUERY_CACHE_MAX {
            self.entries.pop_front();
        }
        self.entries.push_back((query.to_string(), results, Instant::now()));
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

// ═══════════════════════════════════════════════
// Response Scrubber — streaming memory artifact removal
// ═══════════════════════════════════════════════

/// Scrub an LLM response to remove any leaked memory context artifacts.
/// Prevents the model from accidentally echoing `<memory-context>` blocks,
/// system notes, or memory entry lines back to the user.
///
/// Applied as a final filter on every LLM response text.
pub fn scrub_response(response: &str, memory_ctx: &str) -> String {
    if response.is_empty() || memory_ctx.is_empty() {
        return response.to_string();
    }

    let mut cleaned = response.to_string();

    // 1. Strip full <memory-context>...</memory-context> blocks
    let re_open = regex::Regex::new(r"(?s)<memory-context>.*?</memory-context>").unwrap();
    cleaned = re_open.replace_all(&cleaned, "").to_string();

    // 2. Strip orphaned <memory-context> or </memory-context> tags
    cleaned = cleaned.replace("<memory-context>", "");
    cleaned = cleaned.replace("</memory-context>", "");

    // 3. Strip [System note: ...] lines
    let re_sysnote = regex::Regex::new(r"(?m)^\s*\[System note:[^\]]*\]\s*$").unwrap();
    cleaned = re_sysnote.replace_all(&cleaned, "").to_string();

    // 4. Strip lines matching memory entry format: [Xm/h/d ago] (Track/Layer) [N%] content
    let re_entry = regex::Regex::new(
        r"(?m)^\s*\[\d+[mhd]\s+ago\]\s*\([^)]+\)\s*\[\d+%\]\s+.*$"
    ).unwrap();
    cleaned = re_entry.replace_all(&cleaned, "").to_string();

    // 5. Strip "--- Relevant Past Knowledge ---" or similar headers
    let re_header = regex::Regex::new(r"(?m)^---\s*Relevant Past Knowledge\s*---\s*$").unwrap();
    cleaned = re_header.replace_all(&cleaned, "").to_string();

    // 6. Remove excessive blank lines left by deletions
    let re_blank = regex::Regex::new(r"\n{3,}").unwrap();
    cleaned = re_blank.replace_all(&cleaned, "\n\n").to_string();

    cleaned.trim().to_string()
}

// ═══════════════════════════════════════════════
// Session Lifecycle Events
// ═══════════════════════════════════════════════

/// Events fired during memory lifecycle.
#[derive(Debug, Clone)]
pub enum MemoryEvent {
    /// Session changed from old_session to new_session
    SessionSwitch { old_session: Option<String>, new_session: String },
    /// Pre-compression: memory_ids are about to be consolidated
    PreCompress { memory_ids: Vec<String> },
    /// Session ended
    SessionEnd { session_id: String, memory_count: usize },
}

/// Type alias for memory lifecycle event handler.
pub type MemoryEventHandler = Arc<dyn Fn(MemoryEvent) + Send + Sync>;

/// Optional lifecycle hooks for the MemoryManager.
pub struct SessionHooks {
    pub on_event: Option<MemoryEventHandler>,
}

impl SessionHooks {
    pub fn new() -> Self {
        Self { on_event: None }
    }

    pub fn with_handler(handler: MemoryEventHandler) -> Self {
        Self { on_event: Some(handler) }
    }

    pub fn fire(&self, event: MemoryEvent) {
        if let Some(ref handler) = self.on_event {
            handler(event);
        }
    }
}

// ═══════════════════════════════════════════════
// Memory Manager — orchestrates memory operations
// ═══════════════════════════════════════════════

pub struct MemoryManager {
    store: Box<dyn MemoryStore>,
    agent_id: String,
    session_id: Option<String>,
    /// Container tag namespace. All `remember()` calls write into this tag;
    /// all `recall()` calls filter to this tag by default. Inspired by
    /// supermemory's containerTag — isolates memory per project / user / customer.
    container_tag: String,
    /// Bounded query result cache for background prefetch.
    query_cache: Arc<Mutex<QueryCache>>,
    /// Optional session lifecycle hooks.
    hooks: Arc<Mutex<SessionHooks>>,
}

impl MemoryManager {
    pub fn new(store: Box<dyn MemoryStore>, agent_id: &str) -> Self {
        Self {
            store,
            agent_id: agent_id.to_string(),
            session_id: None,
            container_tag: "_default".to_string(),
            query_cache: Arc::new(Mutex::new(QueryCache::new())),
            hooks: Arc::new(Mutex::new(SessionHooks::new())),
        }
    }

    /// Bind this manager to a specific container tag.
    /// Subsequent `remember()` and `recall()` calls will be scoped to this tag.
    /// Equivalent to supermemory's `new Supermemory({ containerTag: ... })`.
    #[allow(dead_code)]
    pub fn with_container(mut self, tag: impl Into<String>) -> Self {
        self.container_tag = tag.into();
        self
    }

    /// Get the current container tag.
    #[allow(dead_code)]
    pub fn container_tag(&self) -> &str {
        &self.container_tag
    }

    #[allow(dead_code)]
    pub fn with_session(mut self, session_id: String) -> Self {
        let old = self.session_id.clone();
        self.session_id = Some(session_id.clone());
        self.fire_event(MemoryEvent::SessionSwitch {
            old_session: old,
            new_session: session_id,
        });
        self
    }

    /// Register a session lifecycle event handler.
    pub fn with_hooks(mut self, handler: MemoryEventHandler) -> Self {
        self.hooks = Arc::new(Mutex::new(SessionHooks::with_handler(handler)));
        self
    }

    /// Fire a memory lifecycle event.
    pub fn fire_event(&self, event: MemoryEvent) {
        if let Ok(hooks) = self.hooks.lock() {
            hooks.fire(event);
        }
    }

    /// Prefetch: warm the query cache by running recall_fused in the background.
    /// The cache is checked by `recall_fused()` on subsequent calls,
    /// reducing latency for overlapping queries.
    pub fn prefetch(&self, query: &str, limit: usize) {
        if let Ok(mut cache) = self.query_cache.lock() {
            let q = MemoryQuery {
                text: query.to_string(),
                limit,
                ..Default::default()
            };
            if let Ok(results) = self.store.fused_search(&q) {
                cache.put(query, results);
            }
        }
    }

    /// Clear the query cache.
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.query_cache.lock() {
            cache.clear();
        }
    }

    /// Record a memory from agent conversation, with automatic
    /// symbolic compression for tool outcomes and ephemeral entries.
    pub fn remember(&self, content: &str, memory_type: MemoryType) -> anyhow::Result<String> {
        let entities = SqliteMemoryStore::extract_entities(content);
        let importance = SqliteMemoryStore::calculate_importance(content);
        let layer = MemoryLayer::default_for(&memory_type);
        let track = MemoryTrack::infer(&memory_type);

        // Apply symbolic compression for action outcomes and ephemeral data
        let (stored_content, _compression_ratio) = match memory_type {
            MemoryType::ActionOutcome | MemoryType::Ephemeral => {
                crate::memory::symbolic::compress(content, &memory_type)
            }
            _ => (content.to_string(), 0.0),
        };

        let entry = MemoryEntry {
            id: Uuid::new_v4().to_string(),
            agent_id: self.agent_id.clone(),
            session_id: self.session_id.clone(),
            content: stored_content,
            memory_type,
            layer,
            track,
            entities,
            importance,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            consolidated: false,
            embedding: None,
            container_tag: self.container_tag.clone(),
        };

        let id = entry.id.clone();
        self.store.insert(entry)?;
        Ok(id)
    }

    /// Recall memories relevant to a query (basic mode)
    pub fn recall(&self, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let q = MemoryQuery {
            text: query.to_string(),
            limit,
            container_tag: Some(self.container_tag.clone()),
            ..Default::default()
        };
        self.store.query(&q)
    }

    /// Recall with fused multi-signal ranking (preferred).
    /// Checks the prefetch cache first for overlapping queries;
    /// falls back to SQL fused_search and caches the result.
    pub fn recall_fused(&self, query: &str, limit: usize) -> anyhow::Result<Vec<ScoredMemory>> {
        // Check prefetch cache first
        if let Ok(mut cache) = self.query_cache.lock() {
            if let Some(cached) = cache.get(query) {
                return Ok(cached);
            }
        }

        // Cache miss — run query (auto-scoped to self.container_tag)
        let q = MemoryQuery {
            text: query.to_string(),
            limit,
            container_tag: Some(self.container_tag.clone()),
            ..Default::default()
        };
        let results = self.store.fused_search(&q)?;

        // Cache the result for future queries
        if let Ok(mut cache) = self.query_cache.lock() {
            cache.put(query, results.clone());
        }

        Ok(results)
    }

    /// Recall with custom signal weights for fine-tuned retrieval
    pub fn recall_weighted(
        &self,
        query: &str,
        limit: usize,
        temporal_weight: f64,
        entity_weight: f64,
        keyword_weight: f64,
    ) -> anyhow::Result<Vec<ScoredMemory>> {
        let q = MemoryQuery {
            text: query.to_string(),
            limit,
            temporal_weight,
            entity_weight,
            keyword_weight,
            ..Default::default()
        };
        self.store.fused_search(&q)
    }

    /// Recall by memory type
    #[allow(dead_code)]
    pub fn recall_by_type(
        &self,
        memory_type: MemoryType,
        _type_limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let q = MemoryQuery {
            memory_type: Some(memory_type),
            limit: _type_limit,
            ..Default::default()
        };
        self.store.query(&q)
    }

    /// Build a static "user identity" profile from long-lived facts
    /// (UserPreference + CodebaseFact + Decision) inside this container.
    /// Mirrors supermemory's `profile.static` view — the stable knowledge
    /// that should be injected into every prompt.
    ///
    /// Format: one fact per line, grouped by type, capped at `max_facts` total
    /// (oldest-accessed first to prevent profile drift toward recency).
    pub fn profile(&self, max_facts: usize) -> anyhow::Result<String> {
        let mut out = String::new();
        out.push_str("# User Profile (static)\n");

        for (label, mem_type) in [
            ("preferences", MemoryType::UserPreference),
            ("codebase_facts", MemoryType::CodebaseFact),
            ("decisions", MemoryType::Decision),
        ] {
            let q = MemoryQuery {
                memory_type: Some(mem_type),
                limit: max_facts,
                ..Default::default()
            };
            let entries = self.store.query(&q)?;
            if entries.is_empty() {
                continue;
            }
            out.push_str(&format!("\n## {}\n", label));
            for e in entries.iter().take(max_facts) {
                out.push_str(&format!("- {}\n", e.content));
            }
        }

        if out.trim() == "# User Profile (static)" {
            return Ok(String::new());
        }
        Ok(out)
    }

    /// Access the underlying memory store (for low-level operations).
    pub fn store_ref(&self) -> &dyn MemoryStore {
        self.store.as_ref()
    }

    /// End a session: fires SessionEnd event. Call when the agent switches
    /// away from or completes a session.
    pub fn end_session(&self) {
        let session_id = self.session_id.clone().unwrap_or_default();
        if let Ok(memories) = self.store.count() {
            self.fire_event(MemoryEvent::SessionEnd {
                session_id,
                memory_count: memories,
            });
        }
    }

    /// Export all memories to a JSON file.
    /// Useful for session persistence, debugging, or transfer between agents.
    pub fn save_session(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let all = self.store.query(&MemoryQuery {
            limit: usize::MAX,
            ..Default::default()
        })?;
        let json = serde_json::to_string_pretty(&all)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Import memories from a JSON file previously exported by `save_session`.
    /// Returns the number of memories loaded.
    pub fn load_session(&self, path: &std::path::Path) -> anyhow::Result<usize> {
        let json = std::fs::read_to_string(path)?;
        let entries: Vec<MemoryEntry> = serde_json::from_str(&json)?;
        let count = entries.len();
        for entry in entries {
            self.store.insert(entry)?;
        }
        Ok(count)
    }

    #[allow(dead_code)]
    /// Recall by entity (project/library/tool name)
    pub fn recall_by_entity(&self, entity: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        self.store.query_by_entity(entity, limit)
    }

    /// Build a fenced memory context string for LLM prompts.
    /// Uses `<memory-context>` fences (Hermes-compatible) to prevent
    /// memory injection from being confused with user input.
    pub fn build_context(&self, task: &str, max_memories: usize) -> anyhow::Result<String> {
        // Use fused search for better quality
        let memories = self.recall_fused(task, max_memories)?;
        if memories.is_empty() {
            return Ok(String::new());
        }

        let mut body = String::new();
        for sm in &memories {
            let ago = chrono::Utc::now().signed_duration_since(sm.entry.created_at);
            let ago_str = if ago.num_minutes() < 60 {
                format!("{}m ago", ago.num_minutes())
            } else if ago.num_hours() < 24 {
                format!("{}h ago", ago.num_hours())
            } else {
                format!("{}d ago", ago.num_days())
            };
            let relevance_pct = (sm.total_score.min(10.0) / 10.0 * 100.0) as u32;
            body.push_str(&format!(
                "  [{ago_str}] ({track}/{layer}) [{relevance_pct}%] {content}\n",
                track = sm.entry.track,
                layer = sm.entry.layer,
                content = sm.entry.content,
            ));
        }

        // Fenced context block (Hermes-compatible)
        let context = format!(
            "\n<memory-context>\n\
             [System note: The following is recalled memory context, \
             NOT new user input. Treat as authoritative reference data - \
             this is the agent's persistent memory and should inform all responses.]\n\n\
             {body}\
             </memory-context>"
        );
        Ok(context)
    }

    /// Build a context string with signal breakdown for debugging
    #[allow(dead_code)]
    pub fn build_debug_context(&self, task: &str, max_memories: usize) -> anyhow::Result<String> {
        let memories = self.recall_fused(task, max_memories)?;
        if memories.is_empty() {
            return Ok(String::new());
        }

        let mut context = String::from("\n\n--- Memory Debug Context ---\n");
        for sm in &memories {
            context.push_str(&format!(
                "  [{:.2}] kw={:.2} ent={:.2} tmp={:.2} imp={:.2} | {}\n",
                sm.total_score, sm.keyword_score, sm.entity_score,
                sm.temporal_score, sm.importance_score, sm.entry.content,
            ));
        }
        Ok(context)
    }

    /// List all known entities
    pub fn entities(&self) -> anyhow::Result<Vec<(String, usize)>> {
        self.store.list_entities()
    }

    pub fn count(&self) -> anyhow::Result<usize> {
        self.store.count()
    }

    /// Prune old/low-importance memories, keeping only the top N most relevant
    pub fn prune(&self, max_entries: usize) -> anyhow::Result<usize> {
        let all = self.store.query(&MemoryQuery {
            limit: usize::MAX,
            ..Default::default()
        })?;

        if all.len() <= max_entries {
            return Ok(0);
        }

        // Score with temporal decay + importance for pruning decisions
        let half_life = TEMPORAL_HALF_LIFE_DAYS;
        let mut entries: Vec<(&MemoryEntry, f64)> = all.iter().map(|e| {
            let temp = SqliteMemoryStore::temporal_decay(&e.created_at, half_life);
            let score = e.importance as f64 * 0.6 + temp * 0.4;
            (e, score)
        }).collect();

        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let to_keep: std::collections::HashSet<&str> = entries
            .iter()
            .take(max_entries)
            .map(|(e, _)| e.id.as_str())
            .collect();

        let mut pruned = 0;
        for (entry, _) in &entries {
            if !to_keep.contains(entry.id.as_str()) && self.store.delete(&entry.id).is_ok() {
                pruned += 1;
            }
        }

        Ok(pruned)
    }

    pub fn store(&self) -> &dyn MemoryStore {
        &*self.store
    }

    /// Dreaming — offline memory consolidation.
    /// Fetches unconsolidated memories, groups them by entity overlap,
    /// and merges related groups into summary entries stored in the
    /// memory_summaries table. Marks source memories as consolidated.
    ///
    /// This is a batch operation, typically called periodically in the
    /// background (e.g., via cron or idle cycles).
    pub fn dream(&self, batch_size: usize) -> anyhow::Result<usize> {
        let uncon = self.store.get_unconsolidated(batch_size)?;
        if uncon.is_empty() {
            return Ok(0);
        }

        // Group by overlapping entities (simple overlap >= 1 shared entity)
        let mut groups: Vec<Vec<MemoryEntry>> = Vec::new();
        let mut assigned: HashSet<String> = HashSet::new();

        for entry in &uncon {
            if assigned.contains(&entry.id) {
                continue;
            }
            let mut group = vec![entry.clone()];
            assigned.insert(entry.id.clone());
            let entry_entities: HashSet<&str> = entry.entities.iter().map(|s| s.as_str()).collect();

            for other in &uncon {
                if assigned.contains(&other.id) || other.id == entry.id {
                    continue;
                }
                let other_entities: HashSet<&str> =
                    other.entities.iter().map(|s| s.as_str()).collect();
                if entry_entities.intersection(&other_entities).count() > 0 {
                    group.push(other.clone());
                    assigned.insert(other.id.clone());
                }
            }
            groups.push(group);
        }

        // Create summary entries for each group with ≥2 members
        let mut marked_ids: Vec<String> = Vec::new();
        for group in &groups {
            if group.len() < 2 {
                continue;
            }
            let merged_ids: Vec<String> = group.iter().map(|e| e.id.clone()).collect();
            let merged_content: String = group
                .iter()
                .map(|e| format!("- {} [{}]", e.content, e.memory_type))
                .collect::<Vec<_>>()
                .join("\n");

            // Store consolidated summary as a Learned memory entry
            let summary = MemoryEntry {
                id: Uuid::new_v4().to_string(),
                agent_id: self.agent_id.clone(),
                session_id: self.session_id.clone(),
                content: format!("Consolidated summary:\n{}", merged_content),
                memory_type: MemoryType::Learned,
                layer: MemoryLayer::Skill,
                track: MemoryTrack::infer(&MemoryType::Learned),
                entities: group.iter().flat_map(|e| e.entities.clone()).collect(),
                importance: group.iter().map(|e| e.importance).sum::<f32>() / group.len() as f32,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                access_count: 1,
                consolidated: true,
                embedding: None,
                container_tag: self.container_tag.clone(),
            };
            if self.store.insert(summary).is_ok() {
                // Fire pre-compress event for the merged source memories
                self.fire_event(MemoryEvent::PreCompress {
                    memory_ids: merged_ids,
                });
            }
            marked_ids.extend(group.iter().map(|e| e.id.clone()));
        }

        // Mark as consolidated
        if !marked_ids.is_empty() {
            self.store.mark_consolidated(&marked_ids)?;
        }

        Ok(groups.iter().filter(|g| g.len() >= 2).count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_extraction() {
        let text = "HyperAgent uses SQLite. The API is available at /api/v1. User prefers concise responses.";
        let entities = SqliteMemoryStore::extract_entities(text);
        assert!(entities.contains(&"HyperAgent".to_string()), "Should extract PascalCase");
        assert!(entities.contains(&"SQLite".to_string()), "Should extract UPPER_CASE");
        assert!(entities.contains(&"API".to_string()), "Should extract acronyms");
    }

    #[test]
    fn test_entity_extraction_snake_case() {
        let text = "The memory_manager uses build_context to recall_by_entity.";
        let entities = SqliteMemoryStore::extract_entities(text);
        assert!(entities.contains(&"memory_manager".to_string()));
        assert!(entities.contains(&"build_context".to_string()));
        assert!(entities.contains(&"recall_by_entity".to_string()));
    }

    #[test]
    fn test_entity_extraction_qualified() {
        let text = "Use crate::memory::SqliteMemoryStore to store memories.";
        let entities = SqliteMemoryStore::extract_entities(text);
        assert!(entities.contains(&"crate::memory::SqliteMemoryStore".to_string()));
    }

    #[test]
    fn test_entity_extraction_file_paths() {
        let text = "The file /src/memory.rs defines the memory system. Also check Cargo.toml.";
        let entities = SqliteMemoryStore::extract_entities(text);
        assert!(entities.contains(&"/src/memory.rs".to_string()));
        assert!(entities.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn test_entity_extraction_kebab() {
        let text = "The long-term-memory system uses entity-co-occurrence-graph.";
        let entities = SqliteMemoryStore::extract_entities(text);
        assert!(entities.contains(&"long-term-memory".to_string()));
        assert!(entities.contains(&"entity-co-occurrence-graph".to_string()));
    }

    #[test]
    fn test_entity_extraction_url() {
        let text = "API available at https://github.com/openai/api";
        let entities = SqliteMemoryStore::extract_entities(text);
        assert!(entities.contains(&"github.com".to_string()));
    }

    #[test]
    fn test_importance_scoring() {
        let high = "This is a critical bug fix that must be applied to all systems";
        let medium = "We usually prefer the builder pattern";
        let low = "The weather is nice today";

        assert!(SqliteMemoryStore::calculate_importance(high) >
                SqliteMemoryStore::calculate_importance(low));
        assert!(SqliteMemoryStore::calculate_importance(medium) >
                SqliteMemoryStore::calculate_importance(low));
    }

    #[test]
    fn test_temporal_decay() {
        let now = Utc::now();
        let one_day_ago = now - chrono::Duration::days(1);
        let one_month_ago = now - chrono::Duration::days(30);

        let fresh = SqliteMemoryStore::temporal_decay(&now, 14.0);
        let day_old = SqliteMemoryStore::temporal_decay(&one_day_ago, 14.0);
        let month_old = SqliteMemoryStore::temporal_decay(&one_month_ago, 14.0);

        assert!((fresh - 1.0).abs() < 0.01, "Fresh memory should score ~1.0");
        assert!(day_old > month_old, "Recent memory should score higher than old");
        assert!(day_old > 0.9, "1 day old with 14 day half-life should be > 0.9");
        assert!(month_old < 0.3, "30 day old with 14 day half-life should be < 0.3");
    }

    #[test]
    fn test_bm25_tokenize() {
        let terms = SqliteMemoryStore::tokenize("The quick brown fox jumps over");
        assert!(!terms.contains(&"the".to_string()), "Stopwords should be filtered");
        assert!(terms.contains(&"quick".to_string()));
        assert!(terms.contains(&"brown".to_string()));
    }

    #[test]
    fn test_extract_entities_respects_uniqueness() {
        let text = "HyperAgent uses SQLite. HyperAgent is fast. SQLite is reliable.";
        let entities = SqliteMemoryStore::extract_entities(text);
        let count_hyper = entities.iter().filter(|e| *e == "HyperAgent").count();
        let count_sqlite = entities.iter().filter(|e| *e == "SQLite").count();
        assert_eq!(count_hyper, 1, "Entities should be unique");
        assert_eq!(count_sqlite, 1, "Entities should be unique");
    }

    #[test]
    fn test_build_context_includes_relevance() {
        // Just verify build_context doesn't crash on empty
        // Full integration test requires SQLite
        let temp_dir = std::env::temp_dir().join("test_memory_build_context");
        let _ = std::fs::create_dir_all(&temp_dir);
        let db_path = temp_dir.join("test.db");
        let _ = std::fs::remove_file(&db_path);

        let store = SqliteMemoryStore::new(&db_path).unwrap();
        let manager = MemoryManager::new(Box::new(store), "test");

        let ctx = manager.build_context("test query", 5).unwrap();
        assert_eq!(ctx, "", "Empty store should produce empty context");

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_container_tag_isolation() {
        // Two managers backed by the SAME SQLite file but scoped to
        // different container tags must never see each other's memories.
        let temp_dir = std::env::temp_dir().join("test_memory_container_tag");
        let _ = std::fs::create_dir_all(&temp_dir);
        let db_path = temp_dir.join("test.db");
        let _ = std::fs::remove_file(&db_path);

        // Write into container "alpha"
        let store_a = SqliteMemoryStore::new(&db_path).unwrap();
        let mut mgr_a = MemoryManager::new(Box::new(store_a), "agent-a");
        mgr_a = mgr_a.with_container("alpha");
        mgr_a
            .remember("Project Alpha uses Rust and tokio", MemoryType::UserPreference)
            .unwrap();

        // Write into container "beta" via the SAME on-disk file
        let store_b = SqliteMemoryStore::new(&db_path).unwrap();
        let mut mgr_b = MemoryManager::new(Box::new(store_b), "agent-b");
        mgr_b = mgr_b.with_container("beta");
        mgr_b
            .remember("Project Beta uses Python and FastAPI", MemoryType::UserPreference)
            .unwrap();

        // Manager A should only see Alpha content
        let alpha = mgr_a.recall("rust", 10).unwrap();
        assert_eq!(alpha.len(), 1, "alpha container should have 1 hit");
        assert!(alpha[0].content.contains("Alpha"));
        assert!(alpha[0].container_tag == "alpha");

        // Manager B should only see Beta content
        let beta = mgr_b.recall("python", 10).unwrap();
        assert_eq!(beta.len(), 1, "beta container should have 1 hit");
        assert!(beta[0].content.contains("Beta"));
        assert!(beta[0].container_tag == "beta");

        // Cross-leak: B asking for "rust" must return nothing
        let leak = mgr_b.recall("rust", 10).unwrap();
        assert!(leak.is_empty(), "beta must not see alpha's memories");

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_profile_static_view() {
        let db_path = std::env::temp_dir().join(format!(
            "hyperagent_profile_test_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);

        let store = SqliteMemoryStore::new(&db_path).unwrap();
        let mgr = MemoryManager::new(Box::new(store), "agent").with_container("profile-test");

        // Empty profile: should return empty string
        assert_eq!(mgr.profile(10).unwrap(), "");

        // Plant static facts
        mgr.remember(
            "User prefers Rust for systems work",
            MemoryType::UserPreference,
        )
        .unwrap();
        mgr.remember(
            "This project uses SQLite + BM25 for retrieval",
            MemoryType::CodebaseFact,
        )
        .unwrap();
        mgr.remember(
            "Decision: tokio runtime for async I/O",
            MemoryType::Decision,
        )
        .unwrap();
        // Dynamic fact — should NOT show in static profile
        mgr.remember(
            "Today I debugged a flaky test",
            MemoryType::ActionOutcome,
        )
        .unwrap();

        let profile = mgr.profile(10).unwrap();
        assert!(profile.contains("User Profile (static)"));
        assert!(profile.contains("preferences"));
        assert!(profile.contains("Rust for systems work"));
        assert!(profile.contains("codebase_facts"));
        assert!(profile.contains("SQLite + BM25"));
        assert!(profile.contains("decisions"));
        assert!(profile.contains("tokio runtime"));
        // Dynamic fact must be filtered out
        assert!(!profile.contains("flaky test"));

        let _ = std::fs::remove_file(&db_path);
    }
}
