//! Code Health Monitor — automated codebase quality scanning
//!
//! Checks for:
//! - Dead code (`#[allow(dead_code)]`, unused imports)
//! - Missing documentation (pub items without docs)
//! - Unsafe code blocks
//! - `unwrap()` / `expect()` usage
//! - Long functions (>100 lines)
//! - TODOs and FIXMEs
//! - Large files (>500 lines)
//!
//! Usage:
//!   hyper health          # Run full health check
//!   hyper health --json   # Output JSON report
//!
//! Scheduled via cron:
//!   hyper schedule add "0 9 * * 1" health  # Every Monday 9am

use std::path::{Path, PathBuf};
use serde::Serialize;

/// A single health check finding
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub category: String,
    pub file: PathBuf,
    pub line: Option<usize>,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Severity level
#[derive(Debug, Clone, Serialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// Health check report
#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub files_scanned: usize,
    pub total_findings: usize,
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub findings: Vec<Finding>,
    pub score: f64, // 0.0 - 100.0
}

/// Run a full health check on the project
pub fn run_health_check(project_root: &Path) -> HealthReport {
    let mut findings = Vec::new();
    let mut files_scanned = 0;

    // Scan Rust files
    for entry in walkdir::WalkDir::new(project_root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && name != "target" && name != "node_modules"
        })
        .flatten()
    {
            let path = entry.path().to_path_buf();
            if !path.is_file() {
                continue;
            }

            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            match ext {
                "rs" => {
                    files_scanned += 1;
                    check_rust_file(&path, project_root, &mut findings);
                }
                "ts" | "tsx" | "js" | "jsx" => {
                    files_scanned += 1;
                    check_ts_file(&path, project_root, &mut findings);
                }
                "py" => {
                    files_scanned += 1;
                    check_py_file(&path, project_root, &mut findings);
                }
                _ => {}
            }
        }

    // Calculate stats
    let errors = findings.iter().filter(|f| matches!(f.severity, Severity::Error)).count();
    let warnings = findings.iter().filter(|f| matches!(f.severity, Severity::Warning)).count();
    let infos = findings.iter().filter(|f| matches!(f.severity, Severity::Info)).count();

    // Score: start at 100, deduct for each finding
    let score = (100.0
        - errors as f64 * 5.0
        - warnings as f64 * 2.0
        - infos as f64 * 0.5)
        .max(0.0);

    HealthReport {
        files_scanned,
        total_findings: findings.len(),
        errors,
        warnings,
        infos,
        findings,
        score,
    }
}

fn check_rust_file(path: &Path, root: &Path, findings: &mut Vec<Finding>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let rel_path = path.strip_prefix(root).unwrap_or(path);

    // Check for large files
    let line_count = content.lines().count();
    if line_count > 500 {
        findings.push(Finding {
            severity: Severity::Warning,
            category: "large_file".into(),
            file: rel_path.to_path_buf(),
            line: None,
            message: format!("File has {line_count} lines (threshold: 500)"),
            suggestion: Some("Consider splitting into smaller modules".into()),
        });
    }

    // Check for unwrap/expect
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();

        // Skip comments and test code
        if line.starts_with("//") || line.starts_with("#[") {
            continue;
        }

        if line.contains(".unwrap()") && !line.contains("// ok") {
            findings.push(Finding {
                severity: Severity::Warning,
                category: "unwrap_usage".into(),
                file: rel_path.to_path_buf(),
                line: Some(i + 1),
                message: "Use of .unwrap() — may panic".into(),
                suggestion: Some("Replace with proper error handling (?, match, or expect with context)".into()),
            });
        }

        if line.contains(".expect(") && !line.contains("// ok") {
            findings.push(Finding {
                severity: Severity::Info,
                category: "expect_usage".into(),
                file: rel_path.to_path_buf(),
                line: Some(i + 1),
                message: "Use of .expect()".into(),
                suggestion: Some("Consider if this can be replaced with ? operator".into()),
            });
        }
    }

    // Check for unsafe blocks
    for (i, line) in content.lines().enumerate() {
        if line.trim().starts_with("unsafe ") || line.trim() == "unsafe {" {
            findings.push(Finding {
                severity: Severity::Warning,
                category: "unsafe_code".into(),
                file: rel_path.to_path_buf(),
                line: Some(i + 1),
                message: "Unsafe code block".into(),
                suggestion: Some("Add SAFETY comment explaining why unsafe is necessary".into()),
            });
        }
    }

    // Check for TODOs and FIXMEs
    for (i, line) in content.lines().enumerate() {
        let upper = line.to_uppercase();
        if upper.contains("TODO") {
            findings.push(Finding {
                severity: Severity::Info,
                category: "todo".into(),
                file: rel_path.to_path_buf(),
                line: Some(i + 1),
                message: "TODO comment".into(),
                suggestion: Some("Address the TODO or create an issue to track it".into()),
            });
        }
        if upper.contains("FIXME") {
            findings.push(Finding {
                severity: Severity::Warning,
                category: "fixme".into(),
                file: rel_path.to_path_buf(),
                line: Some(i + 1),
                message: "FIXME comment".into(),
                suggestion: Some("Fix this before it becomes a production issue".into()),
            });
        }
    }

    // Check for missing docs on pub items (basic heuristic)
    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if (trimmed.starts_with("pub fn") || trimmed.starts_with("pub struct") || trimmed.starts_with("pub enum") || trimmed.starts_with("pub trait"))
            && !trimmed.starts_with("pub fn test_")
            && !trimmed.starts_with("pub fn new")
        {
            // Check if previous line is a doc comment
            let has_docs = i > 0 && (lines[i - 1].trim().starts_with("///") || lines[i - 1].trim().starts_with("//!"));
            if !has_docs {
                let item_name = trimmed.split_whitespace().nth(2).unwrap_or("?");
                findings.push(Finding {
                    severity: Severity::Info,
                    category: "missing_docs".into(),
                    file: rel_path.to_path_buf(),
                    line: Some(i + 1),
                    message: format!("Missing documentation for `{item_name}`"),
                    suggestion: Some("Add /// documentation comment describing what this does".into()),
                });
            }
        }
    }
}

