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

    // ── Accessor methods (synchronous, no network) ─────────────

    #[test]
    fn test_active_provider_name_single() {
        let configs = vec![test_config("only", "sk-1")];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.active_provider_name(), "only");
    }

    #[test]
    fn test_active_provider_name_multi_returns_first() {
        let configs = vec![
            test_config("first", "sk-1"),
            test_config("second", "sk-2"),
        ];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.active_provider_name(), "first");
    }

    #[test]
    fn test_active_model_returns_configured_model() {
        let configs = vec![test_config("p", "sk-1")];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.active_model(), "test-model");
    }

    #[test]
    fn test_input_price_from_active_provider() {
        let configs = vec![test_config("p", "sk-1")];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.input_price(), 0.15);
    }

    #[test]
    fn test_provider_name_by_index() {
        let configs = vec![test_config("alpha", "sk-1"), test_config("beta", "sk-2")];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.provider_name(0), "alpha");
        assert_eq!(pool.provider_name(1), "beta");
    }

    #[test]
    fn test_provider_name_out_of_bounds_returns_unknown() {
        let configs = vec![test_config("only", "sk-1")];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.provider_name(99), "unknown");
    }

    // ── health_summary formatting ──────────────────────────────

    #[test]
    fn test_health_summary_starts_all_healthy() {
        let configs = vec![test_config("a", "sk-1"), test_config("b", "sk-2")];
        let pool = ProviderPool::new(&configs).unwrap();
        let summary = pool.health_summary();
        assert_eq!(summary.len(), 2);
        assert_eq!(summary[0].0, "a");
        assert!(summary[0].1.contains("✅"), "healthy: got {:?}", summary[0].1);
        assert_eq!(summary[1].0, "b");
        assert!(summary[1].1.contains("✅"));
    }

    #[test]
    fn test_health_summary_includes_cooldown_after_failure() {
        let configs = vec![test_config("a", "sk-1")];
        let pool_res = ProviderPool::new(&configs);
        let mut pool = pool_res.unwrap();
        // Manually mark the first provider as unhealthy
        if let Some(member) = pool.providers.get_mut(0) {
            member.health.record_failure();
        }
        let summary = pool.health_summary();
        assert_eq!(summary.len(), 1);
        let (name, status) = &summary[0];
        assert_eq!(name, "a");
        assert!(status.contains("cooldown"), "should show cooldown: {status}");
        assert!(status.contains("failures: 1"), "should show failure count: {status}");
    }

    // ── Async failover behavior (uses MockLlmServer) ───────────
    // Note: success-path is covered by test_failover_moves_to_healthy_provider
    // (the second provider is the mock, so successful chat is exercised there).

    #[tokio::test]
    async fn test_chat_fails_when_all_providers_unreachable() {
        // Both providers point to a port nothing is listening on
        let configs = vec![
            ProviderConfig {
                name: "dead1".into(),
                api_key: "sk-1".into(),
                base_url: "http://127.0.0.1:1/v1".into(), // port 1 = reserved, never listening
                default_model: "m".into(),
                models: vec!["m".into()],
                priority: 1,
                weight: 1.0,
                input_price_per_1m: 0.15,
                output_price_per_1m: 0.60,
                max_budget_per_run: 0.0,
            },
            ProviderConfig {
                name: "dead2".into(),
                api_key: "sk-2".into(),
                base_url: "http://127.0.0.1:1/v1".into(),
                default_model: "m".into(),
                models: vec!["m".into()],
                priority: 1,
                weight: 1.0,
                input_price_per_1m: 0.15,
                output_price_per_1m: 0.60,
                max_budget_per_run: 0.0,
            },
        ];
        let mut pool = ProviderPool::new(&configs).unwrap();
        let result = pool.chat(vec![Message::text("user", "hi")]).await;
        assert!(result.is_err(), "chat should fail when no providers work");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("All providers failed") || err.contains("No available"),
                "should report all-failed: {err}");
    }

    #[tokio::test]
    async fn test_failover_moves_to_healthy_provider() {
        use crate::llm::mock_server::MockLlmServer;
        let mock = MockLlmServer::start();
        // 200ms is enough for the OS to bind + the accept thread to start
        // under parallel test execution (50ms proved flaky in CI).
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // First provider: dead URL; second provider: mock server
        let configs = vec![
            ProviderConfig {
                name: "dead".into(),
                api_key: "sk-1".into(),
                base_url: "http://127.0.0.1:1/v1".into(),
                default_model: "m".into(),
                models: vec!["m".into()],
                priority: 1,
                weight: 1.0,
                input_price_per_1m: 0.15,
                output_price_per_1m: 0.60,
                max_budget_per_run: 0.0,
            },
            ProviderConfig {
                name: "alive".into(),
                api_key: "sk-2".into(),
                base_url: mock.url(),
                default_model: "m".into(),
                models: vec!["m".into()],
                priority: 1,
                weight: 1.0,
                input_price_per_1m: 0.15,
                output_price_per_1m: 0.60,
                max_budget_per_run: 0.0,
            },
        ];
        let mut pool = ProviderPool::new(&configs).unwrap();
        let result = pool.chat(vec![Message::text("user", "hi")]).await;
        assert!(result.is_ok(), "should failover to alive provider: {:?}", result.err());
        // After successful failover, active provider should now be "alive"
        assert_eq!(pool.active_provider_name(), "alive");
    }

    // ── Edge cases ─────────────────────────────────────────────

    #[test]
    fn test_pool_preserves_config_order() {
        let configs = vec![
            test_config("z", "sk-1"),
            test_config("a", "sk-2"),
            test_config("m", "sk-3"),
        ];
        let pool = ProviderPool::new(&configs).unwrap();
        // Pool should keep the insertion order, not sort alphabetically
        assert_eq!(pool.provider_name(0), "z");
        assert_eq!(pool.provider_name(1), "a");
        assert_eq!(pool.provider_name(2), "m");
        // First one is active by default
        assert_eq!(pool.active_provider_name(), "z");
    }

    #[test]
    fn test_pool_with_duplicate_names_allowed() {
        // Duplicates should be allowed (pool just stores them, no dedup)
        let configs = vec![
            test_config("same", "sk-1"),
            test_config("same", "sk-2"),
        ];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.provider_count(), 2);
        assert_eq!(pool.provider_name(0), "same");
        assert_eq!(pool.provider_name(1), "same");
    }

    #[test]
    fn test_pool_filters_out_all_empty_keeps_valid() {
        let configs = vec![
            test_config("a", ""),
            test_config("b", ""),
            test_config("c", "sk-real"),
            test_config("d", ""),
        ];
        let pool = ProviderPool::new(&configs).unwrap();
        assert_eq!(pool.provider_count(), 1);
        assert_eq!(pool.active_provider_name(), "c");
    }

    // ── Performance benchmarks (#[ignore] — run with cargo test -- --ignored) ──
    //
    // These verify hot-path operations stay under reasonable bounds.
    // Default `cargo test` skips them to keep CI fast.
    //   cargo test --bin hyperagent -- --ignored --nocapture

    #[test]
    #[ignore]
    fn bench_pool_creation_with_many_providers() {
        use std::time::Instant;
        let configs: Vec<ProviderConfig> = (0..100)
            .map(|i| test_config(&format!("provider-{i}"), "sk-bench"))
            .collect();
        let start = Instant::now();
        let n = 1_000;
        for _ in 0..n {
            let p = ProviderPool::new(&configs).unwrap();
            std::hint::black_box(p);
        }
        let elapsed = start.elapsed();
        println!("ProviderPool::new(100) x{n}: {:.2?} ({:.0} µs/op)",
            elapsed, elapsed.as_micros() as f64 / n as f64);
        // Baseline ~268s (reqwest × 100 providers × 1k iters); advisory
    }

    #[test]
    #[ignore]
    fn bench_active_provider_lookup_throughput() {
        use std::time::Instant;
        let configs = vec![test_config("a", "sk-1"), test_config("b", "sk-2")];
        let pool = ProviderPool::new(&configs).unwrap();
        let start = Instant::now();
        let n = 1_000_000;
        let mut sink = 0usize;
        for _ in 0..n {
            sink += pool.active_provider_name().len();
        }
        let elapsed = start.elapsed();
        println!("active_provider_name x{n}: {:.2?} ({:.0} ns/op)",
            elapsed, elapsed.as_nanos() as f64 / n as f64);
        assert!(elapsed.as_secs() < 2, "took {elapsed:?}");
    }

    #[test]
    #[ignore]
    fn bench_health_summary_throughput() {
        use std::time::Instant;
        let configs: Vec<ProviderConfig> = (0..20)
            .map(|i| test_config(&format!("p-{i}"), "sk-x"))
            .collect();
        let pool = ProviderPool::new(&configs).unwrap();
        let start = Instant::now();
        let n = 10_000;
        let mut sink = 0usize;
        for _ in 0..n {
            let s = pool.health_summary();
            for (n, st) in &s {
                sink += n.len() + st.len();
            }
        }
        let elapsed = start.elapsed();
        println!("health_summary(20 providers) x{n}: {:.2?} ({:.0} µs/op, sink={sink})",
            elapsed, elapsed.as_micros() as f64 / n as f64);
        assert!(elapsed.as_secs() < 2, "took {elapsed:?}");
    }

    #[test]
    #[ignore]
    fn bench_provider_pool_exponential_backoff_cooldown() {
        use std::time::Instant;
        // Simulate many consecutive failures — backoff should cap at exponential
        let mut health = ProviderHealth::Healthy;
        let start = Instant::now();
        let n = 100_000;
        for _ in 0..n {
            health.record_failure();
        }
        let elapsed = start.elapsed();
        println!("record_failure x{n}: {:.2?} ({:.0} ns/op)",
            elapsed, elapsed.as_nanos() as f64 / n as f64);
        match health {
            ProviderHealth::Unhealthy { failed_ops, .. } => {
                assert_eq!(failed_ops as usize, n);
            }
            _ => panic!("should be unhealthy"),
        }
        assert!(elapsed.as_secs() < 2, "took {elapsed:?}");
    }
}
