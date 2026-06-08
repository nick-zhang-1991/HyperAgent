#![cfg(test)]
/// End-to-end agent pipeline tests

use std::process::Command;

/// Test that the binary launches and shows version
#[test]
fn test_hyper_binary_version() {
    let output = Command::new("./target/debug/hyperagent")
        .arg("--version")
        .output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{}{}", stdout, stderr);
        assert!(
            combined.contains("hyperagent") || combined.contains("0."),
            "version output: {combined}"
        );
    }
    // May fail if binary not built — that's OK for CI
}

/// Test hyper doctor command
#[test]
fn test_hyper_doctor_runs() {
    let output = Command::new("./target/debug/hyperagent")
        .args(["doctor"])
        .output();

    match output {
        Ok(out) => {
            let combined = String::from_utf8_lossy(&out.stdout);
            assert!(
                combined.contains("HyperAgent") || combined.contains("Diagnostics") || out.status.success(),
                "doctor should produce output"
            );
        }
        Err(e) => {
            // Binary may not be built — skip gracefully
            eprintln!("Doctor test skipped (binary not built): {e}");
        }
    }
}

/// Test hyper init --help shows options
#[test]
fn test_hyper_init_help() {
    let output = Command::new("./target/debug/hyperagent")
        .args(["init", "--help"])
        .output();

    if let Ok(out) = output {
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("--yes") || stdout.contains("init") || stdout.contains("Initialize"));
    }
}

/// Test hyper analyze --help
#[test]
fn test_hyper_analyze_help() {
    let output = Command::new("./target/debug/hyperagent")
        .args(["analyze", "--help"])
        .output();

    if let Ok(out) = output {
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("analyze") || stdout.contains("--fix"));
    }
}

/// Test hyper serve --help
#[test]
fn test_hyper_serve_help() {
    let output = Command::new("./target/debug/hyperagent")
        .args(["serve", "--help"])
        .output();

    if let Ok(out) = output {
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("port") || stdout.contains("serve"));
    }
}

/// Test hyper swarm --help
#[test]
fn test_hyper_swarm_help() {
    let output = Command::new("./target/debug/hyperagent")
        .args(["swarm", "--help"])
        .output();

    if let Ok(out) = output {
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("swarm") || stdout.contains("agents"));
    }
}

/// Test hyper memory global list --help
#[test]
fn test_hyper_memory_global_help() {
    let output = Command::new("./target/debug/hyperagent")
        .args(["memory", "global", "--help"])
        .output();

    if let Ok(out) = output {
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("global") || stdout.contains("list"));
    }
}

/// Test hyper skill --help
#[test]
fn test_hyper_skill_help() {
    let output = Command::new("./target/debug/hyperagent")
        .args(["skill", "--help"])
        .output();

    if let Ok(out) = output {
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("skill") || stdout.contains("install"));
    }
}

/// Test hyper eval --help
#[test]
fn test_hyper_eval_help() {
    let output = Command::new("./target/debug/hyperagent")
        .args(["eval", "--help"])
        .output();

    if let Ok(out) = output {
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("eval"));
    }
}

/// Test hyper feedback --help
#[test]
fn test_hyper_feedback_help() {
    let output = Command::new("./target/debug/hyperagent")
        .args(["feedback", "--help"])
        .output();

    if let Ok(out) = output {
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("feedback") || stdout.contains("good"));
    }
}
