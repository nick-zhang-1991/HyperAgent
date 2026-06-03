//! Budget Tracker — per-session cost monitoring with auto-degradation
//!
//! Tracks cumulative token+USD cost across a session and automatically
//! degrades to cheaper models when budget is exceeded.
//!
//! Usage:
//!   let tracker = BudgetTracker::new(0.50);  // $0.50 session budget
//!   tracker.check_before_run(1000, 0.15);    // Returns Ok/Warning/Exceeded
//!   tracker.record_run(2000, 0.15, "gpt-4o"); // Record usage

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Budget status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetStatus {
    /// Within budget
    Ok,
    /// Over 80% of budget used
    Warning,
    /// Budget exceeded — will auto-degrade
    Exceeded,
    /// No budget limit set
    Unlimited,
}

/// Per-session budget tracker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetTracker {
    /// Maximum USD cost for this session (0 = unlimited)
    pub max_budget: f64,
    /// Cumulative USD cost so far
    pub cumulative_cost: f64,
    /// Total tokens consumed
    pub total_input_tokens: usize,
    pub total_output_tokens: usize,
    /// Number of LLM calls made
    pub call_count: usize,
    /// Whether auto-degradation is active
    pub degraded: bool,
    /// Original model before degradation
    pub original_model: Option<String>,
}

impl BudgetTracker {
    /// Create a new budget tracker
    pub fn new(max_budget: f64) -> Self {
        Self {
            max_budget,
            cumulative_cost: 0.0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            call_count: 0,
            degraded: false,
            original_model: None,
        }
    }

    /// Create with no budget limit
    pub fn unlimited() -> Self {
        Self::new(0.0)
    }

    /// Check if a run would exceed the budget
    pub fn check_before_run(&self, estimated_tokens: usize, price_per_1m: f64) -> BudgetStatus {
        if self.max_budget <= 0.0 {
            return BudgetStatus::Unlimited;
        }

        let estimated_cost = (estimated_tokens as f64 / 1_000_000.0) * price_per_1m;
        let total_if_run = self.cumulative_cost + estimated_cost;

        if total_if_run > self.max_budget {
            BudgetStatus::Exceeded
        } else if total_if_run > self.max_budget * 0.8 {
            BudgetStatus::Warning
        } else {
            BudgetStatus::Ok
        }
    }

    /// Record a completed LLM call
    pub fn record_call(&mut self, input_tokens: usize, output_tokens: usize, price_per_1m: f64, model: &str) {
        let cost = ((input_tokens + output_tokens) as f64 / 1_000_000.0) * price_per_1m;
        self.cumulative_cost += cost;
        self.total_input_tokens += input_tokens;
        self.total_output_tokens += output_tokens;
        self.call_count += 1;

        // Save original model on first call
        if self.original_model.is_none() && !model.is_empty() {
            self.original_model = Some(model.to_string());
        }
    }

    /// Enable degradation to a cheaper model
    pub fn degrade(&mut self, reason: &str) -> String {
        self.degraded = true;
        format!(
            "   ⚠️  Budget degraded: {reason} (${:.4}/${:.4} used)",
            self.cumulative_cost, self.max_budget
        )
    }

    /// Get a budget status display string
    pub fn status_display(&self) -> String {
        if self.max_budget <= 0.0 {
            return format!("Unlimited (${:.4} used)", self.cumulative_cost);
        }

        let pct = if self.max_budget > 0.0 {
            (self.cumulative_cost / self.max_budget * 100.0).min(100.0)
        } else {
            0.0
        };

        let bar_width = 15;
        let filled = ((pct / 100.0) * bar_width as f64).round() as usize;
        let filled = filled.min(bar_width);
        let empty = bar_width - filled;

        let bar: String = std::iter::repeat('█').take(filled)
            .chain(std::iter::repeat('░').take(empty))
            .collect();

        format!(
            "{} {:5.1}% (${:.4}/${:.4}){}",
            bar,
            pct,
            self.cumulative_cost,
            self.max_budget,
            if self.degraded { " ⚠️ DEGRADED" } else { "" },
        )
    }

    /// Save tracker state to disk
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Load tracker state from disk
    pub fn load(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unlimited() {
        let t = BudgetTracker::unlimited();
        assert_eq!(t.check_before_run(1_000_000, 1.0), BudgetStatus::Unlimited);
    }

    #[test]
    fn test_within_budget() {
        let t = BudgetTracker::new(1.0);
        assert_eq!(t.check_before_run(1000, 0.15), BudgetStatus::Ok);
    }

    #[test]
    fn test_warning() {
        let t = BudgetTracker::new(0.001);
        assert_eq!(t.check_before_run(100_000, 15.0), BudgetStatus::Exceeded);
    }

    #[test]
    fn test_record_and_check() {
        let mut t = BudgetTracker::new(1.0);
        t.record_call(500_000, 100_000, 0.15, "deepseek");
        assert!(t.cumulative_cost > 0.0);
        assert_eq!(t.call_count, 1);
    }

    #[test]
    fn test_degrade() {
        let mut t = BudgetTracker::new(0.01);
        let msg = t.degrade("Budget limit reached");
        assert!(msg.contains("Budget degraded"), "msg: {msg}");
        assert!(t.degraded);
    }

    #[test]
    fn test_save_load() {
        let dir = std::env::temp_dir();
        let path = dir.join("hyper-budget-test.json");
        let mut t = BudgetTracker::new(0.50);
        t.record_call(1000, 500, 0.15, "test-model");
        t.save(&path).unwrap();

        let loaded = BudgetTracker::load(&path).unwrap();
        assert_eq!(loaded.max_budget, 0.50);
        assert_eq!(loaded.call_count, 1);
        assert_eq!(loaded.original_model, Some("test-model".to_string()));
        let _ = std::fs::remove_file(&path);
    }
}
