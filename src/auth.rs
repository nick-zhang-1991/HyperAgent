//! Authentication and credential management
//!
//! Manages API keys and authentication state.
//! Supports OAuth-inspired login flow with multiple providers.
//!
//! # Usage
//! ```bash
//! hyper auth login           # Interactive setup
//! hyper auth login --provider openai  # Specific provider
//! hyper auth status          # Show current auth state
//! hyper auth logout          # Clear credentials
//! hyper auth token           # Show current token (masked)
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Authentication provider
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthProvider {
    DeepSeek,
    OpenAI,
    Anthropic,
    OpenRouter,
    Custom(String),
}

impl std::fmt::Display for AuthProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthProvider::DeepSeek => write!(f, "deepseek"),
            AuthProvider::OpenAI => write!(f, "openai"),
            AuthProvider::Anthropic => write!(f, "anthropic"),
            AuthProvider::OpenRouter => write!(f, "openrouter"),
            AuthProvider::Custom(name) => write!(f, "custom/{name}"),
        }
    }
}

impl AuthProvider {
    pub fn default_base_url(&self) -> &str {
        match self {
            AuthProvider::DeepSeek => "https://api.deepseek.com/v1",
            AuthProvider::OpenAI => "https://api.openai.com/v1",
            AuthProvider::Anthropic => "https://api.anthropic.com/v1",
            AuthProvider::OpenRouter => "https://openrouter.ai/api/v1",
            AuthProvider::Custom(_) => "",
        }
    }

    pub fn default_model(&self) -> &str {
        match self {
            AuthProvider::DeepSeek => "deepseek-v4-flash",
            AuthProvider::OpenAI => "gpt-4o",
            AuthProvider::Anthropic => "claude-sonnet-4-20250514",
            AuthProvider::OpenRouter => "openrouter/auto",
            AuthProvider::Custom(_) => "",
        }
    }

    pub fn all() -> Vec<AuthProvider> {
        vec![
            AuthProvider::DeepSeek,
            AuthProvider::OpenAI,
            AuthProvider::Anthropic,
            AuthProvider::OpenRouter,
        ]
    }
}

/// Stored authentication credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCredentials {
    pub provider: AuthProvider,
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub label: String,
}

impl AuthCredentials {
    /// Mask the API key for display (show first 4 + last 4 chars)
    pub fn masked_api_key(&self) -> String {
        let key = &self.api_key;
        if key.len() <= 12 {
            format!("{}...{}", &key[..4.min(key.len())], &key[key.len().saturating_sub(4)..])
        } else {
            format!("{}...{}", &key[..8], &key[key.len() - 4..])
        }
    }

    /// Check if credentials appear valid (non-empty key)
    pub fn is_valid(&self) -> bool {
        !self.api_key.is_empty() && self.api_key.len() >= 8
    }
}

/// Resolve config directory path
fn config_dir() -> PathBuf {
    // Standard: ~/.config/hyper/ on Linux, ~/Library/Application Support/hyper/ on macOS
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
        PathBuf::from(home).join("Library/Application Support/hyper")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_else(|_| "~".to_string())));
        PathBuf::from(xdg).join("hyper")
    }
}

/// Path to auth credentials file
fn auth_file_path() -> PathBuf {
    config_dir().join("auth.json")
}

/// Save authentication credentials
pub fn save_credentials(creds: &AuthCredentials) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create config directory: {}", dir.display()))?;

    let path = auth_file_path();
    let json = serde_json::to_string_pretty(creds)
        .context("Failed to serialize credentials")?;
    std::fs::write(&path, &json)
        .with_context(|| format!("Failed to write auth file: {}", path.display()))?;

    Ok(())
}

/// Load authentication credentials
pub fn load_credentials() -> Result<Option<AuthCredentials>> {
    let path = auth_file_path();
    if !path.exists() {
        return Ok(None);
    }

    let json = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read auth file: {}", path.display()))?;
    let creds: AuthCredentials = serde_json::from_str(&json)
        .context("Failed to parse auth credentials")?;

    Ok(Some(creds))
}

/// Delete authentication credentials
pub fn clear_credentials() -> Result<()> {
    let path = auth_file_path();
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to remove auth file: {}", path.display()))?;
    }
    Ok(())
}

