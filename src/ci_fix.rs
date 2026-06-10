//! CI auto-fix bot — analyzes CI failures and generates fixes.
//!
//! ```
//! hyper ci-fix ci.log --push
//! # or pipe from CI:
//! cargo check 2>&1 | hyper ci-fix
//! ```

use std::path::Path;
use crate::i18n;
use anyhow::{Context, Result};

/// Run the CI fix process
pub async fn run(log_path: Option<&Path>, branch: &str, push: bool) -> Result<()> {
    // 1. Read CI log
    let ci_log = match log_path {
        Some(path) => std::fs::read_to_string(path)
            .context(format!("Failed to read CI log: {}", path.display()))?,
        None => {
            // Read from stdin
            let mut input = String::new();
            for line in std::io::stdin().lines() {
                match line {
                    Ok(l) => input.push_str(&l),
                    Err(e) => break,
                }
                input.push('\n');
            }
            if input.trim().is_empty() {
                anyhow::bail!("No CI log provided. Pipe log via stdin or provide file path.");
            }
            input
        }
    };

    println!("{}", i18n::t_with("ci_fix_read", &[&ci_log.len().to_string()]));

    // 2. Analyze with LLM
    let analysis = analyze_ci_failure(&ci_log).await?;
    println!("{}: {}", i18n::t("ci_fix_analysis"), analysis);

    // 3. Apply fix
    let fix_applied = apply_fix(&analysis).await?;
    if !fix_applied {
        println!("❌ Could not auto-fix. Manual intervention needed.");
        return Ok(());
    }

    // 4. Commit and optionally push
    if push {
        let output = std::process::Command::new("git")
            .args(["checkout", "-b", branch])
            .output()
            .ok();
        if output.map_or(false, |o| o.status.success()) {
            std::process::Command::new("git")
                .args(["add", "-A"])
                .output().ok();
            std::process::Command::new("git")
                .args(["commit", "-m", &format!("fix: auto-fix CI failure\n\n{analysis}")])
                .output().ok();
            let push_out = std::process::Command::new("git")
                .args(["push", "origin", branch])
                .output().ok();
            if push_out.map_or(false, |o| o.status.success()) {
                println!("{}", i18n::t_with("ci_fix_applied", &[branch]));
                println!("   Create a PR: gh pr create --fill");
            } else {
                println!("⚠️  Commit created but push failed");
            }
        }
    }

    Ok(())
}

/// Use LLM to analyze CI log and identify the root cause
async fn analyze_ci_failure(log: &str) -> Result<String> {
    let config = crate::config::AppConfig::load();
    let mut pool = crate::llm::ProviderPool::new(&config.providers)?;

    let prompt = format!(
        "You are a CI failure analyzer. Given the following CI build log, identify:\n\
         1. The root cause of the failure (be specific: file, line, error type)\n\
         2. The exact fix needed (code change required)\n\
         3. Risk assessment (safe fix or potentially breaking)\n\n\
         CI LOG:\n```\n{}\n```\n\n\
         Respond with a concise analysis and the exact fix.",
        log.chars().take(8000).collect::<String>()  // limit log size
    );

    let response = pool.chat(vec![
        crate::llm::Message::text("system", "You are a CI failure analyzer. Be specific and actionable."),
        crate::llm::Message::text("user", &prompt),
    ]).await?;

    Ok(response)
}

/// Apply the fix using HyperAgent's code generation
async fn apply_fix(analysis: &str) -> Result<bool> {
    let config = crate::config::AppConfig::load();
    let mut pool = crate::llm::ProviderPool::new(&config.providers)?;

    let fix_prompt = format!(
        "Based on this CI failure analysis, generate the exact code changes needed:\n\n{}\n\n\
         Output the fix as a diff or the complete file content. Keep changes minimal.",
        analysis
    );

    // Use agent tools to apply the fix
    let response = pool.chat(vec![
        crate::llm::Message::text("system",
            "You are a code fixer. Output the COMPLETE corrected file content. Format:\n\
             FILE: path/to/file.rs\n```\n<content>\n```"),
        crate::llm::Message::text("user", &fix_prompt),
    ]).await?;

    // Parse the response for file changes
    let mut files_fixed = 0;
    for line in response.lines() {
        if line.starts_with("FILE:") {
            if let Some(path) = line.strip_prefix("FILE: ").or_else(|| line.strip_prefix("FILE:")) {
                let path = path.trim();
                let mut content = String::new();
                // Read until we hit ``` (next code block)
                for cl in response.lines().skip_while(|l| !l.starts_with("```")) {
                    if cl.trim() == "```" { break; }
                    content.push_str(cl);
                    content.push('\n');
                }
                if !content.is_empty() && std::path::Path::new(path).exists() {
                    std::fs::write(path, content.trim())?;
                    println!("   ✅ Fixed: {}", path);
                    files_fixed += 1;
                }
            }
        }
    }

    Ok(files_fixed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_run_with_nonexistent_file_fails() {
        let path = PathBuf::from("/nonexistent/ci_log_xyz.log");
        let result = run(Some(&path), "fix-branch", false).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Failed to read") || msg.contains("No such file"));
    }

    #[test]
    fn test_module_compiles() {
        // Verify module-level types and constants
        // Just a smoke test for compilation
        let _: Option<&Path> = None;
    }
}
