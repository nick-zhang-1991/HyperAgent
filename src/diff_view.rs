//! Terminal diff viewer — colorized diff display
//!
//! Shows git diffs with ANSI color coding:
//!   green  (+) additions
//!   red    (-) deletions  
//!   cyan   (@@) hunk headers
//!   yellow bold (---/+++) file headers

use std::path::Path;

/// Display a colorized diff to stdout
pub fn show_diff(diff_text: &str) {
    if diff_text.trim().is_empty() {
        println!("   No changes to display.");
        return;
    }

    for line in diff_text.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            // File header — yellow bold
            println!("\x1b[33;1m{line}\x1b[0m");
        } else if line.starts_with("@@") {
            // Hunk header — cyan
            println!("\x1b[36m{line}\x1b[0m");
        } else if line.starts_with('+') {
            // Addition — green
            println!("\x1b[32m{line}\x1b[0m");
        } else if line.starts_with('-') {
            // Deletion — red
            println!("\x1b[31m{line}\x1b[0m");
        } else {
            // Context — normal
            println!("{line}");
        }
    }
}

/// Get diff between two states
pub fn get_diff(root: &Path, against: &str) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["diff", against, "--no-color"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("Git error: {e}"))?;

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(diff)
}

/// Show staged diff
pub fn show_staged_diff(root: &Path) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--no-color"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("Git error: {e}"))?;

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    show_diff(&diff);
    Ok(())
}
