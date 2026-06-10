#![allow(unused)]
//! Usage Analytics & Insights Dashboard
//!
//! For 100M users, understanding usage patterns is critical:
//! - What commands do users run most?
//! - What's the retention curve?
//! - Where do users drop off?
//! - How much are they spending?
//!
//! This module provides:
//! 1. Local analytics storage (SQLite, opt-in, privacy-first)
//! 2. `hyper analytics` command — serves a web dashboard
//! 3. `hyper analytics --json` — machine-readable output for external BI tools
//!
//! Design: privacy-first, zero network calls, fully local by default.
//! Opt-in cloud sync available via `hyper sync` (future).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

// ─── Analytics Store (SQLite) ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    pub timestamp: i64,
    pub event_type: String,
    pub command: Option<String>,
    pub mode: Option<String>,
    pub project_dir: Option<String>,
    pub tokens_input: Option<u64>,
    pub tokens_output: Option<u64>,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub success: Option<bool>,
    pub files_modified: Option<u32>,
    pub error_message: Option<String>,
    pub locale: Option<String>,
    pub version: Option<String>,
    pub os: Option<String>,
}

pub struct AnalyticsStore {
    db_path: PathBuf,
    conn: Option<rusqlite::Connection>,
}

impl AnalyticsStore {
    /// Open or create the analytics SQLite database
    pub fn open() -> Result<Self> {
        let data_dir = dirs_next::data_dir()
            .unwrap_or_else(|| Path::new("~/.local/share").to_path_buf())
            .join("hyper");
        std::fs::create_dir_all(&data_dir).ok();

        let db_path = data_dir.join("analytics.db");
        let conn = rusqlite::Connection::open(&db_path)
            .context("Failed to open analytics database")?;

        // Create tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                command TEXT,
                mode TEXT,
                project_dir TEXT,
                tokens_input INTEGER,
                tokens_output INTEGER,
                cost_usd REAL,
                duration_ms INTEGER,
                success INTEGER,
                files_modified INTEGER,
                error_message TEXT,
                locale TEXT,
                version TEXT,
                os TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
            CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
            CREATE INDEX IF NOT EXISTS idx_events_command ON events(command);
            ",
        )?;

        Ok(AnalyticsStore {
            db_path,
            conn: Some(conn),
        })
    }

    /// Record an analytics event
    pub fn record(&self, event: &AnalyticsEvent) -> Result<()> {
        let conn = self
            .conn
            .as_ref()
            .context("Analytics store not initialized")?;

        conn.execute(
            "INSERT INTO events (timestamp, event_type, command, mode, project_dir,
             tokens_input, tokens_output, cost_usd, duration_ms, success,
             files_modified, error_message, locale, version, os)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                event.timestamp,
                event.event_type,
                event.command,
                event.mode,
                event.project_dir,
                event.tokens_input,
                event.tokens_output,
                event.cost_usd,
                event.duration_ms,
                event.success.map(|b| b as i32),
                event.files_modified,
                event.error_message,
                event.locale,
                event.version,
                event.os,
            ],
        )?;

        Ok(())
    }

    /// Get summary statistics
    pub fn summary(&self) -> Result<AnalyticsSummary> {
        let conn = self
            .conn
            .as_ref()
            .context("Analytics store not initialized")?;

        let total_runs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'run'",
            [],
            |row| row.get(0),
        )?;

        let total_tokens_input: i64 = conn.query_row(
            "SELECT COALESCE(SUM(tokens_input), 0) FROM events",
            [],
            |row| row.get(0),
        )?;

        let total_tokens_output: i64 = conn.query_row(
            "SELECT COALESCE(SUM(tokens_output), 0) FROM events",
            [],
            |row| row.get(0),
        )?;

        let total_cost: f64 = conn.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0) FROM events",
            [],
            |row| row.get(0),
        )?;

        let success_rate: f64 = {
            let total_with_success: i64 = conn.query_row(
                "SELECT COUNT(*) FROM events WHERE success IS NOT NULL",
                [],
                |row| row.get(0),
            )?;
            if total_with_success > 0 {
                let successful: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM events WHERE success = 1",
                    [],
                    |row| row.get(0),
                )?;
                successful as f64 / total_with_success as f64
            } else {
                0.0
            }
        };

        // Daily runs for last 14 days
        let mut stmt = conn.prepare(
            "SELECT date(timestamp, 'unixepoch') as day, COUNT(*) as cnt
             FROM events
             WHERE timestamp > unixepoch() - 14*86400
             GROUP BY day ORDER BY day",
        )?;
        let daily_runs: Vec<(String, i64)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Top commands
        let mut stmt = conn.prepare(
            "SELECT COALESCE(command, 'unknown'), COUNT(*) as cnt
             FROM events
             GROUP BY command ORDER BY cnt DESC LIMIT 10",
        )?;
        let top_commands: Vec<(String, i64)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Top modes
        let mut stmt = conn.prepare(
            "SELECT COALESCE(mode, 'unknown'), COUNT(*) as cnt
             FROM events
             GROUP BY mode ORDER BY cnt DESC LIMIT 5",
        )?;
        let top_modes: Vec<(String, i64)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(AnalyticsSummary {
            total_runs: total_runs as u64,
            total_tokens_input: total_tokens_input as u64,
            total_tokens_output: total_tokens_output as u64,
            total_cost_usd: total_cost,
            success_rate,
            daily_runs,
            top_commands,
            top_modes,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsSummary {
    pub total_runs: u64,
    pub total_tokens_input: u64,
    pub total_tokens_output: u64,
    pub total_cost_usd: f64,
    pub success_rate: f64,
    pub daily_runs: Vec<(String, i64)>,
    pub top_commands: Vec<(String, i64)>,
    pub top_modes: Vec<(String, i64)>,
}

// ─── Analytics Dashboard (Web UI) ──────────────────────────────

/// Start the analytics web dashboard
pub async fn serve_dashboard(port: u16) -> Result<()> {
    let store = Arc::new(Mutex::new(AnalyticsStore::open()?));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("📊 Analytics dashboard: http://localhost:{}", port);

    loop {
        let (stream, _) = listener.accept().await?;
        let store = store.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_request(stream, store).await {
                eprintln!("Analytics request error: {}", e);
            }
        });
    }
}

