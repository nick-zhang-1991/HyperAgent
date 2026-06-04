//! Smart Memory System — inspired by mem0 + codex memories + embeddings
//!
//! Design:
//! - **Add-only extraction** — memories accumulate, never overwritten (mem0 style)
//! - **Entity linking** — entities extracted and linked across memories for boosted retrieval
//! - **Multi-signal retrieval** — semantic (via LLM/embedding) + keyword + entity matching
//! - **Temporal reasoning** — time-aware ranking; current state > recent > old
//! - **Vector embeddings** — optional Ollama-based semantic search for better recall
//! - **SQLite-backed persistence** — using rusqlite
//! - **Two-stage pipeline** (codex inspired):
//!   Stage 1: Extract facts from agent conversation
//!   Stage 2: Consolidate into global memory (periodic merge)

use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Embedding provider for semantic search
use crate::embed::EmbeddingProvider;

/// Default embedding dimension (used when no provider available)
const DEFAULT_EMBED_DIM: usize = 768;

/// A single memory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub content: String,
    pub memory_type: MemoryType,
    pub entities: Vec<String>,
    pub importance: f32, // 0.0 - 1.0
    pub embedding: Option<Vec<f32>>,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub consolidated: bool,
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
}

impl Default for MemoryQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            memory_type: None,
            entity: None,
            max_age: None,
            limit: 10,
        }
    }
}

#[allow(dead_code)]
/// Memory Store trait — abstract over storage backend
pub trait MemoryStore: Send + Sync {
    /// Insert a new memory (add-only, never overwrites)
    fn insert(&self, entry: MemoryEntry) -> anyhow::Result<()>;

    /// Retrieve memories matching the query
    fn query(&self, query: &MemoryQuery) -> anyhow::Result<Vec<MemoryEntry>>;

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

    /// Update an existing memory entry by ID — replaces content, re-extracts entities,
    /// re-computes embedding if an embedder is available on the manager side.
    fn update(&self, id: &str, content: &str, memory_type: &str, importance: f32, embedding_json: Option<String>) -> anyhow::Result<()>;
}

/// SQLite-backed memory store
#[derive(Clone)]
pub struct SqliteMemoryStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

/// Map a SQLite row to a MemoryEntry (shared helper for query + get_unconsolidated)
fn row_to_memory_entry(row: &rusqlite::Row) -> rusqlite::Result<MemoryEntry> {
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
    let entities_str: String = row.get(5)?;
    let entities: Vec<String> = serde_json::from_str(&entities_str).unwrap_or_default();

    let embedding: Option<Vec<f32>> = row
        .get::<_, Option<String>>(7)?
        .and_then(|s| serde_json::from_str(&s).ok());

    let created: String = row.get(8)?;
    let accessed: String = row.get(9)?;

    Ok(MemoryEntry {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        session_id: row.get(2)?,
        content: row.get(3)?,
        memory_type: mem_type,
        entities,
        importance: row.get(6)?,
        embedding,
        created_at: DateTime::parse_from_rfc3339(&created)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        last_accessed: DateTime::parse_from_rfc3339(&accessed)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        access_count: row.get(10)?,
        consolidated: row.get::<_, u32>(11)? != 0,
    })
}

impl SqliteMemoryStore {
    pub fn new(db_path: &Path) -> anyhow::Result<Self> {
        let conn = rusqlite::Connection::open(db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id               TEXT PRIMARY KEY,
                agent_id         TEXT NOT NULL,
                session_id       TEXT,
                content          TEXT NOT NULL,
                memory_type      TEXT NOT NULL,
                entities         TEXT NOT NULL DEFAULT '[]',
                importance       REAL NOT NULL DEFAULT 0.5,
                embedding        TEXT,
                created_at       TEXT NOT NULL,
                last_accessed    TEXT NOT NULL,
                access_count     INTEGER NOT NULL DEFAULT 0,
                consolidated     INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS memory_entities (
                entity       TEXT NOT NULL,
                memory_id    TEXT NOT NULL,
                PRIMARY KEY (entity, memory_id),
                FOREIGN KEY (memory_id) REFERENCES memories(id)
            );
            CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
            CREATE INDEX IF NOT EXISTS idx_memories_consolidated ON memories(consolidated);
            CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at);",
        )?;

