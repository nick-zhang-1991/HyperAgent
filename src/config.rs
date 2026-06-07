//! HyperAgent Runtime Configuration — persistent config with get/set
//!
//! Config file location: `~/.hyper/config.toml`
//!
//! Supports setting any config key and persisting to TOML, with
//! automatic loading in the orchestrator and CLI.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// LLM provider settings
    pub llm: LlmConfig,
    /// Agent behavior settings
    pub agent: AgentConfig,
    /// Tool safety settings
    pub tools: ToolConfig,
    /// General settings
    pub general: GeneralConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Default model name
    #[serde(default = "default_model")]
    pub model: String,
    /// API base URL
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// Temperature (0.0 - 2.0)
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    /// Max tokens per response
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Number of parallel code agents
    #[serde(default = "default_parallel")]
    pub parallel_agents: usize,
    /// Max context tokens per file
    #[serde(default = "default_context_tokens")]
    pub context_tokens: u32,
    /// Whether to confirm before applying changes
    #[serde(default = "default_confirm")]
    pub confirm: bool,
    /// Enable LLM response cache
    #[serde(default = "default_true")]
    pub cache_enabled: bool,
    /// Enable sandboxed execution (Docker)
    #[serde(default = "default_false")]
    pub sandbox_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    /// Per-tool safety levels: "allow", "deny", "ask"
    #[serde(default)]
    pub safety_overrides: HashMap<String, String>,
    /// Mode-to-tool filtering: which tools are available in each mode
    #[serde(default)]
    pub mode_tools: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// UI language (en, zh-CN)
    #[serde(default = "default_lang")]
    pub lang: String,
    /// Enable telemetry
    #[serde(default = "default_false")]
    pub telemetry: bool,
}

fn default_model() -> String { "deepseek-v4-flash".to_string() }
fn default_base_url() -> String { "https://api.deepseek.com/v1".to_string() }
fn default_temperature() -> f64 { 0.1 }
fn default_max_tokens() -> u32 { 16384 }
fn default_parallel() -> usize { 3 }
fn default_context_tokens() -> u32 { 4000 }
fn default_confirm() -> bool { true }
fn default_true() -> bool { true }
fn default_false() -> bool { false }
fn default_lang() -> String { "en".to_string() }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            llm: LlmConfig {
                model: default_model(),
                base_url: default_base_url(),
                temperature: default_temperature(),
                max_tokens: default_max_tokens(),
            },
            agent: AgentConfig {
                parallel_agents: default_parallel(),
                context_tokens: default_context_tokens(),
                confirm: default_confirm(),
                cache_enabled: true,
                sandbox_enabled: false,
            },
            tools: ToolConfig {
                safety_overrides: HashMap::new(),
                mode_tools: HashMap::new(),
            },
            general: GeneralConfig {
                lang: default_lang(),
                telemetry: false,
            },
        }
    }
}

