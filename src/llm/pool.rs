#![allow(unused)]
//! ProviderPool — multi-provider failover with cooldown tracking
//!
//! Automatically falls back to the next provider when the primary fails.
//! Failed providers enter a cooldown period to avoid repeated failures.

use crate::llm::{LlmProvider, Message};
use crate::llm::streaming::StreamingResponse;
use crate::router::ProviderConfig;
use anyhow::Result;
use std::time::{Duration, Instant};

/// Health status of a single provider
#[derive(Debug, Clone)]
pub enum ProviderHealth {
    Healthy,
    Unhealthy { until: Instant, failed_ops: u32 },
}

impl ProviderHealth {
    fn is_available(&self) -> bool {
        match self {
            ProviderHealth::Healthy => true,
            ProviderHealth::Unhealthy { until, .. } => Instant::now() >= *until,
        }
    }

    fn record_failure(&mut self) {
        let cooldown_secs = match self {
            ProviderHealth::Healthy => 10,
            ProviderHealth::Unhealthy { failed_ops, .. } => {
                *failed_ops += 1;
                let capped = std::cmp::min(*failed_ops, 3);
                10u64.pow(capped) * 5
            }
        };
        *self = ProviderHealth::Unhealthy {
            until: Instant::now() + Duration::from_secs(cooldown_secs),
            failed_ops: match self {
                ProviderHealth::Unhealthy { failed_ops, .. } => *failed_ops,
                _ => 1,
            },
        };
    }

    fn record_success(&mut self) {
        *self = ProviderHealth::Healthy;
    }
}

/// A pool of LLM providers with automatic failover
#[derive(Debug, Clone)]
pub struct ProviderPool {
    providers: Vec<PoolMember>,
    current_index: usize,
}

#[derive(Debug, Clone)]
struct PoolMember {
    provider: LlmProvider,
    config: ProviderConfig,
    health: ProviderHealth,
}

impl ProviderPool {
    /// Create a new pool from provider configs
    pub fn new(configs: &[ProviderConfig]) -> Result<Self> {
        let members: Vec<PoolMember> = configs
            .iter()
            .filter(|c| !c.api_key.is_empty())
            .map(|config| {
                let provider = LlmProvider::new(
                    config.default_model.clone(),
                    config.base_url.clone(),
                    config.api_key.clone(),
                ).expect("Failed to create LLM provider");
                PoolMember {
                    provider,
                    config: config.clone(),
                    health: ProviderHealth::Healthy,
                }
            })
            .collect();

        if members.is_empty() {
            anyhow::bail!("No providers with valid API keys configured");
        }

        Ok(Self {
            providers: members,
            current_index: 0,
        })
    }

    /// Get the current active provider
    fn active_provider(&self) -> Option<&PoolMember> {
        // Try from current_index first, then search for any healthy provider
        let n = self.providers.len();
        for offset in 0..n {
            let idx = (self.current_index + offset) % n;
            if self.providers[idx].health.is_available() {
                return Some(&self.providers[idx]);
            }
        }
        None
    }

    fn active_provider_mut(&mut self) -> Option<&mut PoolMember> {
        let n = self.providers.len();
        for offset in 0..n {
            let idx = (self.current_index + offset) % n;
            if self.providers[idx].health.is_available() {
                self.current_index = idx;
                return Some(&mut self.providers[idx]);
            }
        }
        None
    }

    /// Chat with auto-failover on error
    pub async fn chat(&mut self, messages: Vec<Message>) -> Result<String> {
        let start_idx = self.current_index;
        let n = self.providers.len();

        for offset in 0..n {
            let idx = (start_idx + offset) % n;
            if !self.providers[idx].health.is_available() {
                continue;
            }

            self.current_index = idx;
            match self.providers[idx].provider.chat(messages.clone()).await {
                Ok(response) => {
                    self.providers[idx].health.record_success();
                    return Ok(response);
                }
                Err(e) => {
                    let err_msg = format!("{}", e);
                    self.providers[idx].health.record_failure();
                    eprintln!(
                        "   ⚠️  Provider '{}' failed ({}s cooldown): {:?}",
                        self.providers[idx].config.name,
                        match &self.providers[idx].health {
                            ProviderHealth::Unhealthy { until, .. } => {
                                until.saturating_duration_since(Instant::now()).as_secs()
                            }
                            _ => 0,
                        },
                        err_msg.lines().next().unwrap_or(&err_msg),
                    );

                    // If this was the last provider, report the error
                    if offset == n - 1 {
                        return Err(anyhow::anyhow!(
                            "All providers failed. Last error from '{}': {}",
                            self.providers[idx].config.name,
                            err_msg
                        ));
                    }
                }
            }
        }

        anyhow::bail!("No available providers")
    }

