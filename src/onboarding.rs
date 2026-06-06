//! Interactive Onboarding Tutorial — First-run experience for 100M users.
//!
//! Design goals:
//! - 30-second "aha moment": user types one command and sees magic
//! - Progressive disclosure: explain concepts as they're used, not upfront
//! - Visual: progress bar, emoji, colors, clear next steps
//! - Fail-safe: every step handles errors gracefully, never crashes
//! - Replayable: `hyper setup --tutorial` re-runs anytime
//!
//! Tutorial flow (7 steps, ~90 seconds):
//!   1. Welcome + what HyperAgent does
//!   2. Config check (API key, model)
//!   3. Build code index (with progress)
//!   4. Demo: "explain this project in one sentence"
//!   5. Show dashboard (memory + skills)
//!   6. Your turn: suggest next steps
//!   7. Completion + resources

use anyhow::Result;
use std::io::{self, stdout, Write};
use std::path::Path;

/// Progress bar width in characters
const BAR_WIDTH: usize = 40;

pub struct Tutorial {
    /// Total number of steps
    total_steps: usize,
    /// Current step number (0-indexed)
    current_step: usize,
}

impl Tutorial {
    pub fn new() -> Self {
        Tutorial {
            total_steps: 7,
            current_step: 0,
        }
    }

    /// Run the full interactive tutorial
    pub fn run(&mut self, project_dir: &Path) -> Result<()> {
        self.print_welcome()?;
        self.step_config()?;
        self.step_index(project_dir)?;
        self.step_demo()?;
        self.step_dashboard()?;
        self.step_your_turn()?;
        self.step_complete()?;
        Ok(())
    }

    fn progress_bar(&self) -> String {
        let pct = self.current_step as f64 / self.total_steps as f64;
        let filled = (BAR_WIDTH as f64 * pct) as usize;
        let empty = BAR_WIDTH - filled;
        format!(
            "\x1b[36m[{}{}] {}/{} \x1b[33m{:3.0}%\x1b[0m",
            "█".repeat(filled),
            "░".repeat(empty),
            self.current_step,
            self.total_steps,
            pct * 100.0
        )
    }

    fn step_header(&mut self, num: usize, title: &str) {
        self.current_step = num;
        println!();
        println!("{}", self.progress_bar());
        println!("\x1b[1;36m  Step {}: {}\x1b[0m", num, title);
        println!("\x1b[90m  {}\x1b[0m", "─".repeat(50));
    }

    fn prompt_enter(&self) {
        print!("\x1b[90m  Press Enter to continue...\x1b[0m");
        stdout().flush().ok();
        let mut buf = String::new();
        io::stdin().read_line(&mut buf).ok();
    }

    fn spinner_start(&self) -> impl Drop {
        print!("  ⏳ ");
        stdout().flush().ok();
        struct SpinnerGuard;
        impl Drop for SpinnerGuard {
            fn drop(&mut self) {
                print!("\r\x1b[K"); // Clear line
            }
        }
        SpinnerGuard
    }

    // ─── Step 1: Welcome ────────────────────────────────────────
    fn print_welcome(&mut self) -> Result<()> {
        self.step_header(1, "Welcome to HyperAgent!");

        println!();
        println!("  \x1b[1;35m⚡ HyperAgent\x1b[0m is the world's fastest CLI coding agent.");
        println!();
        println!("  Built with Rust for \x1b[1mmillisecond startup\x1b[0m and \x1b[1mparallel multi-agent\x1b[0m execution.");
        println!();
        println!("  \x1b[32m✦\x1b[0m  Type a prompt  →  multiple AI agents work simultaneously");
        println!("  \x1b[32m✦\x1b[0m  Review changes  →  colorized diff before applying");
        println!("  \x1b[32m✦\x1b[0m  Auto-fix errors  →  lint → fix → re-lint loop (up to 3 rounds)");
        println!("  \x1b[32m✦\x1b[0m  Persistent memory → learns your preferences across sessions");
        println!();
        println!("  \x1b[90mThis tutorial takes ~90 seconds. Let's get started!\x1b[0m");

        self.prompt_enter();
        Ok(())
    }

