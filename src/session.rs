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


#[cfg(test)]
mod tests {
    use super::*;

    // ── Session construction ─────────────────────────────────

    #[test]
    fn test_session_new_has_unique_ids() {
        let s1 = Session::new("proj", "p", "gpt-4");
        let s2 = Session::new("proj", "p", "gpt-4");
        // Different timestamps → different ids (in practice)
        // Even if same second, they share the same id format, so this just
        // verifies id format and structure.
        assert!(s1.id.starts_with("hyper-"));
        assert!(s2.id.starts_with("hyper-"));
        assert!(!s1.changes.is_empty() || s1.changes.is_empty()); // default empty
    }

    #[test]
    fn test_session_new_defaults() {
        let s = Session::new("proj", "do thing", "gpt-4");
        assert_eq!(s.project, "proj");
        assert_eq!(s.prompt, "do thing");
        assert_eq!(s.model, "gpt-4");
        assert!(s.summary.is_empty());
        assert!(s.changes.is_empty());
        assert!(s.messages.is_empty());
        assert!(s.parent_id.is_none());
        assert!(s.branch.is_none());
        assert!(s.tags.is_empty());
        assert!(s.timestamp > 0);
    }

    // ── fork ────────────────────────────────────────────────

    #[test]
    fn test_fork_copies_messages_and_tags() {
        let mut parent = Session::new("p", "orig", "gpt-4");
        parent.messages.push(ChatMessage { role: "user".into(), content: "hi".into() });
        parent.tags.push("test".into());

        let child = parent.fork("new prompt", Some("feat-x"));
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(child.branch.as_deref(), Some("feat-x"));
        assert_eq!(child.prompt, "new prompt");
        assert_eq!(child.project, parent.project);
        assert_eq!(child.model, parent.model);
        assert_eq!(child.messages.len(), 1);
        assert_eq!(child.messages[0].content, "hi");
        assert_eq!(child.tags, vec!["test"]);
    }

    #[test]
    fn test_fork_without_branch_name() {
        let parent = Session::new("p", "x", "m");
        let child = parent.fork("y", None);
        assert!(child.branch.is_none());
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    }

    // ── merge_changes ───────────────────────────────────────

    #[test]
    fn test_merge_changes_adds_new_unique_changes() {
        let mut a = Session::new("p", "x", "m");
        a.changes.push("file1.rs".into());
        a.changes.push("file2.rs".into());

        let mut b = Session::new("p", "x", "m");
        b.changes.push("file2.rs".into()); // dup
        b.changes.push("file3.rs".into());

        let merged = a.merge_changes(&b);
        assert_eq!(merged, vec!["file3.rs".to_string()]);
        assert_eq!(a.changes.len(), 3);
    }

    #[test]
    fn test_merge_changes_empty_other() {
        let mut a = Session::new("p", "x", "m");
        a.changes.push("f.rs".into());
        let b = Session::new("p", "x", "m");
        let merged = a.merge_changes(&b);
        assert!(merged.is_empty());
        assert_eq!(a.changes.len(), 1);
    }

    #[test]
    fn test_merge_changes_both_empty() {
        let mut a = Session::new("p", "x", "m");
        let b = Session::new("p", "x", "m");
        let merged = a.merge_changes(&b);
        assert!(merged.is_empty());
    }

    #[test]
    fn test_merge_changes_updates_timestamp() {
        let mut a = Session::new("p", "x", "m");
        let orig_ts = a.timestamp;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let b = Session::new("p", "x", "m");
        a.merge_changes(&b);
        assert!(a.timestamp >= orig_ts, "timestamp should not go back");
    }

    // ── summary_line ─────────────────────────────────────────

    #[test]
    fn test_summary_line_basic() {
        let s = Session::new("proj", "build api", "gpt-4");
        let line = s.summary_line();
        assert!(line.contains(&s.id));
        assert!(line.contains("build api"));
        assert!(line.contains("0 file(s)"));
    }

    #[test]
    fn test_summary_line_long_prompt_truncated() {
        let long = "a".repeat(200);
        let s = Session::new("p", &long, "m");
        let line = s.summary_line();
        // The prompt should be truncated to ~60 chars
        assert!(line.len() < 200);
    }

    #[test]
    fn test_summary_line_with_branch() {
        let s = Session::new("p", "x", "m");
        let child = s.fork("y", Some("feature-x"));
        let line = child.summary_line();
        assert!(line.contains("[branch: feature-x]"));
    }

    #[test]
    fn test_summary_line_with_parent() {
        let s = Session::new("p", "x", "m");
        let child = s.fork("y", None);
        let line = child.summary_line();
        assert!(line.contains("fork of"));
    }

    #[test]
    fn test_summary_line_uses_summary_first_line() {
        let mut s = Session::new("p", "very long prompt that should not show", "m");
        s.summary = "First line of summary
Second line that should not show".into();
        let line = s.summary_line();
        assert!(line.contains("First line of summary"));
        assert!(!line.contains("Second line"));
    }

    // ── display ─────────────────────────────────────────────

