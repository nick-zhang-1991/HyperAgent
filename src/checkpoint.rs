//! Checkpoint system — snapshot-based undo/redo for agent operations
//!
//! Before each agent run, a checkpoint is created (saving the git diff
//! of files that will change). After the run, the checkpoint shows what
//! changed and allows selective undo.
//!
//! # Usage
//! ```bash
//! hyper checkpoint list              # Show all checkpoints
//! hyper checkpoint diff <id>         # Show diff for a checkpoint
//! hyper checkpoint undo <id>         # Revert a specific checkpoint
//! hyper checkpoint undo <id> --keep  # Revert but keep changes staged
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A checkpoint entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub timestamp: u64,
    pub prompt: String,
    pub diff: String,
    pub files_changed: Vec<String>,
    pub status: String, // "active", "reverted", "partial"
}

/// Checkpoint manager
pub struct CheckpointManager {
    dir: PathBuf,
}

impl CheckpointManager {
    /// Create a new checkpoint manager for a project
    pub fn new(root: &Path) -> Self {
        let dir = root.join(".hyper").join("checkpoints");
        Self { dir }
    }

    /// Ensure checkpoints directory exists
    fn ensure_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("Cannot create checkpoints dir: {}", self.dir.display()))
    }

    /// Create a checkpoint from the current git diff
    pub fn create(&self, prompt: &str) -> Result<Checkpoint> {
        self.ensure_dir()?;

        // Get the git diff of working tree changes
        let diff_output = self.capture_git_diff()?;
        let files_changed = self.parse_changed_files(&diff_output);

        if diff_output.trim().is_empty() {
            // No changes to save — still create a checkpoint with empty diff
            // so the system can track that a run happened
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        let id = format!("cp-{:x}", uuid::Uuid::new_v4().as_u128())[..18].to_string();

        let checkpoint = Checkpoint {
            id: id.clone(),
            timestamp,
            prompt: prompt.to_string(),
            diff: diff_output.clone(),
            files_changed,
            status: "active".to_string(),
        };

        // Save checkpoint to file
        let path = self.dir.join(format!("{id}.json"));
        let json = serde_json::to_string_pretty(&checkpoint)?;
        std::fs::write(&path, json)?;

        // Also save the diff as a standalone patch file for easy viewing
        if !diff_output.trim().is_empty() {
            let patch_path = self.dir.join(format!("{id}.patch"));
            std::fs::write(&patch_path, &diff_output)?;
        }

        Ok(checkpoint)
    }

    /// List all checkpoints (newest first)
    pub fn list(&self) -> Result<Vec<Checkpoint>> {
        self.ensure_dir()?;
        let mut checkpoints = Vec::new();

        let entries = std::fs::read_dir(&self.dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false)
                && path.file_stem().map(|s| s.to_string_lossy().starts_with("cp-")).unwrap_or(false)
            {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(cp) = serde_json::from_str::<Checkpoint>(&content) {
                        checkpoints.push(cp);
                    }
                }
            }
        }

        // Sort newest first
        checkpoints.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(checkpoints)
    }

    /// Get a specific checkpoint by ID
    pub fn get(&self, id: &str) -> Result<Checkpoint> {
        let path = self.dir.join(format!("{id}.json"));
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Checkpoint '{id}' not found"))?;
        let cp: Checkpoint = serde_json::from_str(&content)?;
        Ok(cp)
    }

    /// Show diff for a checkpoint (using the patch file or embedded diff)
    pub fn show_diff(&self, id: &str) -> Result<String> {
        let cp = self.get(id)?;

        // Try patch file first (richer content)
        let patch_path = self.dir.join(format!("{id}.patch"));
        if patch_path.exists() {
            let content = std::fs::read_to_string(&patch_path)?;
            if !content.trim().is_empty() {
                return Ok(content);
            }
        }

        // Fall back to embedded diff
        if cp.diff.trim().is_empty() {
            Ok(String::from("(no changes recorded — no files were modified)"))
        } else {
            Ok(cp.diff)
        }
    }

    /// Undo/revert a checkpoint by applying the reverse diff
    pub fn undo(&self, id: &str, keep_staged: bool) -> Result<()> {
        let cp = self.get(id)?;

        if cp.status == "reverted" {
            anyhow::bail!("Checkpoint '{id}' has already been reverted");
        }

        let diff = self.show_diff(id)?;
        if diff.trim().is_empty() || diff.contains("no changes recorded") {
            // Mark as reverted even if no changes
            self.update_status(id, "reverted")?;
            println!("   ✅ Checkpoint '{id}' marked as reverted (no files to restore)");
            return Ok(());
        }

        // Apply the reverse patch using `git apply --reverse`
        let patch_path = self.dir.join(format!("{id}.patch"));
        let mut cmd = std::process::Command::new("git");
        cmd.arg("apply").arg("--reverse");

        if keep_staged {
            cmd.arg("--cached");
        }

        // Write diff to a temp file and pipe it
        let output = cmd
            .arg(&patch_path)
            .output()
            .context("Failed to run git apply --reverse")?;

        if output.status.success() {
            self.update_status(id, "reverted")?;
            println!("   ✅ Reverted checkpoint '{id}': {} files restored", cp.files_changed.len());
            for f in &cp.files_changed {
                println!("     ↻ {}", f);
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If git apply fails, try `git checkout` approach for individual files
            eprintln!("   ⚠️  Git reverse apply failed: {stderr}");
            eprintln!("   Trying file-by-file checkout...");

            let mut restored = 0;
            for file in &cp.files_changed {
                let file_path = PathBuf::from(file);
                if file_path.exists() {
                    let checkout_output = std::process::Command::new("git")
                        .args(["checkout", "--", file])
                        .output()
                        .context("Failed to checkout file")?;
                    if checkout_output.status.success() {
                        restored += 1;
                    }
                }
            }

            if restored > 0 {
                self.update_status(id, "partial")?;
                println!("   ⚠️  Partially reverted: {restored}/{} files", cp.files_changed.len());
            } else {
                anyhow::bail!("Failed to revert checkpoint '{id}': {stderr}");
            }
        }

        Ok(())
    }

    /// Update checkpoint status
    fn update_status(&self, id: &str, status: &str) -> Result<()> {
        let path = self.dir.join(format!("{id}.json"));
        let mut cp: Checkpoint = {
            let content = std::fs::read_to_string(&path)?;
            serde_json::from_str(&content)?
        };
        cp.status = status.to_string();
        let json = serde_json::to_string_pretty(&cp)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Capture git diff of working tree (unstaged + staged changes)
    fn capture_git_diff(&self) -> Result<String> {
        let output = std::process::Command::new("git")
            .args(["diff", "--no-color"])
            .current_dir(&self.dir.parent().unwrap_or(Path::new(".")).parent().unwrap_or(Path::new(".")))
            .output()
            .context("Failed to run git diff")?;

        let unstaged = String::from_utf8_lossy(&output.stdout).to_string();

        // Also capture staged changes
        let staged_output = std::process::Command::new("git")
            .args(["diff", "--cached", "--no-color"])
            .current_dir(&self.dir.parent().unwrap_or(Path::new(".")).parent().unwrap_or(Path::new(".")))
            .output()
            .ok();

        let staged = staged_output
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        if unstaged.trim().is_empty() && staged.trim().is_empty() {
            return Ok(String::new());
        }

        // Combine both diffs
        let mut combined = String::new();
        if !staged.trim().is_empty() {
            combined.push_str("# Staged changes:\n");
            combined.push_str(&staged);
            combined.push('\n');
        }
        if !unstaged.trim().is_empty() {
            combined.push_str("# Unstaged changes:\n");
            combined.push_str(&unstaged);
        }

        Ok(combined)
    }

    /// Parse changed file paths from a git diff
    fn parse_changed_files(&self, diff: &str) -> Vec<String> {
        let mut files = Vec::new();
        for line in diff.lines() {
            if line.starts_with("diff --git a/") {
                // Extract path: "diff --git a/path/to/file b/path/to/file"
                if let Some(path_part) = line.split_whitespace().nth(2) {
                    let file = path_part.trim_start_matches("a/");
                    if !files.contains(&file.to_string()) {
                        files.push(file.to_string());
                    }
                }
            }
        }
        files
    }
}

/// Display checkpoints as a table
pub fn display_checkpoints(checkpoints: &[Checkpoint]) {
    if checkpoints.is_empty() {
        println!("   📦 No checkpoints yet.");
        println!("   Checkpoints are created automatically when running agents.");
        return;
    }

    println!("   {:<14} {:<20} {:<10} {:<8} {:<30}", "ID", "Time", "Files", "Status", "Prompt");
    println!("   {:-<14} {:-<20} {:-<10} {:-<8} {:-<30}", "", "", "", "", "");
    for cp in checkpoints {
        let time = {
            let secs = cp.timestamp;
            let hours = secs / 3600 % 24;
            let mins = secs / 60 % 60;
            format!("{:02}:{:02}", hours, mins)
        };
        let status_icon = match cp.status.as_str() {
            "active" => "🟢",
            "reverted" => "🔄",
            "partial" => "⚠️",
            _ => "⚪",
        };
        let prompt_short = if cp.prompt.len() > 28 {
            format!("{}...", &cp.prompt[..25])
        } else {
            cp.prompt.clone()
        };
        println!("   {:<14} {:<20} {:<10} {:<8} {:<30}",
            cp.id, time, cp.files_changed.len(), status_icon, prompt_short);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_list() {
        let dir = tempdir().unwrap();
        let mgr = CheckpointManager::new(dir.path());
        let cp = mgr.create("test run").unwrap();
        assert!(cp.id.starts_with("cp-"));
        assert_eq!(cp.prompt, "test run");
        assert_eq!(cp.status, "active");

        let list = mgr.list().unwrap();
        assert!(!list.is_empty());
        assert_eq!(list[0].id, cp.id);
    }

    #[test]
    fn test_multiple_checkpoints() {
        let dir = tempdir().unwrap();
        let mgr = CheckpointManager::new(dir.path());
        mgr.create("first").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        mgr.create("second").unwrap();

        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 2);
        // Should be newest first (or equal if same second)
        assert!(list[0].timestamp >= list[1].timestamp);
    }

    #[test]
    fn test_get_nonexistent() {
        let dir = tempdir().unwrap();
        let mgr = CheckpointManager::new(dir.path());
        assert!(mgr.get("cp-nonexistent").is_err());
    }

    #[test]
    fn test_parse_diff() {
        let mgr = CheckpointManager::new(Path::new("/tmp"));
        let diff = r#"diff --git a/src/main.rs b/src/main.rs
index abc..def 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn hello() {
+    println!("world");
 }
diff --git a/src/lib.rs b/src/lib.rs
index 123..456 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,6 +10,7 @@
 pub fn greet() {
     println!("hi");
+    println!("there");
 }
"#;
        let files = mgr.parse_changed_files(diff);
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"src/main.rs".to_string()));
        assert!(files.contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn test_update_status() {
        let dir = tempdir().unwrap();
        let mgr = CheckpointManager::new(dir.path());
        let cp = mgr.create("test").unwrap();
        assert_eq!(cp.status, "active");

        mgr.update_status(&cp.id, "reverted").unwrap();
        let updated = mgr.get(&cp.id).unwrap();
        assert_eq!(updated.status, "reverted");
    }

    #[test]
    fn test_show_diff_no_changes() {
        let dir = tempdir().unwrap();
        let mgr = CheckpointManager::new(dir.path());
        let cp = mgr.create("no changes").unwrap();
        let diff = mgr.show_diff(&cp.id).unwrap();
        // Should indicate no changes
        assert!(diff.contains("no changes") || diff.is_empty());
    }
}