        // Migration: add embedding column for existing databases
        let has_embedding = conn.prepare("SELECT embedding FROM memories LIMIT 1").is_ok();
        if !has_embedding {
            let _ = conn.execute_batch("ALTER TABLE memories ADD COLUMN embedding TEXT;");
        }

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Extract entities from content using simple heuristics
    /// (In production, this would use an LLM, but for now we extract
    /// capitalized phrases, API names, file paths, etc.)
    pub fn extract_entities(content: &str) -> Vec<String> {
        let mut entities = Vec::new();
        // Extract PascalCase/CamelCase identifiers and file paths
        let re = regex::Regex::new(
            r"(?:[A-Z][a-z]+[A-Z][a-zA-Z]*)|(?:[A-Z]{2,}(?:[a-z]+)?)|(?:[A-Z][a-z]+(?:\s[A-Z][a-z]+)+)|(?:[a-zA-Z0-9_/.-]+\.[a-z]{2,})",
        )
        .unwrap();
        for cap in re.find_iter(content) {
            let entity = cap.as_str().to_string();
            if !entities.contains(&entity) {
                entities.push(entity);
            }
        }
        entities
    }

    /// Calculate importance based on content signals
    pub fn calculate_importance(content: &str) -> f32 {
        let mut score: f32 = 0.5;
        // Signal words
        let high_impact = [
            "always",
            "never",
            "must",
            "critical",
            "bug",
            "fix",
            "important",
            "prefers",
            "projects",
            "config",
            "API",
            "architecture",
        ];
        let medium_impact = [
            "usually",
            "often",
            "recommend",
            "pattern",
            "convention",
            "style",
            "prefer",
        ];

        let lower = content.to_lowercase();
        for word in &high_impact {
            if lower.contains(word) {
                score += 0.1;
            }
        }
        for word in &medium_impact {
            if lower.contains(word) {
                score += 0.05;
            }
        }
        score.min(1.0_f32)
    }
}

impl MemoryStore for SqliteMemoryStore {
    fn insert(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let entities_json = serde_json::to_string(&entry.entities)?;
        let embedding_json = entry
            .embedding
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        conn.execute(
            "INSERT INTO memories (id, agent_id, session_id, content, memory_type, entities, importance, embedding, created_at, last_accessed, access_count, consolidated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                entry.id,
                entry.agent_id,
                entry.session_id,
                entry.content,
                entry.memory_type.to_string(),
                entities_json,
                entry.importance,
                embedding_json,
                entry.created_at.to_rfc3339(),
                entry.last_accessed.to_rfc3339(),
                entry.access_count,
                entry.consolidated as u32,
            ],
        )?;

        // Insert entities
        for entity in &entry.entities {
            conn.execute(
                "INSERT OR IGNORE INTO memory_entities (entity, memory_id) VALUES (?1, ?2)",
                params![entity, entry.id],
            )?;
        }

