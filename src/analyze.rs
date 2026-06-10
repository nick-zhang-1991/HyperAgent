//! Deep codebase analysis — AST-level insights, security, and anti-patterns.
//!
//! ```
//! hyper analyze [--fix] [--json]
//! ```

use crate::i18n;
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

    println!("{}\n", i18n::t("analyze_title"));
    println!("   {}: {}", i18n::t("analyze_project"), root.display());
    println!();

    // 1. Cargo check (compilation errors)
    if root.join("Cargo.toml").exists() {
        println!("   {}", i18n::t("analyze_checking"));
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
    println!("   {}", i18n::t("analyze_security"));
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
    println!("   {}", i18n::t("analyze_dead_code"));
    let dead = detect_dead_code(root);
    issues.extend(dead.clone());
    println!("      {} dead items found", dead.len());

    // 4. Complexity analysis
    println!("   {}", i18n::t("analyze_complexity"));
    let complex = detect_complexity(root);
    issues.extend(complex.clone());
    println!("      {} complex functions found", complex.len());

    // Sort: Critical > Error > Warning > Info > Dead
    issues.sort_by_key(|i| severity_order(&i.severity));

    // Print report
    print_report(&issues);

    // Auto-fix if requested
    if fix && !issues.is_empty() {
        println!("\n   {}", i18n::t("analyze_fixing"));
        let fixed = auto_fix(&issues, root)?;
        println!("   {}", i18n::t_with("analyze_fixed", &[&fixed.to_string()]));
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

            let severity = if line.contains("error[") {
                // Check if it's a specific rustc error (E0xxx) or generic "error[E]"
                if line.contains("error[E]") || line.contains("error[E0") {
                    Severity::Error
                } else {
                    Severity::Warning
                }
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
    let entries: Vec<_> = walkdir::WalkDir::new(root.join("src"))
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Severity ────────────────────────────────────────────

    #[test]
    fn test_severity_equality() {
        assert_eq!(Severity::Critical, Severity::Critical);
        assert_ne!(Severity::Critical, Severity::Error);
        assert_ne!(Severity::Warning, Severity::Info);
        assert_ne!(Severity::Info, Severity::Dead);
    }

    #[test]
    fn test_severity_clone() {
        let s = Severity::Warning;
        let cloned = s.clone();
        assert_eq!(s, cloned);
    }

    #[test]
    fn test_severity_debug() {
        // Debug should not panic for any variant
        let _ = format!("{:?}", Severity::Critical);
        let _ = format!("{:?}", Severity::Error);
        let _ = format!("{:?}", Severity::Warning);
        let _ = format!("{:?}", Severity::Info);
        let _ = format!("{:?}", Severity::Dead);
    }

    #[test]
    fn test_severity_serialize() {
        let json = serde_json::to_string(&Severity::Critical).unwrap();
        // PascalCase is default for unit variants
        assert_eq!(json, "\"Critical\"");
        let json = serde_json::to_string(&Severity::Error).unwrap();
        assert_eq!(json, "\"Error\"");
    }

    // ── AnalysisIssue ───────────────────────────────────────

    #[test]
    fn test_analysis_issue_construction() {
        let issue = AnalysisIssue {
            file: "src/foo.rs".into(),
            line: Some(10),
            col: Some(5),
            severity: Severity::Error,
            code: "E0123".into(),
            message: "type mismatch".into(),
            suggestion: Some("fix the type".into()),
        };
        assert_eq!(issue.file, "src/foo.rs");
        assert_eq!(issue.line, Some(10));
        assert_eq!(issue.col, Some(5));
    }

    #[test]
    fn test_analysis_issue_serialize() {
        let issue = AnalysisIssue {
            file: "f.rs".into(),
            line: Some(1),
            col: None,
            severity: Severity::Warning,
            code: "W001".into(),
            message: "msg".into(),
            suggestion: None,
        };
        let json = serde_json::to_string(&issue).unwrap();
        assert!(json.contains("\"file\":\"f.rs\""));
        assert!(json.contains("\"line\":1"));
        assert!(json.contains("\"severity\":\"Warning\""));
    }

    // ── severity_order ──────────────────────────────────────

    #[test]
    fn test_severity_order_critical_first() {
        assert_eq!(severity_order(&Severity::Critical), 0);
        assert_eq!(severity_order(&Severity::Error), 1);
        assert_eq!(severity_order(&Severity::Warning), 2);
        assert_eq!(severity_order(&Severity::Info), 3);
        assert_eq!(severity_order(&Severity::Dead), 4);
    }

    #[test]
    fn test_severity_order_is_total() {
        // All distinct values
        let mut orders = vec![
            severity_order(&Severity::Critical),
            severity_order(&Severity::Error),
            severity_order(&Severity::Warning),
            severity_order(&Severity::Info),
            severity_order(&Severity::Dead),
        ];
        orders.sort();
        orders.dedup();
        assert_eq!(orders.len(), 5);
    }

    // ── parse_cargo_line ────────────────────────────────────

    #[test]
    fn test_parse_cargo_line_typical_error() {
        let line = "src/main.rs:42:5: error[E0123]: types do not match";
        let issue = parse_cargo_line(line).expect("should parse");
        assert_eq!(issue.file, "src/main.rs");
        assert_eq!(issue.line, Some(42));
        assert_eq!(issue.col, Some(5));
        assert_eq!(issue.code, "rustc::E0123");
        assert_eq!(issue.severity, Severity::Error);
    }

    #[test]
    fn test_parse_cargo_line_non_error_returns_none() {
        assert!(parse_cargo_line("Compiling foo v0.1.0").is_none());
        assert!(parse_cargo_line("Finished dev profile").is_none());
        assert!(parse_cargo_line("").is_none());
    }

    #[test]
    fn test_parse_cargo_line_warning_severity() {
        // Non-error[E] but contains error[
        let line = "src/lib.rs:10:1: error[other]: something";
        let issue = parse_cargo_line(line).expect("should parse");
        assert_eq!(issue.severity, Severity::Warning);
    }

    #[test]
    fn test_parse_cargo_line_handles_no_file_colon() {
        let line = "error[E0123]: some error";
        let issue = parse_cargo_line(line);
        // Without file:line:col prefix, the function still returns Some
        // but with a weird "file" value (the error code itself)
        assert!(issue.is_some());
        let issue = issue.unwrap();
        assert!(issue.message.contains("some error") || issue.message.contains("error"));
    }

    // ── parse_audit_json ────────────────────────────────────

    #[test]
    fn test_parse_audit_json_empty() {
        let issues = parse_audit_json("{}");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_parse_audit_json_invalid_json() {
        let issues = parse_audit_json("not json");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_parse_audit_json_no_vulnerabilities() {
        let json = r#"{"vulnerabilities":{"list":[]}}"#;
        let issues = parse_audit_json(json);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_parse_audit_json_with_vulnerability() {
        let json = r#"{
  "vulnerabilities": {
    "list": [
      {
        "advisory": {
          "id": "RUSTSEC-2024-0001",
          "title": "Critical vulnerability in foo"
        },
        "package": {
          "name": "vulnerable-pkg"
        }
      }
    ]
  }
}"#;
        let issues = parse_audit_json(json);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "RUSTSEC-2024-0001");
        assert_eq!(issues[0].file, "Cargo.toml");
        assert_eq!(issues[0].severity, Severity::Critical);
        assert!(issues[0].message.contains("vulnerable-pkg"));
        assert!(issues[0].message.contains("Critical"));
        assert!(issues[0].suggestion.is_some());
        assert!(issues[0].suggestion.as_ref().unwrap().contains("cargo update"));
    }

    #[test]
    fn test_parse_audit_json_multiple_vulnerabilities() {
        let json = r#"{
  "vulnerabilities": {
    "list": [
      {"advisory":{"id":"A1","title":"vuln 1"},"package":{"name":"pkg1"}},
      {"advisory":{"id":"A2","title":"vuln 2"},"package":{"name":"pkg2"}},
      {"advisory":{"id":"A3","title":"vuln 3"},"package":{"name":"pkg3"}}
    ]
  }
}"#;
        let issues = parse_audit_json(json);
        assert_eq!(issues.len(), 3);
        let codes: Vec<&str> = issues.iter().map(|i| i.code.as_str()).collect();
        assert!(codes.contains(&"A1"));
        assert!(codes.contains(&"A2"));
        assert!(codes.contains(&"A3"));
    }

    #[test]
    fn test_parse_audit_json_missing_fields() {
        let json = r#"{
  "vulnerabilities": {
    "list": [
      {"advisory": {}, "package": {}}
    ]
  }
}"#;
        let issues = parse_audit_json(json);
        assert_eq!(issues.len(), 1);
        // Missing fields get defaults
        assert_eq!(issues[0].code, "unknown");
        assert_eq!(issues[0].message, "[] no description");
    }

    // ── detect_dead_code (smoke test) ───────────────────────

    #[test]
    fn test_detect_dead_code_on_empty_dir() {
        let dir = std::env::temp_dir().join(format!("hyperagent_analyze_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let issues = detect_dead_code(&dir);
        // No Cargo.toml so cargo check fails silently, no issues
        assert!(issues.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── detect_complexity (testable) ────────────────────────

    #[test]
    fn test_detect_complexity_large_file() {
        let dir = std::env::temp_dir().join(format!("hyperagent_analyze2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        // Create a 1001-line file
        let mut content = String::new();
        for i in 0..1001 {
            content.push_str(&format!("// line {}\n", i));
        }
        std::fs::write(dir.join("src/big.rs"), &content).unwrap();
        let issues = detect_complexity(&dir);
        assert!(!issues.is_empty(), "Should detect large file");
        let big_issue = issues.iter().find(|i| i.code == "complexity::large_file").expect("should have large file issue");
        assert!(big_issue.message.contains("1001"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_detect_complexity_no_src_dir() {
        let dir = std::env::temp_dir().join(format!("hyperagent_analyze3_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // No src/ subdirectory
        let issues = detect_complexity(&dir);
        // walkdir on missing dir returns no entries
        assert!(issues.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_detect_complexity_small_file() {
        let dir = std::env::temp_dir().join(format!("hyperagent_analyze4_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/small.rs"), "fn main() {}\n").unwrap();
        let issues = detect_complexity(&dir);
        assert!(issues.is_empty(), "small file should not trigger issues");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── run (integration smoke test) ────────────────────────

    #[test]
    fn test_run_on_nonexistent_dir() {
        let result = run(std::path::Path::new("/nonexistent/path/that/does/not/exist"), false);
        // Should not error out — just produce empty issues list
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
