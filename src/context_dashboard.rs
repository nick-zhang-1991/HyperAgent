//! Context Dashboard — token usage visualization at the end of each run
//!
//! Shows:
//! - Context window utilization as a progress bar
//! - Per-phase token costs (plan, code, review)
//! - File contribution breakdown
//! - Estimated USD cost
//!
//! Inspired by Claude Code's "Used X/Y tokens (73%)" display.

use serde::Serialize;
use std::time::Duration;

/// Token usage tracked per phase
#[derive(Debug, Default, Clone, Serialize)]
pub struct PhaseUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

/// Complete context dashboard for a run
#[derive(Debug, Default, Clone, Serialize)]
pub struct ContextDashboard {
    pub max_context: usize,
    pub total_input: usize,
    pub total_output: usize,
    pub phase_plan: PhaseUsage,
    pub phase_code: Option<PhaseUsage>,
    pub phase_review: Option<PhaseUsage>,
    pub files_full: usize,
    pub files_truncated: usize,
    pub total_files: usize,
    pub elapsed: Duration,
    pub cost_estimate: f64,
}

impl ContextDashboard {
    pub fn new(max_context: usize) -> Self {
        Self {
            max_context,
            ..Default::default()
        }
    }

    /// Add plan phase usage
    pub fn add_plan(&mut self, input: usize, output: usize) {
        self.phase_plan = PhaseUsage { input_tokens: input, output_tokens: output };
        self.total_input += input;
        self.total_output += output;
    }

    /// Add code phase usage
    pub fn add_code(&mut self, input: usize, output: usize) {
        self.phase_code = Some(PhaseUsage { input_tokens: input, output_tokens: output });
        self.total_input += input;
        self.total_output += output;
    }

    /// Add review phase usage
    pub fn add_review(&mut self, input: usize, output: usize) {
        self.phase_review = Some(PhaseUsage { input_tokens: input, output_tokens: output });
        self.total_input += input;
        self.total_output += output;
    }

    /// Set file context info
    pub fn set_files(&mut self, full: usize, truncated: usize, total: usize) {
        self.files_full = full;
        self.files_truncated = truncated;
        self.total_files = total;
    }

    /// Set elapsed time
    pub fn set_elapsed(&mut self, elapsed: Duration) {
        self.elapsed = elapsed;
    }

    /// Set cost estimate
    pub fn set_cost(&mut self, cost: f64) {
        self.cost_estimate = cost;
    }

    /// Get context utilization percentage
    pub fn utilization_pct(&self) -> f64 {
        if self.max_context == 0 { return 0.0; }
        (self.total_input as f64 / self.max_context as f64) * 100.0
    }

    /// Render a visual progress bar for context utilization
    pub fn context_bar(&self) -> String {
        let pct = self.utilization_pct();
        let width = 20;
        let filled = ((pct / 100.0) * width as f64).round() as usize;
        let filled = filled.min(width);
        let empty = width - filled;

        let bar: String = std::iter::repeat('█').take(filled)
            .chain(std::iter::repeat('░').take(empty))
            .collect();

        format!("{} {:5.1}% ({}/{}K)", bar, pct, self.total_input / 1000, self.max_context / 1000)
    }

    /// Render the full dashboard to a string
    pub fn render(&self) -> String {
        let mut output = String::new();
        output.push_str("   📊 Context Dashboard\n");
        output.push_str(&format!("   {}\n", self.context_bar()));

        // Per-phase breakdown
        output.push_str(&format!(
            "   📝 Plan:    {:>6} in + {:>6} out = {:>6} total\n",
            self.phase_plan.input_tokens,
            self.phase_plan.output_tokens,
            self.phase_plan.input_tokens + self.phase_plan.output_tokens,
        ));

        if let Some(ref code) = self.phase_code {
            output.push_str(&format!(
                "   👨‍💻 Code:    {:>6} in + {:>6} out = {:>6} total\n",
                code.input_tokens,
                code.output_tokens,
                code.input_tokens + code.output_tokens,
            ));
        }

        if let Some(ref review) = self.phase_review {
            output.push_str(&format!(
                "   🔎 Review:  {:>6} in + {:>6} out = {:>6} total\n",
                review.input_tokens,
                review.output_tokens,
                review.input_tokens + review.output_tokens,
            ));
        }

        // Total
        output.push_str(&format!(
            "   ─────────────────────────────────────\n\
               📦 Total:   {:>6} in + {:>6} out = {:>6} total\n",
            self.total_input,
            self.total_output,
            self.total_input + self.total_output,
        ));

        // File context
        if self.total_files > 0 {
            output.push_str(&format!(
                "   📁 Files:   {} ({} full, {} truncated)\n",
                self.total_files, self.files_full, self.files_truncated,
            ));
        }

        // Cost
        if self.cost_estimate > 0.0 {
            output.push_str(&format!("   💰 Cost:    ${:.4}\n", self.cost_estimate));
        }

        output.push_str(&format!("   ⏱️  Time:    {:.1}s\n", self.elapsed.as_secs_f64()));

        output
    }
}

/// Estimate tokens from character count (rough: 4 chars ≈ 1 token)
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}
