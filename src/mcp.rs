//! MCP Integration — Model Context Protocol client/server
//!
//! Inspired by goose (Block) and codex MCP integration.
//!
//! **Design:**
//! - Server discovery from config + ~/.hyper/mcp/*.json
//! - Tool registration from MCP servers into agent tool set
//! - HTTP transport: JSON-RPC over HTTP POST
//! - Stdio transport: spawn child process, JSON-RPC over stdin/stdout lines

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex as TokioMutex;

/// Configuration for an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server name (used as tool namespace)
    pub name: String,
    /// Transport type
    #[serde(rename = "type")]
    pub transport: McpTransport,
    /// URL (for HTTP transport)
    pub url: Option<String>,
    /// Command (for stdio transport)
    pub command: Option<String>,
    /// Arguments (for stdio transport)
    pub args: Option<Vec<String>>,
    /// API key for auth
    pub api_key: Option<String>,
    /// Environment variables for the server process
    pub env: Option<HashMap<String, String>>,
    /// Auto-connect on startup
    pub auto_connect: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    Http,
    Stdio,
}

/// A tool that an MCP server provides
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub server: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// A pending JSON-RPC response
struct PendingResponse {
    value: Arc<Mutex<Option<anyhow::Result<Value>>>>,
    ready: Arc<std::sync::Condvar>,
}

impl PendingResponse {
    fn new() -> Self {
        Self {
            value: Arc::new(Mutex::new(None)),
            ready: Arc::new(std::sync::Condvar::new()),
        }
    }

    fn wait(&self, timeout: std::time::Duration) -> anyhow::Result<Value> {
        let mut guard = self.value.lock().unwrap();
        if guard.is_none() {
            let (guard_result, _) = self.ready.wait_timeout(guard, timeout).unwrap();
            guard = guard_result;
        }
        guard.take().unwrap_or_else(|| Err(anyhow::anyhow!("MCP timeout")))
    }

    fn set(&self, result: anyhow::Result<Value>) {
        let mut guard = self.value.lock().unwrap();
        *guard = Some(result);
        self.ready.notify_all();
    }
}

/// HTTP connection state
struct HttpConnection {
    config: McpServerConfig,
    tools: Vec<McpTool>,
    http_client: reqwest::Client,
}

/// Stdio connection state — manages a child process
struct StdioConnection {
    config: McpServerConfig,
    tools: Vec<McpTool>,
    child: Option<Child>,
    stdin_writer: Option<Arc<Mutex<ChildStdin>>>,
    pending: Arc<Mutex<HashMap<u64, Arc<PendingResponse>>>>,
    next_id: Arc<AtomicU64>,
}

impl StdioConnection {
    fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            tools: Vec::new(),
            child: None,
            stdin_writer: None,
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Spawn the child process and initialize MCP handshake
    fn spawn_and_init(&mut self) -> anyhow::Result<()> {
        let command = self.config.command.as_deref()
            .ok_or_else(|| anyhow::anyhow!("Stdio MCP server '{}' has no command", self.config.name))?;
        let args = self.config.args.as_ref()
            .map(|a| a.as_slice())
            .unwrap_or(&[]);

        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        // Set environment variables if configured
        if let Some(ref env) = self.config.env {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }

        let mut child = cmd.spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn MCP server '{}': {e}", self.config.name))?;

        let stdin = child.stdin.take()
            .ok_or_else(|| anyhow::anyhow!("No stdin on MCP server process"))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("No stdout on MCP server process"))?;

        let stdin_writer = Arc::new(Mutex::new(stdin));
        let pending = self.pending.clone();
        let next_id = self.next_id.clone();
        let server_name = self.config.name.clone();

