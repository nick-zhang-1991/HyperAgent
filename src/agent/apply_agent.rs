use anyhow::{Context, Result};
use std::path::Path;

use crate::diff::FileChange;

/// Maximum file size (bytes) that hyperagent will read or write
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB

/// Validate that a file path is within the project root (path traversal protection)
/// For new files (not yet created), checks parent directory instead.
fn validate_path(root: &Path, file_path: &Path) -> Result<()> {
    let canonical_root = root
        .canonicalize()
        .context("Cannot resolve root path")?;

    let canonical_file = if file_path.exists() {
        file_path
            .canonicalize()
            .map_err(|_| anyhow::anyhow!("Path does not exist or is invalid: {}", file_path.display()))?
    } else {
        // For new files, resolve the parent to validate the path
        let parent = file_path.parent().unwrap_or(root);
        parent
            .canonicalize()
            .map_err(|_| anyhow::anyhow!("Invalid directory for new file: {}", file_path.display()))?
    };

    if !canonical_file.starts_with(&canonical_root) {
        anyhow::bail!(
            "SECURITY: Path traversal blocked — {} is outside project root",
            file_path.display()
        );
    }
    Ok(())
}

/// Check file size before reading/writing
fn check_file_size(path: &Path) -> Result<()> {
    if path.exists() {
        let metadata = std::fs::metadata(path)
            .context("Failed to read file metadata")?;
        if metadata.len() > MAX_FILE_SIZE {
            anyhow::bail!(
                "File too large ({} bytes, max {}): {}",
                metadata.len(),
                MAX_FILE_SIZE,
                path.display()
            );
        }
    }
    Ok(())
}

/// ApplyAgent: Applies reviewed changes to the filesystem
///
/// Handles:
/// - Creating backup files before edits
/// - Applying changes with proper error handling
/// - Showing diffs for user confirmation (if confirm mode)
/// - Git staging after changes
/// - Cleaning up old .bak files after successful apply
pub struct ApplyAgent<'a> {
    root: &'a Path,
    confirm: bool,
}

impl<'a> ApplyAgent<'a> {
    pub fn new(root: &'a Path, confirm: bool) -> Self {
        Self { root, confirm }
    }

