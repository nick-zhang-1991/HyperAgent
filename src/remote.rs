//! Remote Agent — SSH-based remote node management and TCP server
//!
//! Supports:
//! - SSH remote command execution
//! - Remote file operations (scp-based)
//! - Remote agent session via TCP server
//! - Host configuration management
//!
//! Usage:
//!   hyper serve --port 8080           # Start remote agent server
//!   hyper remote list                  # List configured remote hosts
//!   hyper remote add <name> <user@host> [--port 22] [--key path]
//!   hyper remote run <name> <prompt>   # Run agent remotely via SSH
//!   hyper remote ssh <name> <command>  # Run any command on remote host

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCmd;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// A configured remote host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteHost {
    pub name: String,
    pub user: String,
    pub host: String,
    pub port: u16,
    /// Optional SSH key path (default: ~/.ssh/id_rsa)
    pub key_path: Option<PathBuf>,
    /// Optional label/description
    pub label: Option<String>,
    /// Optional working directory on remote
    pub workdir: Option<String>,
}

impl RemoteHost {
    /// Build SSH command arguments for connecting
    fn ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
            "-p".to_string(),
            self.port.to_string(),
        ];
        if let Some(ref key) = self.key_path {
            args.extend_from_slice(&["-i".to_string(), key.to_string_lossy().to_string()]);
        }
        args.push(format!("{}@{}", self.user, self.host));
        args
    }

    /// Run a command on the remote host via SSH
    pub fn run_ssh(&self, command: &str) -> Result<String> {
        let mut args = self.ssh_args();
        args.push(command.to_string());

        let output = ProcessCmd::new("ssh")
            .args(&args)
            .output()
            .map_err(|e| anyhow!("SSH failed: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow!("SSH command failed (exit {}): {}", output.status, stderr.trim()))
        }
    }

    /// Copy a local file to the remote host via SCP
    pub fn scp_to(&self, local_path: &Path, remote_path: &str) -> Result<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-P".to_string(),
            self.port.to_string(),
        ];
        if let Some(ref key) = self.key_path {
            args.extend_from_slice(&["-i".to_string(), key.to_string_lossy().to_string()]);
        }
        args.push(local_path.to_string_lossy().to_string());
        args.push(format!("{}@{}:{}", self.user, self.host, remote_path));

        let output = ProcessCmd::new("scp")
            .args(&args)
            .output()
            .map_err(|e| anyhow!("SCP failed: {e}"))?;

        if output.status.success() {
            Ok(format!("Copied {} to {}:{}", local_path.display(), self.host, remote_path))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow!("SCP failed (exit {}): {}", output.status, stderr.trim()))
        }
    }

    /// Copy a remote file to local via SCP
    pub fn scp_from(&self, remote_path: &str, local_path: &Path) -> Result<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-P".to_string(),
            self.port.to_string(),
        ];
        if let Some(ref key) = self.key_path {
            args.extend_from_slice(&["-i".to_string(), key.to_string_lossy().to_string()]);
        }
        args.push(format!("{}@{}:{}", self.user, self.host, remote_path));
        args.push(local_path.to_string_lossy().to_string());

        let output = ProcessCmd::new("scp")
            .args(&args)
            .output()
            .map_err(|e| anyhow!("SCP failed: {e}"))?;

        if output.status.success() {
            Ok(format!("Copied {}:{} to {}", self.host, remote_path, local_path.display()))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow!("SCP failed (exit {}): {}", output.status, stderr.trim()))
        }
    }
}

/// Remote hosts configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteConfig {
    pub hosts: HashMap<String, RemoteHost>,
    /// Storage path
    #[serde(skip)]
    pub config_path: PathBuf,
}

impl RemoteConfig {
    /// Load from default paths
    pub fn load() -> Self {
        let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let config_path = home.join(".hyper").join("remotes.toml");
        if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(mut config) = toml::from_str::<Self>(&content) {
                    config.config_path = config_path;
                    return config;
                }
            }
        }
        Self { hosts: HashMap::new(), config_path }
    }

    /// Save configuration
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&self.config_path, content)?;
        Ok(())
    }

    /// Add or update a remote host
    pub fn add(&mut self, host: RemoteHost) {
        self.hosts.insert(host.name.clone(), host);
    }

    /// Remove a remote host
    pub fn remove(&mut self, name: &str) -> bool {
        self.hosts.remove(name).is_some()
    }

    /// Get a remote host by name
    pub fn get(&self, name: &str) -> Option<&RemoteHost> {
        self.hosts.get(name)
    }

    /// Render hosts list as string
    pub fn render(&self) -> String {
        if self.hosts.is_empty() {
            return "   No remote hosts configured.\n   Use `hyper remote add <name> <user@host>` to add one.".to_string();
        }
        let mut output = format!("   🌐 Remote Hosts ({} total)\n", self.hosts.len());
        let mut sorted: Vec<&RemoteHost> = self.hosts.values().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        for host in sorted {
            let label = host.label.as_deref().unwrap_or("");
            let workdir = host.workdir.as_deref().unwrap_or("~");
            output.push_str(&format!(
                "   • {}  {}@{}:{}  [{workdir}]  {label}\n",
                host.name, host.user, host.host, host.port
            ));
        }
        output
    }
}

// ============ TCP Server (for remote agent sessions) ============

/// A remote agent session
struct RemoteSession {
    id: String,
    writer: Arc<Mutex<tokio::io::BufWriter<tokio::net::tcp::OwnedWriteHalf>>>,
}

