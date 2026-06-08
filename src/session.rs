#![allow(unused)]
/// Session management for HyperAgent
/// 
/// Sessions store the conversation history and state so you can
/// resume or fork previous work sessions.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A saved session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project: String,
    pub prompt: String,
    pub model: String,
    pub timestamp: u64,
    pub summary: String,
    pub changes: Vec<String>,
    pub messages: Vec<ChatMessage>,
    /// Parent session ID (for branches)
    pub parent_id: Option<String>,
    /// Branch name if this is a forked session
    pub branch: Option<String>,
    /// Tags for filtering/labeling
    pub tags: Vec<String>,
}

/// A stored chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String, // "user" | "assistant" | "system"
    pub content: String,
}

impl Session {
    pub fn new(project: &str, prompt: &str, model: &str) -> Self {
        let id = format!("hyper-{}", 
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );
        Self {
            id,
            project: project.to_string(),
            prompt: prompt.to_string(),
            model: model.to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            summary: String::new(),
            changes: Vec::new(),
            messages: Vec::new(),
            parent_id: None,
            branch: None,
            tags: Vec::new(),
        }
    }

    /// Fork this session — creates a child session with the same history
    pub fn fork(&self, new_prompt: &str, branch_name: Option<&str>) -> Self {
        let mut child = Self::new(&self.project, new_prompt, &self.model);
        child.parent_id = Some(self.id.clone());
        child.branch = branch_name.map(|s| s.to_string());
        child.messages = self.messages.clone();
        child.tags = self.tags.clone();
        child
    }

    /// Merge changes from another session into this one
    pub fn merge_changes(&mut self, other: &Session) -> Vec<String> {
        let mut merged = Vec::new();
        for change in &other.changes {
            if !self.changes.contains(change) {
                self.changes.push(change.clone());
                merged.push(change.clone());
            }
        }
        self.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        merged
    }

    /// Display session as a short summary line
    pub fn summary_line(&self) -> String {
        let branch_str = self.branch.as_ref()
            .map(|b| format!(" [branch: {b}]"))
            .unwrap_or_default();
        let parent_str = self.parent_id.as_ref()
            .map(|p| format!(" (fork of {})", &p[..p.len().min(16)]))
            .unwrap_or_default();
        format!(
            "{} | {}{}{} | {} file(s) | {}",
            self.id,
            self.summary.lines().next().unwrap_or(&self.prompt).chars().take(60).collect::<String>(),
            branch_str,
            parent_str,
            self.changes.len(),
            chrono::DateTime::from_timestamp(self.timestamp as i64, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "unknown time".to_string())
        )
    }

    /// Display session as a detailed view
    pub fn display(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("📋 Session: {}\n", self.id));
        output.push_str(&format!("  Prompt:    {}\n", self.prompt.chars().take(80).collect::<String>()));
        output.push_str(&format!("  Project:   {}\n", self.project));
        output.push_str(&format!("  Model:     {}\n", self.model));
        output.push_str(&format!("  Summary:   {}\n", self.summary.chars().take(100).collect::<String>()));
        output.push_str(&format!("  Time:      {}\n", chrono::DateTime::from_timestamp(self.timestamp as i64, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "unknown".to_string())));
        output.push_str(&format!("  Files:     {}\n", self.changes.len()));
        output.push_str(&format!("  Messages:  {}\n", self.messages.len()));
        if !self.tags.is_empty() {
            output.push_str(&format!("  Tags:      {}\n", self.tags.join(", ")));
        }
        if let Some(ref b) = self.branch {
            output.push_str(&format!("  Branch:    {}\n", b));
        }
        if let Some(ref p) = self.parent_id {
            output.push_str(&format!("  Parent:    {}\n", p));
        }
        if !self.changes.is_empty() {
            output.push_str(&format!("  Changes:\n"));
            for c in &self.changes {
                output.push_str(&format!("    • {}\n", c));
            }
        }
        output
    }
}

/// Session manager
pub struct SessionManager {
    sessions_dir: PathBuf,
}

/// Shared session token store
use std::collections::HashMap;

/// Manages share tokens for cross-user session sharing
pub struct ShareStore {
    tokens: std::sync::Mutex<HashMap<String, String>>,  // token → session_id
    store_path: std::path::PathBuf,
}

impl ShareStore {
    pub fn new() -> Self {
        let store_path = dirs_next::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("hyperagent")
            .join("share_tokens.json");
        let tokens = std::fs::read_to_string(&store_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            tokens: std::sync::Mutex::new(tokens),
            store_path,
        }
    }

    /// Generate a share token for a session
    pub fn share(&self, session_id: &str) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        let mut map = self.tokens.lock().unwrap();
        map.insert(token.clone(), session_id.to_string());
        let _ = std::fs::write(&self.store_path, serde_json::to_string_pretty(&*map).unwrap());
        token
    }

    /// Resolve a share token to a session ID
    pub fn resolve(&self, token: &str) -> Option<String> {
        let map = self.tokens.lock().unwrap();
        map.get(token).cloned()
    }

    /// Share store path for API access
    pub fn path(&self) -> &std::path::Path {
        &self.store_path
    }
}