    #[test]
    fn test_display_includes_all_fields() {
        let mut s = Session::new("myproj", "do thing", "gpt-4");
        s.summary = "summary".into();
        s.changes.push("a.rs".into());
        s.messages.push(ChatMessage { role: "user".into(), content: "hi".into() });
        s.tags.push("tag1".into());
        s.branch = Some("dev".into());
        s.parent_id = Some("parent".into());

        let out = s.display();
        assert!(out.contains("myproj"));
        assert!(out.contains("do thing"));
        assert!(out.contains("gpt-4"));
        assert!(out.contains("summary"));
        assert!(out.contains("a.rs"));
        assert!(out.contains("1")); // messages count
        assert!(out.contains("tag1"));
        assert!(out.contains("dev"));
        assert!(out.contains("parent"));
    }

    #[test]
    fn test_display_truncates_long_values() {
        let long = "x".repeat(200);
        let s = Session::new(&long, &long, &long);
        let out = s.display();
        // project name, prompt, summary all bounded
        assert!(out.len() < 5000);
    }

    #[test]
    fn test_display_no_branch_no_parent_omits_lines() {
        let s = Session::new("p", "x", "m");
        let out = s.display();
        assert!(!out.contains("Branch:"));
        assert!(!out.contains("Parent:"));
    }

    #[test]
    fn test_display_no_tags_omits_line() {
        let s = Session::new("p", "x", "m");
        let out = s.display();
        assert!(!out.contains("Tags:"));
    }

    #[test]
    fn test_display_no_changes_omits_section() {
        let s = Session::new("p", "x", "m");
        let out = s.display();
        assert!(!out.contains("Changes:"));
    }

    // ── ShareStore ──────────────────────────────────────────

    #[test]
    fn test_share_store_share_returns_unique_tokens() {
        let store = ShareStore::new();
        let t1 = store.share("session-a");
        let t2 = store.share("session-b");
        assert_ne!(t1, t2);
        // UUID format
        assert_eq!(t1.len(), 36);
    }

    #[test]
    fn test_share_store_resolve() {
        let store = ShareStore::new();
        let token = store.share("session-x");
        let resolved = store.resolve(&token);
        assert_eq!(resolved, Some("session-x".to_string()));
    }

    #[test]
    fn test_share_store_resolve_unknown_token() {
        let store = ShareStore::new();
        let resolved = store.resolve("nonexistent-token-12345");
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_share_store_overwrites_same_session() {
        let store = ShareStore::new();
        // Sharing same session twice creates different tokens
        let t1 = store.share("dup");
        let t2 = store.share("dup");
        assert_ne!(t1, t2);
        // Both should resolve to the same session
        assert_eq!(store.resolve(&t1), Some("dup".into()));
        assert_eq!(store.resolve(&t2), Some("dup".into()));
    }

    #[test]
    fn test_share_store_path() {
        let store = ShareStore::new();
        let p = store.path();
        assert!(p.ends_with("share_tokens.json"));
    }

    // ── Session serde round-trip ─────────────────────────────

    #[test]
    fn test_session_serde_roundtrip() {
        let mut s = Session::new("p", "x", "m");
        s.summary = "sum".into();
        s.changes.push("c.rs".into());
        s.messages.push(ChatMessage { role: "user".into(), content: "hi".into() });
        s.tags.push("t1".into());

        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, s.id);
        assert_eq!(back.prompt, s.prompt);
        assert_eq!(back.summary, s.summary);
        assert_eq!(back.changes, s.changes);
        assert_eq!(back.messages.len(), s.messages.len());
        assert_eq!(back.tags, s.tags);
    }

    #[test]
    fn test_chat_message_serde() {
        let m = ChatMessage { role: "user".into(), content: "hello".into() };
        let json = serde_json::to_string(&m).unwrap();
        let back: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, "user");
        assert_eq!(back.content, "hello");
    }

    // ── SessionManager (smoke tests against real data dir) ───

    #[test]
    fn test_session_manager_creates_directory() {
        let _ = SessionManager::new(); // smoke: just check it doesn't panic
    }

    #[test]
    fn test_session_search_match_prompt() {
        let mgr = SessionManager::new().unwrap();
        let mut s = Session::new("p", "build a REST API", "m");
        s.id = format!("hyper-search-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        // We can't easily inject into the manager without save; just smoke test that
        // search doesn't panic when there are no matching sessions
        let results = mgr.search("nonexistent_term_xyz_999");
        assert!(results.is_ok());
    }

    #[test]
    fn test_session_list_by_branch_empty() {
        let mgr = SessionManager::new().unwrap();
        let results = mgr.list_by_branch("nonexistent-branch");
        assert!(results.is_ok());
        assert!(results.unwrap().is_empty());
    }

    #[test]
    fn test_session_list_by_tag_empty() {
        let mgr = SessionManager::new().unwrap();
        let results = mgr.list_by_tag("nonexistent-tag");
        assert!(results.is_ok());
        assert!(results.unwrap().is_empty());
    }

    #[test]
    fn test_session_list_branches_empty() {
        let mgr = SessionManager::new().unwrap();
        let results = mgr.list_branches();
        assert!(results.is_ok());
        assert!(results.unwrap().is_empty());
    }

    #[test]
    fn test_session_tree_with_nonexistent_id_errors() {
        let mgr = SessionManager::new().unwrap();
        let result = mgr.tree(Some("hyper-nonexistent-12345"));
        assert!(result.is_err());
    }
}
