//! MCP Integration — Model Context Protocol client/server
//!
//! Inspired by goose (Block) and codex MCP integration.
//!
//! **Design:**
//! - Server discovery from config + ~/.hyper/mcp/*.json
//! - Tool registration from MCP servers into agent tool set
//! - HTTP transport (stdio deferred to future release)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

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
    Stdio,  // reserved for future
}

/// A tool that an MCP server provides
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub server: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Connection state for an MCP server
struct McpConnection {
    config: McpServerConfig,
    tools: Vec<McpTool>,
    http_client: reqwest::Client,
}

/// MCP Registry — manages all MCP server connections
pub struct McpRegistry {
    connections: Arc<Mutex<Vec<McpConnection>>>,
    #[allow(dead_code)]
    project_root: PathBuf,
}

impl McpRegistry {
    pub fn new(project_root: &Path) -> Self {
        Self {
            connections: Arc::new(Mutex::new(Vec::new())),
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
    pub async fn connect_all(&self, servers: &[McpServerConfig]) {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();

        let mut connections = self.connections.lock().await;

        for config in servers {
            if config.auto_connect == Some(false) {
                continue;
            }
            if config.transport != McpTransport::Http {
                eprintln!("  ⚠️  MCP '{}': stdio transport not yet supported, skipping", config.name);
                continue;
            }
            let url = match &config.url {
                Some(u) => u.clone(),
                None => {
                    eprintln!("  ⚠️  MCP '{}': no URL configured", config.name);
                    continue;
                }
            };

            match self.initialize_http(&client, config, &url).await {
                Ok(tools) => {
                    println!("  🔌 MCP connected: {} ({} tools)", config.name, tools.len());
                    connections.push(McpConnection {
                        config: config.clone(),
                        tools,
                        http_client: client.clone(),
                    });
                }
                Err(e) => {
                    eprintln!("  ⚠️  MCP '{}' connection failed: {e}", config.name);
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
        let connections = self.connections.lock().await;
        let mut all = Vec::new();
        for conn in connections.iter() {
            all.extend(conn.tools.clone());
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

    /// Call an MCP tool
    pub async fn call_tool(&self, tool_name: &str, args: Value) -> anyhow::Result<Value> {
        let connections = self.connections.lock().await;

        for conn in connections.iter() {
            let has_tool = conn.tools.iter().any(|t| t.name == tool_name);
            if !has_tool {
                continue;
            }

            let url = match &conn.config.url {
                Some(u) => u,
                None => continue,
            };

            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": { "name": tool_name, "arguments": args }
            });

            let mut req = conn.http_client.post(url).json(&request);
            if let Some(key) = &conn.config.api_key {
                req = req.header("Authorization", format!("Bearer {key}"));
            }

            let resp = req.send().await?;
            let body: Value = resp.json().await?;
            return Ok(body["result"].clone());
        }

        anyhow::bail!("MCP tool '{tool_name}' not found on any connected server");
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
        let connections = self.connections.lock().await;
        // HTTP clients have no persistent connection to close
        let count = connections.len();
        if count > 0 {
            println!("  🔌 Disconnected from {count} MCP server(s)");
        }
    }
    #[allow(dead_code)]
    /// List connected servers with tool counts
    pub async fn list_connections(&self) -> Vec<String> {
        let connections = self.connections.lock().await;
        connections.iter()
            .map(|c| format!("{} ({} tools, HTTP)", c.config.name, c.tools.len()))
            .collect()
    }
}

// ═══════════════════════════════════════════════════════════════════
//  MCP SERVER — expose HyperAgent memory to any MCP-compatible client
// ═══════════════════════════════════════════════════════════════════
//
// Speaks JSON-RPC 2.0 over newline-delimited stdio (the transport
// Claude Desktop / goose / Cursor expect). Exposes three tools:
//
//   memory_add      — write a memory entry
//   memory_recall   — top-N search over the container, scored
//   memory_context  — get a formatted prompt block for LLM injection
//
// All calls are auto-scoped to the --container tag passed at startup
// (matches supermemory's containerTag isolation model).

use crate::memory::MemoryType;

/// Default schema-version of the MCP server (in initialize response)
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const MCP_SERVER_NAME: &str = "hyperagent-memory";
const MCP_SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tool definitions returned by `tools/list`
fn memory_tool_definitions() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "memory_add",
            "description": "Persist a memory entry. Auto-scoped to the server's container tag.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The fact / preference / observation to remember"
                    },
                    "memory_type": {
                        "type": "string",
                        "enum": ["user_preference", "codebase_fact", "action_outcome", "decision", "bug_fix", "learned", "ephemeral"],
                        "description": "Type of memory (default: learned)"
                    },
                    "importance": {
                        "type": "number",
                        "description": "0.0-1.0, how important is this (default: auto-scored from content)"
                    }
                },
                "required": ["content"]
            }
        },
        {
            "name": "memory_recall",
            "description": "Search the container's memories. Returns scored hits sorted by relevance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural-language search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results to return (default: 5)"
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "memory_context",
            "description": "Get a formatted context block of the most relevant memories for a query, ready to be injected into an LLM prompt.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural-language query describing the context you need"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max memories to include (default: 8)"
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "memory_profile",
            "description": "Return the container's static 'user identity' profile: stable preferences, codebase facts, and decisions. Inject this into every prompt for consistent behaviour.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "max_facts_per_section": {
                        "type": "integer",
                        "description": "Cap on facts per section (preferences / codebase_facts / decisions). Default: 20."
                    }
                }
            }
        }
    ])
}

