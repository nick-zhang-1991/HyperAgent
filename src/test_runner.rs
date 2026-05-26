//! Test runner — unit, integration, and E2E testing
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum TestMode { All, Unit, Integration, E2e, Gen }

#[derive(Debug, Default)]
pub struct TestReport {
    pub total: usize, pub passed: usize, pub failed: usize,
    pub duration_secs: f64, pub details: Vec<TestResult>,
}

#[derive(Debug)]
pub struct TestResult {
    pub name: String, pub passed: bool,
    pub output: String, pub duration_secs: f64,
}

pub async fn run_tests(root: &Path, mode: TestMode) -> Result<TestReport> {
    let start = Instant::now();
    let mut report = TestReport::default();
    if mode == TestMode::Unit || mode == TestMode::All {
        merge(&mut report, run_unit_tests(root).await?);
    }
    if mode == TestMode::Integration || mode == TestMode::All {
        merge(&mut report, run_integration_tests(root).await?);
    }
    if mode == TestMode::E2e || mode == TestMode::All {
        merge(&mut report, run_e2e_tests(root).await?);
    }
    if mode == TestMode::Gen {
        generate_tests(root)?;
    }
    report.duration_secs = start.elapsed().as_secs_f64();
    Ok(report)
}

fn merge(r: &mut TestReport, o: TestReport) {
    r.total += o.total; r.passed += o.passed; r.failed += o.failed; r.details.extend(o.details);
}

async fn run_unit_tests(root: &Path) -> Result<TestReport> {
    let mut report = TestReport::default();
    println!("   Running unit tests...");
    let start = Instant::now();
    let out = Command::new("cargo").args(["test"]).current_dir(root).output()?;
    let elapsed = start.elapsed().as_secs_f64();
    let err = String::from_utf8_lossy(&out.stderr);
    report.total = 1;
    if out.status.success() {
        report.passed = 1;
        report.details.push(TestResult { name: "cargo test".into(), passed: true, output: "OK".into(), duration_secs: elapsed });
    } else {
        report.failed = 1;
        let e = err.lines().find(|l| l.contains("error[")).unwrap_or("FAIL");
        report.details.push(TestResult { name: "cargo test".into(), passed: false, output: e.into(), duration_secs: elapsed });
    }
    Ok(report)
}

async fn run_integration_tests(root: &Path) -> Result<TestReport> {
    let mut report = TestReport::default();
    let d = root.join("tests");
    if !d.exists() { return Ok(report); }
    println!("   Running integration tests...");
    for entry in std::fs::read_dir(&d)? {
        let e = entry?;
        let p = e.path();
        if p.extension().is_some_and(|ext| ext == "rs") {
            let start = Instant::now();
            let n = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let out = Command::new("cargo").args(["test", "--test", &n]).current_dir(root).output()?;
            let elapsed = start.elapsed().as_secs_f64();
            let ok = out.status.success();
            report.details.push(TestResult {
                name: format!("integ::{n}"), passed: ok,
                output: if ok { "OK".into() } else { "FAIL".into() },
                duration_secs: elapsed,
            });
            report.total += 1;
            if ok { report.passed += 1; } else { report.failed += 1; }
        }
    }
    Ok(report)
}

async fn run_e2e_tests(root: &Path) -> Result<TestReport> {
    let mut report = TestReport::default();
    println!("   Running E2E tests...");
    let hyper = which_hyper()?;

    async fn run_one(hyper: &Path, root: &Path, name: &str, args: &[&str], expect: &str) -> TestResult {
        let start = Instant::now();
        let out = Command::new(hyper).args(args).current_dir(root).output();
        let elapsed = start.elapsed().as_secs_f64();
        match out {
            Ok(o) => {
                let s = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                TestResult {
                    name: format!("e2e::{name}"), passed: s.contains(expect),
                    output: if s.contains(expect) { "OK".into() } else { format!("missing {expect}") },
                    duration_secs: elapsed,
                }
            }
            Err(e) => TestResult { name: format!("e2e::{name}"), passed: false, output: format!("{e}"), duration_secs: elapsed },
        }
    }

    let e2e_cases = [("help", &["--help"][..], "HyperAgent"), ("version", &["--version"][..], "0.1"), ("doctor", &["doctor"][..], "HyperAgent")];
    for &(name, args, expect) in &e2e_cases {
        let r = run_one(&hyper, root, name, args, expect).await;
        report.total += 1;
        if r.passed { report.passed += 1; } else { report.failed += 1; }
        report.details.push(r);
    }
    Ok(report)
}

fn generate_tests(root: &Path) -> Result<()> {
    let d = root.join("tests");
    std::fs::create_dir_all(&d)?;
    let p = d.join("integration_test.rs");
    if !p.exists() {
        std::fs::write(&p, r#"use std::process::Command;
#[test] fn help() { let o = Command::new(env!("CARGO_BIN_EXE_hyperagent")).arg("--help").output().unwrap(); assert!(String::from_utf8_lossy(&o.stdout).contains("HyperAgent")); }
#[test] fn version() { let o = Command::new(env!("CARGO_BIN_EXE_hyperagent")).arg("--version").output().unwrap(); assert!(o.status.success()); }
"#)?;
        println!("   Gen: tests/integration_test.rs");
    }
    Ok(())
}

pub fn display_report(r: &TestReport) {
    println!("\n=== Test Report ===");
    println!("  Total: {} | Pass: {} | Fail: {} | {:.2}s", r.total, r.passed, r.failed, r.duration_secs);
    for d in &r.details {
        println!("  {} {} ({:.1}s)", if d.passed { "[PASS]" } else { "[FAIL]" }, d.name, d.duration_secs);
        if !d.passed { println!("    {}", d.output); }
    }
}

fn which_hyper() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_hyperagent") { return Ok(PathBuf::from(p)); }
    if let Ok(h) = std::env::var("HOME") { let l = PathBuf::from(&h).join(".local/bin/hyper"); if l.exists() { return Ok(l); } }
    Ok(PathBuf::from("hyper"))
}
