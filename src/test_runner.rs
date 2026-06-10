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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_test_mode_equality() {
        assert_eq!(TestMode::All, TestMode::All);
        assert_eq!(TestMode::Unit, TestMode::Unit);
        assert_ne!(TestMode::Unit, TestMode::Integration);
        assert_ne!(TestMode::All, TestMode::Gen);
    }

    #[test]
    fn test_test_mode_debug() {
        let m = TestMode::E2e;
        let s = format!("{:?}", m);
        assert!(s.contains("E2e"));
    }

    #[test]
    fn test_test_mode_clone() {
        let m = TestMode::Integration;
        let c = m.clone();
        assert_eq!(m, c);
    }

    #[test]
    fn test_test_report_default() {
        let r = TestReport::default();
        assert_eq!(r.total, 0);
        assert_eq!(r.passed, 0);
        assert_eq!(r.failed, 0);
        assert_eq!(r.duration_secs, 0.0);
        assert!(r.details.is_empty());
    }

    #[test]
    fn test_test_result_construction() {
        let r = TestResult {
            name: "test_foo".into(),
            passed: true,
            output: "ok".into(),
            duration_secs: 0.5,
        };
        assert_eq!(r.name, "test_foo");
        assert!(r.passed);
        assert_eq!(r.duration_secs, 0.5);
    }

    #[test]
    fn test_test_result_failed() {
        let r = TestResult {
            name: "test_fail".into(),
            passed: false,
            output: "assertion failed".into(),
            duration_secs: 1.0,
        };
        assert!(!r.passed);
    }

    #[test]
    fn test_display_report_empty() {
        let r = TestReport::default();
        // Just verify it doesn't panic
        display_report(&r);
    }

    #[test]
    fn test_display_report_with_data() {
        let r = TestReport {
            total: 10,
            passed: 8,
            failed: 2,
            duration_secs: 5.0,
            details: vec![
                TestResult {
                    name: "test_pass1".into(),
                    passed: true,
                    output: "ok".into(),
                    duration_secs: 0.1,
                },
                TestResult {
                    name: "test_fail1".into(),
                    passed: false,
                    output: "FAILED".into(),
                    duration_secs: 0.2,
                },
            ],
        };
        display_report(&r);
    }

    #[test]
    fn test_merge_combines_reports() {
        // Test merge through run_tests is hard without executing; test logic via fields
        let mut r = TestReport {
            total: 5,
            passed: 5,
            failed: 0,
            duration_secs: 1.0,
            details: vec![],
        };
        let o = TestReport {
            total: 3,
            passed: 2,
            failed: 1,
            duration_secs: 2.0,
            details: vec![TestResult {
                name: "x".into(),
                passed: false,
                output: "y".into(),
                duration_secs: 0.5,
            }],
        };
        // Use the merge function (private) - we can only call from this module
        merge(&mut r, o);
        assert_eq!(r.total, 8);
        assert_eq!(r.passed, 7);
        assert_eq!(r.failed, 1);
        assert_eq!(r.details.len(), 1);
    }

    #[test]
    fn test_generate_tests_creates_files() {
        let dir = std::env::temp_dir().join(format!("hyperagent_gen_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // generate_tests needs a Cargo.toml in the root - create one
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        let result = generate_tests(&dir);
        // Should succeed (creates tests dir with generated tests)
        let _ = result;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
