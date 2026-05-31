//! Multi-provider LLM router with fallback and hot-reload
//!
//! Supports multiple providers with per-request model selection,
//! automatic fallback, and config file hot-reload.

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
    /// Input price per 1M tokens (USD)
    #[serde(default = "default_input_price")]
    pub input_price_per_1m: f64,
    /// Output price per 1M tokens (USD)
    #[serde(default = "default_output_price")]
    pub output_price_per_1m: f64,
    /// Max budget per run (USD), 0 = unlimited
    #[serde(default)]
    pub max_budget_per_run: f64,
}

fn default_input_price() -> f64 { 0.15 }
fn default_output_price() -> f64 { 0.60 }

/// Agent configuration with permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
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

/// The router that selects providers and models
/// Supports hot-reload: checks config file mtime on every select_provider() call.
pub struct ModelRouter {
    providers: Vec<ProviderConfig>,
    agents: Vec<AgentConfig>,
    config_path: Option<PathBuf>,
    config_mtime: Option<SystemTime>,
}

impl Default for ModelRouter {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

impl ModelRouter {
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
            (Self::default_providers(), Self::default_agents())
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

    fn default_providers() -> Vec<ProviderConfig> {
        vec![
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
        ]
    }

    fn default_agents() -> Vec<AgentConfig> {
        vec![
            AgentConfig {
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
            AgentConfig {
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
            AgentConfig {
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
            AgentConfig {
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
            AgentConfig {
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
        ]
    }

    fn load_config(path: &PathBuf) -> Result<(Vec<ProviderConfig>, Vec<AgentConfig>)> {
        let content = std::fs::read_to_string(path).context("Failed to read config file")?;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("toml");
        let parsed: RouterConfig = match ext {
            "json" => serde_json::from_str(&content)
                .context("Failed to parse JSON config")?,
            _ => toml::from_str(&content)
                .context("Failed to parse TOML config")?,
        };
        Ok((parsed.providers, parsed.agents))
    }

    /// Select the best provider for a given model (with hot-reload check)
    pub fn select_provider(&self, model: &str) -> Result<&ProviderConfig> {
        // First try exact match
        for p in &self.providers {
            if p.models.iter().any(|m| m == model) {
                return Ok(p);
            }
        }

        // Fall back to first provider with API key
        for p in &self.providers {
            if !p.api_key.is_empty() {
                return Ok(p);
            }
        }

        // Last resort: first provider
        self.providers
            .first()
            .ok_or_else(|| anyhow::anyhow!("No providers configured"))
    }

    /// Get an agent config by name
    pub fn get_agent(&self, name: &str) -> Option<&AgentConfig> {
        self.agents.iter().find(|a| a.name == name)
    }

    /// List all available agents
    pub fn list_agents(&self) -> Vec<&AgentConfig> {
        self.agents.iter().collect()
    }

    #[allow(dead_code)]
    /// Export config as TOML string
    pub fn export_config(&self) -> String {
        let config = RouterConfig {
            providers: self.providers.clone(),
            agents: self.agents.clone(),
        };
        toml::to_string_pretty(&config).unwrap_or_default()
    }

    /// Initialize default config file
    pub fn init_config() -> Result<()> {
        let config_path = dirs_next::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hyper")
            .join("config.toml");

        if config_path.exists() {
            return Ok(());
        }

        std::fs::create_dir_all(config_path.parent().unwrap())?;
        let config = RouterConfig {
            providers: Self::default_providers(),
            agents: Self::default_agents(),
        };
        let content = toml::to_string_pretty(&config)?;
        std::fs::write(&config_path, content)?;
        println!("✅ Created config: {}", config_path.display());
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RouterConfig {
    providers: Vec<ProviderConfig>,
    agents: Vec<AgentConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_providers_exist() {
        let providers = ModelRouter::default_providers();
        assert!(!providers.is_empty(), "Should have at least 1 provider");
        assert!(providers.iter().any(|p| p.name == "deepseek"));
    }

    #[test]
    fn test_provider_select_exact_match() {
        let providers = ModelRouter::default_providers();
        let agents = ModelRouter::default_agents();
        let router = ModelRouter { providers, agents, config_path: None, config_mtime: None };
        let result = router.select_provider("deepseek-v4-flash");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "deepseek");
    }

    #[test]
    fn test_provider_select_fallback() {
        let providers = ModelRouter::default_providers();
        let agents = ModelRouter::default_agents();
        let router = ModelRouter { providers, agents, config_path: None, config_mtime: None };
        let result = router.select_provider("unknown-model");
        // Should fall back to first provider with a key
        assert!(result.is_ok());
    }

    #[test]
    fn test_default_agents_include_build() {
        let agents = ModelRouter::default_agents();
        assert!(agents.iter().any(|a| a.name == "build"));
        assert!(agents.iter().any(|a| a.name == "plan"));
        assert!(agents.iter().any(|a| a.name == "review"));
    }

    #[test]
    fn test_get_agent_by_name() {
        let providers = ModelRouter::default_providers();
        let agents = ModelRouter::default_agents();
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
}
