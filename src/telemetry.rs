//! Telemetry — opt-in usage analytics for HyperAgent
//!
//! Records anonymized usage data locally in SQLite:
//! - Run events (prompt length, files changed, tokens used, elapsed time)
//! - LLM call events (model, tokens, cost)
//! - Error events (type, count)
//! - System info (OS, version, architecture)
//!
//! All data is stored locally. No network requests are made.
//! Opt-in via config: `telemetry = true` in config.toml

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Telemetry event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TelemetryEvent {
    /// A run completed
    RunCompleted {
        prompt_len: usize,
        files_changed: usize,
        tokens_used: usize,
        elapsed_secs: f64,
        cost: f64,
        model: String,
        mode: String,
    },
    /// An LLM call was made
    LlmCall {
        model: String,
        input_tokens: usize,
        output_tokens: usize,
        duration_ms: u64,
        success: bool,
    },
    /// An error occurred
    Error {
        error_type: String,
        message: String,
    },
    /// REPL session started
    SessionStarted {
        version: String,
        os: String,
        arch: String,
    },
    /// A command was executed
    CommandExecuted {
        command: String,
        duration_ms: u64,
        exit_code: i32,
    },
}

/// Telemetry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub storage_path: String,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Opt-in by default
            storage_path: String::new(),
        }
    }
}

/// Telemetry recorder — stores events locally
pub struct Telemetry {
    config: TelemetryConfig,
    conn: Option<rusqlite::Connection>,
    event_count: usize,
}

impl Telemetry {
    /// Create a new telemetry recorder
    pub fn new(config: TelemetryConfig) -> Self {
        let conn = if config.enabled {
            let path = if config.storage_path.is_empty() {
                let home = dirs_next::home_dir().unwrap_or_default();
                home.join(".config").join("hyper").join("telemetry.db")
            } else {
                Path::new(&config.storage_path).to_path_buf()
            };

            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            match rusqlite::Connection::open(&path) {
                Ok(conn) => {
                    let _ = conn.execute_batch(
                        "CREATE TABLE IF NOT EXISTS events (
                            id INTEGER PRIMARY KEY AUTOINCREMENT,
                            timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                            event_type TEXT NOT NULL,
                            data TEXT NOT NULL
                        );
                        CREATE TABLE IF NOT EXISTS daily_stats (
                            date TEXT PRIMARY KEY,
                            runs INTEGER DEFAULT 0,
                            tokens_used INTEGER DEFAULT 0,
                            cost REAL DEFAULT 0.0,
                            errors INTEGER DEFAULT 0
                        );"
                    );
                    Some(conn)
                }
                Err(e) => {
                    eprintln!("   ⚠️ Telemetry init failed: {e}");
                    None
                }
            }
        } else {
            None
        };

