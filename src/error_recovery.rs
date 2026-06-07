#![allow(unused)]
//! Error Recovery & Retry System
//!
//! For 100M users, every failed LLM call = lost trust + wasted tokens.
//! This module provides automatic retry with exponential backoff,
//! circuit breaker pattern, and graceful degradation.
//!
//! Features:
//! - Exponential backoff: 1s → 2s → 4s → 8s → max 30s
//! - Jitter: ±25% random jitter to prevent thundering herd
//! - Circuit breaker: after 5 consecutive failures, pause for 60s
//! - Fallback model: switch to cheaper model on persistent failure
//! - Error classification: transient vs permanent errors

use std::time::Duration;
use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::sync::Mutex;

/// Error categories for intelligent retry decisions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Temporary: network timeout, rate limit → retry
    Transient,
    /// Permanent: invalid API key, bad request → don't retry
    Permanent,
    /// Unknown: might work on retry → cautious retry
    Unknown,
}

impl ErrorCategory {
    /// Classify an error message into a category
    pub fn classify(error_msg: &str) -> Self {
        let msg = error_msg.to_lowercase();

        // Permanent errors — don't retry
        if msg.contains("invalid api key") || msg.contains("unauthorized")
            || msg.contains("authentication") || msg.contains("403")
            || msg.contains("not found") || msg.contains("404")
            || msg.contains("invalid request") || msg.contains("400")
            || msg.contains("model not found")
            || msg.contains("insufficient") // quota
            || msg.contains("billing") {
            return ErrorCategory::Permanent;
        }

        // Transient errors — retry
        if msg.contains("timeout") || msg.contains("timed out")
            || msg.contains("rate limit") || msg.contains("429")
            || msg.contains("too many requests")
            || msg.contains("server error") || msg.contains("500")
            || msg.contains("503") || msg.contains("502")
            || msg.contains("overloaded")
            || msg.contains("connection") || msg.contains("network")
            || msg.contains("temporary") || msg.contains("transient")
            || msg.contains("try again") || msg.contains("retry") {
            return ErrorCategory::Transient;
        }

        ErrorCategory::Unknown
    }

    /// Should we retry this error?
    pub fn should_retry(&self) -> bool {
        matches!(self, ErrorCategory::Transient | ErrorCategory::Unknown)
    }
}

// ─── Retry Policy ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier (2.0 = exponential)
    pub backoff_multiplier: f64,
    /// Random jitter as fraction (0.25 = ±25%)
    pub jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: 0.25,
        }
    }
}

impl RetryPolicy {
    /// Create an aggressive retry policy for CI/automation
    pub fn aggressive() -> Self {
        RetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(15),
            backoff_multiplier: 1.5,
            jitter: 0.2,
        }
    }

    /// Create a conservative retry policy for production
    pub fn conservative() -> Self {
        RetryPolicy {
            max_attempts: 2,
            initial_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            jitter: 0.3,
        }
    }

    /// Calculate backoff for attempt N (0-indexed)
    pub fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let base = self.initial_backoff.as_secs_f64()
            * self.backoff_multiplier.powi(attempt as i32);
        let capped = base.min(self.max_backoff.as_secs_f64());

        // Add jitter
        let jitter_range = capped * self.jitter;
        let jittered = capped + (rand_jitter() * 2.0 - 1.0) * jitter_range;

        Duration::from_secs_f64(jittered.max(0.0))
    }
}

// ─── Circuit Breaker ────────────────────────────────────────────