async fn handle_request(
    mut stream: tokio::net::TcpStream,
    store: Arc<Mutex<AnalyticsStore>>,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    let path = if parts.len() >= 2 { parts[1] } else { "/" };

    // Read headers (skip for now)
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
    }

    if path == "/api/summary" {
        let store = store.lock().await;
        let summary = store.summary().unwrap_or_else(|_| AnalyticsSummary {
            total_runs: 0,
            total_tokens_input: 0,
            total_tokens_output: 0,
            total_cost_usd: 0.0,
            success_rate: 0.0,
            daily_runs: vec![],
            top_commands: vec![],
            top_modes: vec![],
        });
        let json = serde_json::to_string_pretty(&summary).unwrap_or_default();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            json.len(),
            json
        );
        writer.write_all(response.as_bytes()).await?;
    } else {
        let html = analytics_html();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            html.len(),
            html
        );
        writer.write_all(response.as_bytes()).await?;
    }

    Ok(())
}

fn analytics_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>HyperAgent Analytics</title>
<style>
* { margin:0; padding:0; box-sizing:border-box; }
body { font-family: system-ui, -apple-system, sans-serif; background:#0d1117; color:#c9d1d9; padding:2rem; }
h1 { font-size: 1.5rem; margin-bottom: 1rem; color: #58a6ff; }
.card { background:#161b22; border:1px solid #30363d; border-radius:8px; padding:1.5rem; margin-bottom:1rem; }
.grid { display:grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap:1rem; margin-bottom:1rem; }
.stat { text-align:center; }
.stat-value { font-size:2rem; font-weight:bold; color:#58a6ff; }
.stat-label { font-size:0.85rem; color:#8b949e; margin-top:0.25rem; }
table { width:100%; border-collapse:collapse; margin-top:0.5rem; }
th, td { padding:0.5rem 0.75rem; text-align:left; border-bottom:1px solid #21262d; }
th { color:#8b949e; font-weight:500; font-size:0.85rem; }
tr:hover { background:#1c2128; }
.bar { display:inline-block; height:12px; background:#238636; border-radius:3px; vertical-align:middle; margin-left:0.5rem; }
#loading { text-align:center; padding:3rem; color:#8b949e; }
#error { color:#f85149; display:none; }
</style>
</head>
<body>
<h1>📊 HyperAgent Analytics</h1>
<div id="loading">Loading...</div>
<div id="error">Failed to load analytics data.</div>
<div id="content" style="display:none">
    <div class="grid" id="stats"></div>
    <div class="card">
        <h3 style="margin-bottom:0.5rem">📈 Daily Runs (Last 14 Days)</h3>
        <div id="daily-chart" style="display:flex;align-items:flex-end;height:120px;gap:4px;padding-top:1rem;"></div>
    </div>
    <div class="card">
        <h3 style="margin-bottom:0.5rem">🔝 Top Commands</h3>
        <table id="top-commands"></table>
    </div>
    <div class="card">
        <h3 style="margin-bottom:0.5rem">🎯 Top Modes</h3>
        <table id="top-modes"></table>
    </div>
</div>
<script>
fetch('/api/summary')
.then(r => r.json())
.then(data => {
    document.getElementById('loading').style.display = 'none';
    document.getElementById('content').style.display = 'block';
    // Stats
    document.getElementById('stats').innerHTML = [
        {label:'Total Runs', value:data.total_runs.toLocaleString()},
        {label:'Tokens In', value:(data.total_tokens_input/1e6).toFixed(1)+'M'},
        {label:'Tokens Out', value:(data.total_tokens_output/1e6).toFixed(1)+'M'},
        {label:'Total Cost', value:'$'+data.total_cost_usd.toFixed(2)},
        {label:'Success Rate', value:(data.success_rate*100).toFixed(1)+'%'},
    ].map(s => `<div class="card stat"><div class="stat-value">${s.value}</div><div class="stat-label">${s.label}</div></div>`).join('');
    // Daily chart
    var maxVal = Math.max(...data.daily_runs.map(d=>d[1]), 1);
    document.getElementById('daily-chart').innerHTML = data.daily_runs.map(d => {
        var h = (d[1]/maxVal*100).toFixed(0);
        return `<div style="flex:1;display:flex;flex-direction:column;align-items:center;gap:4px">
            <span style="font-size:0.7rem;color:#8b949e">${d[1]}</span>
            <div style="width:100%;height:${h}%;background:#238636;border-radius:3px 3px 0 0;min-height:2px"></div>
            <span style="font-size:0.65rem;color:#8b949e">${d[0].slice(5)}</span>
        </div>`;
    }).join('');
    // Tables
    document.getElementById('top-commands').innerHTML = '<tr><th>Command</th><th>Count</th></tr>' + 
        data.top_commands.map(c => `<tr><td>${c[0]}</td><td>${c[1]}</td></tr>`).join('');
    document.getElementById('top-modes').innerHTML = '<tr><th>Mode</th><th>Count</th></tr>' + 
        data.top_modes.map(m => `<tr><td>${m[0]}</td><td>${m[1]}</td></tr>`).join('');
})
.catch(e => {
    document.getElementById('loading').style.display = 'none';
    document.getElementById('error').style.display = 'block';
    document.getElementById('error').textContent = 'Error: ' + e.message;
});
</script>
</body>
</html>"#.to_string()
}

/// Print analytics summary to terminal
pub fn print_terminal_summary() -> Result<()> {
    let store = AnalyticsStore::open()?;
    let summary = store.summary()?;

    println!();
    println!("  \x1b[1;36m📊 HyperAgent Analytics\x1b[0m");
    println!("  {}", "─".repeat(50));
    println!();
    println!("  Total runs:        \x1b[1m{}\x1b[0m", summary.total_runs);
    println!("  Total tokens in:   \x1b[1m{:.1}M\x1b[0m", summary.total_tokens_input as f64 / 1_000_000.0);
    println!("  Total tokens out:  \x1b[1m{:.1}M\x1b[0m", summary.total_tokens_output as f64 / 1_000_000.0);
    println!("  Total cost:        \x1b[1m${:.4}\x1b[0m", summary.total_cost_usd);
    println!("  Success rate:      \x1b[1m{:.1}%\x1b[0m", summary.success_rate * 100.0);
    println!();
    println!("  \x1b[1mDaily runs (last 14 days):\x1b[0m");
    for (day, count) in &summary.daily_runs {
        let bar = "█".repeat((*count as usize).min(50));
        println!("    {}  {:>4}  {}", day, count, bar);
    }
    println!();
    println!("  \x1b[1mTop commands:\x1b[0m");
    for (cmd, count) in &summary.top_commands {
        println!("    {:20}  {:>6}", cmd, count);
    }
    println!();

    Ok(())
}

/// Generate a JSON report for external BI tools
pub fn print_json_report() -> Result<()> {
    let store = AnalyticsStore::open()?;
    let summary = store.summary()?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analytics_event_construction() {
        let event = AnalyticsEvent {
            timestamp: 1234567890,
            event_type: "run".into(),
            command: Some("test_cmd".into()),
            mode: Some("code".into()),
            project_dir: Some("/tmp/proj".into()),
            tokens_input: Some(100),
            tokens_output: Some(200),
            cost_usd: Some(0.01),
            duration_ms: Some(1000),
            success: Some(true),
            files_modified: Some(3),
            error_message: None,
            locale: Some("en".into()),
            version: Some("0.1.0".into()),
            os: Some("macos".into()),
        };
        assert_eq!(event.timestamp, 1234567890);
        assert_eq!(event.event_type, "run");
        assert_eq!(event.command, Some("test_cmd".into()));
        assert_eq!(event.tokens_input, Some(100));
        assert_eq!(event.success, Some(true));
    }

    #[test]
    fn test_analytics_event_minimal() {
        let event = AnalyticsEvent {
            timestamp: 0,
            event_type: "test".into(),
            command: None,
            mode: None,
            project_dir: None,
            tokens_input: None,
            tokens_output: None,
            cost_usd: None,
            duration_ms: None,
            success: None,
            files_modified: None,
            error_message: None,
            locale: None,
            version: None,
            os: None,
        };
        assert_eq!(event.timestamp, 0);
        assert!(event.command.is_none());
    }

    #[test]
    fn test_analytics_summary_construction() {
        let s = AnalyticsSummary {
            total_runs: 10,
            total_tokens_input: 1000,
            total_tokens_output: 2000,
            total_cost_usd: 0.5,
            success_rate: 0.9,
            daily_runs: vec![("2024-01-01".into(), 5), ("2024-01-02".into(), 5)],
            top_commands: vec![("cmd1".into(), 3)],
            top_modes: vec![("code".into(), 10)],
        };
        assert_eq!(s.total_runs, 10);
        assert_eq!(s.total_tokens_input, 1000);
        assert_eq!(s.total_cost_usd, 0.5);
        assert!((s.success_rate - 0.9).abs() < 0.001);
        assert_eq!(s.daily_runs.len(), 2);
        assert_eq!(s.top_commands.len(), 1);
    }

    #[test]
    fn test_analytics_summary_empty() {
        let s = AnalyticsSummary {
            total_runs: 0,
            total_tokens_input: 0,
            total_tokens_output: 0,
            total_cost_usd: 0.0,
            success_rate: 0.0,
            daily_runs: vec![],
            top_commands: vec![],
            top_modes: vec![],
        };
        assert_eq!(s.total_runs, 0);
    }

    #[test]
    fn test_analytics_event_serialize() {
        let event = AnalyticsEvent {
            timestamp: 100,
            event_type: "run".into(),
            command: None,
            mode: None,
            project_dir: None,
            tokens_input: None,
            tokens_output: None,
            cost_usd: None,
            duration_ms: None,
            success: None,
            files_modified: None,
            error_message: None,
            locale: None,
            version: None,
            os: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"timestamp\":100"));
        assert!(json.contains("\"event_type\":\"run\""));
    }

    #[test]
    fn test_analytics_summary_serialize() {
        let s = AnalyticsSummary {
            total_runs: 5,
            total_tokens_input: 100,
            total_tokens_output: 200,
            total_cost_usd: 0.05,
            success_rate: 1.0,
            daily_runs: vec![],
            top_commands: vec![],
            top_modes: vec![],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"total_runs\":5"));
        assert!(json.contains("\"success_rate\":1.0"));
    }

    #[test]
    fn test_analytics_html_returns_html() {
        let html = analytics_html();
        assert!(html.contains("<!DOCTYPE"));
        assert!(html.contains("<html"));
        assert!(html.contains("</html>"));
        assert!(html.contains("Analytics"));
    }

    #[test]
    fn test_analytics_html_includes_chart() {
        let html = analytics_html();
        // Should include chart script and elements
        assert!(html.contains("chart") || html.contains("Chart"));
    }
}