        Ok(())
    }

    fn query(&self, query: &MemoryQuery) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, agent_id, session_id, content, memory_type, entities, importance, embedding, created_at, last_accessed, access_count, consolidated
             FROM memories WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        // Type filter
        if let Some(ref mem_type) = query.memory_type {
            sql.push_str(&format!(" AND memory_type = ?{}", param_values.len() + 1));
            param_values.push(Box::new(mem_type.to_string()));
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

        // Keyword search in content
        let search_terms: Vec<&str> = query
            .text
            .split_whitespace()
            .filter(|w| {
                w.len() > 2 && !["the", "and", "for", "was", "are", "but", "not"].contains(w)
            })
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

        // Order by relevance (importance × access_count boost × consolidation)
        sql.push_str(" ORDER BY importance * (1.0 + access_count * 0.1) * CASE WHEN consolidated THEN 1.2 ELSE 1.0 END DESC");

        // Limit
        sql.push_str(&format!(" LIMIT ?{}", param_values.len() + 1));
        param_values.push(Box::new(query.limit as i64));

        let mut stmt = conn.prepare(&sql)?;

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let rows = stmt.query_map(params_refs.as_slice(), row_to_memory_entry)?;

        let results: Vec<MemoryEntry> = rows.filter_map(|r| r.ok()).collect();

        // Update last_accessed for retrieved memories
        if !results.is_empty() {
            let now = Utc::now().to_rfc3339();
            let ids: Vec<String> = results.iter().map(|m| m.id.clone()).collect();
            for id in &ids {
                let _ = conn.execute(
                    "UPDATE memories SET last_accessed = ?1, access_count = access_count + 1 WHERE id = ?2",
                    params![now, id],
                );
            }
        }

        Ok(results)
    }

    fn query_by_entity(&self, entity: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let q = MemoryQuery {
            entity: Some(entity.to_string()),
            limit,
            ..Default::default()
        };
        self.query(&q)
    }

    fn list_entities(&self) -> anyhow::Result<Vec<(String, usize)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT entity, COUNT(*) as cnt FROM memory_entities GROUP BY entity ORDER BY cnt DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
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
            "SELECT id, agent_id, session_id, content, memory_type, entities, importance, embedding, created_at, last_accessed, access_count, consolidated
             FROM memories WHERE consolidated = 0 ORDER BY created_at ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_memory_entry)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn delete(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM memory_entities WHERE memory_id = ?1",
            params![id],
        )?;
        conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn update(&self, id: &str, content: &str, memory_type: &str, importance: f32, embedding_json: Option<String>) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let mut entities = Self::extract_entities(content);
        // Deduplicate
        entities.sort();
        entities.dedup();
        let entities_json = serde_json::to_string(&entities)?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE memories SET content = ?1, memory_type = ?2, entities = ?3, importance = ?4, embedding = ?5, last_accessed = ?6, access_count = access_count + 1, consolidated = 0
             WHERE id = ?7",
            params![content, memory_type, entities_json, importance, embedding_json, now, id],
        )?;

        // Re-sync entities
        conn.execute("DELETE FROM memory_entities WHERE memory_id = ?1", params![id])?;
        for entity in &entities {
            conn.execute(
                "INSERT OR IGNORE INTO memory_entities (entity, memory_id) VALUES (?1, ?2)",
                params![entity, id],
            )?;
        }

        Ok(())
    }

    fn count(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

// ═══════════════════════════════════════════════
// Memory Manager — orchestrates memory operations
// ═══════════════════════════════════════════════

pub struct MemoryManager {
    store: Box<dyn MemoryStore>,
    agent_id: String,
    session_id: Option<String>,
    embedder: Option<Arc<dyn EmbeddingProvider>>,
}

impl MemoryManager {
    pub fn new(store: Box<dyn MemoryStore>, agent_id: &str) -> Self {
        Self {
            store,
            agent_id: agent_id.to_string(),
            session_id: None,
            embedder: None,
        }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    #[allow(dead_code)]
    pub fn with_session(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Record a memory from agent conversation.
    /// If an embedding provider is configured, computes and stores a vector embedding.
    pub fn remember(&self, content: &str, memory_type: MemoryType) -> anyhow::Result<String> {
        let entities = SqliteMemoryStore::extract_entities(content);
        let importance = SqliteMemoryStore::calculate_importance(content);

        // Compute embedding if provider available
        let embedding = self.embedder.as_ref().and_then(|e| {
            e.embed(&[content.to_string()])
                .ok()
                .and_then(|v| v.into_iter().next())
        });

        let entry = MemoryEntry {
            id: Uuid::new_v4().to_string(),
            agent_id: self.agent_id.clone(),
            session_id: self.session_id.clone(),
            content: content.to_string(),
            memory_type,
            entities,
            importance,
            embedding,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            consolidated: false,
        };

        let id = entry.id.clone();
        self.store.insert(entry)?;
        Ok(id)
    }

    /// Recall memories relevant to a query.
    /// If embeddings are available, combines semantic similarity with keyword + importance scores.
    pub fn recall(&self, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        // First, do a broader keyword-based fetch
        let q = MemoryQuery {
            text: query.to_string(),
            limit: limit * 3, // Fetch extra for re-ranking
            ..Default::default()
        };
        let mut memories = self.store.query(&q)?;

        if memories.is_empty() {
            return Ok(vec![]);
        }

        // If we have an embedder and some memories have embeddings, do semantic re-ranking
        if let Some(ref embedder) = self.embedder {
            if let Ok(query_vec) = embedder.embed(&[query.to_string()]) {
                if let Some(query_emb) = query_vec.into_iter().next() {
                    // Compute combined score: 60% semantic + 40% traditional
                    for mem in &mut memories {
                        let semantic_score = mem.embedding.as_ref().map_or(0.0, |emb| {
                            crate::embed::cosine_similarity(&query_emb, emb)
                        });
                        let traditional_score = mem.importance * (1.0 + (mem.access_count as f32) * 0.1);
                        // Boost consolidated memories
                        let consolidation_boost = if mem.consolidated { 1.2 } else { 1.0 };
                        // Combined: semantic weighted at 60%, traditional at 40%
                        // Store temporary score in importance field for sorting
                        let combined = semantic_score * 0.6 + traditional_score * 0.4 * consolidation_boost;
                        // We store the combined as a proxy — importance field is used for display
                        mem.importance = combined;
                    }
                }
            }
        }

        // Sort by importance (now = combined score if semantic, or traditional)
        memories.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        memories.truncate(limit);
        Ok(memories)
    }

    #[allow(dead_code)]
    /// Recall by memory type
    pub fn recall_by_type(
        &self,
        memory_type: MemoryType,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let q = MemoryQuery {
            memory_type: Some(memory_type),
            limit,
            ..Default::default()
        };
        self.store.query(&q)
    }

    #[allow(dead_code)]
    /// Recall by entity (project/library/tool name)
    pub fn recall_by_entity(&self, entity: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        self.store.query_by_entity(entity, limit)
    }

    /// Update a memory entry by ID — replaces content, re-extracts entities,
    /// re-computes embedding. Uses the underlying store.update().
    pub fn update_memory(&self, id: &str, content: &str, memory_type: MemoryType) -> anyhow::Result<()> {
        let entities = SqliteMemoryStore::extract_entities(content);
        let importance = SqliteMemoryStore::calculate_importance(content);
        let embedding_json = self.embedder.as_ref().and_then(|e| {
            e.embed(&[content.to_string()])
                .ok()
                .and_then(|v| v.into_iter().next())
                .and_then(|emb| serde_json::to_string(&emb).ok())
        });
        let type_str = memory_type.to_string();
        self.store.update(id, content, &type_str, importance, embedding_json)
    }

    /// Remember a memory, but first check if a similar memory already exists.
    /// If a memory with a similar content pattern (same type + high keyword overlap) is found,
    /// update it instead of creating a new entry. Returns (id, was_updated).
    pub fn remember_or_update(&self, content: &str, memory_type: MemoryType) -> anyhow::Result<(String, bool)> {
        // Search for existing memories of the same type with overlapping keywords
        let keywords: Vec<&str> = content
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .collect();

        if !keywords.is_empty() {
            let existing = self.recall(content, 5)?;
            for mem in &existing {
                if mem.memory_type != memory_type {
                    continue;
                }
                // Check keyword overlap
                let mem_lower = mem.content.to_lowercase();
                let overlap: usize = keywords.iter()
                    .filter(|k| mem_lower.contains(&k.to_lowercase()))
                    .count();
                // If more than 50% keyword overlap, update instead of insert
                if overlap as f64 / keywords.len() as f64 > 0.5 {
                    self.update_memory(&mem.id, content, memory_type)?;
                    return Ok((mem.id.clone(), true));
                }
            }
        }

        let id = self.remember(content, memory_type)?;
        Ok((id, false))
    }

    /// Replace all memories whose content contains `old_text` with `new_content`.
    /// Preserves the original memory type. Returns number of replaced entries.
    pub fn replace_content(&self, old_text: &str, new_content: &str) -> anyhow::Result<usize> {
        // Find memories matching the text pattern
        let q = MemoryQuery {
            text: old_text.to_string(),
            limit: 100,
            ..Default::default()
        };
        let matching = self.store.query(&q)?;
        let mut replaced = 0;

        for mem in &matching {
            if mem.content.contains(old_text) {
                // Replace old_text with new_content within the existing content
                let updated = mem.content.replace(old_text, new_content);
                if updated != mem.content {
                    self.update_memory(&mem.id, &updated, mem.memory_type.clone())?;
                    replaced += 1;
                }
            }
        }

        Ok(replaced)
    }

    /// Build a context string from relevant memories for LLM prompts
    pub fn build_context(&self, task: &str, max_memories: usize) -> anyhow::Result<String> {
        let memories = self.recall(task, max_memories)?;
        if memories.is_empty() {
            return Ok(String::new());
        }

        let mut context = String::from("\n\n--- Relevant Past Knowledge ---\n");
        for mem in memories {
            let ago = chrono::Utc::now().signed_duration_since(mem.created_at);
            let ago_str = if ago.num_minutes() < 60 {
                format!("{}m ago", ago.num_minutes())
            } else if ago.num_hours() < 24 {
                format!("{}h ago", ago.num_hours())
            } else {
                format!("{}d ago", ago.num_days())
            };
            context.push_str(&format!(
                "  [{ago_str}] ({mem_type}) {content}\n",
                mem_type = mem.memory_type,
                content = mem.content,
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

    /// Prune old/low-importance memories, keeping only the top N most recent+important
    pub fn prune(&self, max_entries: usize) -> anyhow::Result<usize> {
        let all = self.store.query(&MemoryQuery {
            limit: usize::MAX,
            ..Default::default()
        })?;

        if all.len() <= max_entries {
            return Ok(0);
        }

        // Sort by importance (desc) + recency, keep top max_entries
        let mut entries: Vec<&MemoryEntry> = all.iter().collect();
        entries.sort_by(|a, b| {
            let a_score = a.importance * (1.0 + (a.access_count as f32) * 0.1);
            let b_score = b.importance * (1.0 + (b.access_count as f32) * 0.1);
            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let to_keep: std::collections::HashSet<&str> = entries
            .iter()
            .take(max_entries)
            .map(|e| e.id.as_str())
            .collect();

        let mut pruned = 0;
        for entry in &all {
            if !to_keep.contains(entry.id.as_str()) && self.store.delete(&entry.id).is_ok() {
                pruned += 1;
            }
        }

        Ok(pruned)
    }

    pub fn store(&self) -> &dyn MemoryStore {
        &*self.store
    }

    /// Export memories to a JSON file for sharing or backup
    pub fn export_to_file(&self, path: &Path) -> anyhow::Result<()> {
        let all_entries = self.recall("", 9999)?;
        let json = serde_json::to_string_pretty(&all_entries)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Import memories from a JSON file into the current store
    pub fn import_from_file(&self, path: &Path, source_label: &str) -> anyhow::Result<usize> {
        let content = std::fs::read_to_string(path)?;
        let entries: Vec<MemoryEntry> = serde_json::from_str(&content)?;
        let mut imported = 0;
        for mut entry in entries {
            entry.content = format!("[shared from {source_label}] {}", entry.content);
            entry.id = Uuid::new_v4().to_string();
            entry.consolidated = false;
            if let Err(e) = self.store().insert(entry) {
                eprintln!("   ⚠️  Import error: {e}");
            } else {
                imported += 1;
            }
        }
        Ok(imported)
    }

    /// Share memory with another .hyper directory
    pub fn share_with(&self, target_dir: &Path) -> anyhow::Result<usize> {
        let target_path = target_dir.join(".hyper").join("memory.db");
        if !target_path.exists() {
            anyhow::bail!("Target project has no memory database");
        }
        let store = crate::memory::SqliteMemoryStore::new(&target_path)?;
        let entries = self.recall("", 100)?;
        let mut shared = 0;
        for mut entry in entries {
            entry.id = Uuid::new_v4().to_string();
            entry.consolidated = false;
            if store.insert(entry).is_ok() {
                shared += 1;
            }
        }
        Ok(shared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_extraction() {
        let text = "HyperAgent uses SQLite. The API is available at /api/v1. User prefers concise responses.";
        let entities = SqliteMemoryStore::extract_entities(text);
        assert!(entities.contains(&"HyperAgent".to_string()));
        assert!(entities.contains(&"SQLite".to_string()));
    }

    #[test]
    fn test_importance_scoring() {
        let high = "This is a critical bug fix that must be applied to all systems";
        let medium = "We usually prefer the builder pattern";
        let low = "The weather is nice today";

        assert!(
            SqliteMemoryStore::calculate_importance(high)
                > SqliteMemoryStore::calculate_importance(low)
        );
        assert!(
            SqliteMemoryStore::calculate_importance(medium)
                > SqliteMemoryStore::calculate_importance(low)
        );
    }
}
