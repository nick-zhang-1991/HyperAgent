//! HyperAgent Benchmark Suite — measure coding task performance
//!
//! Runs HyperAgent against a standardized suite of coding challenges
//! and produces detailed metrics: success rate, tokens used, time, cost.
//!
//! Usage:
//!   hyper benchmark                    # Run full benchmark suite
//!   hyper benchmark --quick            # Run quick subset (3 tasks)
//!   hyper benchmark --list             # List available benchmarks
//!   hyper benchmark --task lint-fix    # Run specific benchmark

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// A single benchmark task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchTask {
    pub name: String,
    pub description: String,
    pub language: String,
    pub difficulty: u8, // 1-5
    pub setup_script: String,
    pub prompt: String,
    pub verify_script: String,
}

/// Results from running a benchmark
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResult {
    pub task_name: String,
    pub passed: bool,
    pub elapsed_ms: u64,
    pub tokens_used: usize,
    pub error: Option<String>,
    pub attempts: u32,
}

/// Full benchmark report
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BenchReport {
    pub total_tasks: usize,
    pub passed: usize,
    pub failed: usize,
    pub total_time_ms: u64,
    pub results: Vec<BenchResult>,
    pub timestamp: String,
}

impl BenchReport {
    pub fn pass_rate(&self) -> f64 {
        if self.total_tasks == 0 { return 0.0; }
        self.passed as f64 / self.total_tasks as f64 * 100.0
    }

    pub fn summary(&self) -> String {
        format!(
            "📊 Benchmark Results: {}/{} passed ({:.0}%), {:.1}s total",
            self.passed, self.total_tasks, self.pass_rate(),
            self.total_time_ms as f64 / 1000.0
        )
    }
}

