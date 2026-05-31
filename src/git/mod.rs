pub mod worktree;

use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Git operations for the agent
pub struct GitOps;

impl GitOps {
    /// Check if we're in a git repo
    pub fn is_repo(root: &Path) -> bool {
        root.join(".git").exists()
    }

    #[allow(dead_code)]
    /// Get the current diff (unstaged changes)
    pub fn get_diff(root: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "--no-color"])
            .current_dir(root)
            .output()?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Stage all changes
    pub fn stage_all(root: &Path) -> Result<()> {
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(root)
            .output()?;
        Ok(())
    }

    #[allow(dead_code)]
    /// Commit with message
    pub fn commit(root: &Path, message: &str) -> Result<()> {
        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(root)
            .output()?;
        Ok(())
    }

    #[allow(dead_code)]
    /// Get tracked files
    pub fn get_tracked_files(root: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["ls-files"])
            .current_dir(root)
            .output()?;
        let files = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();
        Ok(files)
    }

    /// Create a smart commit message from staged changes
    pub fn generate_commit_message(root: &Path) -> Result<String> {
        let diff_stat = Command::new("git")
            .args(["diff", "--cached", "--stat"])
            .current_dir(root)
            .output()?;
        let stat = String::from_utf8_lossy(&diff_stat.stdout).to_string();
        if stat.trim().is_empty() {
            anyhow::bail!("No staged changes");
        }

        // Parse the diff stat to create a meaningful message
        let files_changed: Vec<&str> = stat.lines()
            .filter(|l| l.contains('|'))
            .filter_map(|l| l.split('|').next())
            .map(|s| s.trim())
            .collect();

        let file_count = files_changed.len();
        let insertions = stat.lines()
            .filter_map(|l| {
                let parts: Vec<&str> = l.split(['+', '-', ' ']).collect();
                parts.iter().filter_map(|p| p.parse::<usize>().ok()).next_back()
            })
            .sum::<usize>();

        let message = if file_count <= 3 {
            let files_str = files_changed.join(", ");
            format!("feat: update {files_str} ({insertions} lines changed)")
        } else {
            format!("feat: update {file_count} files ({insertions} lines changed)")
        };

        Ok(message)
    }

    /// Commit with auto-generated or custom message
    pub fn smart_commit(root: &Path, custom_message: Option<&str>) -> Result<String> {
        let message = match custom_message {
            Some(m) => m.to_string(),
            None => Self::generate_commit_message(root)?,
        };

        Command::new("git")
            .args(["commit", "-m", &message])
            .current_dir(root)
            .output()?;

        Ok(message)
    }

    /// Get staged diff for review
    pub fn get_staged_diff(root: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "--cached", "--no-color"])
            .current_dir(root)
            .output()?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    #[allow(dead_code)]
    /// Get changed files vs HEAD
    pub fn get_changed_files(root: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["diff", "--name-only", "--no-color"])
            .current_dir(root)
            .output()?;
        let files = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();
        Ok(files)
    }

    #[allow(dead_code)]
    /// Get file content at a specific commit
    pub fn show_file(root: &Path, commit: &str, path: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["show", &format!("{commit}:{path}")])
            .current_dir(root)
            .output()?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    #[allow(dead_code)]
    /// Get current branch name
    pub fn current_branch(root: &Path) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(root)
            .output()?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