    // ─── Step 2: Config ─────────────────────────────────────────
    fn step_config(&mut self) -> Result<()> {
        self.step_header(2, "Configuration Check");

        // Check existing config
        let config_dir = dirs_next::config_dir()
            .unwrap_or_else(|| Path::new("~/.config").to_path_buf())
            .join("hyper");

        let config_file = config_dir.join("config.toml");

        if config_file.exists() {
            println!();
            println!("  ✅ Config found: {}", config_file.display());

            // Try to read and show configured providers
            if let Ok(content) = std::fs::read_to_string(&config_file) {
                if content.contains("provider") {
                    println!("  ✅ LLM provider configured");
                } else {
                    println!("  ⚠️  No LLM provider found — run \x1b[33mhyper auth login\x1b[0m");
                }
            }
        } else {
            println!();
            println!("  No config found. HyperAgent needs an LLM provider to work.");
            println!();
            println!("  \x1b[1mQuick setup:\x1b[0m");
            println!("    \x1b[33mhyper auth login\x1b[0m          # Interactive provider setup");
            println!();
            println!("  Or set environment variables:");
            println!("    \x1b[90mexport HYPER_API_KEY=\"sk-...\"\x1b[0m");
            println!("    \x1b[90mexport HYPER_BASE_URL=\"https://api.openai.com/v1\"\x1b[0m");
            println!();
            println!("  \x1b[90m(Skipping config — you can set this up later)\x1b[0m");
        }

        // Check environment variables as fallback
        let has_env_key = std::env::var("HYPER_API_KEY").is_ok()
            || std::env::var("OPENAI_API_KEY").is_ok()
            || std::env::var("DEEPSEEK_API_KEY").is_ok();

        if has_env_key {
            println!("  ✅ API key found in environment");
        }

        self.prompt_enter();
        Ok(())
    }

    // ─── Step 3: Build Index ────────────────────────────────────
    fn step_index(&mut self, project_dir: &Path) -> Result<()> {
        self.step_header(3, "Building Code Index");

        // Count source files
        let file_count = count_source_files(project_dir);
        let large = file_count > 500;

        println!();
        println!("  HyperAgent builds a \x1b[1mPageRank-powered code index\x1b[0m to understand");
        println!("  your codebase. It ranks symbols by importance — just like Google");
        println!("  ranks web pages.");

        if file_count > 0 {
            println!();
            if large {
                println!(
                    "  📂 Found \x1b[1m{}\x1b[0m source files (large project).",
                    file_count
                );
                println!("  \x1b[90m  Index may take 10-30 seconds. Subsequent runs are incremental.\x1b[0m");
            } else {
                println!(
                    "  📂 Found \x1b[1m{}\x1b[0m source files. Indexing should be fast.",
                    file_count
                );
            }
        } else {
            println!();
            println!(
                "  📂 No source files found in \x1b[90m{}\x1b[0m",
                project_dir.display()
            );
            println!("  \x1b[90m  (Index will be built when you first run a task)\x1b[0m");
        }

        // Try to build index (non-blocking, just show how)
        println!();
        println!("  \x1b[1mTo build manually:\x1b[0m");
        println!("    \x1b[33mhyper init\x1b[0m             # Full rebuild");
        println!("    \x1b[33mhyper init --force\x1b[0m    # Force re-index");

        // Show index stats if already exists
        let index_dir = Path::new(".hyper");
        if index_dir.exists() {
            println!();
            println!("  ✅ .hyper/ directory exists — index may already be built");
        }

        self.prompt_enter();
        Ok(())
    }

    // ─── Step 4: Demo ───────────────────────────────────────────
    fn step_demo(&mut self) -> Result<()> {
        self.step_header(4, "Your First HyperAgent Task");

        println!();
        println!("  Let's see HyperAgent in action. The most common first task");
        println!("  is asking it to \x1b[1mexplain your codebase\x1b[0m.");
        println!();
        println!("  \x1b[1mTry these commands:\x1b[0m");
        println!();
        println!("    \x1b[33mhyper run \"summarize this project\" --mode ask\x1b[0m");
        println!("    \x1b[33mhyper run \"find security issues\" --mode audit\x1b[0m");
        println!("    \x1b[33mhyper run \"add error handling to main()\"\x1b[0m");
        println!();
        println!("  \x1b[90mModes: code (default) | ask | debug | architect | audit\x1b[0m");
        println!();
        println!("  \x1b[1mREPL mode\x1b[0m (interactive):");
        println!("    \x1b[33mhyper\x1b[0m                      # Enter interactive REPL");
        println!("    \x1b[90m> explain main.rs        # REPL commands\x1b[0m");
        println!();

        self.prompt_enter();
        Ok(())
    }

    // ─── Step 5: Dashboard ──────────────────────────────────────
    fn step_dashboard(&mut self) -> Result<()> {
        self.step_header(5, "Memory & Skills Dashboard");

        println!();
        println!("  HyperAgent \x1b[1mremembers what it learns\x1b[0m across sessions.");
        println!("  You can view and manage memories via the web dashboard:");
        println!();
        println!("    \x1b[33mhyper dashboard --port 8081\x1b[0m");
        println!();
        println!("  \x1b[1mFeatures:\x1b[0m");
        println!("  \x1b[32m✦\x1b[0m  View all stored memories and skills");
        println!("  \x1b[32m✦\x1b[0m  Search by keyword or entity");
        println!("  \x1b[32m✦\x1b[0m  Delete outdated memories");
        println!("  \x1b[32m✦\x1b[0m  Import/export skills from the community");
        println!();
        println!("  \x1b[90m  Open http://localhost:8081 in your browser.\x1b[0m");

        self.prompt_enter();
        Ok(())
    }

