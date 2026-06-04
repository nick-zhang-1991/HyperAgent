//! Remote Agent — TCP-based remote execution server
//!
//! Hosts HyperAgent sessions that can be accessed remotely via TCP.
//! Clients connect, send prompts, receive responses.
//!
//! Protocol:
//!   Client → Server: {"type": "run", "prompt": "Fix the bug", "session": "optional-session-id"}
//!   Server → Client: {"type": "running", "session": "session-id"}
//!   Server → Client: {"type": "progress", "data": "Phase 1/3: planning..."}
//!   Server → Client: {"type": "result", "data": {result object}}
//!   Server → Client: {"type": "error", "message": "error text"}
//!
//! Usage:
//!   hyper serve --port 8080     # Start server
//!   hyper session --connect localhost:8080  # Start a remote session

use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Server configuration
pub struct RemoteConfig {
    pub port: u16,
    pub bind: String,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            port: 9173,
            bind: "0.0.0.0".to_string(),
        }
    }
}

/// A remote agent session
struct RemoteSession {
    id: String,
    writer: Arc<Mutex<tokio::io::BufWriter<TcpStream>>>,
}

impl RemoteSession {
    fn new(id: String, stream: TcpStream) -> Self {
        let writer = Arc::new(Mutex::new(tokio::io::BufWriter::new(stream)));
        Self { id, writer }
    }

    async fn send_json(&self, msg: &serde_json::Value) -> Result<()> {
        let mut w = self.writer.lock().await;
        let line = serde_json::to_string(msg)?;
        w.write_all(line.as_bytes()).await?;
        w.write_all(b"\n").await?;
        w.flush().await?;
        Ok(())
    }

    async fn send_progress(&self, data: &str) -> Result<()> {
        self.send_json(&serde_json::json!({"type": "progress", "session": self.id, "data": data})).await
    }

    async fn send_result(&self, data: &serde_json::Value) -> Result<()> {
        self.send_json(&serde_json::json!({"type": "result", "session": self.id, "data": data})).await
    }
}

/// Start the remote agent server
pub async fn start_server(config: RemoteConfig) -> Result<()> {
    let addr = format!("{}:{}", config.bind, config.port);
    let listener = TcpListener::bind(&addr).await?;
    println!("   🌐 Remote agent server listening on {addr}");

    let mut session_counter = 0u64;

    loop {
        let (stream, peer) = listener.accept().await?;
        session_counter += 1;
        let session_id = format!("sess-{session_counter}");
        println!("   🔗 Connection from {peer} — session {session_id}");

        let session = RemoteSession::new(session_id.clone(), stream);
        tokio::spawn(handle_session(session));
    }
}

/// Handle a single client session
async fn handle_session(session: RemoteSession) {
    // Reconnect to the stream for reading (the RemoteSession has the writer)
    // For a real implementation, we'd share the TcpStream between read + write halves.
    // For the MVP, we just send progress updates.
    let _ = session.send_progress("Session started. Send a prompt to run.");
    // Note: Full implementation requires splitting the TcpStream into reader + writer.
    // The MVP provides the framework structure — real bidirectional comms
    // can be added once the server command is wired up.
    let _ = session.send_result(&serde_json::json!({
        "message": "Remote agent session created. Full bidirectional streaming coming soon.",
        "session": session.id,
    }));
}

/// Add the server command to the CLI
pub fn add_cli_command(cmd: &mut clap::Command) {
    // This is called at CLI build time — pattern from existing CLI setup
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let cfg = RemoteConfig::default();
        assert_eq!(cfg.port, 9173);
    }
}