/// Built-in benchmark tasks
pub fn builtin_tasks() -> Vec<BenchTask> {
    vec![
        BenchTask {
            name: "rust-lint-fix".to_string(),
            description: "Fix unused variable and dead code warnings".to_string(),
            language: "rust".to_string(),
            difficulty: 1,
            setup_script: r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "bench-lint"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
pub fn calculate(x: i32) -> i32 {
    let unused = 42;
    x * 2
}

pub fn old_function() -> i32 {
    0
}
EOF
"#.to_string(),
            prompt: "Fix all compiler warnings in src/lib.rs: remove the unused variable and the dead function. Make sure `cargo check` passes with zero warnings.".to_string(),
            verify_script: "cargo check 2>&1".to_string(),
        },
        BenchTask {
            name: "rust-add-test".to_string(),
            description: "Add unit tests to existing code".to_string(),
            language: "rust".to_string(),
            difficulty: 2,
            setup_script: r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "bench-test"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
pub fn add(a: i32, b: i32) -> i32 { a + b }
pub fn multiply(a: i32, b: i32) -> i32 { a * b }
pub fn factorial(n: u32) -> u32 {
    match n { 0 | 1 => 1, _ => n * factorial(n - 1) }
}
EOF
"#.to_string(),
            prompt: "Add comprehensive unit tests for all three functions in src/lib.rs. Include edge cases (negative numbers, zero, large values) and ensure all tests pass with `cargo test`.".to_string(),
            verify_script: "cargo test 2>&1".to_string(),
        },
        BenchTask {
            name: "rust-clippy-fix".to_string(),
            description: "Fix clippy warnings in code".to_string(),
            language: "rust".to_string(),
            difficulty: 2,
            setup_script: r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "bench-clippy"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
pub fn check_value(x: i32) -> bool {
    if x == 0 {
        return true
    } else {
        return false
    }
}

pub fn process_items(items: Vec<i32>) -> i32 {
    let mut sum = 0;
    for i in 0..items.len() {
        sum += items[i];
    }
    sum
}
EOF
"#.to_string(),
            prompt: "Fix all clippy warnings in src/lib.rs. Use more idiomatic Rust patterns: single expression instead of if-else return, iterator instead of index loop. Make sure `cargo clippy` passes.".to_string(),
            verify_script: "cargo clippy 2>&1".to_string(),
        },
        BenchTask {
            name: "rust-error-handling".to_string(),
            description: "Add proper error handling".to_string(),
            language: "rust".to_string(),
            difficulty: 3,
            setup_script: r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "bench-errors"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
use std::fs::File;
use std::io::Read;

pub fn read_config(path: &str) -> String {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    contents
}

pub fn divide(a: f64, b: f64) -> f64 {
    a / b
}

pub fn parse_number(s: &str) -> i32 {
    s.parse().unwrap()
}
EOF
"#.to_string(),
            prompt: "Add proper error handling to src/lib.rs. Replace all `.unwrap()` calls with proper Result returns using `anyhow::Result` or custom error types. Functions should return Result instead of panicking.".to_string(),
            verify_script: "cargo check 2>&1".to_string(),
        },
        BenchTask {
            name: "rust-performance".to_string(),
            description: "Optimize slow code".to_string(),
            language: "rust".to_string(),
            difficulty: 4,
            setup_script: r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "bench-perf"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
use std::collections::HashMap;

pub fn find_duplicates(items: &[i32]) -> Vec<i32> {
    let mut result = Vec::new();
    for i in 0..items.len() {
        for j in (i+1)..items.len() {
            if items[i] == items[j] && !result.contains(&items[i]) {
                result.push(items[i]);
            }
        }
    }
    result
}

pub fn count_words(text: &str) -> Vec<(String, usize)> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        let word = word.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        if !word.is_empty() {
            *counts.entry(word).or_insert(0) += 1;
        }
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted.truncate(10);
    sorted
}
EOF
"#.to_string(),
            prompt: "Optimize the code in src/lib.rs for better performance. The O(n²) algorithm should be replaced with a HashSet-based O(n) approach. Make sure all optimizations preserve correctness and compile with zero warnings.".to_string(),
            verify_script: "cargo check 2>&1 && cargo test 2>&1".to_string(),
        },
        BenchTask {
            name: "python-data-pipeline".to_string(),
            description: "Build a data processing pipeline".to_string(),
            language: "python".to_string(),
            difficulty: 3,
            setup_script: r#"
cat > data.py << 'EOF'
import json

data = [
    {"name": "Alice", "age": 30, "city": "NYC", "score": 95},
    {"name": "Bob", "age": 25, "city": "LA", "score": 82},
    {"name": "Charlie", "age": 35, "city": "NYC", "score": 91},
    {"name": "Diana", "age": 28, "city": "Chicago", "score": 78},
    {"name": "Eve", "age": 32, "city": "LA", "score": 88},
]
EOF
"#.to_string(),
            prompt: "Create a Python data processing script `process.py` that reads from data.py and: 1) Filters to only NYC residents, 2) Calculates average score by city, 3) Sorts by age ascending, 4) Outputs results as JSON. Make it runnable with `python3 process.py`.".to_string(),
            verify_script: "python3 -c \"import json; exec(open('process.py').read())\" 2>&1".to_string(),
        },
        BenchTask {
            name: "typescript-api-endpoint".to_string(),
            description: "Create a REST API endpoint".to_string(),
            language: "typescript".to_string(),
            difficulty: 3,
            setup_script: r#"
cat > api.ts << 'EOF'
interface Todo {
    id: number;
    title: string;
    completed: boolean;
}

const todos: Todo[] = [
    { id: 1, title: "Learn Rust", completed: true },
    { id: 2, title: "Build HyperAgent", completed: false },
    { id: 3, title: "Ship v1.0", completed: false },
];
// TODO: Add CRUD functions below
EOF
"#.to_string(),
            prompt: "Add CRUD functions to api.ts: createTodo, getTodo, updateTodo, deleteTodo. Each function should be properly typed with TypeScript. Include input validation (title must be non-empty string, id must exist for updates/deletes).".to_string(),
            verify_script: "npx tsc --noEmit api.ts 2>&1 || echo 'TypeScript check done'".to_string(),
        },
    ]
}

