//! Agent Graph Store — parent/child agent spawning with lifecycle tracking
//!
//! Inspired by codex's agent-graph-store crate.
//!
//! Key design:
//! - **Parent/child topology** — each spawned sub-agent has exactly one parent
//! - **Lifecycle states** — Open (active/resumable) / Closed (done/merged)
//! - **SQLite-backed** — persisted across sessions
//! - **Worktree isolation** — each agent gets its own git worktree (cline style)
//! - **Descendant walking** — list all descendants for summary/cleanup

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique identifier for an agent node
pub type AgentId = String;

/// Lifecycle status of a spawned agent edge
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeStatus {
    /// Agent is still alive, resumable
    Open,
    /// Agent has completed, merged back, or was killed
    Closed,
}

/// An agent node in the graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    pub id: AgentId,
    pub name: String,
    pub mode: String,
    pub prompt: String,
    pub worktree: Option<PathBuf>,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub status: EdgeStatus,
    pub summary: Option<String>,
    pub files_changed: Vec<String>,
    pub token_usage: u64,
}

/// Edge connecting parent → child
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SpawnEdge {
    pub parent_id: AgentId,
    pub child_id: AgentId,
    pub status: EdgeStatus,
    pub created_at: DateTime<Utc>,
}

#[allow(dead_code)]
/// Trait for storage backend
pub trait AgentGraph: Send + Sync {
    /// Add or update a parent/child edge
    fn upsert_edge(&self, parent_id: &AgentId, child_id: &AgentId, status: EdgeStatus) -> anyhow::Result<()>;

    /// Add an agent node
    fn add_node(&self, node: &AgentNode) -> anyhow::Result<()>;

    /// Update node status
    fn update_node_status(&self, id: &AgentId, status: EdgeStatus) -> anyhow::Result<()>;

    /// Close a node (set status to Closed, record close time and summary)
    fn close_node(&self, id: &AgentId, summary: &str, files_changed: &[String], token_usage: u64) -> anyhow::Result<()>;

    /// Get direct children of a parent
    fn get_children(&self, parent_id: &AgentId, status: Option<EdgeStatus>) -> anyhow::Result<Vec<AgentNode>>;

    /// Get parent of a child
    fn get_parent(&self, child_id: &AgentId) -> anyhow::Result<Option<AgentNode>>;

    /// Walk all descendants (recursive)
    fn get_descendants(&self, root_id: &AgentId, include_closed: bool) -> anyhow::Result<Vec<AgentNode>>;

    /// Get entire tree as a flat list
    fn get_all_nodes(&self) -> anyhow::Result<Vec<AgentNode>>;

    /// Get count of open/active nodes
    fn open_count(&self) -> anyhow::Result<usize>;

    /// Delete a node and all its edges
    fn delete_node(&self, id: &AgentId) -> anyhow::Result<()>;

    /// Delete entire graph
    fn clear(&self) -> anyhow::Result<()>;
}

/// SQLite-backed agent graph store
pub struct SqliteAgentGraph {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl SqliteAgentGraph {
    pub fn new(db_path: &Path) -> anyhow::Result<Self> {
        let conn = rusqlite::Connection::open(db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_nodes (
                id            TEXT PRIMARY KEY,
                name          TEXT NOT NULL,
                mode          TEXT NOT NULL DEFAULT 'code',
                prompt        TEXT NOT NULL,
                worktree      TEXT,
                created_at    TEXT NOT NULL,
                closed_at     TEXT,
                status        TEXT NOT NULL DEFAULT 'open',
                summary       TEXT,
                files_changed TEXT NOT NULL DEFAULT '[]',
                token_usage   INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS agent_edges (
                parent_id     TEXT NOT NULL,
                child_id      TEXT NOT NULL,
                status        TEXT NOT NULL DEFAULT 'open',
                created_at    TEXT NOT NULL,
                PRIMARY KEY (parent_id, child_id),
                FOREIGN KEY (parent_id) REFERENCES agent_nodes(id),
                FOREIGN KEY (child_id) REFERENCES agent_nodes(id)
            );
            CREATE INDEX IF NOT EXISTS idx_edges_parent ON agent_edges(parent_id);
            CREATE INDEX IF NOT EXISTS idx_edges_child ON agent_edges(child_id);
            CREATE INDEX IF NOT EXISTS idx_nodes_status ON agent_nodes(status);",
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn row_to_node(&self, row: &rusqlite::Row) -> rusqlite::Result<AgentNode> {
        let created: String = row.get(5)?;
        let closed: Option<String> = row.get(6)?;
        let status_str: String = row.get(7)?;
        let files_str: String = row.get(9)?;

        Ok(AgentNode {
            id: row.get(0)?,
            name: row.get(1)?,
            mode: row.get(2)?,
            prompt: row.get(3)?,
            worktree: row.get::<_, Option<String>>(4)?.map(PathBuf::from),
            created_at: DateTime::parse_from_rfc3339(&created)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            closed_at: closed.and_then(|c| {
                DateTime::parse_from_rfc3339(&c)
                    .map(|d| d.with_timezone(&Utc))
                    .ok()
            }),
            status: if status_str == "open" { EdgeStatus::Open } else { EdgeStatus::Closed },
            summary: row.get(8)?,
            files_changed: serde_json::from_str(&files_str).unwrap_or_default(),
            token_usage: row.get(10)?,
        })
    }
}

impl AgentGraph for SqliteAgentGraph {
    fn upsert_edge(&self, parent_id: &AgentId, child_id: &AgentId, status: EdgeStatus) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let status_str = match status {
            EdgeStatus::Open => "open",
            EdgeStatus::Closed => "closed",
        };
        conn.execute(
            "INSERT OR REPLACE INTO agent_edges (parent_id, child_id, status, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![parent_id, child_id, status_str, now],
        )?;
        Ok(())
    }

    fn add_node(&self, node: &AgentNode) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let status_str = match node.status {
            EdgeStatus::Open => "open",
            EdgeStatus::Closed => "closed",
        };
        let files_json = serde_json::to_string(&node.files_changed)?;
        let worktree_str = node.worktree.as_ref().map(|p| p.to_string_lossy().to_string());

        conn.execute(
            "INSERT INTO agent_nodes (id, name, mode, prompt, worktree, created_at, closed_at, status, summary, files_changed, token_usage)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                node.id, node.name, node.mode, node.prompt,
                worktree_str, node.created_at.to_rfc3339(),
                node.closed_at.map(|c| c.to_rfc3339()),
                status_str, node.summary, files_json, node.token_usage,
            ],
        )?;
        Ok(())
    }