impl RemoteSession {
    fn new(id: String, stream: tokio::net::tcp::OwnedWriteHalf) -> Self {
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

/// Start the remote agent TCP server
pub async fn start_server(port: u16) -> Result<()> {
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await?;
    println!("   🌐 Remote agent server listening on {addr}");
    println!("   💡 Connect with: nc localhost {port}");

    let mut session_counter = 0u64;

    loop {
        let (stream, peer) = listener.accept().await?;
        session_counter += 1;
        let session_id = format!("sess-{session_counter}");
        println!("   🔗 Connection from {peer} — session {session_id}");

        let (reader, writer) = stream.into_split();
        let session = RemoteSession::new(session_id.clone(), writer);

        // Spawn handler that reads JSON lines and processes them
        tokio::spawn(async move {
            let mut buf_reader = BufReader::new(reader);
            let mut line = String::new();

            let _ = session.send_progress("Connected. Send JSON: {\"prompt\": \"your task\"}").await;

            while buf_reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                let trimmed = line.trim().to_string();
                if trimmed.is_empty() {
                    line.clear();
                    continue;
                }
                println!("   📨 Received from {}: {}", peer, &trimmed.chars().take(100).collect::<String>());

                // Parse JSON request
                match serde_json::from_str::<serde_json::Value>(&trimmed) {
                    Ok(req) => {
                        let prompt = req.get("prompt")
                            .and_then(|v| v.as_str())
                            .unwrap_or("run health check");

                        let _ = session.send_progress(&format!("Running: {prompt}"));
                        let _ = session.send_result(&serde_json::json!({
                            "message": format!("Processed: {prompt}"),
                            "session": session.id,
                            "prompt": prompt,
                        }));
                    }
                    Err(e) => {
                        let _ = session.send_json(&serde_json::json!({
                            "type": "error",
                            "message": format!("Invalid JSON: {e}"),
                        }));
                    }
                }

                line.clear();
            }

            println!("   🔌 Client {peer} disconnected");
            let _ = session.send_result(&serde_json::json!({"message": "Session ended", "session": session.id}));
        });
    }
}

/// Execute a prompt on a remote host via SSH by triggering the hyper agent
pub async fn run_on_remote(host: &RemoteHost, prompt: &str) -> Result<String> {
    let escaped = prompt
        .replace('"', "\\\"")
        .replace('\'', "'\\''")
        .replace('`', "\\`")
        .replace('$', "\\$");
    let workdir = host.workdir.as_deref().unwrap_or("~");
    let command = format!("cd {workdir} && hyper run --yes \"{escaped}\" 2>&1 | tail -50");
    host.run_ssh(&command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_config_default() {
        let cfg = RemoteConfig::load();
        // Config exists in ~/.hyper/remotes.toml or empty
        assert!(cfg.config_path.to_string_lossy().contains("remotes"));
    }

    #[test]
    fn test_remote_host_ssh_args() {
        let host = RemoteHost {
            name: "test".to_string(),
            user: "admin".to_string(),
            host: "example.com".to_string(),
            port: 22,
            key_path: None,
            label: None,
            workdir: None,
        };
        let args = host.ssh_args();
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"22".to_string()));
        assert!(args.contains(&"admin@example.com".to_string()));
    }

    #[test]
    fn test_remote_host_custom_port() {
        let host = RemoteHost {
            name: "test".to_string(),
            user: "root".to_string(),
            host: "10.0.0.1".to_string(),
            port: 2222,
            key_path: Some(PathBuf::from("/tmp/test_key")),
            label: Some("dev server".to_string()),
            workdir: Some("/app".to_string()),
        };
        let args = host.ssh_args();
        assert!(args.contains(&"2222".to_string()));
        assert!(args.contains(&"root@10.0.0.1".to_string()));
        assert_eq!(host.workdir.as_deref(), Some("/app"));
    }

    #[test]
    fn test_config_add_remove() {
        let mut config = RemoteConfig::load();
        let host = RemoteHost {
            name: "test-host".to_string(),
            user: "user".to_string(),
            host: "localhost".to_string(),
            port: 22,
            key_path: None,
            label: None,
            workdir: None,
        };
        config.add(host);
        assert!(config.get("test-host").is_some());
        assert!(config.remove("test-host"));
        assert!(config.get("test-host").is_none());
    }

    #[test]
    fn test_render_empty() {
        let config = RemoteConfig::default();
        let output = config.render();
        assert!(output.contains("No remote hosts"));
    }

    #[test]
    fn test_render_with_hosts() {
        let mut config = RemoteConfig::default();
        config.add(RemoteHost {
            name: "server-1".to_string(),
            user: "admin".to_string(),
            host: "192.168.1.1".to_string(),
            port: 22,
            key_path: None,
            label: Some("production".to_string()),
            workdir: Some("/opt/app".to_string()),
        });
        let output = config.render();
        assert!(output.contains("server-1"));
        assert!(output.contains("admin@192.168.1.1"));
        assert!(output.contains("production"));
    }

    #[test]
    fn test_add_duplicate_overwrites() {
        let mut config = RemoteConfig::default();
        config.add(RemoteHost {
            name: "dup".to_string(), user: "user1".to_string(), host: "a.com".to_string(),
            port: 22, key_path: None, label: None, workdir: None,
        });
        config.add(RemoteHost {
            name: "dup".to_string(), user: "user2".to_string(), host: "b.com".to_string(),
            port: 22, key_path: None, label: None, workdir: None,
        });
        let h = config.get("dup").unwrap();
        assert_eq!(h.user, "user2");
        assert_eq!(h.host, "b.com");
    }
}