    /// Chat with streaming support and auto-failover
    pub async fn chat_stream(&mut self, messages: Vec<Message>) -> Result<StreamingResponse> {
        let start_idx = self.current_index;
        let n = self.providers.len();

        for offset in 0..n {
            let idx = (start_idx + offset) % n;
            if !self.providers[idx].health.is_available() {
                continue;
            }

            self.current_index = idx;
            match self.providers[idx].provider.chat_stream(messages.clone()).await {
                Ok(stream) => {
                    self.providers[idx].health.record_success();
                    return Ok(stream);
                }
                Err(e) => {
                    self.providers[idx].health.record_failure();
                    eprintln!(
                        "   ⚠️  Provider '{}' streaming failed, trying next...",
                        self.providers[idx].config.name,
                    );
                    if offset == n - 1 {
                        return Err(anyhow::anyhow!(
                            "All providers failed streaming. Last error: {e}",
                        ));
                    }
                }
            }
        }

        anyhow::bail!("No available providers for streaming")
    }

    /// Get the name of the current active provider
    pub fn active_provider_name(&self) -> &str {
        self.active_provider()
            .map(|m| m.config.name.as_str())
            .unwrap_or("none")
    }

    /// Get the model of the current active provider
    pub fn active_model(&self) -> &str {
        self.active_provider()
            .map(|m| m.provider.model.as_str())
            .unwrap_or("unknown")
    }

    /// Get the input price of the active provider
    pub fn input_price(&self) -> f64 {
        self.active_provider()
            .map(|m| m.config.input_price_per_1m)
            .unwrap_or(0.15)
    }

    /// Get the name at a specific index (for status display)
    pub fn provider_name(&self, idx: usize) -> &str {
        self.providers.get(idx)
            .map(|m| m.config.name.as_str())
            .unwrap_or("unknown")
    }

    /// Number of providers in pool
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Get provider health summary for display
    pub fn health_summary(&self) -> Vec<(String, String)> {
        self.providers.iter().map(|m| {
            let status = match &m.health {
                ProviderHealth::Healthy => "✅".to_string(),
                ProviderHealth::Unhealthy { until, failed_ops } => {
                    let remaining = until.saturating_duration_since(Instant::now()).as_secs();
                    format!("⏸️  cooldown {remaining}s (failures: {failed_ops})")
                }
            };
            (m.config.name.clone(), status)
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(name: &str, key: &str) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            api_key: key.to_string(),
            base_url: "http://localhost:9999/v1".to_string(),
            default_model: "test-model".to_string(),
            models: vec!["test-model".to_string()],
            priority: 1,
            weight: 1.0,
            input_price_per_1m: 0.15,
            output_price_per_1m: 0.60,
            max_budget_per_run: 0.0,
        }
    }

    #[test]
    fn test_pool_creation_with_keys() {
        let configs = vec![test_config("deepseek", "sk-test"), test_config("openai", "sk-test2")];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.provider_count(), 2);
        assert_eq!(pool.active_provider_name(), "deepseek");
    }

    #[test]
    fn test_pool_filters_empty_keys() {
        let configs = vec![
            test_config("empty", ""),
            test_config("valid", "sk-real"),
        ];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.provider_count(), 1);
        assert_eq!(pool.active_provider_name(), "valid");
    }

    #[test]
    fn test_pool_fails_with_no_keys() {
        let configs = vec![test_config("empty", "")];
        let result = ProviderPool::new(&configs);
        assert!(result.is_err());
    }

    #[test]
    fn test_health_cooldown() {
        let mut health = ProviderHealth::Healthy;
        assert!(health.is_available());

        health.record_failure();
        match &health {
            ProviderHealth::Unhealthy { failed_ops, .. } => assert_eq!(*failed_ops, 1),
            _ => panic!("Should be unhealthy"),
        }

        health.record_success();
        assert!(matches!(health, ProviderHealth::Healthy));
    }

    #[test]
    fn test_health_exponential_backoff() {
        let mut health = ProviderHealth::Healthy;
        health.record_failure(); // 1
        health.record_failure(); // 2
        health.record_failure(); // 3
        health.record_failure(); // 4 (capped at 3)

        match &health {
            ProviderHealth::Unhealthy { failed_ops, .. } => {
                assert_eq!(*failed_ops, 4); // started at 0, incremented 4 times
            }
            _ => panic!("Should be unhealthy"),
        }
    }
}