    fn update_node_status(&self, id: &AgentId, status: EdgeStatus) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let status_str = match status {
            EdgeStatus::Open => "open",
            EdgeStatus::Closed => "closed",
        };
        conn.execute(
            "UPDATE agent_nodes SET status = ?1 WHERE id = ?2",
            rusqlite::params![status_str, id],
        )?;
        Ok(())
    }

    fn close_node(&self, id: &AgentId, summary: &str, files_changed: &[String], token_usage: u64) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let files_json = serde_json::to_string(files_changed)?;
        conn.execute(
            "UPDATE agent_nodes SET status = 'closed', closed_at = ?1, summary = ?2, files_changed = ?3, token_usage = ?4 WHERE id = ?5",
            rusqlite::params![now, summary, files_json, token_usage as i64, id],
        )?;
        // Also close any open incoming edges
        conn.execute(
            "UPDATE agent_edges SET status = 'closed' WHERE child_id = ?1 AND status = 'open'",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    fn get_children(&self, parent_id: &AgentId, status: Option<EdgeStatus>) -> anyhow::Result<Vec<AgentNode>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT n.* FROM agent_nodes n
             INNER JOIN agent_edges e ON e.child_id = n.id
             WHERE e.parent_id = ?1"
        );
        if let Some(s) = status {
            let s_str = match s { EdgeStatus::Open => "open", EdgeStatus::Closed => "closed" };
            sql.push_str(&format!(" AND e.status = '{s_str}' AND n.status = '{s_str}'"));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![parent_id], |row| self.row_to_node(row))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn get_parent(&self, child_id: &AgentId) -> anyhow::Result<Option<AgentNode>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT n.* FROM agent_nodes n
             INNER JOIN agent_edges e ON e.parent_id = n.id
             WHERE e.child_id = ?1
             LIMIT 1"
        )?;
        let mut rows = stmt.query_map(rusqlite::params![child_id], |row| self.row_to_node(row))?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    fn get_descendants(&self, root_id: &AgentId, include_closed: bool) -> anyhow::Result<Vec<AgentNode>> {
        let conn = self.conn.lock().unwrap();
        // BFS walk
        let mut descendants = Vec::new();
        let mut visited = vec![root_id.clone()];
        let mut queue = vec![root_id.clone()];

        while let Some(current) = queue.pop() {
            let status_filter = if include_closed {
                ""
            } else {
                " AND e.status = 'open' AND n.status = 'open'"
            };
            let sql = format!(
                "SELECT n.* FROM agent_nodes n
                 INNER JOIN agent_edges e ON e.child_id = n.id
                 WHERE e.parent_id = ?1{status_filter}"
            );
            let mut stmt = conn.prepare(&sql)?;
            let children: Vec<AgentNode> = stmt.query_map(rusqlite::params![current], |row| self.row_to_node(row))?
                .filter_map(|r| r.ok())
                .collect();

            for child in children {
                if !visited.contains(&child.id) {
                    descendants.push(child.clone());
                    visited.push(child.id.clone());
                    queue.push(child.id.clone());
                }
            }
        }

        Ok(descendants)
    }

    fn get_all_nodes(&self) -> anyhow::Result<Vec<AgentNode>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM agent_nodes ORDER BY created_at ASC"
        )?;
        let rows = stmt.query_map([], |row| self.row_to_node(row))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn open_count(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_nodes WHERE status = 'open'",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    fn delete_node(&self, id: &AgentId) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM agent_edges WHERE parent_id = ?1 OR child_id = ?1", rusqlite::params![id])?;
        conn.execute("DELETE FROM agent_nodes WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    fn clear(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM agent_edges", [])?;
        conn.execute("DELETE FROM agent_nodes", [])?;
        Ok(())
    }
}