/// Interactive login: prompt for provider and API key, then save
pub fn interactive_login(custom_provider: Option<String>) -> Result<AuthCredentials> {
    use std::io::{self, Write};

    let provider = if let Some(name) = custom_provider {
        // Try to match known providers
        match name.to_lowercase().as_str() {
            "deepseek" => AuthProvider::DeepSeek,
            "openai" => AuthProvider::OpenAI,
            "anthropic" => AuthProvider::Anthropic,
            "openrouter" => AuthProvider::OpenRouter,
            _ => AuthProvider::Custom(name),
        }
    } else {
        // Show provider selection
        println!("  Select LLM provider:");
        let providers = AuthProvider::all();
        for (i, p) in providers.iter().enumerate() {
            println!("    {}) {} — {} (default model: {})", i + 1, p, p.default_base_url(), p.default_model());
        }
        println!("    {}) Custom endpoint", providers.len() + 1);
        print!("  Enter number (1-{}): ", providers.len() + 1);
        let _ = io::stdout().flush();

        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        let choice = input.trim().parse::<usize>().unwrap_or(1);

        if choice == providers.len() + 1 {
            print!("  Enter custom provider name: ");
            let _ = io::stdout().flush();
            let mut name = String::new();
            io::stdin().read_line(&mut name).ok();
            AuthProvider::Custom(name.trim().to_string())
        } else if choice >= 1 && choice <= providers.len() {
            providers[choice - 1].clone()
        } else {
            AuthProvider::DeepSeek
        }
    };

    // Check environment variable first
    let env_key = match &provider {
        AuthProvider::DeepSeek => std::env::var("DEEPSEEK_API_KEY").ok(),
        AuthProvider::OpenAI => std::env::var("OPENAI_API_KEY").ok(),
        AuthProvider::Anthropic => std::env::var("ANTHROPIC_API_KEY").ok(),
        AuthProvider::OpenRouter => std::env::var("OPENROUTER_API_KEY").ok(),
        AuthProvider::Custom(_) => None,
    };

    let api_key = if let Some(key) = env_key {
        println!("   ✅ Using API key from environment variable");
        key
    } else {
        print!("  Enter API key (sk-...): ");
        let _ = io::stdout().flush();
        let mut key = String::new();
        io::stdin().read_line(&mut key).ok();
        key.trim().to_string()
    };

    let base_url = if matches!(&provider, AuthProvider::Custom(_)) {
        print!("  Enter base URL: ");
        let _ = io::stdout().flush();
        let mut url = String::new();
        io::stdin().read_line(&mut url).ok();
        url.trim().to_string()
    } else {
        provider.default_base_url().to_string()
    };

    let default_model = if matches!(&provider, AuthProvider::Custom(_)) {
        print!("  Enter default model name: ");
        let _ = io::stdout().flush();
        let mut model = String::new();
        io::stdin().read_line(&mut model).ok();
        model.trim().to_string()
    } else {
        provider.default_model().to_string()
    };

    let creds = AuthCredentials {
        provider: provider.clone(),
        api_key,
        base_url,
        default_model,
        created_at: chrono::Utc::now(),
        label: format!("{} — {}", provider, provider.default_base_url()),
    };

    save_credentials(&creds)?;
    Ok(creds)
}

/// Get the active API key (from auth file or env var)
pub fn get_active_api_key() -> Option<String> {
    // Priority: env var > auth file
    if let Ok(key) = std::env::var("HYPER_LLM_API_KEY") {
        return Some(key);
    }
    if let Ok(creds) = load_credentials() {
        if let Some(creds) = creds {
            if creds.is_valid() {
                return Some(creds.api_key);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_display() {
        assert_eq!(AuthProvider::DeepSeek.to_string(), "deepseek");
        assert_eq!(AuthProvider::OpenAI.to_string(), "openai");
    }

    #[test]
    fn test_credentials_masking() {
        let creds = AuthCredentials {
            provider: AuthProvider::DeepSeek,
            api_key: "sk-test1234567890abcdef".to_string(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            default_model: "deepseek-v4-flash".to_string(),
            created_at: chrono::Utc::now(),
            label: "test".to_string(),
        };
        let masked = creds.masked_api_key();
        assert!(masked.contains("sk-test"));
        assert!(masked.contains("cdef"));
        assert!(!masked.contains("1234567890abcdef"));
    }

    #[test]
    fn test_credentials_valid() {
        let creds = AuthCredentials {
            provider: AuthProvider::OpenAI,
            api_key: "sk-validkey123".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            default_model: "gpt-4o".to_string(),
            created_at: chrono::Utc::now(),
            label: "test".to_string(),
        };
        assert!(creds.is_valid());
    }

    #[test]
    fn test_credentials_invalid() {
        let creds = AuthCredentials {
            provider: AuthProvider::OpenAI,
            api_key: "short".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            default_model: "gpt-4o".to_string(),
            created_at: chrono::Utc::now(),
            label: "test".to_string(),
        };
        assert!(!creds.is_valid());
    }
}