        Self {
            config,
            conn,
            event_count: 0,
        }
    }

    /// Create an opt-out telemetry (disabled)
    pub fn disabled() -> Self {
        Self {
            config: TelemetryConfig::default(),
            conn: None,
            event_count: 0,
        }
    }

    /// Record a telemetry event
    pub fn record(&mut self, event: TelemetryEvent) {
        let conn = match self.conn {
            Some(ref mut c) => c,
            None => return,
        };

        let event_type = match &event {
            TelemetryEvent::RunCompleted { .. } => "run_completed",
            TelemetryEvent::LlmCall { .. } => "llm_call",
            TelemetryEvent::Error { .. } => "error",
            TelemetryEvent::SessionStarted { .. } => "session_started",
            TelemetryEvent::CommandExecuted { .. } => "command_executed",
        };

        let data = match serde_json::to_string(&event) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("   ⚠️ Telemetry serialization error: {e}");
                return;
            }
        };

        if let Err(e) = conn.execute(
            "INSERT INTO events (event_type, data) VALUES (?1, ?2)",
            rusqlite::params![event_type, data],
        ) {
            eprintln!("   ⚠️ Telemetry write error: {e}");
            return;
        }

        self.event_count += 1;

        // Update daily stats
        if let TelemetryEvent::RunCompleted { tokens_used, cost, .. } = &event {
            let _ = conn.execute(
                "INSERT INTO daily_stats (date, runs, tokens_used, cost)
                 VALUES (date('now'), 1, ?1, ?2)
                 ON CONFLICT(date) DO UPDATE SET
                    runs = runs + 1,
                    tokens_used = tokens_used + ?1,
                    cost = cost + ?2",
                rusqlite::params![tokens_used, cost],
            );
        }

        if let TelemetryEvent::Error { .. } = &event {
            let _ = conn.execute(
                "INSERT INTO daily_stats (date, runs, tokens_used, cost, errors)
                 VALUES (date('now'), 0, 0, 0, 1)
                 ON CONFLICT(date) DO UPDATE SET errors = errors + 1",
                [],
            );
        }
    }

    /// Get daily stats for display
    pub fn daily_stats(&self) -> String {
        let conn = match self.conn {
            Some(ref c) => c,
            None => return "Telemetry disabled".to_string(),
        };

        let mut output = String::from("   📊 Telemetry — Daily Stats\n");

        let mut stmt = match conn.prepare(
            "SELECT date, runs, tokens_used, cost, errors FROM daily_stats ORDER BY date DESC LIMIT 7"
        ) {
            Ok(s) => s,
            Err(_) => return format!("   📊 Telemetry — {} events recorded", self.event_count),
        };

        let rows = stmt.query_map([], |row| {
            let date: String = row.get(0)?;
            let runs: usize = row.get(1)?;
            let tokens: usize = row.get(2)?;
            let cost: f64 = row.get(3)?;
            let errors: usize = row.get(4)?;
            Ok((date, runs, tokens, cost, errors))
        });

        if let Ok(rows) = rows {
            for row in rows.flatten() {
                output.push_str(&format!(
                    "   {} — {} runs, {} tokens, ${:.4}, {} errors\n",
                    row.0, row.1, row.2, row.3, row.4
                ));
            }
        }

        output.push_str(&format!("   Total events recorded: {}", self.event_count));
        output
    }

    /// Get total events count
    pub fn event_count(&self) -> usize {
        self.event_count
    }

    /// Check if telemetry is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.conn.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled() {
        let mut t = Telemetry::disabled();
        assert!(!t.is_enabled());
        // Should not crash
        t.record(TelemetryEvent::RunCompleted {
            prompt_len: 10,
            files_changed: 1,
            tokens_used: 100,
            elapsed_secs: 5.0,
            cost: 0.001,
            model: "test".into(),
            mode: "code".into(),
        });
        assert_eq!(t.event_count(), 0);
    }

    #[test]
    fn test_enabled() {
        let dir = std::env::temp_dir();
        let path = dir.join("hyper-telemetry-test.db");
        let _ = std::fs::remove_file(&path);

        let config = TelemetryConfig {
            enabled: true,
            storage_path: path.to_string_lossy().to_string(),
        };

        let mut t = Telemetry::new(config);
        assert!(t.is_enabled());

        t.record(TelemetryEvent::RunCompleted {
            prompt_len: 20,
            files_changed: 3,
            tokens_used: 5000,
            elapsed_secs: 30.0,
            cost: 0.005,
            model: "gpt-4o".into(),
            mode: "code".into(),
        });
        assert_eq!(t.event_count(), 1);

        t.record(TelemetryEvent::Error {
            error_type: "ApiError".into(),
            message: "Rate limited".into(),
        });
        assert_eq!(t.event_count(), 2);

        let stats = t.daily_stats();
        assert!(stats.contains("runs"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_events() {
        let dir = std::env::temp_dir();
        let path = dir.join("hyper-telemetry-events-test.db");
        let _ = std::fs::remove_file(&path);

        let config = TelemetryConfig {
            enabled: true,
            storage_path: path.to_string_lossy().to_string(),
        };

        let mut t = Telemetry::new(config);

        t.record(TelemetryEvent::LlmCall {
            model: "deepseek".into(),
            input_tokens: 1000,
            output_tokens: 500,
            duration_ms: 2000,
            success: true,
        });

        t.record(TelemetryEvent::SessionStarted {
            version: "0.1.0".into(),
            os: "macos".into(),
            arch: "arm64".into(),
        });

        t.record(TelemetryEvent::CommandExecuted {
            command: "init".into(),
            duration_ms: 100,
            exit_code: 0,
        });

        assert_eq!(t.event_count(), 3);

        let _ = std::fs::remove_file(&path);
    }
}