// ═══════════════════════════════════════════════
// Worktree Manager — isolate sub-agents via git worktrees
// ═══════════════════════════════════════════════

pub struct WorktreeManager {
    #[allow(dead_code)]
    project_root: PathBuf,
    #[allow(dead_code)]
    agent_graph: Arc<dyn AgentGraph>,
}

impl WorktreeManager {
    pub fn new(project_root: &Path, agent_graph: Arc<dyn AgentGraph>) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            agent_graph,
        }
    }

    #[allow(dead_code)]
    /// Create an isolated worktree for a sub-agent
    pub fn create_worktree(&self, node: &AgentNode) -> anyhow::Result<PathBuf> {
        let worktree_dir = self.project_root.join(".hyper").join("worktrees").join(&node.id);
        if worktree_dir.exists() {
            std::fs::remove_dir_all(&worktree_dir)?;
        }
        std::fs::create_dir_all(worktree_dir.parent().unwrap())?;

        // Check if git is available and worktree on main repo
        let git_dir = self.project_root.join(".git");
        if git_dir.exists() {
            let _branch_name = format!("hyper-{}-{}", node.name, &node.id[..8]);
            let output = Command::new("git")
                .args(["worktree", "add", &worktree_dir.to_string_lossy(), "--detach"])
                .current_dir(&self.project_root)
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // If worktree creation fails (non-git dir, etc.), fall back to copy-once
                eprintln!("  ⚠️  Worktree creation failed (falling back to copy): {stderr}");
                copy_dir(&self.project_root, &worktree_dir)?;
            }
        } else {
            // No git — copy project files to worktree
            copy_dir(&self.project_root, &worktree_dir)?;
        }

        Ok(worktree_dir)
    }

    #[allow(dead_code)]
    /// Clean up a worktree after agent completes
    pub fn remove_worktree(&self, id: &AgentId) -> anyhow::Result<()> {
        let worktree_dir = self.project_root.join(".hyper").join("worktrees").join(id);
        if worktree_dir.exists() {
            // Try git worktree remove first
            let git_result = Command::new("git")
                .args(["worktree", "remove", &worktree_dir.to_string_lossy()])
                .current_dir(&self.project_root)
                .output();

            match git_result {
                Ok(o) if o.status.success() => {},
                _ => {
                    // Fall back to recursive delete
                    let _ = std::fs::remove_dir_all(&worktree_dir);
                }
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    /// Merge changes from a worktree back to main
    pub fn merge_worktree(&self, node: &AgentNode) -> anyhow::Result<Vec<String>> {
        let worktree = match &node.worktree {
            Some(w) => w.clone(),
            None => return Ok(Vec::new()),
        };

        let mut changed_files = Vec::new();

        // Walk the worktree and copy changed files back
        for entry in walkdir::WalkDir::new(&worktree)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let relative = entry.path().strip_prefix(&worktree)
                .unwrap_or(entry.path());

            // Skip .hyper and .git
            let path_str = relative.to_string_lossy();
            if path_str.starts_with(".hyper/") || path_str.starts_with(".git/") {
                continue;
            }

            let source = entry.path();
            let target = self.project_root.join(relative);

            // Compare contents
            let changed = if target.exists() {
                let src_content = std::fs::read(source).unwrap_or_default();
                let tgt_content = std::fs::read(&target).unwrap_or_default();
                src_content != tgt_content
            } else {
                true
            };

            if changed {
                if let Some(parent) = target.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::copy(source, &target) {
                    Ok(_) => changed_files.push(relative.to_string_lossy().to_string()),
                    Err(e) => eprintln!("  ⚠️  Failed to merge {}: {e}", relative.display()),
                }
            }
        }

        Ok(changed_files)
    }
}

#[allow(dead_code)]
fn copy_dir(src: &Path, dst: &Path) -> anyhow::Result<()> {
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            !path.starts_with(src.join(".git")) &&
            !path.starts_with(src.join(".hyper")) &&
            !path.starts_with(src.join("target")) &&
            !path.starts_with(src.join("node_modules"))
        })
    {
        let relative = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(relative);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
