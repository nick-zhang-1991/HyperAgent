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