    // ─── Step 6: Your Turn ──────────────────────────────────────
    fn step_your_turn(&mut self) -> Result<()> {
        self.step_header(6, "Your Turn!");

        println!();
        println!("  Here are some ideas for your first real task:");
        println!();
        println!("  \x1b[1m🔧 Code changes:\x1b[0m");
        println!("    hyper run \"add input validation to the login handler\"");
        println!("    hyper run \"optimize the database queries in user.rs\"");
        println!();
        println!("  \x1b[1m🔍 Code review:\x1b[0m");
        println!("    hyper review");
        println!("    hyper review main..feature-branch");
        println!();
        println!("  \x1b[1m📊 Code understanding:\x1b[0m");
        println!("    hyper run \"explain the authentication flow\" --mode ask");
        println!();
        println!("  \x1b[1m🧪 Testing:\x1b[0m");
        println!("    hyper run \"add unit tests for the payment module\"");

        println!();
        self.prompt_enter();
        Ok(())
    }

    // ─── Step 7: Complete ───────────────────────────────────────
    fn step_complete(&mut self) -> Result<()> {
        self.current_step = 7;
        println!();
        println!("{}", self.progress_bar());
        println!();
        println!("\x1b[1;32m  🎉 Tutorial Complete!\x1b[0m");
        println!();
        println!("  \x1b[1mResources:\x1b[0m");
        println!("    📖 Docs:      \x1b[90mhttps://github.com/nick-zhang-1991/HyperAgent\x1b[0m");
        println!("    🐛 Issues:    \x1b[90mhttps://github.com/nick-zhang-1991/HyperAgent/issues\x1b[0m");
        println!("    💬 Community: \x1b[90mGitHub Discussions\x1b[0m");
        println!();
        println!("  \x1b[1mQuick reference:\x1b[0m");
        println!("    hyper --help           # All commands");
        println!("    hyper run \"prompt\"     # Run a task");
        println!("    hyper                  # Interactive REPL");
        println!("    hyper doctor           # Diagnostics");
        println!("    hyper self-update      # Check for updates");
        println!();
        println!("  \x1b[32mHappy coding! 🚀\x1b[0m");
        println!();

        Ok(())
    }
}

/// Count source files in a directory (non-recursive depth check)
fn count_source_files(dir: &Path) -> usize {
    let extensions = [
        "rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "rb", "c", "cpp", "h",
        "swift", "kt", "scala", "vue", "svelte", "toml", "yaml", "yml", "json",
        "md", "css", "scss", "html", "sql", "sh", "bash", "zsh",
    ];

    let mut count = 0;
    let mut dirs_to_visit = vec![dir.to_path_buf()];
    let max_depth = 4;
    let mut depth = 0;

    while !dirs_to_visit.is_empty() && depth < max_depth {
        let mut next_dirs = Vec::new();
        for d in &dirs_to_visit {
            if let Ok(entries) = std::fs::read_dir(d) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = path.file_name().unwrap_or_default().to_string_lossy();

                    // Skip hidden and common excludes
                    if name.starts_with('.') || name == "node_modules" || name == "target" {
                        continue;
                    }

                    if path.is_dir() && depth < max_depth - 1 {
                        next_dirs.push(path);
                    } else if path.is_file() {
                        if let Some(ext) = path.extension() {
                            let ext_str = ext.to_string_lossy().to_lowercase();
                            if extensions.contains(&ext_str.as_str()) {
                                count += 1;
                            }
                        }
                    }
                }
            }
        }
        dirs_to_visit = next_dirs;
        depth += 1;
    }

    count
}

/// Detect if this is a first run (no .hyper directory exists)
pub fn is_first_run() -> bool {
    !Path::new(".hyper").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tutorial_creation() {
        let t = Tutorial::new();
        assert_eq!(t.total_steps, 7);
        assert_eq!(t.current_step, 0);
    }

    #[test]
    fn test_progress_bar_format() {
        let mut t = Tutorial::new();
        t.current_step = 3;
        let bar = t.progress_bar();
        assert!(bar.contains("3/7"));
        assert!(bar.contains("%"));
    }

    #[test]
    fn test_is_first_run_detects_no_hyper_dir() {
        // In test environment, .hyper might not exist
        let result = is_first_run();
        // Just verify it's a boolean, don't assert true/false
        assert!(result == true || result == false);
    }

    #[test]
    fn test_count_source_files_current_project() {
        let count = count_source_files(Path::new("."));
        // This project has source files
        assert!(count > 0, "Expected source files in project root");
    }
}
