#![allow(unused)]
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Manages git worktree lifecycle for parallel agent isolation
///
/// Each parallel agent gets its own git worktree so they can safely
/// modify files without conflicting. After agents complete, we diff
/// each worktree against the common HEAD and collect changes.
pub struct WorktreeManager {
    main_repo: PathBuf,
    worktrees: Vec<Worktree>,
    /// Prefix for worktree directory names
    prefix: String,
}

struct Worktree {
    name: String,
    path: PathBuf,
}

impl WorktreeManager {
    /// Create a new worktree manager tied to a git repo
    pub fn new(main_repo: &Path, prefix: &str) -> Self {
        Self {
            main_repo: main_repo.to_path_buf(),
            worktrees: Vec::new(),
            prefix: prefix.to_string(),
        }
    }

    /// Create N parallel worktrees, each starting from HEAD
    /// Returns the list of worktree root paths
    pub async fn create_worktrees(&mut self, count: usize) -> Result<Vec<PathBuf>> {
        if count == 0 {
            return Ok(vec![]);
        }

        // Ensure we're in a git repo at HEAD with no uncommitted changes
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.main_repo)
            .output()
            .context("git status failed")?;

        let dirty = String::from_utf8_lossy(&status.stdout);
        if !dirty.trim().is_empty() {
            anyhow::bail!(
                "Worktree isolation requires clean working directory. \
                 Found uncommitted changes:\n{}",
                dirty
            );
        }

        let mut paths = Vec::with_capacity(count);