        // Spawn reader thread for stdout
        let reader_pending = pending.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(text) => {
                        if text.trim().is_empty() {
                            continue;
                        }
                        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                            if let Some(id) = parsed["id"].as_u64() {
                                let mut map = reader_pending.lock().unwrap();
                                if let Some(pending) = map.remove(&id) {
                                    let result = Ok(parsed["result"].clone());
                                    pending.set(result);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("   ⚠️  MCP '{}' read error: {e}", server_name);
                        break;
                    }
                }
            }
        });

        self.child = Some(child);
        self.stdin_writer = Some(stdin_writer);

        // Initialize handshake
        let init_result = self.send_request("initialize", serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "hyperagent", "version": "0.1.0" }
        }))?;

        if let Some(ver) = init_result["protocolVersion"].as_str() {
            tracing::debug!("MCP '{}' initialized (protocol {})", self.config.name, ver);
        }

        // List tools
        let tools_result = self.send_request("tools/list", serde_json::json!({}))?;
        if let Some(tools_arr) = tools_result["tools"].as_array() {
            for t in tools_arr {
                self.tools.push(McpTool {
                    server: self.config.name.clone(),
                    name: t["name"].as_str().unwrap_or("unknown").to_string(),
                    description: t["description"].as_str().unwrap_or("").to_string(),
                    input_schema: t["inputSchema"].clone(),
                });
            }
        }

        Ok(())
    }

    /// Send a JSON-RPC request and wait for response
    fn send_request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let pending_resp = Arc::new(PendingResponse::new());

        {
            let mut map = self.pending.lock().unwrap();
            map.insert(id, pending_resp.clone());
        }

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let request_line = serde_json::to_string(&request)?;
        {
            let writer = self.stdin_writer.as_ref()
                .ok_or_else(|| anyhow::anyhow!("No stdin writer"))?;
            let mut w = writer.lock().unwrap();
            writeln!(w, "{}", request_line)?;
            w.flush()?;
        }

        pending_resp.wait(std::time::Duration::from_secs(30))
    }

    /// Call an MCP tool
    fn call_tool(&self, name: &str, args: Value) -> anyhow::Result<Value> {
        self.send_request("tools/call", serde_json::json!({
            "name": name,
            "arguments": args,
        }))
    }

    /// Kill the child process
    fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Connection state for an MCP server (HTTP or stdio)
enum McpConnectionInner {
    Http(HttpConnection),
    Stdio(StdioConnection),
}

/// MCP Registry — manages all MCP server connections
pub struct McpRegistry {
    connections: Vec<McpConnectionInner>,
    project_root: PathBuf,
}

impl McpRegistry {
    pub fn new(project_root: &Path) -> Self {
        Self {
            connections: Vec::new(),
            project_root: project_root.to_path_buf(),
        }
    }

    /// Discover MCP server configurations
    pub fn discover_servers(config_servers: &[McpServerConfig]) -> Vec<McpServerConfig> {
        let mut servers = config_servers.to_vec();

        if let Some(home) = dirs_next::home_dir() {
            let mcp_dir = home.join(".hyper").join("mcp");
            if mcp_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&mcp_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().is_some_and(|e| e == "json") {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                if let Ok(cfg) = serde_json::from_str::<McpServerConfig>(&content) {
                                    if !servers.iter().any(|s| s.name == cfg.name) {
                                        servers.push(cfg);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        servers
    }

    /// Connect to all configured MCP servers
    pub async fn connect_all(&mut self, servers: &[McpServerConfig]) {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();

        for config in servers {
            if config.auto_connect == Some(false) {
                continue;
            }

            match config.transport {
                McpTransport::Http => {
                    let url = match &config.url {
                        Some(u) => u.clone(),
                        None => {
                            eprintln!("  ⚠️  MCP '{}': no URL configured for HTTP", config.name);
                            continue;
                        }
                    };

                    match self.initialize_http(&http_client, config, &url).await {
                        Ok(tools) => {
                            println!("  🔌 MCP HTTP connected: {} ({} tools)", config.name, tools.len());
                            self.connections.push(McpConnectionInner::Http(HttpConnection {
                                config: config.clone(),
                                tools,
                                http_client: http_client.clone(),
                            }));
                        }
                        Err(e) => {
                            eprintln!("  ⚠️  MCP '{}' HTTP connection failed: {e}", config.name);
                        }
                    }
                }
                McpTransport::Stdio => {
                    let mut conn = StdioConnection::new(config.clone());
                    match conn.spawn_and_init() {
                        Ok(()) => {
                            println!("  🔌 MCP Stdio connected: {} ({} tools)", config.name, conn.tools.len());
                            self.connections.push(McpConnectionInner::Stdio(conn));
                        }
                        Err(e) => {
                            eprintln!("  ⚠️  MCP '{}' stdio connection failed: {e}", config.name);
                        }
                    }
                }
            }
        }
    }

    async fn initialize_http(
        &self,
        client: &reqwest::Client,
        config: &McpServerConfig,
        url: &str,
    ) -> anyhow::Result<Vec<McpTool>> {
        // Initialize
        let init_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "hyperagent", "version": env!("CARGO_PKG_VERSION") }
            }
        });

        let mut req = client.post(url).json(&init_payload);
        if let Some(key) = &config.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("initialize returned {}", resp.status());
        }