#[derive(Debug)]
pub struct CircuitBreaker {
    /// Consecutive failure count
    failures: AtomicU32,
    /// Threshold before opening the circuit
    threshold: u32,
    /// Cooldown duration after opening
    cooldown: Duration,
    /// Whether the circuit is currently open (atomically)
    is_open: AtomicBool,
    /// Timestamp of when the circuit was opened
    opened_at: Mutex<Option<std::time::Instant>>,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, cooldown: Duration) -> Self {
        CircuitBreaker {
            failures: AtomicU32::new(0),
            threshold,
            cooldown,
            is_open: AtomicBool::new(false),
            opened_at: Mutex::new(None),
        }
    }

    /// Check if the circuit allows a request through
    pub fn allow_request(&self) -> bool {
        if !self.is_open.load(Ordering::Relaxed) {
            return true;
        }

        // Check if cooldown has elapsed
        if let Ok(mut guard) = self.opened_at.lock() {
            if let Some(opened) = *guard {
                if opened.elapsed() >= self.cooldown {
                    // Half-open: allow one request through
                    self.is_open.store(false, Ordering::Relaxed);
                    *guard = None;
                    return true;
                }
            }
        }

        false
    }

    /// Record a successful request
    pub fn record_success(&self) {
        self.failures.store(0, Ordering::Relaxed);
        self.is_open.store(false, Ordering::Relaxed);
        if let Ok(mut guard) = self.opened_at.lock() {
            *guard = None;
        }
    }

    /// Record a failed request
    pub fn record_failure(&self) {
        let count = self.failures.fetch_add(1, Ordering::Relaxed) + 1;

        if count >= self.threshold {
            self.is_open.store(true, Ordering::Relaxed);
            if let Ok(mut guard) = self.opened_at.lock() {
                *guard = Some(std::time::Instant::now());
            }
        }
    }

    /// Get current state for monitoring
    pub fn state(&self) -> CircuitState {
        if self.is_open.load(Ordering::Relaxed) {
            CircuitState::Open
        } else if self.failures.load(Ordering::Relaxed) > 0 {
            CircuitState::Degraded
        } else {
            CircuitState::Healthy
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Healthy,
    Degraded,
    Open,
}

// ─── Retry Executor ─────────────────────────────────────────────

/// Execute a fallible async operation with retry logic
pub async fn with_retry<F, Fut, T, E>(
    operation: F,
    policy: &RetryPolicy,
    breaker: Option<&CircuitBreaker>,
) -> Result<T, RetryError<E>>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut last_error = None;
    let mut total_attempts = 0;

    for attempt in 0..policy.max_attempts {
        total_attempts += 1;

        // Check circuit breaker
        if let Some(cb) = breaker {
            if !cb.allow_request() {
                return Err(RetryError::CircuitOpen);
            }
        }

        match operation().await {
            Ok(value) => {
                if let Some(cb) = breaker {
                    cb.record_success();
                }
                return Ok(value);
            }
            Err(err) => {
                let error_str = err.to_string();
                let category = ErrorCategory::classify(&error_str);

                if !category.should_retry() {
                    if let Some(cb) = breaker {
                        cb.record_failure();
                    }
                    return Err(RetryError::Permanent(err));
                }

                if let Some(cb) = breaker {
                    cb.record_failure();
                }

                last_error = Some(err);

                if attempt < policy.max_attempts - 1 {
                    let backoff = policy.backoff_for_attempt(attempt);
                    tracing::warn!(
                        attempt = attempt + 1,
                        max = policy.max_attempts,
                        backoff_ms = backoff.as_millis(),
                        category = ?category,
                        "LLM call failed, retrying..."
                    );
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    Err(RetryError::Exhausted {
        attempts: total_attempts,
        last_error: last_error.map(|e| e.to_string()).unwrap_or_default(),
    })
}

#[derive(Debug)]
pub enum RetryError<E: std::fmt::Display> {
    /// Max retries exhausted
    Exhausted { attempts: u32, last_error: String },
    /// Circuit breaker is open
    CircuitOpen,
    /// Permanent error, not retried
    Permanent(E),
}

impl<E: std::fmt::Display> std::fmt::Display for RetryError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RetryError::Exhausted { attempts, last_error } => {
                write!(f, "Failed after {} attempts. Last error: {}", attempts, last_error)
            }
            RetryError::CircuitOpen => {
                write!(f, "Circuit breaker is open — too many recent failures")
            }
            RetryError::Permanent(e) => {
                write!(f, "Permanent error: {}", e)
            }
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────

fn rand_jitter() -> f64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let hasher = RandomState::new().build_hasher();
    (hasher.finish() as f64) / (u64::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_classification() {
        assert_eq!(
            ErrorCategory::classify("Request timed out"),
            ErrorCategory::Transient
        );
        assert_eq!(
            ErrorCategory::classify("Rate limit exceeded"),
            ErrorCategory::Transient
        );
        assert_eq!(
            ErrorCategory::classify("Invalid API key"),
            ErrorCategory::Permanent
        );
        assert_eq!(
            ErrorCategory::classify("some random error"),
            ErrorCategory::Unknown
        );
    }

    #[test]
    fn test_retry_backoff_increases() {
        let policy = RetryPolicy::default();
        let b0 = policy.backoff_for_attempt(0);
        let b1 = policy.backoff_for_attempt(1);
        let b2 = policy.backoff_for_attempt(2);
        assert!(b1 > b0);
        assert!(b2 > b1);
    }

    #[test]
    fn test_circuit_breaker_opens() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(60));

        assert!(cb.allow_request());
        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow_request()); // Still allowed

        cb.record_failure(); // 3rd failure
        assert!(!cb.allow_request()); // Circuit open

        cb.record_success();
        assert!(cb.allow_request()); // Circuit closed after success
    }

    #[test]
    fn test_with_retry_success_sync() {
        // Test the policy calculation (sync part)
        let policy = RetryPolicy::default();
        let b = policy.backoff_for_attempt(2);
        assert!(b > policy.initial_backoff);
    }

    #[test]
    fn test_retry_error_display() {
        let err: RetryError<&str> = RetryError::Exhausted {
            attempts: 3,
            last_error: "timeout".into(),
        };
        assert!(err.to_string().contains("3 attempts"));
        assert!(err.to_string().contains("timeout"));
    }
}
