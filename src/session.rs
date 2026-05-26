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
        }
    }
}

/// Session manager
pub struct SessionManager {
    sessions_dir: PathBuf,
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

    /// Delete a session
    pub fn delete(&self, id: &str) -> Result<()> {
        let path = self.sessions_dir.join(format!("{id}.json"));
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}