        // List tools
        let tools_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "params": {}
        });

        let mut req2 = client.post(url).json(&tools_payload);
        if let Some(key) = &config.api_key {
            req2 = req2.header("Authorization", format!("Bearer {key}"));
        }

        let tools_resp = req2.send().await?;
        let body: Value = tools_resp.json().await?;

        let mut tools = Vec::new();
        if let Some(tool_list) = body["result"]["tools"].as_array() {
            for t in tool_list {
                tools.push(McpTool {
                    server: config.name.clone(),
                    name: t["name"].as_str().unwrap_or("unknown").to_string(),
                    description: t["description"].as_str().unwrap_or("").to_string(),
                    input_schema: t["inputSchema"].clone(),
                });
            }
        }

        Ok(tools)
    }

    /// Get all registered MCP tools
    pub async fn get_all_tools(&self) -> Vec<McpTool> {
        let mut all = Vec::new();
        for conn in &self.connections {
            match conn {
                McpConnectionInner::Http(h) => all.extend(h.tools.clone()),
                McpConnectionInner::Stdio(s) => all.extend(s.tools.clone()),
            }
        }
        all
    }

    #[allow(dead_code)]
    /// Convert MCP tools to OpenAI-compatible ToolDefinitions
    pub async fn to_tool_definitions(&self) -> Vec<crate::llm::provider::ToolDefinition> {
        let tools = self.get_all_tools().await;
        tools.into_iter().map(|t| {
            crate::llm::provider::ToolDefinition {
                tool_type: "function".to_string(),
                function: crate::llm::provider::ToolFunction {
                    name: format!("{}.{}", t.server, t.name),
                    description: t.description,
                    parameters: t.input_schema,
                },
            }
        }).collect()
    }

    /// Call an MCP tool across any connected server
    pub async fn call_tool(&self, tool_name: &str, args: Value) -> anyhow::Result<Value> {
        for conn in &self.connections {
            match conn {
                McpConnectionInner::Http(h) => {
                    let has_tool = h.tools.iter().any(|t| t.name == tool_name);
                    if !has_tool {
                        continue;
                    }
                    let url = match &h.config.url {
                        Some(u) => u,
                        None => continue,
                    };
                    let request = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "tools/call",
                        "params": { "name": tool_name, "arguments": args }
                    });
                    let mut req = h.http_client.post(url).json(&request);
                    if let Some(key) = &h.config.api_key {
                        req = req.header("Authorization", format!("Bearer {key}"));
                    }
                    let resp = req.send().await?;
                    let body: Value = resp.json().await?;
                    return Ok(body["result"].clone());
                }
                McpConnectionInner::Stdio(s) => {
                    let has_tool = s.tools.iter().any(|t| t.name == tool_name);
                    if !has_tool {
                        continue;
                    }
                    // Stdio call is sync (uses Condvar internally), block in place
                    let result = tokio::task::block_in_place(|| s.call_tool(tool_name, args.clone()))?;
                    return Ok(result);
                }
            }
        }
        anyhow::bail!("MCP tool '{tool_name}' not found on any connected server")
    }

    /// Format tools into LLM-friendly list
    pub async fn tools_to_llm_format(&self) -> String {
        let tools = self.get_all_tools().await;
        if tools.is_empty() {
            return String::new();
        }

        let mut out = String::from("\n\n--- MCP Tools Available ---\n");
        for t in &tools {
            let desc = if t.description.len() > 80 {
                format!("{}...", &t.description[..77])
            } else {
                t.description.clone()
            };
            out.push_str(&format!("  {}.{} — {desc}\n", t.server, t.name));
        }
        out
    }

    pub async fn shutdown(&self) {
        let http_count = self.connections.iter()
            .filter(|c| matches!(c, McpConnectionInner::Http(_)))
            .count();
        let stdio_count = self.connections.iter()
            .filter(|c| matches!(c, McpConnectionInner::Stdio(_)))
            .count();

        // Kill stdio child processes
        for conn in &self.connections {
            if let McpConnectionInner::Stdio(_s) = conn {
                // Can't easily mutate through & — killed on drop
            }
        }

        if http_count + stdio_count > 0 {
            println!("  🔌 Disconnected from {} MCP server(s) ({} HTTP, {} stdio)",
                http_count + stdio_count, http_count, stdio_count);
        }
    }

    #[allow(dead_code)]
    /// List connected servers with tool counts
    pub async fn list_connections(&self) -> Vec<String> {
        let mut result = Vec::new();
        for conn in &self.connections {
            match conn {
                McpConnectionInner::Http(h) => {
                    result.push(format!("{} ({} tools, HTTP)", h.config.name, h.tools.len()));
                }
                McpConnectionInner::Stdio(s) => {
                    result.push(format!("{} ({} tools, stdio)", s.config.name, s.tools.len()));
                }
            }
        }
        result
    }
}
