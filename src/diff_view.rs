//! Terminal diff viewer — colorized unified and side-by-side diff display
//!
//! Shows git diffs with ANSI color coding:
//!   green  (+) additions
//!   red    (-) deletions  
//!   cyan   (@@) hunk headers
//!   yellow bold (---/+++) file headers

use std::path::Path;

/// Maximum terminal width to use for side-by-side view
fn terminal_width() -> usize {
    if let Some(size) = term_size::dimensions() {
        size.0.min(160)
    } else {
        80
    }
}

/// Display a colorized diff to stdout (unified view)
pub fn show_diff(diff_text: &str) {
    if diff_text.trim().is_empty() {
        println!("   No changes to display.");
        return;
    }

    for line in diff_text.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            println!("\x1b[33;1m{line}\x1b[0m");
        } else if line.starts_with("@@") {
            println!("\x1b[36m{line}\x1b[0m");
        } else if line.starts_with('+') {
            println!("\x1b[32m{line}\x1b[0m");
        } else if line.starts_with('-') {
            println!("\x1b[31m{line}\x1b[0m");
        } else {
            println!("{line}");
        }
    }
}

/// Display a diff side-by-side
pub fn show_side_by_side_diff(diff_text: &str) {
    if diff_text.trim().is_empty() {
        println!("   No changes to display.");
        return;
    }

    let width = terminal_width();
    let half = width / 2 - 2; // leave room for separator " │ "

    // Print header
    println!("\x1b[33;1m┌─ OLD {:─<1$}┐\x1b[0m", "", half.saturating_sub(5));
    println!("\x1b[33;1m└─ NEW {:─<1$}┘\x1b[0m", "", half.saturating_sub(5));

    // Parse diff into hunks and render each
    let mut file_path = String::new();
    let mut hunk_lines: Vec<(char, String)> = Vec::new();
    let mut in_hunk = false;

    for line in diff_text.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            // Flush current hunk
            if !hunk_lines.is_empty() {
                render_hunk_side_by_side(&hunk_lines, half, &file_path);
                hunk_lines.clear();
            }
            if let Some(stripped) = line.strip_prefix("--- ") {
                file_path = stripped.trim().to_string();
            }
            in_hunk = false;
        } else if line.starts_with("@@") {
            // Flush previous hunk
            if !hunk_lines.is_empty() {
                render_hunk_side_by_side(&hunk_lines, half, &file_path);
                hunk_lines.clear();
            }
            println!("\x1b[36m{:─<w$}\x1b[0m", "", w = width);
            println!("\x1b[36m {}\x1b[0m", line);
            in_hunk = true;
        } else if in_hunk {
            if let Some(stripped) = line.strip_prefix('-') {
                hunk_lines.push(('-', stripped.to_string()));
            } else if let Some(stripped) = line.strip_prefix('+') {
                hunk_lines.push(('+', stripped.to_string()));
            } else {
                hunk_lines.push((' ', line.to_string()));
            }
        }
    }

    // Flush last hunk
    if !hunk_lines.is_empty() {
        render_hunk_side_by_side(&hunk_lines, half, &file_path);
    }
}

/// Render a single hunk side-by-side
fn render_hunk_side_by_side(lines: &[(char, String)], half: usize, _file_path: &str) {
    let old_lines: Vec<&(char, String)> = lines.iter()
        .filter(|(c, _)| *c != '+')
        .collect();
    let new_lines: Vec<&(char, String)> = lines.iter()
        .filter(|(c, _)| *c != '-')
        .collect();

    let max_len = old_lines.len().max(new_lines.len());

    for i in 0..max_len {
        let (old_side, old_kind) = if i < old_lines.len() {
            let (c, s) = (old_lines[i].0, &old_lines[i].1);
            let truncated = truncate_with_ellipsis(s, half);
            (truncated, c)
        } else {
            (String::new(), ' ')
        };

        let (new_side, new_kind) = if i < new_lines.len() {
            let (c, s) = (new_lines[i].0, &new_lines[i].1);
            let truncated = truncate_with_ellipsis(s, half);
            (truncated, c)
        } else {
            (String::new(), ' ')
        };

        let old_padded = format!("{:n$}", old_side, n = half);
        let new_padded = format!("{:<n$}", new_side, n = half);

        let old_colored = colorize_line(&old_padded, old_kind);
        let new_colored = colorize_line(&new_padded, new_kind);

        println!("{old_colored} \x1b[90m│\x1b[0m {new_colored}");
    }
}

/// Truncate a string and add ellipsis if it exceeds max width
fn truncate_with_ellipsis(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}

/// Colorize a line based on its diff kind
fn colorize_line(line: &str, kind: char) -> String {
    match kind {
        '-' => format!("\x1b[41m\x1b[37m{line}\x1b[0m"), // red bg, white text
        '+' => format!("\x1b[42m\x1b[30m{line}\x1b[0m"), // green bg, black text
        _   => line.to_string(),
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

/// Show staged diff (unified)
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

/// Show staged diff (side-by-side)
pub fn show_staged_diff_side_by_side(root: &Path) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--no-color"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("Git error: {e}"))?;

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    show_side_by_side_diff(&diff);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short_string() {
        let result = truncate_with_ellipsis("hello", 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        let long = "this is a very long string that exceeds the max";
        let result = truncate_with_ellipsis(long, 20);
        assert_eq!(result.chars().count(), 20, "should have exactly 20 characters");
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_truncate_exact() {
        let s = "exactly ten!";
        let result = truncate_with_ellipsis(s, s.len());
        assert_eq!(result, s);
    }

    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate_with_ellipsis("", 10), "");
    }

    #[test]
    fn test_show_diff_empty() {
        // Should not panic
        show_diff("");
        show_side_by_side_diff("");
    }

    #[test]
    fn test_colorize_line() {
        let result = colorize_line("deleted", '-');
        assert!(result.contains("41m"), "deleted lines should have red background");
        assert!(result.contains("37m"), "deleted lines should have white text");

        let result = colorize_line("added", '+');
        assert!(result.contains("42m"), "added lines should have green background");
        assert!(result.contains("30m"), "added lines should have black text");

        let result = colorize_line("context", ' ');
        assert_eq!(result, "context");
    }
}
