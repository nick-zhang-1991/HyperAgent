//! Self-evaluation benchmark framework.
//!
//! ```
//! hyper bench eval          # run all benchmark tasks
//! hyper bench eval --json   # JSON output for CI
//! ```

use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

/// A benchmark task
struct EvalTask {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    prompt: &'static str,
    check: fn(&PathBuf) -> Result<bool>,
}

#[derive(Debug, Serialize)]
pub struct EvalResult {
    pub task_id: String,
    pub name: String,
    pub category: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EvalReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pass_rate: f64,
    pub by_category: Vec<CategoryResult>,
    pub results: Vec<EvalResult>,
}

#[derive(Debug, Serialize)]
pub struct CategoryResult {
    pub category: String,
    pub total: usize,
    pub passed: usize,
    pub pass_rate: f64,
}

/// Run all evaluation tasks
pub fn run_all(project_root: &PathBuf, json: bool) -> Result<EvalReport> {
    let tasks = all_tasks();
    let mut results = Vec::new();

    println!("🧪 HyperAgent Self-Evaluation\n");
    println!("   {} tasks across {} categories\n", tasks.len(), unique_categories(&tasks));

    for task in &tasks {
        print!("   [{}/{}] {} ... ", results.len() + 1, tasks.len(), task.name);
        let start = Instant::now();

        let result = run_single_task(task, project_root);
        let duration = start.elapsed().as_millis() as u64;

        match &result {
            Ok(_) => println!("✅ ({duration}ms)"),
            Err(e) => println!("❌ {e}"),
        }

        results.push(EvalResult {
            task_id: task.id.to_string(),
            name: task.name.to_string(),
            category: task.category.to_string(),
            passed: result.is_ok(),
            duration_ms: duration,
            error: result.err().map(|e| e.to_string()),
        });
    }

    let passed = results.iter().filter(|r| r.passed).count();
    let report = EvalReport {
        total: tasks.len(),
        passed,
        failed: tasks.len() - passed,
        pass_rate: passed as f64 / tasks.len() as f64 * 100.0,
        by_category: compute_categories(&tasks, &results),
        results,
    };

    if !json {
        print_report(&report);
    }

    Ok(report)
}

fn run_single_task(task: &EvalTask, root: &PathBuf) -> Result<()> {
    let tmp = std::env::temp_dir().join(format!("hyper_eval_{}", task.id));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;

    // Generate code using hyper
    let hyper_bin = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("hyperagent"));

    let output = Command::new(&hyper_bin)
        .args(["run", "--mode", "code", task.prompt])
        .current_dir(&tmp)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            // Verify the output compiles
            let check = (task.check)(&tmp);
            match check {
                Ok(true) => Ok(()),
                Ok(false) => anyhow::bail!("Check failed: output does not meet requirements"),
                Err(e) => anyhow::bail!("Check error: {e}"),
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("Agent failed: {}", stderr.lines().last().unwrap_or("unknown"));
        }
        Err(e) => anyhow::bail!("Failed to run agent: {e}"),
    }
}

fn unique_categories(tasks: &[EvalTask]) -> usize {
    let mut cats: Vec<&str> = tasks.iter().map(|t| t.category).collect();
    cats.sort();
    cats.dedup();
    cats.len()
}

fn compute_categories(tasks: &[EvalTask], results: &[EvalResult]) -> Vec<CategoryResult> {
    let mut cats: Vec<String> = tasks.iter().map(|t| t.category.to_string()).collect();
    cats.sort();
    cats.dedup();

    cats.iter().map(|cat| {
        let cat_results: Vec<&EvalResult> = results.iter()
            .filter(|r| r.category == *cat)
            .collect();
        let passed = cat_results.iter().filter(|r| r.passed).count();
        CategoryResult {
            category: cat.clone(),
            total: cat_results.len(),
            passed,
            pass_rate: if cat_results.is_empty() { 0.0 } else { passed as f64 / cat_results.len() as f64 * 100.0 },
        }
    }).collect()
}

fn print_report(report: &EvalReport) {
    println!();
    println!("   ┌─ Eval Report ─────────────────────────────────────────┐");
    println!("   │                                                        │");
    println!("   │   Total:  {:>3}                                        │", report.total);
    println!("   │   Passed: {:>3}  ({}%)                                │", report.passed, format!("{:.1}", report.pass_rate));
    println!("   │   Failed: {:>3}                                        │", report.failed);
    println!("   │                                                        │");
    for cat in &report.by_category {
        println!("   │   {}: {:>3}/{} ({:.1}%)",
            cat.category, cat.passed, cat.total, cat.pass_rate);
    }
    println!("   └────────────────────────────────────────────────────────┘");
}

fn all_tasks() -> Vec<EvalTask> {
    vec![
        EvalTask {
            id: "fibonacci",
            name: "Fibonacci function",
            category: "code-gen",
            prompt: "Write a Rust function `fib(n: u32) -> u64` that returns the nth Fibonacci number. Include a main function with tests.",
            check: |dir| {
                let src = dir.join("src").join("main.rs");
                if !src.exists() { return Ok(false); }
                Ok(std::fs::read_to_string(&src)?.contains("fn fib"))
            },
        },
        EvalTask {
            id: "struct_new",
            name: "Struct with new()",
            category: "code-gen",
            prompt: "Define a Rust struct `User` with fields `name: String` and `age: u32`. Add a `new(name: &str, age: u32) -> Self` constructor.",
            check: |dir| {
                let src = dir.join("src").join("main.rs");
                if !src.exists() { return Ok(false); }
                let content = std::fs::read_to_string(&src)?;
                Ok(content.contains("struct User") && content.contains("fn new"))
            },
        },
        EvalTask {
            id: "error_handling",
            name: "Error handling with Result",
            category: "code-gen",
            prompt: "Write a Rust function `read_file(path: &str) -> Result<String, std::io::Error>` that reads a file and returns its contents. Handle the error case properly.",
            check: |dir| {
                let src = dir.join("src").join("main.rs");
                if !src.exists() { return Ok(false); }
                Ok(std::fs::read_to_string(&src)?.contains("Result"))
            },
        },
        EvalTask {
            id: "iterator",
            name: "Iterator fold",
            category: "algorithms",
            prompt: "Write a Rust function `sum_of_squares(nums: &[i32]) -> i32` that computes the sum of squares using iterator fold.",
            check: |dir| {
                let src = dir.join("src").join("main.rs");
                if !src.exists() { return Ok(false); }
                let content = std::fs::read_to_string(&src)?;
                Ok(content.contains("sum_of_squares") && content.contains("fold"))
            },
        },
    ]
}
