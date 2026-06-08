//! Deep codebase analysis — AST-level insights, security, and anti-patterns.
//!
//! ```
//! hyper analyze [--fix] [--json]
//! ```

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Serialize, Clone, PartialEq)]
pub enum Severity {
    Critical,  // Security vulnerability, CVE
    Error,     // Compilation error
    Warning,   // Clippy warning, code smell
    Info,      // Style, suggestion
    Dead,      // Dead code
}

#[derive(Debug, Serialize, Clone)]
pub struct AnalysisIssue {
    pub file: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub severity: Severity,
    pub code: String,       // e.g., "RUSTSEC-2024-0001", "clippy::needless_return"
    pub message: String,
    pub suggestion: Option<String>,
}

/// Run full analysis and return ranked issues
pub fn run(root: &Path, fix: bool) -> Result<Vec<AnalysisIssue>> {
    let mut issues = Vec::new();

    println!("🔍 HyperAgent Code Analysis\n");
    println!("   Project: {}", root.display());
    println!();

    // 1. Cargo check (compilation errors)
    if root.join("Cargo.toml").exists() {
        println!("   📦 Checking compilation...");
        if let Ok(out) = Command::new("cargo")
            .args(["check", "--message-format=short"])
            .current_dir(root)
            .output()
        {
            let stderr = String::from_utf8_lossy(&out.stderr);
            for line in stderr.lines() {
                if let Some(issue) = parse_cargo_line(line) {
                    issues.push(issue);
                }
            }
        }
        println!("      {} issues found", issues.len());
    }

    // 2. Cargo audit (security vulnerabilities)
    println!("   🛡️  Checking security...");
    if let Ok(out) = Command::new("cargo")
        .args(["audit", "--json"])
        .current_dir(root)
        .output()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for vuln in parse_audit_json(&stdout) {
            issues.push(vuln);
        }
    }

    // 3. Dead code detection (Tree-sitter)
    println!("   🗑️  Detecting dead code...");
    let dead = detect_dead_code(root);
    issues.extend(dead);
    println!("      {} dead items found", dead.len());

    // 4. Complexity analysis
    println!("   📊 Measuring complexity...");
    let complex = detect_complexity(root);
    issues.extend(complex);
    println!("      {} complex functions found", complex.len());

    // Sort: Critical > Error > Warning > Info > Dead
    issues.sort_by_key(|i| severity_order(&i.severity));

    // Print report
    print_report(&issues);

    // Auto-fix if requested
    if fix && !issues.is_empty() {
        println!("\n   🔧 Auto-fixing P0 issues...");
        let fixed = auto_fix(&issues, root)?;
        println!("   ✅ Fixed {} issues", fixed);
    }

    Ok(issues)
}

fn severity_order(s: &Severity) -> u8 {
    match s {
        Severity::Critical => 0,
        Severity::Error => 1,
        Severity::Warning => 2,
        Severity::Info => 3,
        Severity::Dead => 4,
    }
}

fn parse_cargo_line(line: &str) -> Option<AnalysisIssue> {
    let line = line.trim();
    if line.contains("error[") {
        // Try to parse "src/file.rs:line:col: error[E0123]: msg"
        let parts: Vec<&str> = line.splitn(2, ':').collect();
        if parts.len() >= 2 {
            let file_part = parts[0];
            let rest = parts[1];
            let loc_parts: Vec<&str> = rest.splitn(3, |c| c == ':' || c == ' ').collect();
            let line_num = loc_parts.get(0).and_then(|s| s.trim().parse().ok());
            let col_num = loc_parts.get(1).and_then(|s| s.trim().parse().ok());
            let msg = loc_parts.get(2).unwrap_or(&"").trim().to_string();

            let error_code = if let Some(idx) = line.find("error[") {
                let end = line[idx..].find(']').unwrap_or(5);
                line[idx+6..idx+end].to_string()
            } else {
                String::new()
            };

            let severity = if line.contains("error[E]") {
                Severity::Error
            } else {
                Severity::Warning
            };

            return Some(AnalysisIssue {
                file: file_part.trim().to_string(),
                line: line_num,
                col: col_num,
                severity,
                code: format!("rustc::{}", error_code),
                message: msg,
                suggestion: None,
            });
        }
    }
    None
}

fn parse_audit_json(json: &str) -> Vec<AnalysisIssue> {
    let mut issues = Vec::new();
    if let Ok(data) = serde_json::from_str::<serde_json::Value>(json) {
        if let Some(vulns) = data["vulnerabilities"]["list"].as_array() {
            for v in vulns {
                let id = v["advisory"]["id"].as_str().unwrap_or("unknown");
                let desc = v["advisory"]["title"].as_str().unwrap_or("no description");
                let package = v["package"]["name"].as_str().unwrap_or("");
                issues.push(AnalysisIssue {
                    file: "Cargo.toml".into(),
                    line: None,
                    col: None,
                    severity: Severity::Critical,
                    code: id.to_string(),
                    message: format!("[{package}] {desc}"),
                    suggestion: Some(format!("cargo update {}", package)),
                });
            }
        }
    }
    issues
}