/// Run the MCP memory server on stdio. Blocks until stdin closes.
pub async fn serve_stdio(container: &str, db: Option<&str>) -> anyhow::Result<()> {
    use crate::memory::{MemoryManager, SqliteMemoryStore};
    use std::io::{BufRead, Write};

    // Resolve DB path (default: ~/.hyper/memory.db)
    let db_path = match db {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let home = dirs_next::home_dir()
                .ok_or_else(|| anyhow::anyhow!("could not resolve home dir; pass --db"))?;
            home.join(".hyper").join("memory.db")
        }
    };
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    eprintln!("🔌 HyperAgent MCP memory server starting");
    eprintln!("   container: {container}");
    eprintln!("   db:        {}", db_path.display());

    // Build the manager (Box<dyn MemoryStore> -> MemoryManager)
    let store = SqliteMemoryStore::new(&db_path)?;
    let mgr = MemoryManager::new(Box::new(store), "mcp-server").with_container(container);

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut input = stdin.lock();

    let mut buf = String::new();
    loop {
        buf.clear();
        let n = input.read_line(&mut buf)?;
        if n == 0 {
            // EOF — caller closed stdin, exit cleanly
            break;
        }
        let line = buf.trim();
        if line.is_empty() {
            continue;
        }

        let response = handle_request(line, &mgr);
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    eprintln!("🔌 MCP memory server stopped");
    Ok(())
}

/// Handle a single JSON-RPC 2.0 request and produce a response
fn handle_request(line: &str, mgr: &crate::memory::MemoryManager) -> serde_json::Value {
    let req: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return jsonrpc_error(None, -32700, format!("parse error: {e}")),
    };

    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = req["method"].as_str().unwrap_or("");
    let params = &req["params"];

    match method {
        "initialize" => jsonrpc_ok(id, serde_json::json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": MCP_SERVER_NAME, "version": MCP_SERVER_VERSION }
        })),
        "notifications/initialized" => {
            // Notification — no response expected, but we must not error
            serde_json::Value::Null
        }
        "tools/list" => jsonrpc_ok(id, serde_json::json!({
            "tools": memory_tool_definitions()
        })),
        "tools/call" => {
            let tool_name = params["name"].as_str().unwrap_or("");
            let args = &params["arguments"];
            match call_memory_tool(tool_name, args, mgr) {
                Ok(content) => jsonrpc_ok(id, serde_json::json!({
                    "content": [{ "type": "text", "text": content }],
                    "isError": false
                })),
                Err(e) => jsonrpc_ok(id, serde_json::json!({
                    "content": [{ "type": "text", "text": format!("error: {e}") }],
                    "isError": true
                })),
            }
        }
        "ping" => jsonrpc_ok(id, serde_json::json!({})),
        other => jsonrpc_error(Some(id.clone()), -32601, format!("method not found: {other}")),
    }
}