impl AppConfig {
    /// Config file path: ~/.hyper/config.toml
    pub fn path() -> PathBuf {
        let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".hyper").join("config.toml")
    }

    /// Load config from file, or create default if not found
    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    match toml::from_str(&content) {
                        Ok(cfg) => return cfg,
                        Err(e) => {
                            eprintln!("   ⚠️  Config parse error: {e} — using defaults");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("   ⚠️  Config read error: {e} — using defaults");
                }
            }
        }
        Self::default()
    }

    /// Save config to file
    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        println!("   ✅ Config saved: {}", path.display());
        Ok(())
    }

    /// Set a config value by dotted key path (e.g. "llm.model", "agent.parallel_agents")
    /// Returns an error if the key is not recognized.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "llm.model" => self.llm.model = value.to_string(),
            "llm.base_url" => self.llm.base_url = value.to_string(),
            "llm.temperature" => {
                self.llm.temperature = value.parse()
                    .map_err(|_| anyhow::anyhow!("Invalid temperature: {value} — expected a number"))?
            }
            "llm.max_tokens" => {
                self.llm.max_tokens = value.parse()
                    .map_err(|_| anyhow::anyhow!("Invalid max_tokens: {value} — expected a number"))?
            }
            "agent.parallel_agents" => {
                self.agent.parallel_agents = value.parse()
                    .map_err(|_| anyhow::anyhow!("Invalid parallel_agents: {value} — expected a number"))?
            }
            "agent.context_tokens" => {
                self.agent.context_tokens = value.parse()
                    .map_err(|_| anyhow::anyhow!("Invalid context_tokens: {value} — expected a number"))?
            }
            "agent.confirm" => {
                self.agent.confirm = value.parse::<bool>()
                    .map_err(|_| anyhow::anyhow!("Invalid confirm: {value} — expected true/false"))?
            }
            "agent.cache_enabled" => {
                self.agent.cache_enabled = value.parse::<bool>()
                    .map_err(|_| anyhow::anyhow!("Invalid cache_enabled: {value} — expected true/false"))?
            }
            "agent.sandbox_enabled" => {
                self.agent.sandbox_enabled = value.parse::<bool>()
                    .map_err(|_| anyhow::anyhow!("Invalid sandbox_enabled: {value} — expected true/false"))?
            }
            "tools.safety_overrides" => {
                // Format: "tool_name=allow,tool2=deny"
                self.tools.safety_overrides.clear();
                for pair in value.split(',') {
                    let pair = pair.trim();
                    if let Some((k, v)) = pair.split_once('=') {
                        let k = k.trim().to_string();
                        let v = v.trim().to_string();
                        if !["allow", "deny", "ask"].contains(&v.as_str()) {
                            anyhow::bail!("Invalid safety level '{v}' for {k} — expected allow/deny/ask");
                        }
                        self.tools.safety_overrides.insert(k, v);
                    }
                }
            }
            "general.lang" => self.general.lang = value.to_string(),
            "general.telemetry" => {
                self.general.telemetry = value.parse::<bool>()
                    .map_err(|_| anyhow::anyhow!("Invalid telemetry: {value} — expected true/false"))?
            }
            _ => anyhow::bail!("Unknown config key: {key}. Use 'hyper config list' to see available keys."),
        }
        Ok(())
    }

    /// Get a config value by dotted key path
    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "llm.model" => Some(self.llm.model.clone()),
            "llm.base_url" => Some(self.llm.base_url.clone()),
            "llm.temperature" => Some(self.llm.temperature.to_string()),
            "llm.max_tokens" => Some(self.llm.max_tokens.to_string()),
            "agent.parallel_agents" => Some(self.agent.parallel_agents.to_string()),
            "agent.context_tokens" => Some(self.agent.context_tokens.to_string()),
            "agent.confirm" => Some(self.agent.confirm.to_string()),
            "agent.cache_enabled" => Some(self.agent.cache_enabled.to_string()),
            "agent.sandbox_enabled" => Some(self.agent.sandbox_enabled.to_string()),
            "general.lang" => Some(self.general.lang.clone()),
            "general.telemetry" => Some(self.general.telemetry.to_string()),
            _ => None,
        }
    }

    /// List all config keys with descriptions
    pub fn list_keys() -> Vec<(&'static str, &'static str)> {
        vec![
            ("llm.model", "Default LLM model name"),
            ("llm.base_url", "LLM API base URL"),
            ("llm.temperature", "LLM temperature (0.0-2.0)"),
            ("llm.max_tokens", "Max tokens per response"),
            ("agent.parallel_agents", "Number of parallel agents"),
            ("agent.context_tokens", "Max context tokens per file"),
            ("agent.confirm", "Confirm before applying changes (true/false)"),
            ("agent.cache_enabled", "Enable LLM response cache (true/false)"),
            ("agent.sandbox_enabled", "Enable Docker sandbox (true/false)"),
            ("tools.safety_overrides", "Per-tool safety levels: tool=allow,other=deny"),
            ("general.lang", "UI language (en, zh-CN)"),
            ("general.telemetry", "Enable telemetry (true/false)"),
        ]
    }

    /// Display config in a formatted table
    pub fn display(&self) -> String {

    /// Tools excluded from a given mode, based on config or defaults.
    pub fn excluded_tools_for_mode(mode: &str) -> Vec<&'static str> {
        match mode {
            "ask" => vec![
                "run_bash", "python_repl", "browser", "analyze_image",
                "platform_setup",
            ],
            "code" => vec![
                "web_search", "browser",
            ],
            "debug" => vec![
                "web_search", "python_repl", "browser",
            ],
            "architect" => vec![
                "run_bash", "python_repl", "browser",
            ],
            _ => vec![], // "general" and unknown modes get all tools
        }
    }
        let mut out = String::new();
        out.push_str("\n📋 HyperAgent Configuration\n\n");
        out.push_str(&format!("  {:<30} {}\n", "Key", "Value"));
        out.push_str(&format!("  {}\n", "─".repeat(60)));

        for (key, desc) in Self::list_keys() {
            let val = self.get(key).unwrap_or_default();
            // Mask sensitive values
            let display_val = if key.contains("api_key") || key.contains("base_url") {
                if val.len() > 8 {
                    format!("{}...", &val[..8])
                } else {
                    val
                }
            } else {
                val
            };
            out.push_str(&format!("  {:<30} {}\n", key, display_val));
        }

        // Show safety overrides
        if !self.tools.safety_overrides.is_empty() {
            out.push_str(&format!("\n  Tool Safety Overrides:\n"));
            for (tool, level) in &self.tools.safety_overrides {
                out.push_str(&format!("    {:<28} {}\n", tool, level));
            }
        }

        // Show mode tools
        if !self.tools.mode_tools.is_empty() {
            out.push_str(&format!("\n  Mode Tool Filters:\n"));
            for (mode, tools) in &self.tools.mode_tools {
                out.push_str(&format!("    {:<28} {}\n", mode, tools.join(", ")));
            }
        }

        out
    }
}
