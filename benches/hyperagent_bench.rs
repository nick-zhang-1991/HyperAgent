//! HyperAgent benchmarks — black-box performance measurement
//!
//! Measures key operations by executing the compiled binary and reporting wall-clock time.
//! Run with: cargo bench
//!
//! Note: Since HyperAgent is a binary-only crate, benchmarks execute the binary
//! via subprocess (black-box style) — this measures real end-to-end performance.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// CLI benchmark: measure `hyper eval --task gen-fibonacci` wall time
fn bench_eval_single_task(binary: &Path) {
    let start = Instant::now();
    let output = Command::new(binary)
        .args(["eval", "--task", "gen-fibonacci"])
        .output()
        .expect("Failed to run eval");
    let elapsed = start.elapsed();

    if output.status.success() {
        println!("  ✅ eval gen-fibonacci: {:.2}s", elapsed.as_secs_f64());
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("  ❌ eval gen-fibonacci failed ({:.2}s): {}", elapsed.as_secs_f64(), stderr.lines().next().unwrap_or(""));
    }
}

/// CLI benchmark: measure `hyper completions bash` generation time
fn bench_completions_generation(binary: &Path, shell: &str) {
    let start = Instant::now();
    let output = Command::new(binary)
        .args(["completions", shell])
        .output()
        .expect("Failed to run completions");
    let elapsed = start.elapsed();
    let lines = String::from_utf8_lossy(&output.stdout).lines().count();

    if output.status.success() {
        println!("  ✅ completions {shell}: {:.2}s ({lines} lines)", elapsed.as_secs_f64());
    } else {
        println!("  ❌ completions {shell} failed ({:.2}s)", elapsed.as_secs_f64());
    }
}

/// CLI benchmark: measure `hyper doctor` execution time
fn bench_doctor(binary: &Path) {
    // Create a temp directory so doctor has something to check
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let start = Instant::now();
    let output = Command::new(binary)
        .args(["doctor"])
        .current_dir(root)
        .output()
        .expect("Failed to run doctor");
    let elapsed = start.elapsed();

    if output.status.success() {
        println!("  ✅ doctor: {:.2}s", elapsed.as_secs_f64());
    } else {
        println!("  ❌ doctor failed ({:.2}s)", elapsed.as_secs_f64());
    }
}

/// CLI benchmark: measure `hyper config` output
fn bench_config(binary: &Path) {
    let start = Instant::now();
    let output = Command::new(binary)
        .args(["config"])
        .output()
        .expect("Failed to run config");
    let elapsed = start.elapsed();

    if output.status.success() {
        let lines = String::from_utf8_lossy(&output.stdout).lines().count();
        println!("  ✅ config: {:.2}s ({lines} lines)", elapsed.as_secs_f64());
    } else {
        println!("  ❌ config failed ({:.2}s)", elapsed.as_secs_f64());
    }
}

/// CLI benchmark: measure `hyper eval --list` output
fn bench_eval_list(binary: &Path) {
    let start = Instant::now();
    let output = Command::new(binary)
        .args(["eval", "--list"])
        .output()
        .expect("Failed to run eval --list");
    let elapsed = start.elapsed();

    if output.status.success() {
        let lines = String::from_utf8_lossy(&output.stdout).lines().count();
        println!("  ✅ eval --list: {:.2}s ({lines} lines)", elapsed.as_secs_f64());
    } else {
        println!("  ❌ eval --list failed ({:.2}s)", elapsed.as_secs_f64());
    }
}

fn main() {
    // Find the binary path (assumes `cargo bench` runs from project root)
    let binary = find_binary();

    println!("\n📊 HyperAgent Benchmarks\n");
    println!("  Binary: {}", binary.display());
    println!();

    // Warm-up run
    let _ = Command::new(&binary).args(["config"]).output();

    // Run benchmarks
    println!("  ┌─ CLI Commands ──────────────────────────────┐");
    bench_config(&binary);
    bench_completions_generation(&binary, "bash");
    bench_completions_generation(&binary, "zsh");
    bench_completions_generation(&binary, "fish");
    bench_doctor(&binary);
    bench_eval_list(&binary);
    println!("  └──────────────────────────────────────────────┘");

    // Eval task (if it works)
    println!("  ┌─ Agent Pipeline ────────────────────────────┐");
    bench_eval_single_task(&binary);
    println!("  └──────────────────────────────────────────────┘");

    println!();
    println!("  ⚡ Done. Timings are wall-clock on this machine.");
    println!("  For precise profiling, use: cargo build --release && perf record ./target/release/hyperagent run \"...\"");
    println!();
}

/// Find the compiled hyperagent binary
fn find_binary() -> PathBuf {
    // Try: target/release/hyperagent (bench mode), then target/debug/hyperagent
    let candidates = vec![
        PathBuf::from("target/release/hyperagent"),
        PathBuf::from("target/debug/hyperagent"),
        PathBuf::from("target/release/hyper"),
        PathBuf::from("target/debug/hyper"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return candidate.clone();
        }
    }

    // Fallback: check if hyper is on PATH
    if let Ok(output) = Command::new("which").arg("hyper").output() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    eprintln!("⚠️  Binary not found. Build first with: cargo build");
    std::process::exit(1);
}
