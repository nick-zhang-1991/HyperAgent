//! Cloud Sync — Keep memories, skills, and config in sync across machines.
//!
//! For 100M users, multi-machine sync is essential. Users expect their
//! settings and learned preferences to follow them everywhere.
//!
//! Architecture:
//!   Local ──push──→ Cloud Server ──pull──→ Another Machine
//!   Local ←──pull── Cloud Server ←──push── Another Machine
//!
//! Transport: HTTPS REST API (configurable endpoint)
//! Auth: API key from HYPER_SYNC_KEY env var
//! Sync targets: memories, skills, config, rules
//!
//! Commands:
//!   hyper sync push    — Upload local data to cloud
//!   hyper sync pull    — Download cloud data to local
//!   hyper sync status  — Show sync status (last push/pull times)

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_SYNC_ENDPOINT: &str = "https://sync.hyperagent.dev/api/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub endpoint: String,
    pub api_key: Option<String>,
    pub last_push: Option<u64>,
    pub last_pull: Option<u64>,
    pub auto_sync: bool,
    pub sync_memories: bool,
    pub sync_skills: bool,
    pub sync_config: bool,
    pub sync_rules: bool,
}

impl SyncConfig {
    pub fn load() -> Result<Self> {
        let path = sync_config_path();
        if path.exists() {
            let json = std::fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&json)?)
        } else {
            Ok(SyncConfig {
                endpoint: DEFAULT_SYNC_ENDPOINT.to_string(),
                api_key: std::env::var("HYPER_SYNC_KEY").ok(),
                last_push: None,
                last_pull: None,
                auto_sync: false,
                sync_memories: true,
                sync_skills: true,
                sync_config: true,
                sync_rules: true,
            })
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = sync_config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }
}

/// Push local data to cloud
pub async fn push(config: &SyncConfig) -> Result<SyncReport> {
    if !config.is_configured() {
        bail!("Sync not configured. Set HYPER_SYNC_KEY and endpoint in ~/.hyper/sync.json");
    }

    let client = reqwest::Client::new();
    let mut report = SyncReport::default();

    // Push memories
    if config.sync_memories {
        if let Ok(memories) = collect_memories() {
            let resp = client
                .post(format!("{}/memories", config.endpoint))
                .header("Authorization", format!("Bearer {}", config.api_key.as_ref().unwrap()))
                .json(&serde_json::json!({"memories": memories}))
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => report.memories_pushed = memories.len() as u32,
                _ => report.errors.push("Failed to push memories".into()),
            }
        }
    }

    // Push skills
    if config.sync_skills {
        if let Ok(skills) = collect_skills() {
            let resp = client
                .post(format!("{}/skills", config.endpoint))
                .header("Authorization", format!("Bearer {}", config.api_key.as_ref().unwrap()))
                .json(&serde_json::json!({"skills": skills}))
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => report.skills_pushed = skills.len() as u32,
                _ => report.errors.push("Failed to push skills".into()),
            }
        }
    }

    // Push config
    if config.sync_config {
        if let Ok(config_data) = collect_config() {
            let resp = client
                .post(format!("{}/config", config.endpoint))
                .header("Authorization", format!("Bearer {}", config.api_key.as_ref().unwrap()))
                .json(&config_data)
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => report.config_pushed = true,
                _ => report.errors.push("Failed to push config".into()),
            }
        }
    }

    Ok(report)
}

/// Pull cloud data to local
pub async fn pull(config: &SyncConfig) -> Result<SyncReport> {
    if !config.is_configured() {
        bail!("Sync not configured.");
    }

    let client = reqwest::Client::new();
    let mut report = SyncReport::default();

    // Pull memories
    if config.sync_memories {
        let resp = client
            .get(format!("{}/memories", config.endpoint))
            .header("Authorization", format!("Bearer {}", config.api_key.as_ref().unwrap()))
            .send()
            .await;
        if let Ok(r) = resp {
            if r.status().is_success() {
                if let Ok(data) = r.json::<serde_json::Value>().await {
                    if let Some(memories) = data.get("memories").and_then(|v| v.as_array()) {
                        if let Err(e) = apply_memories(memories) {
                            report.errors.push(format!("Failed to apply memories: {}", e));
                        } else {
                            report.memories_pulled = memories.len() as u32;
                        }
                    }
                }
            }
        }
    }

    // Pull skills
    if config.sync_skills {
        let resp = client
            .get(format!("{}/skills", config.endpoint))
            .header("Authorization", format!("Bearer {}", config.api_key.as_ref().unwrap()))
            .send()
            .await;
        if let Ok(r) = resp {
            if r.status().is_success() {
                if let Ok(data) = r.json::<serde_json::Value>().await {
                    if let Some(skills) = data.get("skills").and_then(|v| v.as_array()) {
                        if let Err(e) = apply_skills(skills) {
                            report.errors.push(format!("Failed to apply skills: {}", e));
                        } else {
                            report.skills_pulled = skills.len() as u32;
                        }
                    }
                }
            }
        }
    }

    Ok(report)
}

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub memories_pushed: u32,
    pub memories_pulled: u32,
    pub skills_pushed: u32,
    pub skills_pulled: u32,
    pub config_pushed: bool,
    pub errors: Vec<String>,
}