        // Get current HEAD commit hash to anchor all worktrees
        let head_output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.main_repo)
            .output()
            .context("git rev-parse HEAD failed")?;
        let head_hash = String::from_utf8_lossy(&head_output.stdout).trim().to_string();

        let parent = self.main_repo.parent()
            .unwrap_or(Path::new("/tmp"));

        for i in 0..count {
            let worktree_name = format!("{}-agent-{}", self.prefix, i);
            let worktree_path = parent.join(&worktree_name);

            let output = Command::new("git")
                .args([
                    "worktree", "add",
                    "--force",  // allow if previous unclean removal
                    worktree_path.to_str().unwrap(),
                    &head_hash,
                ])
                .current_dir(&self.main_repo)
                .output()
                .with_context(|| format!("Failed to create worktree {worktree_name}"))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // If worktree already exists, try prunning first
                if stderr.contains("already exists") {
                    let _ = Command::new("git")
                        .args(["worktree", "prune"])
                        .current_dir(&self.main_repo)
                        .output();

                    Command::new("git")
                        .args([
                            "worktree", "add",
                            worktree_path.to_str().unwrap(),
                            &head_hash,
                        ])
                        .current_dir(&self.main_repo)
                        .output()
                        .with_context(|| format!("Retry create worktree {worktree_name}"))?;
                } else {
                    anyhow::bail!("git worktree add failed: {stderr}");
                }
            }

            self.worktrees.push(Worktree {
                name: worktree_name.clone(),
                path: worktree_path.clone(),
            });
            paths.push(worktree_path);

            println!("   🌳 Worktree created: {worktree_name}");
        }

        Ok(paths)
    }

    /// Get diff between a specific worktree and the main repo HEAD
    /// Returns unified diff as a string
    pub fn diff_worktree(&self, index: usize) -> Result<String> {
        let wt = &self.worktrees[index];
        if !wt.path.exists() {
            return Ok(String::new());
        }

        // Stage all changes in the worktree first
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(&wt.path)
            .output();

        // Get diff vs HEAD (within the worktree's own git ref)
        let output = Command::new("git")
            .args(["diff", "HEAD", "--no-color"])
            .current_dir(&wt.path)
            .output()
            .context("git diff in worktree failed")?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get list of changed files in a worktree
    pub fn changed_files(&self, index: usize) -> Result<Vec<String>> {
        let wt = &self.worktrees[index];
        if !wt.path.exists() {
            return Ok(vec![]);
        }

        let output = Command::new("git")
            .args(["diff", "HEAD", "--name-only", "--no-color"])
            .current_dir(&wt.path)
            .output()?;

        let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(files)
    }

    /// Get file content from a worktree (after changes)
    pub fn read_file(&self, index: usize, relative_path: &str) -> Result<Option<String>> {
        let wt = &self.worktrees[index];
        let full_path = wt.path.join(relative_path);
        if full_path.exists() {
            let content = std::fs::read_to_string(&full_path)
                .with_context(|| format!("Failed to read {full_path:?}"))?;
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }

    /// Check if a worktree's changes compile (Rust projects)
    pub fn check_compilation(&self, index: usize) -> Result<(bool, String)> {
        let wt = &self.worktrees[index];
        if !wt.path.exists() {
            return Ok((true, String::new()));
        }

        let has_cargo = wt.path.join("Cargo.toml").exists();
        let has_tsconfig = wt.path.join("tsconfig.json").exists();

        if !has_cargo && !has_tsconfig {
            return Ok((true, String::new()));
        }

        let (cmd, args) = if has_cargo {
            ("cargo", vec!["check"])
        } else {
            ("npx", vec!["tsc", "--noEmit"])
        };

        let output = Command::new(cmd)
            .args(&args)
            .current_dir(&wt.path)
            .output()
            .with_context(|| format!("{cmd} check failed"))?;

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if output.status.success() {
            Ok((true, stderr))
        } else {
            Ok((false, stderr))
        }
    }

    /// Clean up all worktrees
    pub async fn cleanup(&mut self) -> Result<()> {
        for wt in &self.worktrees {
            // Remove the worktree from git's tracking
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force", &wt.name])
                .current_dir(&self.main_repo)
                .output();

            // Clean up the directory if it still exists
            if wt.path.exists() {
                let _ = std::fs::remove_dir_all(&wt.path);
            }
        }

        // Prune stale worktree references
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(&self.main_repo)
            .output();

        self.worktrees.clear();
        Ok(())
    }

    /// Number of active worktrees
    pub fn len(&self) -> usize {
        self.worktrees.len()
    }
}

impl Drop for WorktreeManager {
    fn drop(&mut self) {
        if !self.worktrees.is_empty() {
            // Best-effort cleanup in drop (can't use async here)
            for wt in &self.worktrees {
                let _ = Command::new("git")
                    .args(["worktree", "remove", "--force", &wt.name])
                    .current_dir(&self.main_repo)
                    .output();
                if wt.path.exists() {
                    let _ = std::fs::remove_dir_all(&wt.path);
                }
            }
            let _ = Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(&self.main_repo)
                .output();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_repo() -> Result<(PathBuf, tempfile::TempDir)> {
        let tmp = tempfile::tempdir()?;
        let repo_path = tmp.path().join("test-repo");
        fs::create_dir_all(&repo_path)?;

        Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()?;

        // Configure git user for the test repo
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo_path)
            .output()?;
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&repo_path)
            .output()?;

        // Create an initial commit
        fs::write(repo_path.join("README.md"), "# Test")?;
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo_path)
            .output()?;
        Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(&repo_path)
            .output()?;

        Ok((repo_path, tmp))
    }

    #[tokio::test]
    async fn test_create_worktree() -> Result<()> {
        let (repo_path, _tmp) = setup_test_repo()?;
        let mut mgr = WorktreeManager::new(&repo_path, "test");
        let paths = mgr.create_worktrees(2).await?;

        assert_eq!(paths.len(), 2);
        assert!(paths[0].join("README.md").exists(), "Worktree should have initial files");

        mgr.cleanup().await?;
        assert_eq!(mgr.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_worktree_diff() -> Result<()> {
        let (repo_path, _tmp) = setup_test_repo()?;
        let mut mgr = WorktreeManager::new(&repo_path, "test");
        let paths = mgr.create_worktrees(1).await?;

        // Make a change in the worktree
        fs::write(paths[0].join("README.md"), "# Test\nModified content")?;

        let diff = mgr.diff_worktree(0)?;
        assert!(diff.contains("Modified content"), "Diff should capture the change: {diff}");

        let files = mgr.changed_files(0)?;
        assert!(!files.is_empty(), "Should detect changed files");

        mgr.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_worktree_no_changes() -> Result<()> {
        let (repo_path, _tmp) = setup_test_repo()?;
        let mut mgr = WorktreeManager::new(&repo_path, "test");
        let _paths = mgr.create_worktrees(1).await?;

        let diff = mgr.diff_worktree(0)?;
        assert!(diff.is_empty(), "No changes should produce empty diff: {diff:?}");

        let files = mgr.changed_files(0)?;
        assert!(files.is_empty(), "No files should be listed");

        mgr.cleanup().await?;
        Ok(())
    }

    #[test]
    fn test_worktree_manager_init() {
        let mgr = WorktreeManager::new(Path::new("/tmp/repo"), "hyper");
        assert_eq!(mgr.len(), 0);
    }
}