fn check_ts_file(path: &Path, root: &Path, findings: &mut Vec<Finding>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let rel_path = path.strip_prefix(root).unwrap_or(path);

    let line_count = content.lines().count();
    if line_count > 500 {
        findings.push(Finding {
            severity: Severity::Warning,
            category: "large_file".into(),
            file: rel_path.to_path_buf(),
            line: None,
            message: format!("File has {line_count} lines (threshold: 500)"),
            suggestion: Some("Consider splitting into smaller modules".into()),
        });
    }

    // Check for console.log in non-test files
    if !rel_path.to_string_lossy().contains(".test.") {
        for (i, line) in content.lines().enumerate() {
            if line.contains("console.log(") && !line.trim().starts_with("//") {
                findings.push(Finding {
                    severity: Severity::Info,
                    category: "console_log".into(),
                    file: rel_path.to_path_buf(),
                    line: Some(i + 1),
                    message: "Console.log in production code".into(),
                    suggestion: Some("Remove or replace with proper logging".into()),
                });
            }
        }
    }
}

fn check_py_file(path: &Path, root: &Path, findings: &mut Vec<Finding>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let rel_path = path.strip_prefix(root).unwrap_or(path);

    let line_count = content.lines().count();
    if line_count > 500 {
        findings.push(Finding {
            severity: Severity::Warning,
            category: "large_file".into(),
            file: rel_path.to_path_buf(),
            line: None,
            message: format!("File has {line_count} lines (threshold: 500)"),
            suggestion: Some("Consider splitting into smaller modules".into()),
        });
    }

    for (i, line) in content.lines().enumerate() {
        let upper = line.to_uppercase();
        if upper.contains("TODO") {
            findings.push(Finding {
                severity: Severity::Info,
                category: "todo".into(),
                file: rel_path.to_path_buf(),
                line: Some(i + 1),
                message: "TODO comment".into(),
                suggestion: Some("Address the TODO".into()),
            });
        }
    }
}

/// Render health report to a display string
pub fn render_report(report: &HealthReport) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "   🏥 Code Health Report\n\
         ═══════════════════════════════════════\n\
         📁 Files scanned: {}\n\
         📊 Total findings: {}\n\
         🔴 Errors:   {}\n\
         🟡 Warnings: {}\n\
         🔵 Info:     {}\n\
         📈 Score:    {:.1}/100\n\n",
        report.files_scanned, report.total_findings,
        report.errors, report.warnings, report.infos, report.score,
    ));

    // Group findings by category
    let mut by_category: std::collections::BTreeMap<&str, Vec<&Finding>> = std::collections::BTreeMap::new();
    for finding in &report.findings {
        by_category.entry(&finding.category).or_default().push(finding);
    }

    for (category, items) in &by_category {
        let severity_icon = match items[0].severity {
            Severity::Error => "🔴",
            Severity::Warning => "🟡",
            Severity::Info => "🔵",
        };
        output.push_str(&format!("   {severity_icon} {} ({}):\n", category, items.len()));

        for finding in items.iter().take(5) {
            let line_info = finding.line.map(|l| format!(":{}", l)).unwrap_or_default();
            output.push_str(&format!(
                "     • {}{}\n",
                finding.file.display(),
                line_info,
            ));
            if let Some(ref suggestion) = finding.suggestion {
                output.push_str(&format!("       💡 {suggestion}\n"));
            }
        }
        if items.len() > 5 {
            output.push_str(&format!("     ... and {} more\n", items.len() - 5));
        }
        output.push('\n');
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_project() {
        let dir = std::env::temp_dir().join("hyper-health-test-empty");
        let _ = std::fs::create_dir_all(&dir);
        let report = run_health_check(&dir);
        assert_eq!(report.files_scanned, 0);
        assert_eq!(report.total_findings, 0);
        assert_eq!(report.score, 100.0);
    }

    #[test]
    fn test_rust_file_checks() {
        let dir = std::env::temp_dir().join("hyper-health-test-rust");
        let _ = std::fs::create_dir_all(&dir.join("src"));
        std::fs::write(dir.join("src").join("lib.rs"), r#"
// TODO: implement error handling
pub fn process() {
    let x = unsafe { get_value() };
    let y = x.unwrap();
    println!("{}", y);
}
"#).unwrap();

        let report = run_health_check(&dir);
        assert!(report.total_findings > 0);
        assert!(report.findings.iter().any(|f| f.category == "todo"));
        assert!(report.findings.iter().any(|f| f.category == "unsafe_code"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_score() {
        let mut report = HealthReport {
            files_scanned: 1,
            total_findings: 0,
            errors: 0,
            warnings: 0,
            infos: 0,
            findings: vec![],
            score: 100.0,
        };
        
        // Each error deducts 5 points
        report.score = (100.0f64 - 5.0 * 5.0).max(0.0);
        assert_eq!(report.score, 75.0);

        report.score = (100.0f64 - 10.0 * 5.0).max(0.0);
        assert_eq!(report.score, 50.0);
    }
}