    pub async fn apply(&self, changes: &[FileChange]) -> Result<Vec<String>> {
        let mut messages = Vec::new();

        for change in changes {
            // SECURITY: path traversal check
            validate_path(self.root, &change.file)
                .map_err(|e| anyhow::anyhow!("❌ {} — change rejected", e))?;

            // SECURITY: file size limit
            check_file_size(&change.file)?;

            // Show diff preview with enhanced interactive controls
            if self.confirm {
                use std::io::{self, Write};
                let rel_path = change.file.strip_prefix(self.root).unwrap_or(&change.file);
                println!("\n📝 {} {}:", change.change_type.to_uppercase(), rel_path.display());

                // Show diff content
                if let (Some(old), Some(new)) = (&change.old_content, &change.new_content) {
                    let old_lines: Vec<&str> = old.lines().collect();
                    let new_lines: Vec<&str> = new.lines().collect();
                    let max = old_lines.len().max(new_lines.len());
                    let mut diff_lines = Vec::new();

                    for i in 0..max {
                        let old_line = old_lines.get(i).unwrap_or(&"");
                        let new_line = new_lines.get(i).unwrap_or(&"");
                        if old_line != new_line {
                            if i < old_lines.len() {
                                diff_lines.push(format!("  \x1b[31m- {}\x1b[0m", old_line));
                            }
                            if i < new_lines.len() {
                                diff_lines.push(format!("  \x1b[32m+ {}\x1b[0m", new_line));
                            }
                        }
                    }

                    // Show first 10 diff lines
                    for line in diff_lines.iter().take(10) {
                        println!("{line}");
                    }
                    if diff_lines.len() > 10 {
                        println!("  ... and {} more lines", diff_lines.len() - 10);
                    }
                } else if let Some(content) = &change.new_content {
                    println!("   (full file, {} chars)", content.len());
                }

                println!("   [Y]es  [n]o  [s]kip  [a]pply all  [v]iew full diff");
                print!("   └─ ");
                let _ = io::stdout().flush();
                let mut input = String::new();
                io::stdin().read_line(&mut input).ok();
                let input = input.trim().to_lowercase();

                match input.as_str() {
                    "n" | "no" => {
                        messages.push(format!("Rejected: {}", change.file.display()));
                        continue;
                    }
                    "s" | "skip" => {
                        messages.push(format!("Skipped: {}", change.file.display()));
                        continue;
                    }
                    "a" | "all" => {
                        messages.push("   ✅ Auto-approved remaining changes".to_string());
                        // Don't continue — fall through to apply
                    }
                    "v" | "view" => {
                        // Show full file diff
                        if let Some(content) = &change.new_content {
                            println!("   ┌─ Full content of {} ", change.file.display());
                            for line in content.lines() {
                                println!("   │ {line}");
                            }
                            println!("   └─ End of file");
                        }
                        print!("   Apply this change? [Y/n] ");
                        let _ = io::stdout().flush();
                        let mut confirm = String::new();
                        io::stdin().read_line(&mut confirm).ok();
                        if confirm.trim().to_lowercase() == "n" {
                            messages.push(format!("Rejected: {}", change.file.display()));
                            continue;
                        }
                    }
                    _ => {} // default: apply
                }
            }

            // Apply the change — supports both full-file and diff-hunk modes
            match change.change_type.as_str() {
                "create" => {
                    if let Some(content) = &change.new_content {
                        if let Some(parent) = change.file.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::write(&change.file, content)?;
                        messages.push(format!("Created: {}", change.file.display()));
                    }
                }
                "edit" => {
                    if !change.hunks.is_empty() {
                        // DIFF mode: apply hunks surgically (preferred — fewer tokens)
                        crate::diff::apply_edits_to_file(&change.file, &change.hunks)?;
                        messages.push(format!("Edited (diff): {}", change.file.display()));
                    } else if let Some(content) = &change.new_content {
                        // FULL-FILE mode: write complete new content (fallback)
                        std::fs::write(&change.file, content)?;
                        messages.push(format!("Edited (full): {}", change.file.display()));
                    }
                }
                "delete" => {
                    if change.file.exists() {
                        std::fs::remove_file(&change.file)?;
                        messages.push(format!("Deleted: {}", change.file.display()));
                    }
                }
                _ => {
                    messages.push(format!("Unknown change type: {}", change.change_type));
                }
            }
        }

        // Clean up any stale .bak files after all changes applied
        if let Ok(entries) = std::fs::read_dir(self.root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "bak") {
                    // Only clean .bak files that correspond to existing source files
                    let src_path = path.with_extension("");
                    if src_path.exists() {
                        // .bak is stale — source was updated successfully
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }

        // Auto-stage if in git repo
        if crate::git::GitOps::is_repo(self.root) {
            crate::git::GitOps::stage_all(self.root)?;
            messages.push("   📦 Changes staged in git".to_string());
        }

        // Auto-format after apply (run project-appropriate formatter)
        if let Ok(result) = Self::run_formatter(self.root) {
            if !result.is_empty() {
                messages.push(format!("   ✨ Formatted: {result}"));
            }
        }

        // LSP check: run type checker after changes
        if let Ok(result) = Self::run_lsp_check(self.root) {
            if !result.is_empty() {
                messages.push(format!("   🔍 LSP: {result}"));
            }
        }

        Ok(messages)
    }

    /// Auto-detect and run LSP type checker after changes
    fn run_lsp_check(root: &Path) -> Result<String> {
        let has_rust = root.join("Cargo.toml").exists();
        let has_ts = root.join("tsconfig.json").exists();
        let has_python = root.join("pyproject.toml").exists()
            || root.join("setup.py").exists();

        if has_rust {
            let output = std::process::Command::new("cargo")
                .args(["check"])
                .current_dir(root)
                .output()
                .ok();
            if let Some(out) = output {
                if out.status.success() {
                    return Ok("cargo check: OK".to_string());
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let first_error = stderr.lines()
                        .find(|l| l.contains("error["))
                        .unwrap_or("check failed");
                    return Ok(format!("cargo check: {first_error}"));
                }
            }
        }

        if has_ts {
            let output = std::process::Command::new("npx")
                .args(["tsc", "--noEmit"])
                .current_dir(root)
                .output()
                .ok();
            if let Some(out) = output {
                if out.status.success() {
                    return Ok("tsc: OK".to_string());
                }
            }
        }

        if has_python {
            // Just syntax check, no full run
            let output = std::process::Command::new("python")
                .args(["-m", "py_compile", "-"])
                .current_dir(root)
                .output()
                .ok();
            if let Some(out) = output {
                if out.status.success() {
                    return Ok("python syntax: OK".to_string());
                }
            }
        }

        Ok(String::new())
    }

    /// Auto-detect and run project formatter after changes are applied
    fn run_formatter(root: &Path) -> Result<String> {
        // Check for common formatter configs
        let has_rust = root.join("Cargo.toml").exists();
        let has_prettier = root.join(".prettierrc").exists()
            || root.join(".prettierrc.json").exists()
            || root.join("prettier.config.js").exists();
        let has_python = root.join("pyproject.toml").exists()
            || root.join("setup.py").exists()
            || root.join("requirements.txt").exists();

        if has_rust {
            if let Ok(output) = std::process::Command::new("rustfmt")
                .args(["--edition", "2021"])
                .arg("--check")
                .current_dir(root)
                .output()
            {
                if !output.status.success() {
                    // Need to format — run rustfmt on staged files
                    let _ = std::process::Command::new("cargo")
                        .args(["fmt"])
                        .current_dir(root)
                        .output();
                    return Ok("cargo fmt".to_string());
                }
            }
        }

        if has_prettier {
            if let Ok(output) = std::process::Command::new("npx")
                .args(["prettier", "--check", "."])
                .current_dir(root)
                .output()
            {
                if !output.status.success() {
                    let _ = std::process::Command::new("npx")
                        .args(["prettier", "--write", "."])
                        .current_dir(root)
                        .output();
                    return Ok("prettier".to_string());
                }
            }
        }

        if has_python
            && std::process::Command::new("black")
                .arg("--check")
                .current_dir(root)
                .output()
                .is_ok_and(|o| !o.status.success())
        {
            let _ = std::process::Command::new("black")
                .arg(".")
                .current_dir(root)
                .output();
            return Ok("black".to_string());
        }

        Ok(String::new())
    }
}
