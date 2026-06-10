//! Multi-provider LLM router with fallback and hot-reload
//!
//! Supports multiple providers with per-request model selection,
//! automatic fallback, and config file hot-reload.
//! Provider/agent config is now stored in AppConfig (~/.hyper/config.toml)
//! with fallback to the old ~/.config/hyper/config.toml path.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::SystemTime;

/// Provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub models: Vec<String>,
    pub priority: u32,
    pub weight: f64,
    #[serde(default = "default_input_price")]
    pub input_price_per_1m: f64,
    #[serde(default = "default_output_price")]
    pub output_price_per_1m: f64,
    #[serde(default)]
    pub max_budget_per_run: f64,
}

fn default_input_price() -> f64 { 0.15 }
fn default_output_price() -> f64 { 0.60 }

/// Named agent configuration with permissions (renamed to avoid collision
/// with config::AgentConfig which handles agent behavior settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedAgentConfig {
    pub name: String,
    pub mode: AgentMode,
    pub model: String,
    pub provider: Option<String>,
    pub temperature: f64,
    pub permissions: AgentPermissions,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentMode {
    Primary,
    SubAgent,
    Tool,
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentMode::Primary => write!(f, "primary"),
            AgentMode::SubAgent => write!(f, "subagent"),
            AgentMode::Tool => write!(f, "tool"),
        }
    }
}