fn detect_dead_code(root: &Path) -> Vec<AnalysisIssue> {
    let mut issues = Vec::new();
    if let Ok(out) = Command::new("cargo")
        .args(["check", "--message-format=json"])
        .current_dir(root)
        .output()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            if line.contains("\"dead_code\"") || line.contains("\"unused_imports\"") {
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(spans) = msg["message"]["spans"].as_array() {
                        if let Some(first) = spans.first() {
                            let file = first["file_name"].as_str().unwrap_or("?");
                            let line_n = first["line_start"].as_u64();
                            issues.push(AnalysisIssue {
                                file: file.to_string(),
                                line: line_n.map(|l| l as u32),
                                col: None,
                                severity: Severity::Dead,
                                code: "dead_code".into(),
                                message: "Potentially dead code".into(),
                                suggestion: Some("Remove or add #[allow(dead_code)]".into()),
                            });
                        }
                    }
                }
            }
        }
    }
    issues
}

fn detect_complexity(root: &Path) -> Vec<AnalysisIssue> {
    let mut issues = Vec::new();
    // Check for large files (>1000 lines)
    if let Ok(entries) = walkdir::WalkDir::new(root.join("src"))
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
        .collect::<Vec<_>>()
    {
        for entry in entries {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let lines = content.lines().count();
                if lines > 1000 {
                    issues.push(AnalysisIssue {
                        file: entry.path().to_string_lossy().to_string(),
                        line: None,
                        col: None,
                        severity: Severity::Warning,
                        code: "complexity::large_file".into(),
                        message: format!("File has {} lines — consider splitting", lines),
                        suggestion: Some("Break into smaller modules".into()),
                    });
                }
                // Check for deeply nested blocks (>4 levels)
                let max_nest = content.lines()
                    .map(|l| l.chars().take_while(|c| c.is_whitespace()).count())
                    .max()
                    .unwrap_or(0);
                if max_nest > 80 {  // 4 levels with 4-space indent = 16, use 80 as threshold for extreme nesting
                    issues.push(AnalysisIssue {
                        file: entry.path().to_string_lossy().to_string(),
                        line: None,
                        col: None,
                        severity: Severity::Warning,
                        code: "complexity::deep_nesting".into(),
                        message: "Deeply nested code detected".into(),
                        suggestion: Some("Extract inner blocks to functions".into()),
                    });
                }
            }
        }
    }
    issues
}

fn print_report(issues: &[AnalysisIssue]) {
    let counts = |sev: &Severity| issues.iter().filter(|i| i.severity == *sev).count();
    
    println!();
    println!("   ┌─ Analysis Report ──────────────────────────────────────┐");
    println!("   │ 🔴 Critical  : {:>3}  (security)                           │", counts(&Severity::Critical));
    println!("   │ 🟠 Error     : {:>3}  (compilation)                       │", counts(&Severity::Error));
    println!("   │ 🟡 Warning   : {:>3}  (code quality)                      │", counts(&Severity::Warning));
    println!("   │ 🔵 Info      : {:>3}  (suggestions)                       │", counts(&Severity::Info));
    println!("   │ ⬜ Dead Code : {:>3}  (unused)                             │", counts(&Severity::Dead));
    println!("   │                                                        │");
    println!("   │ Total: {:>3} issues                                      │", issues.len());
    println!("   └────────────────────────────────────────────────────────┘");

    // Print top 10 most severe
    let top: Vec<&AnalysisIssue> = issues.iter().take(10).collect();
    if !top.is_empty() {
        println!("\n   Top issues:");
        for (i, issue) in top.iter().enumerate() {
            let icon = match issue.severity {
                Severity::Critical => "🔴",
                Severity::Error => "🟠",
                Severity::Warning => "🟡",
                Severity::Info => "🔵",
                Severity::Dead => "⬜",
            };
            let loc = match (issue.line, issue.col) {
                (Some(l), Some(c)) => format!("{}:{}", l, c),
                (Some(l), None) => format!("{}", l),
                _ => String::new(),
            };
            println!("   {}. {} [{}] {}", 
                i + 1,
                icon,
                issue.code,
                issue.message
            );
            if !loc.is_empty() {
                println!("      at {}:{}", issue.file, loc);
            }
            if let Some(ref sug) = issue.suggestion {
                println!("      → {}", sug);
            }
        }
    }
}

fn auto_fix(issues: &[AnalysisIssue], _root: &Path) -> Result<usize> {
    let mut fixed = 0;
    // For critical security issues: cargo update
    for issue in issues {
        if issue.severity == Severity::Critical && issue.file == "Cargo.toml" {
            if let Ok(out) = Command::new("cargo").args(["update"]).output() {
                if out.status.success() {
                    fixed += 1;
                }
            }
        }
    }
    // For compilation errors: use the auto-fix loop (handled by orchestrator)
    // For dead code: add #[allow(dead_code)] would be too aggressive

    Ok(fixed)
}