/// Dispatch a `tools/call` to the right memory backend call
fn call_memory_tool(
    name: &str,
    args: &serde_json::Value,
    mgr: &crate::memory::MemoryManager,
) -> anyhow::Result<String> {
    match name {
        "memory_add" => {
            let content = args["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing 'content'"))?;
            let mem_type = match args["memory_type"].as_str() {
                Some("user_preference") => MemoryType::UserPreference,
                Some("codebase_fact") => MemoryType::CodebaseFact,
                Some("action_outcome") => MemoryType::ActionOutcome,
                Some("decision") => MemoryType::Decision,
                Some("bug_fix") => MemoryType::BugFix,
                Some("ephemeral") => MemoryType::Ephemeral,
                _ => MemoryType::Learned,
            };
            let id = mgr.remember(content, mem_type)?;
            Ok(format!("stored memory id={id}"))
        }
        "memory_recall" => {
            let query = args["query"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing 'query'"))?;
            let limit = args["limit"].as_u64().unwrap_or(5) as usize;
            let hits = mgr.recall(query, limit)?;
            let mut out = String::new();
            for (i, h) in hits.iter().enumerate() {
                out.push_str(&format!(
                    "{}. [{} | {}%] {}\n",
                    i + 1,
                    h.container_tag,
                    (h.importance * 100.0) as u32,
                    h.content
                ));
            }
            if out.is_empty() {
                out = "(no memories found in this container)".into();
            }
            Ok(out)
        }
        "memory_context" => {
            let query = args["query"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing 'query'"))?;
            let limit = args["limit"].as_u64().unwrap_or(8) as usize;
            let ctx = mgr.build_context(query, limit)?;
            if ctx.is_empty() {
                Ok("(no relevant memories in this container)".into())
            } else {
                Ok(ctx)
            }
        }
        "memory_profile" => {
            let max = args["max_facts_per_section"].as_u64().unwrap_or(20) as usize;
            let profile = mgr.profile(max)?;
            if profile.is_empty() {
                Ok("(no static profile facts in this container yet)".into())
            } else {
                Ok(profile)
            }
        }
        other => anyhow::bail!("unknown tool: {other}"),
    }
}

fn jsonrpc_ok(id: serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn jsonrpc_error(id: Option<serde_json::Value>, code: i32, message: String) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(serde_json::Value::Null),
        "error": { "code": code, "message": message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryManager, MemoryType, SqliteMemoryStore};

    /// Drive the in-process JSON-RPC dispatcher without spawning a real
    /// stdio server — fast and hermetic.
    #[test]
    fn mcp_server_roundtrip() {
        // Temp DB
        let tmp = std::env::temp_dir().join(format!(
            "hyperagent_mcp_test_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);
        let store = SqliteMemoryStore::new(&tmp).unwrap();
        let mgr = MemoryManager::new(Box::new(store), "test-agent").with_container("mcp-test");

        // ── initialize ──
        let r = handle_request(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            &mgr,
        );
        assert_eq!(r["result"]["serverInfo"]["name"], MCP_SERVER_NAME);

        // ── tools/list ──
        let r = handle_request(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            &mgr,
        );
        let tool_names: Vec<String> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert!(tool_names.contains(&"memory_add".into()));
        assert!(tool_names.contains(&"memory_recall".into()));
        assert!(tool_names.contains(&"memory_context".into()));

        // ── memory_add ──
        let r = handle_request(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_add","arguments":{"content":"User prefers Rust for systems work","memory_type":"user_preference"}}}"#,
            &mgr,
        );
        assert_eq!(r["result"]["isError"], false);
        assert!(r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("stored memory id="));

        // ── memory_recall ──
        let r = handle_request(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"memory_recall","arguments":{"query":"rust","limit":3}}}"#,
            &mgr,
        );
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Rust"), "recall should find the Rust fact: {text}");

        // ── memory_context ──
        let r = handle_request(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"memory_context","arguments":{"query":"language preferences","limit":3}}}"#,
            &mgr,
        );
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Rust") || !text.contains("no relevant memories"));

        // ── isolation: writing into a DIFFERENT container must not show
        //    up in our recall. Use a second manager on the same DB. ──
        let store2 = SqliteMemoryStore::new(&tmp).unwrap();
        let mgr2 = MemoryManager::new(Box::new(store2), "other-agent")
            .with_container("other-container");
        mgr2
            .remember("completely unrelated python fact", MemoryType::Learned)
            .unwrap();

        let r = handle_request(
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"memory_recall","arguments":{"query":"python","limit":5}}}"#,
            &mgr,
        );
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            !text.contains("unrelated"),
            "container isolation must prevent leak: {text}"
        );

        // ── error: unknown method ──
        let r = handle_request(
            r#"{"jsonrpc":"2.0","id":7,"method":"foo/bar","params":{}}"#,
            &mgr,
        );
        assert_eq!(r["error"]["code"], -32601);

        let _ = std::fs::remove_file(&tmp);
    }
}