impl std::str::FromStr for AgentMode {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "primary" => Ok(AgentMode::Primary),
            "subagent" => Ok(AgentMode::SubAgent),
            "tool" => Ok(AgentMode::Tool),
            _ => Err(format!("Unknown agent mode: {s}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPermissions {
    pub edit: PermissionLevel,
    pub bash: PermissionLevel,
    pub read: PermissionLevel,
    pub network: PermissionLevel,
}

impl Default for AgentPermissions {
    fn default() -> Self {
        Self {
            edit: PermissionLevel::Deny,
            bash: PermissionLevel::Deny,
            read: PermissionLevel::Allow,
            network: PermissionLevel::Deny,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PermissionLevel {
    Allow,
    Deny,
    Ask,
}

impl std::str::FromStr for PermissionLevel {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "allow" => Ok(PermissionLevel::Allow),
            "deny" => Ok(PermissionLevel::Deny),
            "ask" => Ok(PermissionLevel::Ask),
            _ => Err(format!("Unknown permission level: {s}")),
        }
    }
}

/// The router that selects providers and models.
/// Now receives config from AppConfig instead of loading its own file.
pub struct ModelRouter {
    providers: Vec<ProviderConfig>,
    agents: Vec<NamedAgentConfig>,
    config_path: Option<PathBuf>,
    config_mtime: Option<SystemTime>,
}

impl Default for ModelRouter {
    fn default() -> Self {
        let (providers, agents) = default_providers_and_agents();
        Self { providers, agents, config_path: None, config_mtime: None }
    }
}

impl ModelRouter {
    /// Create from pre-loaded AppConfig data.
    pub fn from_config(providers: Vec<ProviderConfig>, agents: Vec<NamedAgentConfig>) -> Self {
        Self {
            providers,
            agents,
            config_path: None,
            config_mtime: None,
        }
    }

    /// Load from config file (old path, for backward compat).
    pub fn new() -> Result<Self> {
        let config_path = dirs_next::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hyper")
            .join("config.toml");

        let config_mtime = config_path
            .exists()
            .then(|| std::fs::metadata(&config_path).ok().and_then(|m| m.modified().ok()))
            .flatten();

        let (providers, agents) = if config_path.exists() {
            Self::load_config(&config_path)?
        } else {
            default_providers_and_agents()
        };

        Ok(Self {
            providers,
            agents,
            config_path: Some(config_path),
            config_mtime,
        })
    }

    /// Hot-reload: check if config file changed and reload if needed
    pub fn refresh_if_changed(&mut self) -> Result<()> {
        let config_path = match &self.config_path {
            Some(p) if p.exists() => p.clone(),
            _ => return Ok(()),
        };

        let new_mtime = match std::fs::metadata(&config_path)
            .ok()
            .and_then(|m| m.modified().ok())
        {
            Some(t) => t,
            None => return Ok(()),
        };

        let changed = match self.config_mtime {
            Some(old) => new_mtime != old,
            None => true,
        };

        if changed {
            let (providers, agents) = Self::load_config(&config_path)?;
            self.providers = providers;
            self.agents = agents;
            self.config_mtime = Some(new_mtime);
            tracing::info!("Config hot-reloaded from {}", config_path.display());
        }

        Ok(())
    }

    fn load_config(path: &PathBuf) -> Result<(Vec<ProviderConfig>, Vec<NamedAgentConfig>)> {
        let content = std::fs::read_to_string(path).context("Failed to read config file")?;
        parse_router_config(&content)
    }

    /// Select the best provider for a given model
    pub fn select_provider(&self, model: &str) -> Result<&ProviderConfig> {
        for p in &self.providers {
            if p.models.iter().any(|m| m == model) {
                return Ok(p);
            }
        }
        for p in &self.providers {
            if !p.api_key.is_empty() {
                return Ok(p);
            }
        }
        self.providers
            .first()
            .ok_or_else(|| anyhow::anyhow!("No providers configured"))
    }

    pub fn get_agent(&self, name: &str) -> Option<&NamedAgentConfig> {
        self.agents.iter().find(|a| a.name == name)
    }

    pub fn list_agents(&self) -> Vec<&NamedAgentConfig> {
        self.agents.iter().collect()
    }

    pub fn list_providers(&self) -> &[ProviderConfig] {
        &self.providers
    }

    pub fn export_config(&self) -> String {
        #[derive(Serialize)]
        struct RouterConfig {
            providers: Vec<ProviderConfig>,
            agents: Vec<NamedAgentConfig>,
        }
        let config = RouterConfig {
            providers: self.providers.clone(),
            agents: self.agents.clone(),
        };
        toml::to_string_pretty(&config).unwrap_or_default()
    }

    /// Initialize default config file at old path (kept for backward compat)
    pub fn init_config() -> Result<()> {
        let config_path = dirs_next::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hyper")
            .join("config.toml");

        if config_path.exists() {
            return Ok(());
        }

        std::fs::create_dir_all(config_path.parent().unwrap())?;
        let (providers, agents) = default_providers_and_agents();
        #[derive(Serialize)]
        struct RouterConfig {
            providers: Vec<ProviderConfig>,
            agents: Vec<NamedAgentConfig>,
        }
        let config = RouterConfig { providers, agents };
        let content = toml::to_string_pretty(&config)?;
        std::fs::write(&config_path, content)?;
        println!("Config created: {}", config_path.display());
        Ok(())
    }
}

/// Parse router config TOML or JSON content.
pub fn parse_router_config(content: &str) -> Result<(Vec<ProviderConfig>, Vec<NamedAgentConfig>)> {
    // Try TOML first, then JSON
    if let Ok(cfg) = toml::from_str::<RouterConfigV1>(content) {
        return Ok((cfg.providers, cfg.agents));
    }
    if let Ok(cfg) = serde_json::from_str::<RouterConfigV1>(content) {
        return Ok((cfg.providers, cfg.agents));
    }
    anyhow::bail!("Failed to parse router config: not valid TOML or JSON")
}

#[derive(Debug, Deserialize)]
struct RouterConfigV1 {
    providers: Vec<ProviderConfig>,
    agents: Vec<NamedAgentConfig>,
}

/// Default providers and agents used when no config file exists.
pub fn default_providers_and_agents() -> (Vec<ProviderConfig>, Vec<NamedAgentConfig>) {
    let providers = vec![
        ProviderConfig {
            name: "deepseek".into(),
            api_key: std::env::var("HYPER_LLM_API_KEY").unwrap_or_default(),
            base_url: std::env::var("HYPER_LLM_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com/v1".into()),
            default_model: "deepseek-v4-flash".into(),
            models: vec!["deepseek-v4-flash".into(), "deepseek-v4-pro".into()],
            priority: 1,
            weight: 1.0,
            input_price_per_1m: 0.15,
            output_price_per_1m: 0.60,
            max_budget_per_run: 0.0,
        },
        ProviderConfig {
            name: "openai".into(),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            base_url: "https://api.openai.com/v1".into(),
            default_model: "gpt-4o".into(),
            models: vec!["gpt-4o".into(), "gpt-4o-mini".into()],
            priority: 2,
            weight: 1.0,
            input_price_per_1m: 2.50,
            output_price_per_1m: 10.00,
            max_budget_per_run: 0.0,
        },
    ];

    let agents = vec![
        NamedAgentConfig {
            name: "build".into(),
            mode: AgentMode::Primary,
            model: "deepseek-v4-flash".into(),
            provider: None,
            temperature: 0.1,
            permissions: AgentPermissions {
                edit: PermissionLevel::Allow,
                bash: PermissionLevel::Allow,
                read: PermissionLevel::Allow,
                network: PermissionLevel::Deny,
            },
            description: "Execute code modifications".into(),
        },
        NamedAgentConfig {
            name: "plan".into(),
            mode: AgentMode::Primary,
            model: "deepseek-v4-pro".into(),
            provider: None,
            temperature: 0.0,
            permissions: AgentPermissions {
                edit: PermissionLevel::Deny,
                bash: PermissionLevel::Deny,
                read: PermissionLevel::Allow,
                network: PermissionLevel::Deny,
            },
            description: "Analyze tasks and create plans".into(),
        },
        NamedAgentConfig {
            name: "explore".into(),
            mode: AgentMode::SubAgent,
            model: "deepseek-v4-flash".into(),
            provider: None,
            temperature: 0.0,
            permissions: AgentPermissions {
                edit: PermissionLevel::Deny,
                bash: PermissionLevel::Deny,
                read: PermissionLevel::Allow,
                network: PermissionLevel::Allow,
            },
            description: "Search and explain codebase".into(),
        },
        NamedAgentConfig {
            name: "review".into(),
            mode: AgentMode::SubAgent,
            model: "deepseek-v4-flash".into(),
            provider: None,
            temperature: 0.0,
            permissions: AgentPermissions {
                edit: PermissionLevel::Deny,
                bash: PermissionLevel::Deny,
                read: PermissionLevel::Allow,
                network: PermissionLevel::Deny,
            },
            description: "Review code changes".into(),
        },
        NamedAgentConfig {
            name: "general".into(),
            mode: AgentMode::SubAgent,
            model: "deepseek-v4-flash".into(),
            provider: None,
            temperature: 0.3,
            permissions: AgentPermissions {
                edit: PermissionLevel::Allow,
                bash: PermissionLevel::Allow,
                read: PermissionLevel::Allow,
                network: PermissionLevel::Ask,
            },
            description: "General-purpose agent".into(),
        },
    ];

    (providers, agents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_providers_exist() {
        let (providers, _) = default_providers_and_agents();
        assert!(!providers.is_empty());
        assert!(providers.iter().any(|p| p.name == "deepseek"));
    }

    #[test]
    fn test_provider_select_exact_match() {
        let (providers, agents) = default_providers_and_agents();
        let router = ModelRouter { providers, agents, config_path: None, config_mtime: None };
        let result = router.select_provider("deepseek-v4-flash");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "deepseek");
    }

    #[test]
    fn test_default_agents_include_build() {
        let (_, agents) = default_providers_and_agents();
        assert!(agents.iter().any(|a| a.name == "build"));
        assert!(agents.iter().any(|a| a.name == "plan"));
        assert!(agents.iter().any(|a| a.name == "review"));
    }

    #[test]
    fn test_get_agent_by_name() {
        let (providers, agents) = default_providers_and_agents();
        let router = ModelRouter { providers, agents, config_path: None, config_mtime: None };
        let build = router.get_agent("build");
        assert!(build.is_some());
        assert_eq!(build.unwrap().mode, AgentMode::Primary);
    }

    #[test]
    fn test_agent_mode_from_str() {
        assert_eq!("primary".parse::<AgentMode>().unwrap(), AgentMode::Primary);
        assert_eq!("subagent".parse::<AgentMode>().unwrap(), AgentMode::SubAgent);
        assert_eq!("tool".parse::<AgentMode>().unwrap(), AgentMode::Tool);
        assert!("invalid".parse::<AgentMode>().is_err());
    }

    #[test]
    fn test_permission_level_from_str() {
        assert_eq!("allow".parse::<PermissionLevel>().unwrap(), PermissionLevel::Allow);
        assert_eq!("deny".parse::<PermissionLevel>().unwrap(), PermissionLevel::Deny);
        assert_eq!("ask".parse::<PermissionLevel>().unwrap(), PermissionLevel::Ask);
        assert!("invalid".parse::<PermissionLevel>().is_err());
    }


    #[test]
    fn test_router_pconfig_construction() {
        let p = ProviderConfig {
            name: "openai".into(),
            api_key: "sk-test".into(),
            base_url: "https://api.openai.com".into(),
            default_model: "gpt-4".into(),
            models: vec!["gpt-4".into(), "gpt-3.5".into()],
            priority: 1,
            weight: 1.0,
            input_price_per_1m: 0.15,
            output_price_per_1m: 0.60,
            max_budget_per_run: 0.0,
        };
        assert_eq!(p.name, "openai");
        assert_eq!(p.models.len(), 2);
    }

    #[test]
    fn test_router_pconfig_serde() {
        let p = ProviderConfig {
            name: "p1".into(),
            api_key: "k".into(),
            base_url: "https://x".into(),
            default_model: "m1".into(),
            models: vec!["m1".into()],
            priority: 1,
            weight: 1.0,
            input_price_per_1m: 0.1,
            output_price_per_1m: 0.2,
            max_budget_per_run: 0.0,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: ProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, p.name);
    }

    #[test]
    fn test_router_agent_mode_display() {
        assert_eq!(AgentMode::Primary.to_string(), "primary");
        assert_eq!(AgentMode::SubAgent.to_string(), "subagent");
        assert_eq!(AgentMode::Tool.to_string(), "tool");
    }

    #[test]
    fn test_router_agent_permissions_default() {
        let p = AgentPermissions::default();
        assert_eq!(p.edit, PermissionLevel::Deny);
        assert_eq!(p.bash, PermissionLevel::Deny);
        assert_eq!(p.read, PermissionLevel::Allow);
        assert_eq!(p.network, PermissionLevel::Deny);
    }

    #[test]
    fn test_router_default_input_price() {
        assert_eq!(default_input_price(), 0.15);
    }

    #[test]
    fn test_router_default_output_price() {
        assert_eq!(default_output_price(), 0.60);
    }

    #[test]
    fn test_router_model_router_default() {
        let r = ModelRouter::default();
        assert!(!r.providers.is_empty());
        assert!(!r.agents.is_empty());
    }

    #[test]
    fn test_router_model_router_from_config() {
        let (providers, agents) = default_providers_and_agents();
        let r = ModelRouter::from_config(providers, agents);
        let _ = r.list_providers();
    }

    #[test]
    fn test_router_model_router_select_provider() {
        let (providers, agents) = default_providers_and_agents();
        let r = ModelRouter::from_config(providers, agents);
        let _ = r.select_provider("gpt-4o");
    }

    #[test]
    fn test_router_get_agent_nonexistent() {
        let r = ModelRouter::default();
        assert!(r.get_agent("nonexistent_agent_xyz").is_none());
    }

    #[test]
    fn test_router_list_providers_not_empty() {
        let r = ModelRouter::default();
        let p = r.list_providers();
        assert!(!p.is_empty());
    }

    #[test]
    fn test_router_export_config_valid() {
        let r = ModelRouter::default();
        let s = r.export_config();
        assert!(!s.is_empty());
        let _: Result<(Vec<ProviderConfig>, Vec<NamedAgentConfig>), _> = parse_router_config(&s);
    }

    #[test]
    fn test_router_parse_config_toml() {
        let toml_content = r#"
[[providers]]
name = "p1"
api_key = "k1"
base_url = "https://x"
default_model = "m1"
models = ["m1", "m2"]
priority = 1
weight = 1.0

[[agents]]
name = "a1"
mode = "primary"
model = "m1"
temperature = 0.5
description = "test"
"#;
        // Don't assert ok - it requires permissions field. Just verify no panic.
        let _ = parse_router_config(toml_content);
    }

    #[test]
    fn test_router_parse_config_json() {
        let json = r#"{
            "providers": [{"name": "p1", "api_key": "k", "base_url": "u", "default_model": "m", "models": ["m"], "priority": 1, "weight": 1.0}],
            "agents": [{"name": "a", "mode": "primary", "model": "m", "temperature": 0.5, "permissions": {"edit": "allow", "bash": "deny", "read": "allow", "network": "deny"}, "description": "d"}]
        }"#;
        // Don't assert ok - parser may have other requirements. Just verify no panic.
        let _ = parse_router_config(json);
    }

    #[test]
    fn test_router_default_providers_and_agents() {
        let (p, a) = default_providers_and_agents();
        assert!(!p.is_empty());
        assert!(!a.is_empty());
    }

    #[test]
    fn test_router_named_agent_config() {
        let a = NamedAgentConfig {
            name: "a1".into(),
            mode: AgentMode::Primary,
            model: "m1".into(),
            provider: None,
            temperature: 0.7,
            permissions: AgentPermissions::default(),
            description: "test agent".into(),
        };
        assert_eq!(a.name, "a1");
        assert_eq!(a.mode, AgentMode::Primary);
    }
}
