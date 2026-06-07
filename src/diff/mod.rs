use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A file change to apply
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub file: PathBuf,
    pub change_type: String, // "edit", "create", "delete"
    pub old_content: Option<String>,
    pub new_content: Option<String>,
    pub hunks: Vec<DiffHunk>,
}

/// A single diff hunk (similar to unified diff format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub content: String,
}

/// Parse a unified-diff text block into DiffHunks
/// Format:
///   @@ -old_start,old_lines +new_start,new_lines @@
///    context
///   -removed
///   +added
pub fn text_to_hunks(diff_text: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;

    for line in diff_text.lines() {
        if line.starts_with("@@") {
            // Finalize previous hunk
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            // Parse new hunk header: @@ -old_start,old_lines +new_start,new_lines @@
            if let Some(hunk) = parse_hunk_header(line) {
                current_hunk = Some(hunk);
            }
        } else if let Some(ref mut hunk) = current_hunk {
            if !hunk.content.is_empty() {
                hunk.content.push('\n');
            }
            hunk.content.push_str(line);
        }
    }

    // Last hunk
    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }

    hunks
}

fn parse_hunk_header(line: &str) -> Option<DiffHunk> {
    let line = line.trim_start_matches("@@").trim_end_matches("@@").trim();
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let parse_range = |s: &str| -> Option<(usize, usize)> {
        let s = s.trim_start_matches('-').trim_start_matches('+');
        if let Some((start, count)) = s.split_once(',') {
            Some((start.parse().ok()?, count.parse().ok()?))
        } else {
            Some((s.parse().ok()?, 1))
        }
    };

    let (old_start, old_lines) = parse_range(parts[0])?;
    let (new_start, new_lines) = parse_range(parts[1])?;

    Some(DiffHunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        content: String::new(),
    })
}

/// Apply hunks to a file's content (surgical edit)
pub fn apply_edits_to_file(path: &PathBuf, hunks: &[DiffHunk]) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    let content = std::fs::read_to_string(path)?;
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

    // Apply hunks in reverse order (bottom-to-top to preserve line numbers)
    let mut sorted_hunks = hunks.to_vec();
    sorted_hunks.sort_by(|a, b| b.old_start.cmp(&a.old_start));

    for hunk in &sorted_hunks {
        let start = hunk.old_start.saturating_sub(1); // Convert to 0-indexed
        let end = (start + hunk.old_lines).min(lines.len());

        // Parse the new content from the hunk
        let new_lines: Vec<String> = hunk
            .content
            .lines()
            .filter(|l| !l.starts_with('-') || l.starts_with("---"))
            .map(|l| {
                if let Some(stripped) = l.strip_prefix('+').or_else(|| l.strip_prefix(' ')) {
                    stripped.to_string()
                } else {
                    l.to_string()
                }
            })
            .collect();

        // Replace old lines with new lines
        lines.splice(start..end, new_lines);
    }

    std::fs::write(path, lines.join("\n"))?;
    // Preserve trailing newline if original had it
    if content.ends_with('\n') && !lines.is_empty() {
        let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
        use std::io::Write;
        writeln!(f)?;
    }
    Ok(())
}

