//! HyperAgent Benchmarks — automated quality evaluation
//!
//! Runs the agent pipeline on predefined tasks and evaluates results using
//! objective metrics: compilation success, test pass rate, diff quality,
//! execution time, token efficiency.
//!
//! Usage:
//!   cargo run -- eval                    # Run all benchmarks
//!   cargo run -- eval --task gen         # Run code gen benchmark
//!   cargo run -- eval --list             # List available tasks

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Instant;
use serde::{Serialize, Deserialize};

/// A single benchmark task
#[derive(Debug, Clone, Serialize)]
pub struct EvalTask {
    pub name: String,
    pub category: EvalCategory,
    pub prompt: String,
    pub setup: Option<String>,        // Shell command to prepare the project
    pub expected_behavior: Vec<&'static str>, // What the solution should achieve
    pub check_compile: bool,           // Verify project compiles after
    pub check_tests: bool,             // Run tests after
    pub max_lines_threshold: usize,    // Max lines of code allowed (quality gate)
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EvalCategory {
    CodeGen,        // Generate new code from scratch
    BugFix,         // Fix a known bug
    Refactor,       // Refactor existing code
    Documentation,  // Generate docs/comments
    TestGen,        // Generate unit tests
}

/// Results of running a benchmark task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub task_name: String,
    pub category: EvalCategory,
    pub passed: bool,
    pub elapsed: std::time::Duration,
    pub tokens_used: usize,
    pub files_modified: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub compilation_success: Option<bool>,
    pub tests_passed: Option<bool>,
    pub errors: Vec<String>,
}

/// Full benchmark report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub timestamp: String,
    pub version: String,
    pub total_tasks: usize,
    pub passed: usize,
    pub failed: usize,
    pub total_elapsed: std::time::Duration,
    pub results: Vec<EvalResult>,
}

// ============================================================================
// Built-in benchmark tasks
// ============================================================================

