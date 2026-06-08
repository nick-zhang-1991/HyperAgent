//! Prometheus metrics for HyperAgent.
//!
//! Simple in-memory counters exposed as Prometheus text format
//! for scraping by Prometheus or any metrics collector.
//!
//! Usage:
//!   Metrics::increment_requests();
//!   Metrics::record_tokens(150);
//!   let text = Metrics::render();

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Global metrics registry with atomic counters (lock-free).
pub struct Metrics;

static REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static TOKENS_INPUT: AtomicU64 = AtomicU64::new(0);
static TOKENS_OUTPUT: AtomicU64 = AtomicU64::new(0);
static ERRORS_TOTAL: AtomicU64 = AtomicU64::new(0);
static AGENT_RUNS: AtomicU64 = AtomicU64::new(0);
static FILES_MODIFIED: AtomicU64 = AtomicU64::new(0);
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

static START_TIME: AtomicU64 = AtomicU64::new(0);

impl Metrics {
    /// Initialize start time (called once at startup).
    pub fn init() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        START_TIME.store(now, Ordering::Relaxed);
    }

    pub fn increment_requests() {
        REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_errors() {
        ERRORS_TOTAL.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tokens(input: u64, output: u64) {
        TOKENS_INPUT.fetch_add(input, Ordering::Relaxed);
        TOKENS_OUTPUT.fetch_add(output, Ordering::Relaxed);
    }

    pub fn increment_agent_runs() {
        AGENT_RUNS.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_files_modified(count: u64) {
        FILES_MODIFIED.fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_cache(was_hit: bool) {
        if was_hit {
            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
        } else {
            CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Render all metrics in Prometheus text exposition format.
    pub fn render() -> String {
        let uptime = uptime_secs();
        let hits = CACHE_HITS.load(Ordering::Relaxed);
        let misses = CACHE_MISSES.load(Ordering::Relaxed);
        let total = hits + misses;
        let ratio = if total > 0 {
            format!("{:.2}", hits as f64 / total as f64)
        } else {
            "0.00".to_string()
        };

        format!(
            "# HELP hyperagent_uptime_seconds Server uptime in seconds\n\
             # TYPE hyperagent_uptime_seconds gauge\n\
             hyperagent_uptime_seconds {}\n\
             \n\
             # HELP hyperagent_requests_total Total HTTP requests handled\n\
             # TYPE hyperagent_requests_total counter\n\
             hyperagent_requests_total {}\n\
             \n\
             # HELP hyperagent_errors_total Total errors encountered\n\
             # TYPE hyperagent_errors_total counter\n\
             hyperagent_errors_total {}\n\
             \n\
             # HELP hyperagent_tokens_input_total Total input tokens processed\n\
             # TYPE hyperagent_tokens_input_total counter\n\
             hyperagent_tokens_input_total {}\n\
             \n\
             # HELP hyperagent_tokens_output_total Total output tokens generated\n\
             # TYPE hyperagent_tokens_output_total counter\n\
             hyperagent_tokens_output_total {}\n\
             \n\
             # HELP hyperagent_agent_runs_total Total agent runs executed\n\
             # TYPE hyperagent_agent_runs_total counter\n\
             hyperagent_agent_runs_total {}\n\
             \n\
             # HELP hyperagent_files_modified_total Total files modified by agent\n\
             # TYPE hyperagent_files_modified_total counter\n\
             hyperagent_files_modified_total {}\n\
             \n\
             # HELP hyperagent_cache_hits_total Total LLM response cache hits\n\
             # TYPE hyperagent_cache_hits_total counter\n\
             hyperagent_cache_hits_total {}\n\
             \n\
             # HELP hyperagent_cache_misses_total Total LLM response cache misses\n\
             # TYPE hyperagent_cache_misses_total counter\n\
             hyperagent_cache_misses_total {}\n\
             \n\
             # HELP hyperagent_cache_hit_ratio Cache hit ratio (0.0-1.0)\n\
             # TYPE hyperagent_cache_hit_ratio gauge\n\
             hyperagent_cache_hit_ratio {}\n",
            uptime,
            REQUESTS_TOTAL.load(Ordering::Relaxed),
            ERRORS_TOTAL.load(Ordering::Relaxed),
            TOKENS_INPUT.load(Ordering::Relaxed),
            TOKENS_OUTPUT.load(Ordering::Relaxed),
            AGENT_RUNS.load(Ordering::Relaxed),
            FILES_MODIFIED.load(Ordering::Relaxed),
            hits,
            misses,
            ratio,
        )
    }

    /// Returns JSON health check data.
    pub fn health_json() -> String {
        let uptime = uptime_secs();
        format!(
            r#"{{"status":"ok","uptime_seconds":{},"requests_total":{},"errors_total":{}}}"#,
            uptime,
            REQUESTS_TOTAL.load(Ordering::Relaxed),
            ERRORS_TOTAL.load(Ordering::Relaxed),
        )
    }
}

fn uptime_secs() -> u64 {
    let start = START_TIME.load(Ordering::Relaxed);
    if start == 0 {
        return 0;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_init() {
        Metrics::init();
        let rendered = Metrics::render();
        assert!(rendered.contains("hyperagent_uptime_seconds"));
        assert!(rendered.contains("hyperagent_requests_total"));
        assert!(rendered.contains("hyperagent_errors_total"));
    }

    #[test]
    fn test_metrics_counters() {
        Metrics::increment_requests();
        Metrics::increment_errors();
        Metrics::record_tokens(100, 50);
        Metrics::increment_agent_runs();
        Metrics::record_files_modified(3);
        Metrics::record_cache(true);
        Metrics::record_cache(false);

        let rendered = Metrics::render();
        assert!(rendered.contains("hyperagent_requests_total 1"));
        assert!(rendered.contains("hyperagent_cache_hit_ratio 0.50"));
    }

    #[test]
    fn test_health_json() {
        let json = Metrics::health_json();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"uptime_seconds\""));
    }
}