/// Render a compact, colored diff preview for a set of file changes.
/// Returns a string with ANSI color codes suitable for terminal display.
/// Uses red for deletions (-), green for additions (+), and dim for context ( ).
pub fn render_diff_preview(changes: &[FileChange], max_hunks_per_file: usize) -> String {
    let red = "\x1b[31m";
    let green = "\x1b[32m";
    let dim = "\x1b[2m";
    let bold = "\x1b[1m";
    let reset = "\x1b[0m";

    let mut output = String::new();
    output.push_str(&format!("\n{}📝 Diff Preview:{} {} file(s)\n", bold, reset, changes.len()));

    for change in changes {
        let file_str = change.file.display();
        let type_icon = match change.change_type.as_str() {
            "create" => "➕",
            "delete" => "❌",
            _ => "✏️",
        };
        output.push_str(&format!("  {} {}\n", type_icon, file_str));

        // Show file-level old→new size summary
        let old_size = change.old_content.as_ref().map(|c| c.len()).unwrap_or(0);
        let new_size = change.new_content.as_ref().map(|c| c.len()).unwrap_or(0);
        if old_size > 0 || new_size > 0 {
            let delta = if new_size >= old_size {
                format!("+{}", new_size - old_size)
            } else {
                format!("-{}", old_size - new_size)
            };
            output.push_str(&format!(
                "    {}Size: {}B → {}B ({}B){}  {}hunks: {}{}\n",
                dim, old_size, new_size, delta, reset,
                dim, change.hunks.len(), reset,
            ));
        }

        // Show hunks (limited)
        for (i, hunk) in change.hunks.iter().enumerate() {
            if i >= max_hunks_per_file {
                output.push_str(&format!(
                    "    {}... and {} more hunk(s){}",
                    dim,
                    change.hunks.len() - max_hunks_per_file,
                    reset
                ));
                break;
            }
            output.push_str(&format!(
                "    {}@@ -{},{} +{},{} @@{}\n",
                dim, hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines, reset
            ));
            for line in hunk.content.lines() {
                if line.starts_with('-') {
                    output.push_str(&format!("    {}{}{}\n", red, line, reset));
                } else if line.starts_with('+') {
                    output.push_str(&format!("    {}{}{}\n", green, line, reset));
                } else {
                    output.push_str(&format!("    {} {}\n", dim, line));
                }
            }
        }
        if !change.hunks.is_empty() {
            output.push('\n');
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_text_to_hunks_simple() {
        let diff = "@@ -1,2 +1,3 @@\n line1\n-old line\n+new line\n+added line";
        let hunks = text_to_hunks(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].old_lines, 2);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].new_lines, 3);
    }

    #[test]
    fn test_text_to_hunks_multiple() {
        let diff = "@@ -1,1 +1,1 @@\n-old\n+new\n@@ -10,2 +10,2 @@\n context\n-a\n+b";
        let hunks = text_to_hunks(diff);
        assert_eq!(hunks.len(), 2);
    }

    #[test]
    fn test_text_to_hunks_empty() {
        let hunks = text_to_hunks("");
        assert!(hunks.is_empty());
    }

    #[test]
    fn test_parse_hunk_header_standard() {
        let hunk = parse_hunk_header("@@ -1,2 +3,4 @@");
        assert!(hunk.is_some());
        let h = hunk.unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_lines, 2);
        assert_eq!(h.new_start, 3);
        assert_eq!(h.new_lines, 4);
    }

    #[test]
    fn test_parse_hunk_header_no_count() {
        let hunk = parse_hunk_header("@@ -1 +2 @@");
        assert!(hunk.is_some());
        let h = hunk.unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_lines, 1);
        assert_eq!(h.new_start, 2);
        assert_eq!(h.new_lines, 1);
    }

    #[test]
    fn test_parse_hunk_header_invalid() {
        assert!(parse_hunk_header("not a hunk").is_none());
        assert!(parse_hunk_header("").is_none());
    }

    #[test]
    fn test_apply_edits_to_file() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("test.txt");
        let mut f = std::fs::File::create(&path)?;
        writeln!(f, "line1")?;
        writeln!(f, "old line")?;
        writeln!(f, "line3")?;
        f.flush()?;

        let hunks = text_to_hunks("@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3");
        apply_edits_to_file(&path, &hunks)?;

        let content = std::fs::read_to_string(&path)?;
        assert_eq!(content, "line1\nnew line\nline3\n");
        Ok(())
    }

    #[test]
    fn test_apply_edits_preserves_unchanged() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "a\nb\nc\nd\ne\n")?;

        let hunks = text_to_hunks("@@ -3,1 +3,1 @@\n-c\n+x");
        apply_edits_to_file(&path, &hunks)?;

        let content = std::fs::read_to_string(&path)?;
        assert_eq!(content, "a\nb\nx\nd\ne\n");
        Ok(())
    }

    #[test]
    fn test_file_change_serde_roundtrip() {
        let change = FileChange {
            file: PathBuf::from("src/main.rs"),
            change_type: "edit".to_string(),
            old_content: Some("old".to_string()),
            new_content: Some("new".to_string()),
            hunks: vec![],
        };
        let json = serde_json::to_string(&change).unwrap();
        let parsed: FileChange = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.file, PathBuf::from("src/main.rs"));
        assert_eq!(parsed.change_type, "edit");
        assert_eq!(parsed.old_content, Some("old".to_string()));
    }
}