impl SessionManager {
    pub fn new() -> Result<Self> {
        let base = dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hyperagent")
            .join("sessions");
        std::fs::create_dir_all(&base)?;
        Ok(Self { sessions_dir: base })
    }

    /// Save a session
    pub fn save(&self, session: &Session) -> Result<()> {
        let path = self.sessions_dir.join(format!("{}.json", session.id));
        let json = serde_json::to_string_pretty(session)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// List all sessions
    pub fn list(&self) -> Result<Vec<Session>> {
        let mut sessions = Vec::new();
        if !self.sessions_dir.exists() {
            return Ok(sessions);
        }
        for entry in std::fs::read_dir(&self.sessions_dir)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == "json") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(session) = serde_json::from_str::<Session>(&content) {
                        sessions.push(session);
                    }
                }
            }
        }
        sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(sessions)
    }

    /// List sessions by branch
    pub fn list_by_branch(&self, branch: &str) -> Result<Vec<Session>> {
        let all = self.list()?;
        Ok(all.into_iter()
            .filter(|s| s.branch.as_deref() == Some(branch))
            .collect())
    }

    /// List all branches
    pub fn list_branches(&self) -> Result<Vec<String>> {
        let sessions = self.list()?;
        let mut branches: Vec<String> = sessions.into_iter()
            .filter_map(|s| s.branch)
            .collect();
        branches.sort();
        branches.dedup();
        Ok(branches)
    }

    /// Load a specific session
    pub fn load(&self, id: &str) -> Result<Session> {
        let path = self.sessions_dir.join(format!("{id}.json"));
        let content = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Get the last session
    pub fn last(&self) -> Result<Session> {
        let sessions = self.list()?;
        sessions.into_iter().next()
            .ok_or_else(|| anyhow::anyhow!("No sessions found"))
    }

    /// Merge two sessions — returns the merged changes
    pub fn merge_sessions(&self, target_id: &str, source_id: &str) -> Result<Vec<String>> {
        let mut target = self.load(target_id)?;
        let source = self.load(source_id)?;
        let merged = target.merge_changes(&source);
        target.summary = format!(
            "Merged {} with {}: {} new file(s)",
            target_id, source_id, merged.len()
        );
        self.save(&target)?;
        Ok(merged)
    }

    /// Delete a session
    pub fn delete(&self, id: &str) -> Result<()> {
        let path = self.sessions_dir.join(format!("{id}.json"));
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        // Also delete children (branches)
        let all = self.list()?;
        for session in all {
            if session.parent_id.as_deref() == Some(id) {
                let child_path = self.sessions_dir.join(format!("{}.json", session.id));
                let _ = std::fs::remove_file(child_path);
            }
        }
        Ok(())
    }
}

impl SessionManager {
    /// Search sessions by query text (search prompt, summary, messages, tags)
    pub fn search(&self, query: &str) -> Result<Vec<Session>> {
        let all = self.list()?;
        let q = query.to_lowercase();
        Ok(all.into_iter()
            .filter(|s| {
                s.prompt.to_lowercase().contains(&q)
                    || s.summary.to_lowercase().contains(&q)
                    || s.id.to_lowercase().contains(&q)
                    || s.tags.iter().any(|t| t.to_lowercase().contains(&q))
                    || s.messages.iter().any(|m| m.content.to_lowercase().contains(&q))
            })
            .collect())
    }

    /// List sessions by tag
    pub fn list_by_tag(&self, tag: &str) -> Result<Vec<Session>> {
        let all = self.list()?;
        Ok(all.into_iter()
            .filter(|s| s.tags.iter().any(|t| t == tag))
            .collect())
    }

    /// Get session tree (parent + children) as an indented string
    pub fn tree(&self, root_id: Option<&str>) -> Result<String> {
        let sessions = self.list()?;
        let root = match root_id {
            Some(id) => {
                sessions.iter().find(|s| s.id == *id)
                    .ok_or_else(|| anyhow::anyhow!("Session not found: {id}"))?
                    .clone()
            }
            None => {
                sessions.first().cloned()
                    .ok_or_else(|| anyhow::anyhow!("No sessions"))?
            }
        };

        let mut output = String::new();
        output.push_str("📋 Session Tree\n\n");
        output.push_str(&format!("  {} ─ {}\n", root.id, root.summary_line()));

        let children: Vec<&Session> = sessions.iter()
            .filter(|s| s.parent_id.as_deref() == Some(&root.id))
            .collect();
        for child in &children {
            output.push_str(&format!("  ├── {} ─ {}\n", child.id, child.summary_line()));
            let grandchildren: Vec<&Session> = sessions.iter()
                .filter(|s| s.parent_id.as_deref() == Some(&child.id))
                .collect();
            for grand in &grandchildren {
                output.push_str(&format!("  │   └── {} ─ {}\n", grand.id, grand.summary_line()));
            }
        }

        Ok(output)
    }
}