impl SyncReport {
    pub fn print(&self) {
        println!();
        println!("  \x1b[1;36m☁️  Sync Report\x1b[0m");
        println!("  {}", "─".repeat(40));
        if self.memories_pushed > 0 {
            println!("  Memories pushed:  \x1b[32m{}\x1b[0m", self.memories_pushed);
        }
        if self.memories_pulled > 0 {
            println!("  Memories pulled:  \x1b[32m{}\x1b[0m", self.memories_pulled);
        }
        if self.skills_pushed > 0 {
            println!("  Skills pushed:    \x1b[32m{}\x1b[0m", self.skills_pushed);
        }
        if self.skills_pulled > 0 {
            println!("  Skills pulled:    \x1b[32m{}\x1b[0m", self.skills_pulled);
        }
        if self.config_pushed {
            println!("  Config pushed:    \x1b[32m✅\x1b[0m");
        }
        for err in &self.errors {
            println!("  \x1b[31m⚠ {}\x1b[0m", err);
        }
        if self.memories_pushed + self.memories_pulled + self.skills_pushed + self.skills_pulled == 0 && self.errors.is_empty() {
            println!("  \x1b[90mNo changes to sync\x1b[0m");
        }
        println!();
    }
}

// ─── Data Collectors ────────────────────────────────────────────

fn sync_config_path() -> PathBuf {
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("hyper");
    data_dir.join("sync.json")
}

fn collect_memories() -> Result<Vec<serde_json::Value>> {
    // Read from SQLite memory store
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("hyper");
    let db_path = data_dir.join("memory.db");

    if !db_path.exists() {
        return Ok(vec![]);
    }

    let conn = rusqlite::Connection::open(&db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, content, memory_type, created_at, importance, access_count FROM memories ORDER BY created_at DESC LIMIT 500",
    )?;

    let memories: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "content": row.get::<_, String>(1)?,
                "type": row.get::<_, String>(2)?,
                "created_at": row.get::<_, String>(3)?,
                "importance": row.get::<_, f64>(4)?,
                "access_count": row.get::<_, i64>(5)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(memories)
}

fn collect_skills() -> Result<Vec<serde_json::Value>> {
    let skills_dir = dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".hyper")
        .join("skills");

    if !skills_dir.exists() {
        return Ok(vec![]);
    }

    let mut skills = Vec::new();
    for entry in std::fs::read_dir(&skills_dir)? {
        let entry = entry?;
        let skill_md = entry.path().join("SKILL.md");
        if skill_md.exists() {
            if let Ok(content) = std::fs::read_to_string(&skill_md) {
                skills.push(serde_json::json!({
                    "name": entry.file_name().to_string_lossy(),
                    "content": content,
                }));
            }
        }
    }

    Ok(skills)
}

fn collect_config() -> Result<serde_json::Value> {
    let config_dir = dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("hyper");
    let config_file = config_dir.join("config.toml");

    if config_file.exists() {
        let content = std::fs::read_to_string(&config_file)?;
        Ok(serde_json::json!({"config": content}))
    } else {
        Ok(serde_json::json!({"config": null}))
    }
}

fn apply_memories(memories: &[serde_json::Value]) -> Result<()> {
    // Merge into local SQLite store
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("hyper");
    std::fs::create_dir_all(&data_dir).ok();
    let db_path = data_dir.join("memory.db");

    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memories (
            id TEXT PRIMARY KEY, content TEXT, memory_type TEXT,
            created_at TEXT, importance REAL, access_count INTEGER
        )",
    )?;

    for mem in memories {
        let id = mem["id"].as_str().unwrap_or("");
        let content = mem["content"].as_str().unwrap_or("");
        let mtype = mem["type"].as_str().unwrap_or("general");
        let created = mem["created_at"].as_str().unwrap_or("");
        let importance = mem["importance"].as_f64().unwrap_or(0.5);
        let access = mem["access_count"].as_i64().unwrap_or(0);

        conn.execute(
            "INSERT OR REPLACE INTO memories (id, content, memory_type, created_at, importance, access_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, content, mtype, created, importance, access],
        )?;
    }

    Ok(())
}

fn apply_skills(skills: &[serde_json::Value]) -> Result<()> {
    let skills_dir = dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".hyper")
        .join("skills");

    std::fs::create_dir_all(&skills_dir)?;

    for skill in skills {
        let name = skill["name"].as_str().unwrap_or("unknown");
        let content = skill["content"].as_str().unwrap_or("");
        let skill_dir = skills_dir.join(name);
        std::fs::create_dir_all(&skill_dir)?;
        std::fs::write(skill_dir.join("SKILL.md"), content)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig {
            endpoint: "https://test.example.com".into(),
            api_key: None,
            last_push: None,
            last_pull: None,
            auto_sync: false,
            sync_memories: true,
            sync_skills: true,
            sync_config: true,
            sync_rules: true,
        };
        assert!(!config.is_configured());
    }

    #[test]
    fn test_sync_config_with_key() {
        let config = SyncConfig {
            endpoint: "https://test.example.com".into(),
            api_key: Some("key123".into()),
            last_push: None,
            last_pull: None,
            auto_sync: false,
            sync_memories: true,
            sync_skills: true,
            sync_config: true,
            sync_rules: true,
        };
        assert!(config.is_configured());
    }

    #[test]
    fn test_sync_report_print() {
        let report = SyncReport {
            memories_pushed: 10,
            memories_pulled: 5,
            skills_pushed: 3,
            skills_pulled: 2,
            config_pushed: true,
            errors: vec![],
        };
        report.print(); // Just ensure it doesn't panic
    }
}