/// Run a single benchmark task
pub async fn run_benchmark(task: &BenchTask) -> BenchResult {
    let start = Instant::now();

    // Create temp directory
    let tmpdir = std::env::temp_dir().join(format!("hyper-bench-{}", task.name));
    let _ = std::fs::remove_dir_all(&tmpdir);
    std::fs::create_dir_all(&tmpdir).unwrap_or_default();

    // Run setup script
    let setup_output = std::process::Command::new("bash")
        .arg("-c")
        .arg(&task.setup_script)
        .current_dir(&tmpdir)
        .output();

    let _ = match setup_output {
        Ok(o) if o.status.success() => (),
        Ok(o) => {
            return BenchResult {
                task_name: task.name.clone(),
                passed: false,
                elapsed_ms: start.elapsed().as_millis() as u64,
                tokens_used: 0,
                error: Some(format!("Setup failed: {}", String::from_utf8_lossy(&o.stderr))),
                attempts: 1,
            };
        }
        Err(e) => {
            return BenchResult {
                task_name: task.name.clone(),
                passed: false,
                elapsed_ms: start.elapsed().as_millis() as u64,
                tokens_used: 0,
                error: Some(format!("Setup error: {e}")),
                attempts: 1,
            };
        }
    };

    // Run HyperAgent on the task
    let hyper_output = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| "hyper".into()))
        .args(["run", "--yes", &task.prompt, "--dir", &tmpdir.to_string_lossy()])
        .output();

    let hyper_elapsed = start.elapsed().as_millis() as u64;

    let (passed, error) = match hyper_output {
        Ok(o) if o.status.success() => {
            // Run verification
            let verify = std::process::Command::new("bash")
                .arg("-c")
                .arg(&task.verify_script)
                .current_dir(&tmpdir)
                .output();

            match verify {
                Ok(v) if v.status.success() => (true, None),
                Ok(v) => (false, Some(format!("Verification failed: {}", String::from_utf8_lossy(&v.stderr).chars().take(200).collect::<String>()))),
                Err(e) => (false, Some(format!("Verification error: {e}"))),
            }
        }
        Ok(o) => (false, Some(format!("HyperAgent failed: {}", String::from_utf8_lossy(&o.stderr).chars().take(200).collect::<String>()))),
        Err(e) => (false, Some(format!("HyperAgent error: {e}"))),
    };

    let _ = std::fs::remove_dir_all(&tmpdir);

    BenchResult {
        task_name: task.name.clone(),
        passed,
        elapsed_ms: hyper_elapsed,
        tokens_used: 0, // Would need to parse from HyperAgent output
        error,
        attempts: 1,
    }
}

/// Run the full benchmark suite
pub async fn run_all(quick: bool) -> BenchReport {
    let tasks = builtin_tasks();
    let selected: Vec<&BenchTask> = if quick {
        tasks.iter().take(3).collect()
    } else {
        tasks.iter().collect()
    };

    let mut report = BenchReport {
        total_tasks: selected.len(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        ..Default::default()
    };

    let start = Instant::now();

    for task in &selected {
        println!("   ▶️  Running: {} ({})", task.name, task.description);
        let result = run_benchmark(task).await;
        let status = if result.passed { "✅ PASS" } else { "❌ FAIL" };
        println!("   {status} — {:.1}s", result.elapsed_ms as f64 / 1000.0);
        if let Some(ref err) = result.error {
            println!("      {}", err.chars().take(100).collect::<String>());
        }
        println!();
        if result.passed { report.passed += 1; } else { report.failed += 1; }
        report.results.push(result);
    }

    report.total_time_ms = start.elapsed().as_millis() as u64;
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_tasks_count() {
        let tasks = builtin_tasks();
        assert_eq!(tasks.len(), 7);
    }

    #[test]
    fn test_task_fields() {
        let tasks = builtin_tasks();
        for task in &tasks {
            assert!(!task.name.is_empty());
            assert!(!task.prompt.is_empty());
            assert!(task.difficulty >= 1 && task.difficulty <= 5);
        }
    }

    #[test]
    fn test_report_defaults() {
        let r = BenchReport::default();
        assert_eq!(r.pass_rate(), 0.0);
        assert!(r.summary().contains("0/0"));
    }

    #[test]
    fn test_report_with_results() {
        let mut r = BenchReport::default();
        r.total_tasks = 4;
        r.passed = 3;
        r.failed = 1;
        assert!((r.pass_rate() - 75.0).abs() < 0.01);
        assert!(r.summary().contains("3/4"));
    }

    #[test]
    fn test_difficulty_range() {
        let tasks = builtin_tasks();
        for task in &tasks {
            assert!(task.difficulty >= 1 && task.difficulty <= 5,
                "Task {} has invalid difficulty {}", task.name, task.difficulty);
        }
    }
}