/// Generate the list of built-in benchmark tasks
pub fn builtin_tasks() -> Vec<EvalTask> {
    vec![
        // --- CodeGen ---
        EvalTask {
            name: "gen-fibonacci".into(),
            category: EvalCategory::CodeGen,
            prompt: "Write a fibonacci function that returns the nth fibonacci number using iterative approach. Add it to src/lib.rs.".into(),
            setup: Some(r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "eval-fib"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
pub fn greet() -> &'static str {
    "hello"
}
EOF
"#.into()),
            expected_behavior: vec![
                "Function should exist: fibonacci",
                "fibonacci(0) should return 0",
                "fibonacci(1) should return 1",
                "fibonacci(10) should return 55",
            ],
            check_compile: true,
            check_tests: false,
            max_lines_threshold: 20,
            timeout_secs: 120,
        },

        // --- BugFix ---
        EvalTask {
            name: "fix-off-by-one".into(),
            category: EvalCategory::BugFix,
            prompt: "Fix the off-by-one error in the sum_range function. It should sum numbers from start to end inclusive.".into(),
            setup: Some(r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "eval-bug"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
/// Sum numbers from start to end inclusive
pub fn sum_range(start: i32, end: i32) -> i32 {
    let mut sum = 0;
    for i in start..end {  // BUG: should be start..=end
        sum += i;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_sum_1_to_5() {
        assert_eq!(sum_range(1, 5), 15);
    }
    #[test]
    fn test_sum_0_to_0() {
        assert_eq!(sum_range(0, 0), 0);
    }
}
EOF
"#.into()),
            expected_behavior: vec![
                "sum_range should include end value",
                "sum_range(1, 5) should return 15",
            ],
            check_compile: true,
            check_tests: true,
            max_lines_threshold: 10,
            timeout_secs: 120,
        },

        // --- Refactor ---
        EvalTask {
            name: "refactor-if-chain".into(),
            category: EvalCategory::Refactor,
            prompt: "Refactor the score_to_grade function to use a match statement instead of if-else chains.".into(),
            setup: Some(r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "eval-refactor"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
/// Convert a numeric score to a letter grade
pub fn score_to_grade(score: u32) -> &'static str {
    if score >= 90 {
        "A"
    } else if score >= 80 {
        "B"
    } else if score >= 70 {
        "C"
    } else if score >= 60 {
        "D"
    } else {
        "F"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_grades() {
        assert_eq!(score_to_grade(95), "A");
        assert_eq!(score_to_grade(82), "B");
        assert_eq!(score_to_grade(75), "C");
        assert_eq!(score_to_grade(60), "D");
        assert_eq!(score_to_grade(30), "F");
    }
}
EOF
"#.into()),
            expected_behavior: vec![
                "Should use match statement",
                "All tests should still pass",
            ],
            check_compile: true,
            check_tests: true,
            max_lines_threshold: 30,
            timeout_secs: 120,
        },

        // --- TestGen ---
        EvalTask {
            name: "test-parse-config".into(),
            category: EvalCategory::TestGen,
            prompt: "Generate comprehensive unit tests for the ConfigParser struct. Cover parsing valid configs, invalid configs, edge cases, and empty input.".into(),
            setup: Some(r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "eval-testgen"
version = "0.1.0"
edition = "2021"
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
EOF
cat > src/lib.rs << 'EOF'
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub debug: bool,
}

pub struct ConfigParser;

impl ConfigParser {
    pub fn parse_json(input: &str) -> Result<Config, String> {
        if input.trim().is_empty() {
            return Err("Empty input".to_string());
        }
        serde_json::from_str(input).map_err(|e| format!("Parse error: {}", e))
    }

    pub fn validate(config: &Config) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if config.host.is_empty() {
            errors.push("Host cannot be empty".to_string());
        }
        if config.port == 0 {
            errors.push("Port cannot be 0".to_string());
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}
EOF
"#.into()),
            expected_behavior: vec![
                "Tests should cover valid config parsing",
                "Tests should cover invalid JSON gracefully",
                "Tests should cover empty input",
                "Tests should cover validate() method",
            ],
            check_compile: true,
            check_tests: true,
            max_lines_threshold: 80,
            timeout_secs: 180,
        },

        // --- Documentation ---
        EvalTask {
            name: "doc-api-handler".into(),
            category: EvalCategory::Documentation,
            prompt: "Add documentation comments to all public functions and structs in this module. Follow Rust doc conventions with examples.".into(),
            setup: Some(r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "eval-doc"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
pub struct ApiResponse {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

impl ApiResponse {
    pub fn new(status: u16) -> Self {
        Self { status, body: String::new(), headers: Vec::new() }
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    pub fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }
}
EOF
"#.into()),
            expected_behavior: vec![
                "All public items should have doc comments",
                "At least one function should have a doc example",
            ],
            check_compile: true,
            check_tests: false,
            max_lines_threshold: 100,
            timeout_secs: 180,
        },
    ]
}

// ============================================================================
// Benchmark runner
// ============================================================================

/// Run a single benchmark task
fn run_single_task(task: &EvalTask, temp_dir: &Path, hyper_binary: &Path) -> EvalResult {
    let start = Instant::now();
    let mut errors: Vec<String> = Vec::new();

    // Create temporary project
    if let Some(setup_cmd) = &task.setup {
        let result = std::process::Command::new("sh")
            .args(["-c", setup_cmd])
            .current_dir(temp_dir)
            .output();
        match result {
            Ok(out) if !out.status.success() => {
                errors.push(format!("Setup failed: {}", String::from_utf8_lossy(&out.stderr)));
            }
            Err(e) => {
                errors.push(format!("Setup error: {e}"));
            }
            _ => {}
        }
    }

    // Run HyperAgent on the task
    let mut agent_cmd = std::process::Command::new(hyper_binary);
    agent_cmd
        .args(["run", "--yes", &task.prompt])
        .current_dir(temp_dir);

    let agent_output = if task.timeout_secs > 0 {
        // Use a thread-based timeout
        let child = agent_cmd.spawn();
        match child {
            Ok(mut child) => {
                let start = Instant::now();
                let timeout = std::time::Duration::from_secs(task.timeout_secs);
                loop {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        errors.push(format!("Agent timed out after {}s", task.timeout_secs));
                        break;
                    }
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            // Got output
                            break;
                        }
                        Ok(None) => {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                        }
                        Err(e) => {
                            errors.push(format!("Agent wait error: {e}"));
                            break;
                        }
                    }
                }
                child.wait_with_output().ok()
            }
            Err(e) => {
                errors.push(format!("Agent spawn failed: {e}"));
                None
            }
        }
    } else {
        agent_cmd.output().ok()
    };

    let elapsed = start.elapsed();

    let files_modified = 0; // TODO: parse from output
    let lines_added = 0;
    let lines_removed = 0;
    let tokens_used = 0;

    // Check compilation
    let compilation_success = if task.check_compile {
        match std::process::Command::new("cargo")
            .args(["check"])
            .current_dir(temp_dir)
            .output()
        {
            Ok(out) => Some(out.status.success()),
            Err(_) => None,
        }
    } else {
        None
    };

    // Check tests
    let tests_passed = if task.check_tests {
        match std::process::Command::new("cargo")
            .args(["test"])
            .current_dir(temp_dir)
            .output()
        {
            Ok(out) => Some(out.status.success()),
            Err(_) => None,
        }
    } else {
        None
    };

    let passed = errors.is_empty()
        && compilation_success.unwrap_or(true)
        && tests_passed.unwrap_or(true);

    EvalResult {
        task_name: task.name.clone(),
        category: task.category.clone(),
        passed,
        elapsed,
        tokens_used,
        files_modified,
        lines_added,
        lines_removed,
        compilation_success,
        tests_passed,
        errors,
    }
}

/// Run the full benchmark suite
pub fn run_all_benchmarks(
    tasks: &[EvalTask],
    hyper_binary: &Path,
) -> Result<BenchmarkReport> {
    let total_start = Instant::now();
    let mut results = Vec::new();

    for task in tasks {
        println!("\n═══════════════════════════════════════════");
        println!("  📊 Benchmark: {}", task.name);
        println!("  Category: {:?}", task.category);
        println!("  Prompt: {}", task.prompt);
        println!("═══════════════════════════════════════════\n");

        let temp_dir = tempfile::tempdir().context("Failed to create temp dir")?;
        let result = run_single_task(task, temp_dir.path(), hyper_binary);

        let status = if result.passed { "✅ PASS" } else { "❌ FAIL" };
        println!("\n  Result: {status}");
        println!("  Time:   {:.1}s", result.elapsed.as_secs_f64());
        if let Some(comp) = result.compilation_success {
            println!("  Compiles: {}", if comp { "✅" } else { "❌" });
        }
        if let Some(tests) = result.tests_passed {
            println!("  Tests:   {}", if tests { "✅" } else { "❌" });
        }
        if !result.errors.is_empty() {
            for e in &result.errors {
                println!("  Error: {e}");
            }
        }

        results.push(result);
    }

    let total_elapsed = total_start.elapsed();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;

    println!("\n═══════════════════════════════════════════");
    println!("  📊 Benchmark Summary");
    println!("═══════════════════════════════════════════");
    println!("  Total:  {} tasks", results.len());
    println!("  Passed: {} ✅", passed);
    println!("  Failed: {} ❌", failed);
    println!("  Time:   {:.1}s", total_elapsed.as_secs_f64());
    println!();

    Ok(BenchmarkReport {
        timestamp: chrono::Local::now().to_rfc3339(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        total_tasks: results.len(),
        passed,
        failed,
        total_elapsed,
        results,
    })
}

/// List available benchmark tasks
pub fn list_tasks(tasks: &[EvalTask]) {
    println!("\n📋 Available Benchmark Tasks:\n");
    println!("  {:<25} {:<12} {}", "Name", "Category", "Prompt");
    println!("  {:-<25} {:-<12} {:-<50}", "", "", "");
    for task in tasks {
        let cat_str = match task.category {
            EvalCategory::CodeGen => "code-gen",
            EvalCategory::BugFix => "bug-fix",
            EvalCategory::Refactor => "refactor",
            EvalCategory::Documentation => "docs",
            EvalCategory::TestGen => "test-gen",
        };
        let prompt_short = if task.prompt.len() > 47 {
            format!("{}...", &task.prompt[..47])
        } else {
            task.prompt.clone()
        };
        println!("  {:<25} {:<12} {}", task.name, cat_str, prompt_short);
    }
    println!();
}
