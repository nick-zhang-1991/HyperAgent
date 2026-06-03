//! HyperAgent CLI - Subcommand-based interface
//!
//! Usage:
//!   hyper run <prompt>     - Run a coding task (default)
//!   hyper review [ref]     - Review git diff
//!   hyper init             - Build code index
//!   hyper session <cmd>    - Manage sessions
//!   hyper doctor           - System diagnostics
//!   hyper config           - Show config
//!   hyper agents           - List/run agents

use anyhow::Result;
use clap::{Parser, Subcommand, CommandFactory};
use clap_complete::{Generator, Shell};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::agent::orchestrator::Orchestrator;
use crate::hooks::HookRegistry;
use crate::index::HyperIndex;
use crate::llm::LlmProvider;
use crate::memory::{MemoryManager, SqliteMemoryStore};
use crate::session::{Session, SessionManager};

/// HyperAgent - Ultra-Fast CLI Coding Agent
///
/// An intelligent coding agent with global code understanding
/// and multi-agent parallel execution.
#[derive(Parser, Debug)]
#[command(name = "hyper", version, about)]
#[command(propagate_version = true)]
pub struct Cli {
    /// The subcommand (omit to enter interactive REPL)
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a coding task with the agent
    Run {
        /// The prompt or task description (reads from stdin if not provided)
        prompt: Option<String>,

        /// Path to an image file (for vision-capable models)
        #[arg(short, long)]
        image: Option<PathBuf>,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Number of parallel code agents
        #[arg(long, default_value = "3")]
        agents: usize,

        /// Max tokens for code context per file
        #[arg(long, default_value_t = 4000)]
        context_tokens: u32,

        /// Model to use (overrides config)
        #[arg(long)]
        model: Option<String>,

        /// LLM provider base URL
        #[arg(long, env = "HYPER_LLM_BASE_URL")]
        base_url: Option<String>,

        /// LLM API key
        #[arg(long, env = "HYPER_LLM_API_KEY")]
        api_key: Option<String>,

        /// Alias for --yes
        #[arg(long)]
        yolo: bool,

        /// Skip confirmation before applying changes
        #[arg(long)]
        yes: bool,

        /// Output NDJSON events (for CI/pipe consumption)
        #[arg(long)]
        json: bool,

        /// Don't persist session or memory to disk
        #[arg(long)]
        ephemeral: bool,

        /// Rebuild index from scratch
        #[arg(long)]
        reindex: bool,

        /// Continue from a previous session
        #[arg(long)]
        session: Option<String>,

        /// Agent mode (code/architect/ask/debug)
        #[arg(long, default_value = "code")]
        mode: String,
    },

    /// Review code changes from git diff
    Review {
        /// Git ref to review against (default: unstaged changes)
        #[arg(default_value = "HEAD")]
        against: String,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Model to use
        #[arg(long)]
        model: Option<String>,
    },

    /// Initialize/build code index for a project
    Init {
        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Force rebuild (alias: --reindex)
        #[arg(long, alias = "reindex")]
        force: bool,
    },

    /// Show index statistics
    Stats {
        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,
    },

    /// Manage sessions
    #[clap(subcommand)]
    Session(SessionAction),

    /// List and switch agent modes
    #[clap(subcommand)]
    Mode(ModeAction),

    /// Manage agent memories (learned knowledge)
    #[clap(subcommand)]
    Memory(MemoryAction),

    /// Manage agent graph (parent/child spawning)
    #[clap(subcommand)]
    Graph(GraphAction),

    /// Manage MCP server connections
    #[clap(subcommand)]
    Mcp(McpAction),

    /// Kanban board for parallel task execution
    #[clap(subcommand)]
    Kanban(KanbanAction),

    /// Manage lifecycle hooks
    #[clap(subcommand)]
    Hooks(HooksAction),

    /// Run system diagnostics
    Doctor,

    /// Show current configuration
    Config {
        /// Show all config including defaults
        #[arg(long)]
        verbose: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for (bash, zsh, fish, powershell, elvish)
        shell: String,
    },

    /// Interactive setup wizard (first-time configuration)
    Setup,

    /// List and run available agents
    Agents {
        /// Agent name to run (lists all if not provided)
        name: Option<String>,

        /// Agent prompt/task
        message: Vec<String>,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,
    },

    /// Initialize default configuration file
    ConfigInit,

    /// Commit staged changes with auto-generated message
    Commit {
        /// Custom commit message (auto-generated if not provided)
        #[arg(short, long)]
        message: Option<String>,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,
    },

    /// Launch terminal dashboard (TUI)
    #[cfg(feature = "tui")]
    Tui,

    /// Search the web
    Search {
        /// Search query
        query: Vec<String>,

        /// Max results to show
        #[arg(long, default_value = "5")]
        max: usize,
    },

    /// Scaffold a new project from template
    Scaffold {
        /// Project name
        name: String,

        /// Project type (rust, python, ts)
        #[arg(short, long, default_value = "rust")]
        type_: String,

        /// Output directory
        #[arg(short, long, default_value = ".")]
        dir: PathBuf,
    },

    /// Build and deploy Docker image
    Deploy {
        /// Docker image tag
        #[arg(short, long, default_value = "hyperagent-app")]
        tag: String,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,
    },

    /// Show colorized diff for staged or unstaged changes
    Diff {
        /// Git ref to diff against (default: unstaged changes)
        #[arg(default_value = "HEAD")]
        against: String,

        /// Show staged changes instead
        #[arg(short, long)]
        staged: bool,

        /// Show side-by-side diff view
        #[arg(long)]
        side_by_side: bool,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,
    },

    /// Run benchmark evaluations
    Eval {
        /// Specific task name to run (runs all if not specified)
        #[arg(long)]
        task: Option<String>,

        /// List available tasks
        #[arg(long)]
        list: bool,
    },

    /// Build/query knowledge base (RAG)
    Knowledge {
        /// Action: build | search
        action: String,

        /// Search query (required for search action)
        query: Option<Vec<String>>,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,
    },

    /// Analyze project dependencies
    Deps {
        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,
    },

    /// Generate unit tests for a file
    TestGen {
        /// File to generate tests for
        file: PathBuf,

        /// Specific function to test
        #[arg(short, long)]
        function: Option<String>,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,
    },

    /// Run unit, integration, or E2E tests
    Test {
        /// Test mode: unit, integration, e2e, gen
        #[arg(short, long, default_value = "all")]
        mode: String,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,
    },

    /// Find references to a symbol across the codebase
    FindRefs {
        /// Symbol name to search for
        symbol: String,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Show context lines around matches
        #[arg(long, default_value_t = 0)]
        context: usize,
    },

    /// Rename a symbol across all files (cross-file refactoring)
    Rename {
        /// Current symbol name
        old: String,

        /// New symbol name
        new: String,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Dry-run: show what would change without applying
        #[arg(long)]
        dry_run: bool,
    },

    /// Show the last agent run log (timing, changes, errors)
    Log {
        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Show full log instead of summary
        #[arg(long)]
        verbose: bool,
    },

    /// Undo the last agent run — revert all changes via git checkout
    Undo {
        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Skip confirmation
        #[arg(long)]
        yes: bool,
    },

    /// Auto-generate CHANGELOG.md from conventional commits
    Changelog {
        /// Output file path (default: CHANGELOG.md)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Number of recent commits to include (default: all)
        #[arg(long)]
        commits: Option<usize>,
    },

    /// Create a GitHub PR from current branch with auto-generated description
    Pr {
        /// Title (auto-generated from branch name + commits if not provided)
        #[arg(short, long)]
        title: Option<String>,

        /// Base branch (default: main)
        #[arg(short, long, default_value = "main")]
        base: String,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Push before creating PR
        #[arg(long)]
        push: bool,

        /// Open PR URL in browser
        #[arg(long)]
        open: bool,

        /// Dry-run: show what would be created without submitting
        #[arg(long)]
        dry_run: bool,
    },

    /// Watch mode — auto-run agent when files change
    Watch {
        /// Prompt to run on each change
        prompt: Vec<String>,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Debounce interval in seconds (default: 2)
        #[arg(long, default_value_t = 2)]
        debounce: u64,

        /// File patterns to watch (e.g., '*.rs')
        #[arg(long)]
        pattern: Option<String>,

        /// Skip confirmation
        #[arg(long)]
        yes: bool,
    },

    /// Explain code with LLM — analyze a file or function
    Explain {
        /// File path or function name to explain
        target: String,

        /// Project root directory
        #[arg(long, short, default_value = ".")]
        dir: PathBuf,

        /// Model to use
        #[arg(long)]
        model: Option<String>,
    },

    /// Authenticate with an LLM provider
    Auth {
        /// Auth action (login, status, logout, token)
        #[arg(default_value = "status")]
        action: String,

        /// Provider name (deepseek, openai, anthropic, openrouter, or custom name)
        #[arg(long)]
        provider: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum SessionAction {
    /// List all sessions
    List,
    /// View a specific session
    View {
        /// Session ID or "last"
        id: String,
    },
    /// Delete a session
    Delete {
        /// Session ID
        id: String,
    },
    /// Fork a session to create a new one
    Fork {
        /// Source session ID or "last"
        id: String,
        /// New prompt for the forked session
        prompt: Option<String>,
    },
    /// Export sessions as JSON
    Export {
        /// Output file (default: stdout)
        #[arg(short)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ModeAction {
    /// List available agent modes
    List,
    /// Show details of a specific mode
    Show {
        /// Mode name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum MemoryAction {
    /// List recent memories
    List {
        /// Number of memories to show
        #[arg(default_value = "10")]
        limit: usize,
        /// Filter by type
        #[arg(long)]
        mem_type: Option<String>,
    },
    /// Search memories
    Search {
        /// Search query
        query: String,
    },
    /// List known entities
    Entities,
    /// Forget a memory
    Forget {
        /// Memory ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum GraphAction {
    /// Show agent graph tree
    Tree,
    /// List all agent nodes
    List,
    /// Show open agent count
    Status,
    /// Clear the graph
    Clear,
}

#[derive(Subcommand, Debug)]
pub enum McpAction {
    /// Connect to MCP servers
    Connect,
    /// List connected servers and their tools
    List,
    /// Disconnect all servers
    Disconnect,
}

#[derive(Subcommand, Debug)]
pub enum KanbanAction {
    /// Show kanban board
    Board,
    /// Add a card
    Add {
        /// Card title
        title: String,
        /// Card description
        description: String,
        /// Priority (critical/high/medium/low)
        #[arg(short, long, default_value = "medium")]
        priority: String,
        /// Agent mode
        #[arg(short, long, default_value = "code")]
        mode: String,
        /// Dependencies (comma-separated card IDs)
        #[arg(short, long)]
        depends: Option<String>,
    },
    /// Start executing ready cards
    Start,
    /// Show graphviz DOT
    Dot,
    /// Clear the board
    Clear,
}

#[derive(Subcommand, Debug)]
pub enum HooksAction {
    /// List registered hooks
    List,
    /// Fire a hook event (for testing)
    Fire {
        /// Event name (e.g. pre_plan, post_code)
        event: String,
    },
}

impl Cli {
    pub async fn run(&self) -> Result<()> {
        match &self.command {
            Some(Commands::Run {
                prompt,
                dir,
                agents,
                context_tokens: _,
                model,
                base_url,
                api_key,
                yolo,
                yes,
                json,
                ephemeral,
                reindex,
                session,
                mode,
                image,
            }) => {
                // Resolve prompt: CLI arg > stdin pipe
                let resolved_prompt = match prompt {
                    Some(p) => p.clone(),
                    None => {
                        // Try reading from stdin pipe
                        use std::io::{self, Read};
                        let mut buf = String::new();
                        let stdin = io::stdin();
                        let mut handle = stdin.lock();
                        match handle.read_to_string(&mut buf) {
                            Ok(n) if n > 0 => buf.trim().to_string(),
                            _ => anyhow::bail!(
                                "No prompt provided. Usage: hyper run \"your task\" | cat file | hyper run"
                            ),
                        }
                    }
                };
                let auto_approve = *yes || *yolo;
                let image_clone = image.clone();
                self.run_agent(
                    &resolved_prompt, dir, *agents, model.clone(), base_url.clone(),
                    api_key.clone(), auto_approve, *json, *ephemeral, *reindex, mode, session, image_clone
                ).await
            }

            Some(Commands::Review { against, dir, model }) => {
                self.review_diff(against, dir, model.as_deref()).await
            }

            Some(Commands::Init { dir, force }) => self.build_index(dir, *force).await,

            Some(Commands::Stats { dir }) => self.show_stats(dir).await,

            Some(Commands::Session(action)) => self.handle_session(action).await,

            Some(Commands::Mode(action)) => self.handle_mode(action).await,

            Some(Commands::Memory(action)) => self.handle_memory(action).await,

            Some(Commands::Graph(action)) => self.handle_graph(action).await,

            Some(Commands::Mcp(action)) => self.handle_mcp(action).await,

            Some(Commands::Kanban(action)) => self.handle_kanban(action).await,

            Some(Commands::Hooks(action)) => self.handle_hooks(action).await,

            Some(Commands::Doctor) => self.run_doctor().await,

            Some(Commands::Config { verbose }) => self.show_config(*verbose),

            Some(Commands::Completions { shell }) => {
                generate_completions(shell);
                Ok(())
            }

            Some(Commands::Eval { task, list }) => {
                if *list {
                    crate::eval::list_tasks(&crate::eval::builtin_tasks());
                    Ok(())
                } else if let Some(task_name) = task {
                    let tasks: Vec<crate::eval::EvalTask> = crate::eval::builtin_tasks()
                        .into_iter()
                        .filter(|t| t.name == *task_name)
                        .collect();
                    if tasks.is_empty() {
                        anyhow::bail!("Unknown task '{task_name}'. Use --list to see available tasks.");
                    }
                    let binary = std::env::current_exe()?;
                    crate::eval::run_all_benchmarks(&tasks, &binary)?;
                    Ok(())
                } else {
                    let tasks = crate::eval::builtin_tasks();
                    let binary = std::env::current_exe()?;
                    crate::eval::run_all_benchmarks(&tasks, &binary)?;
                    Ok(())
                }
            }

            Some(Commands::Setup) => {
                run_setup();
                Ok(())
            }

            Some(Commands::Agents { name, message, dir }) => {
                self.list_agents(name, message, dir).await
            }

            Some(Commands::ConfigInit) => {
                crate::router::ModelRouter::init_config()
            }

            Some(Commands::Commit { message, dir }) => {
                self.commit_changes(message.as_deref(), dir).await
            }

            #[cfg(feature = "tui")]
            Some(Commands::Tui) => {
                crate::tui::run_tui().await
            }

            Some(Commands::Search { query, max }) => {
                self.web_search(query.join(" "), *max).await
            }

            Some(Commands::Scaffold { name, type_, dir }) => {
                crate::scaffold::scaffold(name, type_, dir)?;
                Ok(())
            }

            Some(Commands::Deploy { tag, dir }) => {
                self.deploy_project(tag, dir).await
            }

            Some(Commands::Diff { against, staged, side_by_side, dir }) => {
                if *side_by_side {
                    self.show_diff_side_by_side(against, *staged, dir).await
                } else {
                    self.show_diff(against, *staged, dir).await
                }
            }

            Some(Commands::Knowledge { action, query, dir }) => {
                self.handle_knowledge(action, query, dir).await
            }

            Some(Commands::Deps { dir }) => {
                let graph = crate::dep_graph::analyze(dir)?;
                crate::dep_graph::display_graph(&graph);
                Ok(())
            }

            Some(Commands::TestGen { file, function, dir }) => {
                self.generate_tests(file, function.as_deref(), dir).await
            }

            Some(Commands::Test { mode, dir }) => {
                let test_mode = match mode.as_str() {
                    "unit" => crate::test_runner::TestMode::Unit,
                    "integration" => crate::test_runner::TestMode::Integration,
                    "e2e" => crate::test_runner::TestMode::E2e,
                    "gen" => crate::test_runner::TestMode::Gen,
                    _ => crate::test_runner::TestMode::All,
                };
                println!("🧪 Running {mode} tests...");
                let report = crate::test_runner::run_tests(dir, test_mode).await?;
                crate::test_runner::display_report(&report);
                if report.failed > 0 {
                    std::process::exit(1);
                }
                Ok(())
            }

            Some(Commands::FindRefs { symbol, dir, context: _ }) => {
                let excludes = ["target", ".git", "node_modules", ".hyper"];
                let refs = crate::refactor::find_references(dir, symbol, &excludes)?;
                let total: usize = refs.iter().map(|(_, l)| l.len()).sum();
                println!("🔍 References for '{symbol}': found {total} in {} files\n", refs.len());
                for (path, lines) in &refs {
                    let relative = path.strip_prefix(dir).unwrap_or(path);
                    let content = std::fs::read_to_string(path).unwrap_or_default();
                    for line_num in lines {
                        if let Some(line) = content.lines().nth(line_num - 1) {
                            println!("   {}:{}  {}", relative.display(), line_num, line.trim());
                        }
                    }
                }
                Ok(())
            }

            Some(Commands::Rename { old, new, dir, dry_run }) => {
                let excludes = ["target", ".git", "node_modules", ".hyper"];
                let result = crate::refactor::apply_rename(dir, old, new, &excludes, *dry_run)?;
                if result.files_modified.is_empty() && !dry_run {
                    println!("   ℹ️  No files were modified.");
                }
                Ok(())
            }

            Some(Commands::Log { dir, verbose }) => {
                let git_log_dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
                let last_log = std::process::Command::new("git")
                    .args(["log", "-1", "--stat", "--pretty=format:%H|%an|%ar|%s"])
                    .current_dir(&git_log_dir)
                    .output()
                    .ok();
                match last_log {
                    Some(out) if out.status.success() => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let parts: Vec<&str> = stdout.split('|').collect();
                        if parts.len() >= 4 {
                            println!("\n📋 Last Git Commit");
                            println!("   Hash:   {}", parts[0].chars().take(12).collect::<String>());
                            println!("   Author: {}", parts[1]);
                            println!("   When:   {}", parts[2]);
                            println!("   Message: {}", parts[3]);
                        }
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        // Show the stat lines (file changes)
                        for line in stderr.lines() {
                            if line.contains("changed") || line.contains("file") {
                                println!("   Files:  {}", line.trim());
                            }
                        }
                        if *verbose {
                            println!("\nFull diff:");
                            let _ = std::process::Command::new("git")
                                .args(["diff", "HEAD~1..HEAD", "--stat"])
                                .current_dir(&git_log_dir)
                                .status();
                        }
                    }
                    None | Some(_) => {
                        println!("   ℹ️  No git history found — run `hyper run` first");
                    }
                }
                Ok(())
            }

            Some(Commands::Undo { dir, yes }) => {
                let confirm = !yes;
                if confirm {
                    print!("   ⚠️  Revert all changes from last commit? [y/N] ");
                    use std::io::{self, Write};
                    let _ = io::stdout().flush();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).ok();
                    if input.trim().to_lowercase() != "y" {
                        println!("   Cancelled");
                        return Ok(());
                    }
                }
                let status = std::process::Command::new("git")
                    .args(["checkout", "HEAD", "--", "."])
                    .current_dir(dir)
                    .status()
                    .ok();
                match status {
                    Some(s) if s.success() => {
                        println!("   ✅ Reverted all changes to HEAD");
                        // Also unstage
                        let _ = std::process::Command::new("git")
                            .args(["reset", "HEAD"])
                            .current_dir(dir)
                            .status();
                        println!("   ✅ Unstaged all changes");
                    }
                    _ => {
                        println!("   ⚠️  Failed to undo — not in a git repo?");
                    }
                }
                Ok(())
            }

            Some(Commands::Changelog { output, dir, commits }) => {
                let output_path = output.clone().unwrap_or_else(|| dir.join("CHANGELOG.md"));
                let max_count = commits.unwrap_or(100);
                let output = std::process::Command::new("git")
                    .args(["log", "--oneline", "--no-decorate", &format!("-{max_count}")])
                    .current_dir(dir)
                    .output()
                    .ok();
                match output {
                    Some(out) if out.status.success() => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let mut changelog = String::from(
                            "# Changelog\n\nAll notable changes to this project will be documented in this file.\n\n"
                        );

                        // Parse conventional commits and group by type
                        let mut features = Vec::new();
                        let mut fixes = Vec::new();
                        let mut perf = Vec::new();
                        let mut docs = Vec::new();
                        let mut refactors = Vec::new();
                        let mut other = Vec::new();
                        let mut date = String::new();

                        // Get the date of the first commit
                        if let Some(first_line) = stdout.lines().last() {
                            if let Some(hash) = first_line.split_whitespace().next() {
                                let date_output = std::process::Command::new("git")
                                    .args(["log", "--format=%as", "-1", hash])
                                    .current_dir(dir)
                                    .output()
                                    .ok();
                                if let Some(d) = date_output {
                                    date = String::from_utf8_lossy(&d.stdout).trim().to_string();
                                }
                            }
                        }

                        for line in stdout.lines() {
                            let msg = line.trim();
                            if msg.starts_with("feat:") || msg.starts_with("feat(") {
                                features.push(msg.to_string());
                            } else if msg.starts_with("fix:") || msg.starts_with("fix(") {
                                fixes.push(msg.to_string());
                            } else if msg.starts_with("perf:") || msg.starts_with("perf(") {
                                perf.push(msg.to_string());
                            } else if msg.starts_with("docs:") || msg.starts_with("docs(") {
                                docs.push(msg.to_string());
                            } else if msg.starts_with("refactor:") || msg.starts_with("refactor(") || msg.starts_with("refact:") {
                                refactors.push(msg.to_string());
                            } else if !msg.is_empty() {
                                other.push(msg.to_string());
                            }
                        }

                        let date_str = if date.is_empty() { "Unreleased".to_string() } else { date };
                        changelog.push_str(&format!("## [{date_str}]\n\n"));

                        if !features.is_empty() {
                            changelog.push_str("### 🚀 Features\n\n");
                            for f in &features { changelog.push_str(&format!("- {f}\n")); }
                            changelog.push('\n');
                        }
                        if !fixes.is_empty() {
                            changelog.push_str("### 🐛 Bug Fixes\n\n");
                            for f in &fixes { changelog.push_str(&format!("- {f}\n")); }
                            changelog.push('\n');
                        }
                        if !perf.is_empty() {
                            changelog.push_str("### ⚡ Performance\n\n");
                            for p in &perf { changelog.push_str(&format!("- {p}\n")); }
                            changelog.push('\n');
                        }
                        if !docs.is_empty() {
                            changelog.push_str("### 📚 Documentation\n\n");
                            for d in &docs { changelog.push_str(&format!("- {d}\n")); }
                            changelog.push('\n');
                        }
                        if !refactors.is_empty() {
                            changelog.push_str("### 🔧 Refactoring\n\n");
                            for r in &refactors { changelog.push_str(&format!("- {r}\n")); }
                            changelog.push('\n');
                        }
                        if !other.is_empty() {
                            changelog.push_str("### Others\n\n");
                            for o in &other { changelog.push_str(&format!("- {o}\n")); }
                            changelog.push('\n');
                        }

                        if let Err(e) = std::fs::write(&output_path, &changelog) {
                            eprintln!("   ⚠️  Failed to write {}: {e}", output_path.display());
                        } else {
                            println!("   ✅ Changelog generated: {}", output_path.display());
                            println!("   📊 {} commits parsed ({} feat, {} fix, {} perf, {} docs, {} refactor)",
                                stdout.lines().count(), features.len(), fixes.len(),
                                perf.len(), docs.len(), refactors.len());
                        }
                    }
                    _ => {
                        println!("   ⚠️  Failed to generate changelog — not a git repo?");
                    }
                }
                Ok(())
            }

            Some(Commands::Pr { title, base, dir, push, open, dry_run }) => {
                let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());

                // Get current branch
                let branch_output = std::process::Command::new("git")
                    .args(["rev-parse", "--abbrev-ref", "HEAD"])
                    .current_dir(&canonical_dir)
                    .output().ok();
                let branch = branch_output.and_then(|o| {
                    if o.status.success() {
                        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                    } else { None }
                }).unwrap_or_else(|| "current".to_string());

                // Get recent commits for description
                let log_output = std::process::Command::new("git")
                    .args(["log", &format!("origin/{}..HEAD", base), "--oneline", "--no-decorate"])
                    .current_dir(&canonical_dir)
                    .output().ok();

                // Generate title from branch name if not provided
                let pr_title = title.clone().unwrap_or_else(|| {
                    branch.replace('-', " ").replace('_', " ")
                        .split_whitespace().map(|w| {
                            let mut c = w.chars();
                            c.next().map(|f| f.to_uppercase().to_string() + c.as_str()).unwrap_or_default()
                        }).collect::<Vec<_>>().join(" ")
                });

                // Build description
                let mut description = format!("## Summary\n\nAutomated PR from branch `{branch}` → `{base}`.\n\n");
                if let Some(log_out) = log_output {
                    let log_text = String::from_utf8_lossy(&log_out.stdout);
                    if !log_text.trim().is_empty() {
                        description.push_str("### Commits\n\n```\n");
                        description.push_str(&log_text);
                        description.push_str("```\n\n");
                    }
                }

                // Get diff stat
                let diffstat = std::process::Command::new("git")
                    .args(["diff", &format!("origin/{}..HEAD", base), "--stat"])
                    .current_dir(&canonical_dir)
                    .output().ok();
                if let Some(ds) = diffstat {
                    let ds_text = String::from_utf8_lossy(&ds.stdout);
                    if !ds_text.trim().is_empty() {
                        description.push_str("### Changes\n\n```\n");
                        description.push_str(&ds_text);
                        description.push_str("```\n");
                    }
                }

                if *dry_run {
                    println!("\n📋 PR Preview (dry-run):");
                    println!("   From: {}", branch);
                    println!("   To:   {}", base);
                    println!("   Title: {}", pr_title);
                    println!("\n   Description preview:");
                    for line in description.lines().take(15) {
                        println!("   │ {}", line);
                    }
                    if description.lines().count() > 15 {
                        println!("   │ ... and {} more lines", description.lines().count() - 15);
                    }
                    return Ok(());
                }

                // Optionally push
                if *push {
                    println!("   📤 Pushing branch '{}'...", branch);
                    let push_result = std::process::Command::new("git")
                        .args(["push", "origin", &branch])
                        .current_dir(&canonical_dir)
                        .status().ok();
                    match push_result {
                        Some(s) if s.success() => println!("   ✅ Push successful"),
                        _ => println!("   ⚠️  Push failed — PR may still work if branch is already pushed"),
                    }
                }

                // Create PR via gh CLI
                println!("   🔧 Creating PR...");
                let mut gh_cmd = std::process::Command::new("gh");
                gh_cmd.args(["pr", "create", "--base", &base, "--title", &pr_title, "--body", &description])
                    .current_dir(&canonical_dir);

                match gh_cmd.output() {
                    Ok(out) if out.status.success() => {
                        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        println!("   ✅ PR created: {}", url);
                        if *open {
                            if let Err(e) = std::process::Command::new("open").arg(&url).status() {
                                println!("   ⚠️  Could not open browser: {e}");
                            }
                        }
                    }
                    Ok(out) => {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        println!("   ⚠️  PR creation failed: {stderr}");
                        println!("   💡 Make sure `gh` is installed and authenticated: brew install gh && gh auth login");
                    }
                    Err(e) => {
                        println!("   ⚠️  gh CLI not found: {e}");
                        println!("   💡 Install: brew install gh && gh auth login");
                    }
                }
                Ok(())
            }

            Some(Commands::Watch { prompt, dir, debounce, pattern, yes }) => {
                let watch_dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
                let prompt_text = if prompt.is_empty() {
                    "Fix any compilation errors".to_string()
                } else {
                    prompt.join(" ")
                };
                let _confirm = !yes;

                println!("🔍 Watching {:?} for changes...", watch_dir);
                println!("   Prompt: {}", prompt_text);
                println!("   Debounce: {}s", debounce);
                if let Some(p) = pattern {
                    println!("   Pattern: {}", p);
                }
                println!("   Press Ctrl+C to stop\n");

                // Use notify crate for file watching
                use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
                use std::sync::mpsc;
                use std::time::Duration;

                let (tx, rx) = mpsc::channel::<Result<Event, notify::Error>>();
                let mut watcher = RecommendedWatcher::new(tx, Config::default())
                    .map_err(|e| anyhow::anyhow!("Watcher creation failed: {e}"))?;

                watcher.watch(&watch_dir, RecursiveMode::Recursive)
                    .map_err(|e| anyhow::anyhow!("Watch failed: {e}"))?;

                let mut last_event = std::time::Instant::now();
                loop {
                    match rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(Ok(event)) => {
                            let should_process = match &event.kind {
                                EventKind::Modify(_) | EventKind::Create(_) => {
                                    if let Some(p) = pattern {
                                        event.paths.iter().any(|p2| {
                                            p2.to_string_lossy().ends_with(p.trim_start_matches('*'))
                                        })
                                    } else {
                                        // Filter out .git, target, node_modules
                                        !event.paths.iter().any(|p| {
                                            let s = p.to_string_lossy();
                                            s.contains("/.git/") || s.contains("/target/") || s.contains("/node_modules/")
                                        })
                                    }
                                }
                                _ => false,
                            };

                            if should_process {
                                let now = std::time::Instant::now();
                                if now.duration_since(last_event) >= Duration::from_secs(*debounce) {
                                    last_event = now;
                                    println!("\n⚡ Change detected! Running agent...\n");
                                    let _ = self.run_agent(&prompt_text, dir, 2, None, None, None, *yes, false, false, false, "code", &None, None).await;
                                    println!("\n🔍 Watching for more changes... (Ctrl+C to stop)");
                                }
                            }
                        }
                        Ok(Err(e)) => eprintln!("   ⚠️  Watch error: {e}"),
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Normal timeout — continue loop
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            println!("   Watcher disconnected");
                            break;
                        }
                    }
                }
                Ok(())
            }

            Some(Commands::Explain { target, dir, model }) => {
                let provider = crate::llm::LlmProvider::from_env_or(model.clone(), None, None)?;
                let target_path = dir.join(&target);
                let content = if target_path.exists() && target_path.is_file() {
                    std::fs::read_to_string(&target_path)
                        .map_err(|e| anyhow::anyhow!("Cannot read {}: {e}", target_path.display()))?
                } else if dir.join(format!("{}.rs", target)).exists() {
                    std::fs::read_to_string(dir.join(format!("{}.rs", target)))
                        .map_err(|e| anyhow::anyhow!("Cannot read {}.rs: {e}", target))?
                } else {
                    // Try to search for the function
                    println!("   🔍 Searching for '{}' in codebase...", target);
                    let excludes = ["target", ".git", "node_modules"];
                    let refs = crate::refactor::find_references(dir, &target, &excludes)?;
                    if refs.is_empty() {
                        anyhow::bail!("No file or symbol '{}' found in project", target);
                    }
                    let first_file = &refs[0].0;
                    std::fs::read_to_string(first_file)
                        .map_err(|e| anyhow::anyhow!("Cannot read {}: {e}", first_file.display()))?
                };

                let system_prompt = "You are a code explainer. Explain what the following code does, its architecture, and any potential issues or improvements. Be concise but thorough.";
                let user_message = format!("Explain this code:\n\n```\n{content}\n```");

                println!("\n📖 Explaining: {}\n", target);
                let response = provider.chat(vec![
                    crate::llm::Message {
                        role: "system".to_string(),
                        content: system_prompt.to_string(),
                    },
                    crate::llm::Message {
                        role: "user".to_string(),
                        content: user_message,
                    },
                ]).await?;
                println!("{}", response);
                Ok(())
            }

            Some(Commands::Auth { action, provider }) => {
                self.handle_auth(action, provider.as_deref())
            }

            // No subcommand → interactive REPL
            None => crate::repl::run_repl().await,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_agent(
        &self,
        prompt: &str,
        dir: &Path,
        agents: usize,
        model: Option<String>,
        base_url: Option<String>,
        api_key: Option<String>,
        yes: bool,
        json_output: bool,
        ephemeral: bool,
        reindex: bool,
        mode: &str,
        _session_id: &Option<String>,
        image: Option<PathBuf>,
    ) -> Result<()> {
        // NDJSON event emission helper (must be defined early for use throughout)
        macro_rules! emit_json {
            ($event:expr, $($key:ident: $val:expr),*) => {
                if json_output {
                    let mut map = serde_json::Map::new();
                    map.insert("event".to_string(), serde_json::Value::String($event.to_string()));
                    $(
                        map.insert(stringify!($key).to_string(), serde_json::json!($val));
                    )*
                    println!("{}", serde_json::Value::Object(map));
                }
            };
        }

        // Check for AGENTS.md project context
        let agents_md = dir.join("AGENTS.md");
        let project_context = if agents_md.exists() {
            match std::fs::read_to_string(&agents_md) {
                Ok(content) => {
                    if !json_output {
                        println!("📄 Loaded AGENTS.md project context");
                    }
                    Some(content)
                }
                Err(_) => None,
            }
        } else {
            None
        };

        // Check for .hyperrules project rules
        let rules_text = match crate::rules::ProjectRules::load(dir) {
            Ok(Some(rules)) => {
                if !json_output {
                    println!("   📋 Loaded .hyperrules: {}", crate::rules::rules_summary(&rules.full_text));
                }
                Some(rules.full_text)
            }
            _ => None,
        };

        // Combine project context with rules
        let combined_context = match (&project_context, &rules_text) {
            (Some(ctx), Some(rules)) => Some(format!("{ctx}\n\n{rules}")),
            (Some(ctx), None) => Some(ctx.clone()),
            (None, Some(rules)) => Some(rules.clone()),
            (None, None) => None,
        };

        // Emit run_start event for CI consumption
        emit_json!("run_start",
            mode: mode,
            agents: agents,
            prompt: prompt.chars().take(200).collect::<String>()
        );

        // Build or load index
        if !json_output {
            println!("📚 Indexing codebase...");
        }
        let index = if reindex {
            let mut idx = HyperIndex::new(dir)?;
            idx.build()?;
            idx
        } else {
            HyperIndex::new_or_load(dir)?
        };

        // Initialize LLM provider — priority: CLI args > config file > env vars
        let provider = if model.is_some() || base_url.is_some() || api_key.is_some() {
            LlmProvider::from_env_or(model, base_url, api_key)?
        } else {
            // Try to get provider from config file
            match crate::router::ModelRouter::new() {
                Ok(router) => {
                    // Use the "build" agent's model from config, or default
                    let agent_config = router.get_agent("build")
                        .or_else(|| router.get_agent("general"));
                    let model_name = agent_config.map(|a| a.model.as_str())
                        .unwrap_or("deepseek-v4-flash");
                    match router.select_provider(model_name) {
                        Ok(p) => {
                            println!("   ⚙️  Using provider: {} ({})", p.name, p.default_model);
                            LlmProvider::new(
                                if model_name != p.default_model { model_name } else { &p.default_model },
                                &p.base_url,
                                &p.api_key,
                            )?
                        }
                        Err(_) => LlmProvider::from_env_or(None, None, None)?
                    }
                }
                Err(_) => LlmProvider::from_env_or(None, None, None)?
            }
        };

        // Initialize memory (SQLite-backed, per-project)
        let memory_path = dir.join(".hyper").join("memory.db");
        std::fs::create_dir_all(dir.join(".hyper")).ok();
        let memory = SqliteMemoryStore::new(&memory_path).ok()
            .map(|store| MemoryManager::new(Box::new(store), "hyperagent"));

        // Initialize hooks
        let hooks = Some(HookRegistry::new(dir));

        // Create orchestrator with ALL capabilities
        let mut orchestrator = Orchestrator::new(index, provider, dir.to_path_buf(), agents.max(1), !yes);
        // Wire up provider pool for automatic failover
        if let Ok(router) = crate::router::ModelRouter::new() {
            let configs = router.list_providers();
            if configs.len() > 1 {
                if let Ok(pool) = crate::llm::ProviderPool::new(configs) {
                    let num_providers = pool.provider_count();
                    if num_providers > 1 {
                        println!("   🔄 Failover pool: {} providers", num_providers);
                        orchestrator = orchestrator.with_provider_pool(pool);
                    }
                }
            }
        }
        orchestrator = orchestrator.with_mode(mode);

        // Handle image input for vision-capable models
        let prompt_with_image = match &image {
            Some(img_path) => {
                let img_path_str = img_path.to_string_lossy().to_lowercase();

                // Clipboard paste support
                if img_path_str == "clipboard" || img_path_str == "pasteboard" || img_path_str == "clip" || img_path_str == "pb" {
                    #[cfg(target_os = "macos")]
                    {
                        // Use osascript to read image from clipboard as base64
                        let script = "osascript -e 'set imgData to the clipboard as «class PNGf»' -e 'set imgBytes to (id of imgData)' 2>/dev/null";
                        let output = std::process::Command::new("sh")
                            .args(["-c", script])
                            .output()
                            .map_err(|e| anyhow::anyhow!("Failed to read clipboard: {e}"))?;

                        if output.status.success() {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            let trimmed = stdout.trim();
                            if !trimmed.is_empty() {
                                format!("{prompt}\n\n[Image from clipboard]\n")
                            } else {
                                anyhow::bail!("Clipboard does not contain an image");
                            }
                        } else {
                            anyhow::bail!("Clipboard does not contain an image");
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = img_path;
                        anyhow::bail!("Clipboard paste is only supported on macOS");
                    }
                } else {
                    // File-based image
                    if !img_path.exists() {
                        anyhow::bail!("Image not found: {}", img_path.display());
                    }
                    let img_data = std::fs::read(img_path)?;
                    use base64::Engine;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&img_data);

                    // Detect MIME type from magic bytes
                    let mime = if img_data.len() > 8 {
                        let header = &img_data[..img_data.len().min(12)];
                        if header.starts_with(b"\x89PNG") { "image/png" }
                        else if header.starts_with(b"\xff\xd8\xff") { "image/jpeg" }
                        else if header.starts_with(b"GIF8") { "image/gif" }
                        else if header.starts_with(b"RIFF") && header.len() > 8
                            && &header[8..12] == b"WEBP" { "image/webp" }
                        else if header.starts_with(b"BM") { "image/bmp" }
                        else { "image/png" }
                    } else { "image/png" };

                    format!("{prompt}\n\n![image](data:{mime};base64,{b64})\n")
                }
            }
            None => prompt.to_string(),
        };

        let context_label = if rules_text.is_some() && project_context.is_some() {
            "--- Project Context (AGENTS.md + .hyperrules) ---"
        } else if rules_text.is_some() {
            "--- Project Rules (.hyperrules) ---"
        } else {
            "--- Project Context (AGENTS.md) ---"
        };

        let augmented_prompt = match &combined_context {
            Some(ctx) => {
                orchestrator = orchestrator.with_project_context(ctx.clone());
                format!("{prompt_with_image}\n\n{context_label}\n{ctx}")
            }
            None => prompt_with_image,
        };

        if let Some(mem) = memory {
            orchestrator = orchestrator.with_memory(mem);
        }
        if let Some(h) = hooks {
            orchestrator = orchestrator.with_hooks(h);
        }

        // Connect MCP servers (discover from config + ~/.hyper/mcp/*.json)
        let mcp_registry = crate::mcp::McpRegistry::new(dir);
        let mcp_servers = crate::mcp::McpRegistry::discover_servers(&[]);
        if !mcp_servers.is_empty() {
            mcp_registry.connect_all(&mcp_servers).await;
            orchestrator = orchestrator.with_mcp(mcp_registry);
            println!("   🔌 MCP tools loaded");
        }

        // Run the agent pipeline
        let start_time = std::time::Instant::now();
        let result = orchestrator.run(&augmented_prompt).await?;
        let elapsed = start_time.elapsed();

        emit_json!("run_complete",
            files_modified: result.files_modified,
            tokens_used: result.tokens_used,
            elapsed_secs: elapsed.as_secs_f64(),
            memories_recorded: result.memories_recorded,
            model: result.model_name,
            mode: mode
        );

        // Save session (skip in ephemeral mode)
        if !ephemeral {
            if let Ok(sm) = SessionManager::new() {
                let mut session = Session::new(
                    dir.to_string_lossy().as_ref(),
                    prompt,
                    &result.model_name,
                );
                session.summary = format!(
                    "Modified {} files in {:.1}s using {} tokens | {} memories | mode: {}",
                    result.files_modified,
                    result.elapsed.as_secs_f64(),
                    result.tokens_used,
                    result.memories_recorded,
                    mode
                );
                let _ = sm.save(&session);
            }
        }

        // Print results (skip human-readable in JSON mode)
        if !json_output {
            println!();
            println!("─── Summary ────────────────────────────────────────");
            println!("  Files modified: {}", result.files_modified);
            println!("  Tokens:         ~{}", result.tokens_used);
            println!("  Wall time:      {:.1}s", result.elapsed.as_secs_f64());
            println!("  Memories saved: {}", result.memories_recorded);
            println!("  Model:          {}", result.model_name);
            println!("  Mode:           {mode}");
            println!("────────────────────────────────────────────────────");
        }

        // Desktop notification on completion (unless JSON mode in CI)
        if !json_output && result.files_modified > 0 {
            let _ = crate::notify::notify(&crate::notify::NotificationEvent::RunComplete {
                files_modified: result.files_modified,
                elapsed_secs: result.elapsed.as_secs_f64(),
                tokens_used: result.tokens_used,
            });
        }

        // Cost tracking
        let cost_per_1k = 0.15; // ~$0.15/M tokens for deepseek-v4-flash
        let cost = (result.tokens_used as f64 / 1000.0) * cost_per_1k;
        println!("  Cost:           ~${:.4}", cost);

        Ok(())
    }

    async fn review_diff(&self, against: &str, dir: &PathBuf, _model: Option<&str>) -> Result<()> {
        use std::process::Command as Cmd;
        println!("🔍 Reviewing diff against {against}...");

        // Get git diff
        let output = Cmd::new("git")
            .args(["diff", against, "--no-color"])
            .current_dir(dir)
            .output()?;

        let diff = String::from_utf8_lossy(&output.stdout).to_string();
        if diff.trim().is_empty() {
            println!("✅ No changes to review — clean as a whistle!");
            return Ok(());
        }

        println!("   Found {} bytes of diff\n", diff.len());

        // Get diff stat
        let stat = Cmd::new("git")
            .args(["diff", "--stat", against])
            .current_dir(dir)
            .output()?;
        let stat_str = String::from_utf8_lossy(&stat.stdout);

        println!("{}", stat_str);

        // Built-in review logic
        self.analyze_diff(&diff, dir).await
    }

    async fn analyze_diff(&self, diff: &str, _dir: &PathBuf) -> Result<()> {
        // Simple static analysis of the diff for common issues
        let mut issues: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        let mut total_added = 0usize;
        let mut total_removed = 0usize;
        let mut files_changed: Vec<String> = Vec::new();

        for line in diff.lines() {
            if let Some(stripped) = line.strip_prefix("+++ b/") {
                files_changed.push(stripped.to_string());
            } else if line.starts_with('+') && !line.starts_with("+++") {
                total_added += 1;
                // Pattern checks
                if line.contains("TODO") || line.contains("FIXME") || line.contains("HACK") {
                    warnings.push(format!("⚠️  TODO/FIXME/HACK left in code: {}", line.trim()));
                }
                if line.contains("console.log") || line.contains("dbg!(") || line.contains("eprintln!") {
                    warnings.push(format!("⚠️  Debug statement left: {}", line.trim()));
                }
                if line.contains("unwrap()") && !line.contains("// OK") {
                    issues.push(format!("🚨  Potential panic via unwrap(): {}", line.trim()));
                }
            } else if line.starts_with('-') && !line.starts_with("---") {
                total_removed += 1;
            }
        }

        if files_changed.is_empty() {
            println!("❓ Could not parse diff output");
            return Ok(());
        }

        println!("\n📊 Diff Summary");
        println!("   Files changed: {}", files_changed.len());
        println!("   Lines added:   {total_added}");
        println!("   Lines removed: {total_removed}");
        println!();

        for file in &files_changed {
            println!("   • {file}");
        }
        println!();

        // Output issues
        if issues.is_empty() && warnings.is_empty() {
            println!("✅ No issues found in diff");
        } else {
            if !issues.is_empty() {
                println!("🚨 Potential Issues:");
                for issue in &issues {
                    println!("   {issue}");
                }
                println!();
            }
            if !warnings.is_empty() {
                println!("⚠️  Warnings:");
                for w in &warnings {
                    println!("   {w}");
                }
                println!();
            }
        }

        println!("💡 Tip: Run `hyper run \"fix warnings above\"` to auto-fix");

        Ok(())
    }

    async fn build_index(&self, dir: &Path, force: bool) -> Result<()> {
        if force {
            // Delete cache
            let cache_dir = dir.join(".hyper");
            if cache_dir.exists() {
                std::fs::remove_dir_all(&cache_dir)?;
                println!("🧹 Cleared cached index");
            }
        }
        let mut index = HyperIndex::new(dir)?;
        let stats = index.build()?;
        println!("✅ HyperIndex built successfully:");
        println!("   Files indexed: {}", stats.files);
        println!("   Symbols found: {}", stats.symbols);
        println!("   References:    {}", stats.references);
        println!("   Cache:         {}", stats.cache_size);
        Ok(())
    }

    async fn show_stats(&self, dir: &Path) -> Result<()> {
        let mut index = HyperIndex::new(dir)?;
        if index.has_cache() {
            let stats = index.build()?;
            println!("📊 HyperIndex Statistics");
            println!("   Project:       {}", dir.display());
            println!("   Files indexed: {}", stats.files);
            println!("   Symbols found: {}", stats.symbols);
            println!("   Languages:     {}", stats.languages);
            println!("   References:    {}", stats.references);
            println!("   Cache:         {}", stats.cache_size);
        } else {
            println!("📊 No index found. Run `hyper init` to build one.");
        }
        Ok(())
    }

    async fn handle_mode(&self, action: &ModeAction) -> Result<()> {
        let registry = crate::modes::ModeRegistry::default();
        match action {
            ModeAction::List => {
                println!("🎭 Agent Modes:");
                println!();
                for mode in registry.list() {
                    println!("  {:<12} — {}", mode.name, mode.description);
                    println!("  {:12}  Edit: {:?}, Run: {:?}, Network: {:?}, Git: {:?}, Search: {:?}",
                        "",
                        mode.permissions.edit_files,
                        mode.permissions.run_commands,
                        mode.permissions.network,
                        mode.permissions.git,
                        mode.permissions.search,
                    );
                    println!();
                }
            }
            ModeAction::Show { name } => {
                match registry.get(name) {
                    Some(mode) => {
                        println!("🎭 Mode: {} ({})", mode.name, mode.description);
                        println!("   Permissions:");
                        println!("     Edit files:  {:?}", mode.permissions.edit_files);
                        println!("     Read files:  {:?}", mode.permissions.read_files);
                        println!("     Run commands: {:?}", mode.permissions.run_commands);
                        println!("     Network:     {:?}", mode.permissions.network);
                        println!("     Git:         {:?}", mode.permissions.git);
                        println!("     Search:      {:?}", mode.permissions.search);
                        if let Some(model) = &mode.model {
                            println!("   Default model: {model}");
                        }
                        if let Some(temp) = mode.temperature {
                            println!("   Temperature:   {temp}");
                        }
                    }
                    None => println!("❌ Mode '{name}' not found. Run `hyper mode list` to see available modes."),
                }
            }
        }
        Ok(())
    }

    async fn handle_memory(&self, action: &MemoryAction) -> Result<()> {
        // Default memory store at ~/.hyper/memory.db
        let mem_dir = dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".hyper");
        std::fs::create_dir_all(&mem_dir)?;
        let store = crate::memory::SqliteMemoryStore::new(&mem_dir.join("memory.db"))?;
        let manager = crate::memory::MemoryManager::new(Box::new(store), "cli");

        match action {
            MemoryAction::List { limit, mem_type } => {
                let memories = if let Some(t) = mem_type {
                    let mt = match t.as_str() {
                        "user" | "user_preference" => crate::memory::MemoryType::UserPreference,
                        "codebase" | "codebase_fact" => crate::memory::MemoryType::CodebaseFact,
                        "decision" => crate::memory::MemoryType::Decision,
                        "bug" | "bug_fix" => crate::memory::MemoryType::BugFix,
                        "action" | "action_outcome" => crate::memory::MemoryType::ActionOutcome,
                        _ => crate::memory::MemoryType::Learned,
                    };
                    manager.recall_by_type(mt, *limit)?
                } else {
                    manager.recall("", *limit)?
                };

                if memories.is_empty() {
                    println!("🧠 No memories yet.");
                    return Ok(());
                }
                println!("🧠 Memories ({} shown):", memories.len());
                for m in &memories {
                    let ago = chrono::Utc::now().signed_duration_since(m.created_at);
                    let ago_str = if ago.num_minutes() < 60 {
                        format!("{}m", ago.num_minutes())
                    } else if ago.num_hours() < 24 {
                        format!("{}h", ago.num_hours())
                    } else {
                        format!("{}d", ago.num_days())
                    };
                    println!("  [{ago_str}] [{:?}] {} — {}",
                        m.memory_type,
                        &m.content[..m.content.len().min(80)],
                        &m.id[..8],
                    );
                }
            }
            MemoryAction::Search { query } => {
                let memories = manager.recall(query, 10)?;
                if memories.is_empty() {
                    println!("🔍 No memories matching '{query}'.");
                    return Ok(());
                }
                println!("🔍 Memories matching '{query}':");
                for m in &memories {
                    println!("  [{:?}] (import={:.2}) {}", m.memory_type, m.importance, &m.content[..m.content.len().min(100)]);
                }
            }
            MemoryAction::Entities => {
                let entities = manager.entities()?;
                if entities.is_empty() {
                    println!("🧠 No entities found.");
                    return Ok(());
                }
                println!("🧠 Known Entities:");
                for (entity, count) in &entities {
                    println!("  {entity} ({count} memories)");
                }
            }
            MemoryAction::Forget { id } => {
                manager.store().delete(id)?;
                println!("🗑️  Deleted memory: {id}");
            }
        }
        Ok(())
    }

    async fn handle_graph(&self, action: &GraphAction) -> Result<()> {
        use crate::agent_graph::AgentGraph;
        let db_dir = dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".hyper");
        std::fs::create_dir_all(&db_dir)?;
        let graph = crate::agent_graph::SqliteAgentGraph::new(&db_dir.join("agent_graph.db"))?;

        match action {
            GraphAction::List => {
                let nodes = graph.get_all_nodes()?;
                if nodes.is_empty() {
                    println!("📡 No agent nodes yet.");
                    return Ok(());
                }
                println!("📡 Agent Graph Nodes:");
                for n in &nodes {
                    let status = match n.status {
                        crate::agent_graph::EdgeStatus::Open => "🟢",
                        crate::agent_graph::EdgeStatus::Closed => "🔴",
                    };
                    let ago = chrono::Utc::now().signed_duration_since(n.created_at);
                    println!("  {} {:12} [{}] {:50} ({:?} ago, {}tokens)",
                        status, &n.id[..12], n.mode,
                        &n.name[..n.name.len().min(48)],
                        ago, n.token_usage);
                }
            }
            GraphAction::Tree => {
                // Simple tree display — root nodes first, then children
                let nodes = graph.get_all_nodes()?;
                let root_nodes: Vec<_> = nodes.iter()
                    .filter(|n| graph.get_parent(&n.id).map(|p| p.is_none()).unwrap_or(true))
                    .collect();

                if root_nodes.is_empty() {
                    println!("📡 No agent nodes yet.");
                    return Ok(());
                }
                println!("📡 Agent Graph Tree:");
                for root in &root_nodes {
                    let status = match root.status {
                        crate::agent_graph::EdgeStatus::Open => "🟢",
                        crate::agent_graph::EdgeStatus::Closed => "🔴",
                    };
                    println!("  {} {:12} [{}] {}", status, &root.id[..12], root.mode, root.name);

                    if let Ok(children) = graph.get_children(&root.id, None) {
                        for child in &children {
                            let cs = match child.status {
                                crate::agent_graph::EdgeStatus::Open => "🟢",
                                crate::agent_graph::EdgeStatus::Closed => "🔴",
                            };
                            println!("    {} {:12} [{}] {}", cs, &child.id[..12], child.mode, child.name);
                        }
                    }
                }
            }
            GraphAction::Status => {
                let open = graph.open_count()?;
                let total = graph.get_all_nodes()?.len();
                println!("📡 Agent Graph: {open} open / {total} total nodes");
            }
            GraphAction::Clear => {
                graph.clear()?;
                println!("🗑️  Agent graph cleared.");
            }
        }
        Ok(())
    }

    async fn handle_mcp(&self, action: &McpAction) -> Result<()> {
        let project_root = std::env::current_dir()?;
        let registry = crate::mcp::McpRegistry::new(&project_root);

        match action {
            McpAction::Connect => {
                println!("🔌 Connecting to MCP servers...");
                // Discover servers from config
                let servers = crate::mcp::McpRegistry::discover_servers(&[]);
                if servers.is_empty() {
                    println!("  ℹ️  No MCP servers configured.");
                    println!("  💡 Create ~/.hyper/mcp/<name>.json to add servers.");
                    return Ok(());
                }
                registry.connect_all(&servers).await;
            }
            McpAction::List => {
                let tools = registry.get_all_tools().await;
                if tools.is_empty() {
                    println!("🔌 No MCP servers connected.");
                    println!("  Run `hyper mcp connect` to connect.");
                    return Ok(());
                }
                println!("🔌 MCP Tools:");
                for t in &tools {
                    println!("  {}.{} — {}", t.server, t.name, t.description);
                }
            }
            McpAction::Disconnect => {
                registry.shutdown().await;
            }
        }
        Ok(())
    }

    async fn handle_kanban(&self, action: &KanbanAction) -> Result<()> {
        use crate::agent::orchestrator::Orchestrator;
        use crate::index::HyperIndex;
        use crate::kanban::{CardResult, CardStatus};
        use crate::llm::LlmProvider;
        let project_root = std::env::current_dir()?;
        let board = crate::kanban::KanbanBoard::new(&project_root, 3);

        match action {
            KanbanAction::Board => {
                let summary = board.summary().await;
                println!("📋 Kanban Board");
                println!("   Total: {}, Todo: {}, In Progress: {}, Done: {}, Blocked: {}, Failed: {}",
                    summary.total, summary.todo, summary.in_progress,
                    summary.done, summary.blocked, summary.failed);
                println!("{}", board.render().await);
            }
            KanbanAction::Add { title, description, priority, mode, depends } => {
                let prio = match priority.as_str() {
                    "critical" => crate::kanban::Priority::Critical,
                    "high" => crate::kanban::Priority::High,
                    "medium" => crate::kanban::Priority::Medium,
                    "low" => crate::kanban::Priority::Low,
                    _ => crate::kanban::Priority::Medium,
                };
                let deps: Vec<String> = depends.as_ref()
                    .map(|d| d.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default();
                let id = board.add_card(title, description, prio, mode, deps, vec![]).await;
                println!("📋 Card added: {id}");
            }
            KanbanAction::Start => {
                let ready = board.get_ready_cards().await;
                if ready.is_empty() {
                    println!("📋 No cards ready to start.");
                    return Ok(());
                }
                println!("📋 Starting {} card(s) in parallel (max {} agents)...",
                    ready.len(), board.max_concurrency);
                println!();

                // Spawn one agent per ready card
                let mut handles = Vec::new();
                for card in ready {
                    let project_root = project_root.clone();
                    let description = card.description.clone();
                    let mode = card.agent_mode.clone();
                    let card_id = card.id.clone();
                    let board = board.board_clone();

                    let handle = tokio::spawn(async move {
                        // Create agent worktree dir
                        let worktree = project_root.join(".hyper").join("worktrees").join(&card_id);
                        std::fs::create_dir_all(&worktree).ok();

                        // Initialize index
                        match HyperIndex::new_or_load(&project_root) {
                            Ok(idx) => {
                                let provider = match LlmProvider::from_env_or(None, None, None) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        // Mark as failed
                                        let mut c = board.lock().await;
                                        if let Some(card) = c.get_mut(&card_id) {
                                            card.status = CardStatus::Failed;
                                            card.result = Some(CardResult {
                                                summary: format!("LLM setup failed: {e}"),
                                                files_changed: vec![],
                                                tokens_used: 0,
                                                exit_code: 1,
                                            });
                                        }
                                        return;
                                    }
                                };

                                let agent_id = format!("agent-{}", &card_id[..8]);
                                let mut orch = Orchestrator::new(
                                    idx, provider, project_root.clone(), 1, false
                                );
                                orch = orch.with_mode(&mode);

                                // Start the card
                                {
                                    let mut c = board.lock().await;
                                    if let Some(card) = c.get_mut(&card_id) {
                                        card.status = CardStatus::InProgress;
                                        card.started_at = Some(chrono::Utc::now());
                                        card.assigned_agent_id = Some(agent_id.clone());
                                    }
                                }

                                println!("  🚀 Card '{}' — starting...", &card.title[..card.title.len().min(40)]);

                                let result = orch.run(&description).await;

                                match result {
                                    Ok(r) => {
                                        let mut c = board.lock().await;
                                        if let Some(card) = c.get_mut(&card_id) {
                                            card.status = CardStatus::Done;
                                            card.completed_at = Some(chrono::Utc::now());
                                            card.result = Some(CardResult {
                                                summary: format!("Modified {} files in {:.1}s",
                                                    r.files_modified, r.elapsed.as_secs_f64()),
                                                files_changed: vec![],
                                                tokens_used: r.tokens_used as u64,
                                                exit_code: 0,
                                            });
                                        }
                                        println!("  ✅ Card '{}' — done ({:.1}s, {} files)",
                                            &card.title[..card.title.len().min(40)],
                                            r.elapsed.as_secs_f64(),
                                            r.files_modified);
                                    }
                                    Err(e) => {
                                        let mut c = board.lock().await;
                                        if let Some(card) = c.get_mut(&card_id) {
                                            card.status = CardStatus::Failed;
                                            card.result = Some(CardResult {
                                                summary: format!("Failed: {e}"),
                                                files_changed: vec![],
                                                tokens_used: 0,
                                                exit_code: 1,
                                            });
                                        }
                                        println!("  ❌ Card '{}' — failed: {e}",
                                            &card.title[..card.title.len().min(40)]);
                                    }
                                }
                            }
                            Err(e) => {
                                let mut c = board.lock().await;
                                if let Some(card) = c.get_mut(&card_id) {
                                    card.status = CardStatus::Failed;
                                    card.result = Some(CardResult {
                                        summary: format!("Index build failed: {e}"),
                                        files_changed: vec![],
                                        tokens_used: 0,
                                        exit_code: 1,
                                    });
                                }
                            }
                        }
                    });
                    handles.push(handle);
                }

                // Wait for all to complete
                for handle in handles {
                    let _ = handle.await;
                }

                // Print final board summary
                let summary = board.summary().await;
                println!();
                println!("📋 All cards finished:");
                println!("   🟢 Done: {} | 🟡 Failed: {}", summary.done, summary.failed);
                println!("   📊 {} total, {} remaining todo",
                    summary.total, summary.todo);
            }
            KanbanAction::Dot => {
                println!("{}", board.to_dot().await);
            }
            KanbanAction::Clear => {
                board.clear().await;
                println!("🗑️  Kanban board cleared.");
            }
        }
        Ok(())
    }

    async fn handle_hooks(&self, action: &HooksAction) -> Result<()> {
        let project_root = std::env::current_dir()?;
        let registry = crate::hooks::HookRegistry::new(&project_root);

        match action {
            HooksAction::List => {
                let hooks = registry.list_hooks();
                if hooks.is_empty() {
                    println!("🪝 No hooks registered. Define in config.toml under [hooks].");
                } else {
                    println!("🪝 Registered Hooks:");
                    for h in &hooks {
                        println!("  {:12} — {}",
                            h.event.to_string(),
                            h.description.as_deref().unwrap_or("(no description)"));
                    }
                }
            }
            HooksAction::Fire { event } => {
                let evt = match event.as_str() {
                    "pre_plan" => crate::hooks::HookEvent::PrePlan,
                    "post_plan" => crate::hooks::HookEvent::PostPlan,
                    "pre_code" => crate::hooks::HookEvent::PreCode,
                    "post_code" => crate::hooks::HookEvent::PostCode,
                    "pre_apply" => crate::hooks::HookEvent::PreApply,
                    "post_apply" => crate::hooks::HookEvent::PostApply,
                    "pre_review" => crate::hooks::HookEvent::PreReview,
                    "post_review" => crate::hooks::HookEvent::PostReview,
                    "pre_run" => crate::hooks::HookEvent::PreRun,
                    "post_run" => crate::hooks::HookEvent::PostRun,
                    "on_error" => crate::hooks::HookEvent::OnError,
                    "on_complete" => crate::hooks::HookEvent::OnComplete,
                    _ => {
                        println!("❌ Unknown event '{event}'. Valid: pre_plan, post_plan, pre_code, post_code, pre_apply, post_apply, pre_review, post_review, pre_run, post_run, on_error, on_complete");
                        return Ok(());
                    }
                };
                registry.fire(&evt, None)?;
                println!("🪝 Fired event: {event}");
            }
        }
        Ok(())
    }

    async fn handle_session(&self, action: &SessionAction) -> Result<()> {
        let sm = SessionManager::new()?;
        match action {
            SessionAction::List => {
                let sessions = sm.list()?;
                if sessions.is_empty() {
                    println!("No saved sessions.");
                    return Ok(());
                }
                println!("📋 Saved Sessions:");
                println!();
                for s in &sessions {
                    let time = chrono::DateTime::from_timestamp(s.timestamp as i64, 0)
                        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    println!("  {}  {}  {}  {}", &s.id[..15], time, s.model, s.prompt.chars().take(50).collect::<String>());
                }
            }
            SessionAction::View { id } => {
                let session = if id == "last" {
                    sm.last()?
                } else {
                    sm.load(id)?
                };
                println!("📄 Session: {}", session.id);
                println!("   Project:  {}", session.project);
                println!("   Model:    {}", session.model);
                println!("   Time:     {}", session.timestamp);
                println!("   Prompt:   {}", session.prompt);
                println!("   Summary:  {}", session.summary);
            }
            SessionAction::Delete { id } => {
                sm.delete(id)?;
                println!("🗑️  Deleted session: {id}");
            }
            SessionAction::Fork { id, prompt } => {
                let source = if id == "last" {
                    sm.last()?
                } else {
                    sm.load(id)?
                };
                let new_prompt = prompt.clone().unwrap_or(source.prompt.clone());
                let forked = Session::new(&source.project, &new_prompt, &source.model);
                sm.save(&forked)?;
                println!("🔀 Forked session: {} → {}", &source.id[..15], &forked.id[..15]);
            }
            SessionAction::Export { output } => {
                let sessions = sm.list()?;
                let json = serde_json::to_string_pretty(&sessions)?;
                match output {
                    Some(path) => {
                        std::fs::write(path, &json)?;
                        println!("📤 Exported {} sessions", sessions.len());
                    }
                    None => println!("{json}"),
                }
            }
        }
        Ok(())
    }

    async fn run_doctor(&self) -> Result<()> {
        println!("🏥 HyperAgent Doctor — System Diagnostics");
        println!();

        // Check Rust toolchain
        println!("🔧 Rust toolchain:");
        match std::process::Command::new("rustc").arg("--version").output() {
            Ok(o) => println!("   ✅ {}", String::from_utf8_lossy(&o.stdout).trim()),
            Err(_) => println!("   ❌ rustc not found"),
        }

        // Check git
        println!("🔧 Git:");
        match std::process::Command::new("git").arg("--version").output() {
            Ok(o) => println!("   ✅ {}", String::from_utf8_lossy(&o.stdout).trim()),
            Err(_) => println!("   ❌ git not found"),
        }

        // Check LLM config
        println!("🔧 LLM Provider:");
        match std::env::var("HYPER_LLM_API_KEY") {
            Ok(_) => println!("   ✅ HYPER_LLM_API_KEY set"),
            Err(_) => println!("   ⚠️  HYPER_LLM_API_KEY not set"),
        }
        match std::env::var("HYPER_LLM_BASE_URL") {
            Ok(url) => println!("   ✅ HYPER_LLM_BASE_URL = {url}"),
            Err(_) => println!("   ⚠️  HYPER_LLM_BASE_URL not set (defaults to DeepSeek)"),
        }

        // Check for AGENTS.md
        println!("🔧 Project config:");
        if std::path::Path::new("AGENTS.md").exists() {
            println!("   ✅ AGENTS.md found");
        } else {
            println!("   ℹ️  No AGENTS.md — create one for project context");
        }

        // Check session storage
        println!("🔧 Session storage:");
        match SessionManager::new() {
            Ok(sm) => {
                match sm.list() {
                    Ok(sessions) => println!("   ✅ {} saved sessions", sessions.len()),
                    Err(_) => println!("   ⚠️  Could not read sessions"),
                }
            }
            Err(_) => println!("   ❌ Could not create session directory"),
        }

        println!();
        println!("💡 Tip: Set HYPER_LLM_API_KEY=sk-your-key to get started");
        println!("   Or use: echo 'HYPER_LLM_API_KEY=sk-...' >> .env");

        Ok(())
    }

    fn show_config(&self, verbose: bool) -> Result<()> {
        println!("📋 HyperAgent Configuration");
        println!();
        println!("   Version:  {}", env!("CARGO_PKG_VERSION"));
        println!();
        println!("   Environment Variables:");
        println!("   HYPER_LLM_API_KEY   = {}", 
            std::env::var("HYPER_LLM_API_KEY")
                .map(|_| "*** (set)")
                .unwrap_or("(not set)")
        );
        println!("   HYPER_LLM_BASE_URL  = {}",
            std::env::var("HYPER_LLM_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com/v1 (default)".to_string())
        );
        println!();
        println!("   Agent Defaults:");
        println!("   Parallel agents: 3");
        println!("   Context tokens:  4000");
        println!("   Confirm changes: yes");
        println!();
        if verbose {
            println!("   Build Info:");
            println!("   Compiler: {}", env!("CARGO_PKG_RUST_VERSION"));
            println!("   Profile:  {}", 
                if cfg!(debug_assertions) { "debug" } else { "release" }
            );
            println!("   Features: multi-agent, tree-sitter, pagerank, session");
        }
        Ok(())
    }

    async fn web_search(&self, query: String, max: usize) -> Result<()> {
        println!("🔍 Searching for: {query}");
        let results = crate::web_search::search(&query, max).await?;
        crate::web_search::display_results(&results);
        Ok(())
    }

    async fn show_diff(&self, against: &str, staged: bool, dir: &Path) -> Result<()> {
        use crate::diff_view;
        if staged {
            diff_view::show_staged_diff(dir)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        } else {
            let diff = diff_view::get_diff(dir, against)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            diff_view::show_diff(&diff);
        }
        Ok(())
    }

    /// Show diff in side-by-side view
    async fn show_diff_side_by_side(&self, against: &str, staged: bool, dir: &Path) -> Result<()> {
        use crate::diff_view;
        if staged {
            diff_view::show_staged_diff_side_by_side(dir)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        } else {
            let diff = diff_view::get_diff(dir, against)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            diff_view::show_side_by_side_diff(&diff);
        }
        Ok(())
    }

    async fn deploy_project(&self, tag: &str, dir: &PathBuf) -> Result<()> {
        println!("🐳 Building Docker image: {tag}");
        println!("   From: {}", dir.display());
        println!();

        // Check if Dockerfile exists
        if !dir.join("Dockerfile").exists() {
            anyhow::bail!("No Dockerfile found in {}", dir.display());
        }

        // Security check: verify docker command
        let docker_cmd = format!("docker build -t {tag} .");
        let policy = crate::security::SecurityPolicy::default();
        let safety = crate::security::check_command_safety(&docker_cmd, &policy);
        if !crate::security::confirm_dangerous_action(&safety, false) {
            anyhow::bail!("Deploy aborted by security policy");
        }

        // Build Docker image
        let status = std::process::Command::new("docker")
            .args(["build", "-t", tag, "."])
            .current_dir(dir)
            .status()
            .map_err(|e| anyhow::anyhow!("Docker not found: {e}"))?;

        if !status.success() {
            anyhow::bail!("Docker build failed");
        }

        println!("   ✅ Built: {tag}");

        // Prompt to push
        println!();
        println!("   Image ready. Push with:");
        println!("   docker push {tag}");

        Ok(())
    }

    async fn commit_changes(&self, message: Option<&str>, dir: &PathBuf) -> Result<()> {
        use crate::git::GitOps;
        if !GitOps::is_repo(dir) {
            anyhow::bail!("Not a git repository: {}", dir.display());
        }

        // Show staged diff for review
        if let Ok(diff) = GitOps::get_staged_diff(dir) {
            if diff.trim().is_empty() {
                // Auto-stage all changes
                GitOps::stage_all(dir)?;
                println!("   📦 Staged all changes");
            }
        }

        match GitOps::smart_commit(dir, message) {
            Ok(msg) => {
                println!("   ✅ Committed: {msg}");

                // Also try to push
                if let Ok(output) = std::process::Command::new("git")
                    .args(["push", "origin", "master"])
                    .current_dir(dir)
                    .output()
                {
                    if output.status.success() {
                        println!("   🚀 Pushed to origin/master");
                    }
                }
            }
            Err(e) => {
                anyhow::bail!("Commit failed: {e}");
            }
        }
        Ok(())
    }

    async fn list_agents(&self, name: &Option<String>, message: &[String], _dir: &PathBuf) -> Result<()> {
        let router = crate::router::ModelRouter::new()?;
        match name {
            Some(agent_name) => {
                // Check if agent exists with permissions
                match router.get_agent(agent_name) {
                    Some(agent) => {
                        let msg = if message.is_empty() {
                            agent_name.clone()
                        } else {
                            message.join(" ")
                        };
                        println!("🧠 Agent: {} ({})", agent.name, agent.description);
                        println!("   Model:     {}", agent.model);
                        println!("   Mode:      {:?}", agent.mode);
                        println!("   Temp:      {}", agent.temperature);
                        println!("   Edit:      {:?}", agent.permissions.edit);
                        println!("   Bash:      {:?}", agent.permissions.bash);
                        println!("   Read:      {:?}", agent.permissions.read);
                        println!("   Network:   {:?}", agent.permissions.network);
                        println!();
                        println!("ℹ️  Use `hyper run \"{msg}\"` to run this task");
                    }
                    None => {
                        println!("❌ Agent '{agent_name}' not found");
                        println!("   Available: {:?}", router.list_agents().iter().map(|a| a.name.clone()).collect::<Vec<_>>());
                    }
                }
            }
            None => {
                println!("🧠 HyperAgent Agents");
                println!();
                println!("  {:12} {:10}  Description", "Name", "Mode");
                println!("  {}", "-".repeat(60));
                for agent in router.list_agents() {
                    let mode_str = match agent.mode {
                        crate::router::AgentMode::Primary => "primary",
                        crate::router::AgentMode::SubAgent => "subagent",
                        crate::router::AgentMode::Tool => "tool",
                    };
                    println!("  {:12} {:10}  {}", agent.name, mode_str, agent.description);
                }
                println!();
                println!("  Use: hyper agents <name> \"<message>\"");
                println!("  Or:  hyper run \"<prompt>\"");
            }
        }
        Ok(())
    }

    async fn handle_knowledge(&self, action: &str, query: &Option<Vec<String>>, dir: &Path) -> Result<()> {
        let kb = crate::knowledge::KnowledgeBase::new(dir);
        match action {
            "build" => {
                println!("📚 Building knowledge base...");
                let count = kb.build(dir)?;
                println!("   Indexed {count} document chunks");
            }
            "search" => {
                let q = query.as_ref()
                    .map(|v| v.join(" "))
                    .unwrap_or_default();
                if q.is_empty() {
                    anyhow::bail!("Search query required. Usage: hyper knowledge search \"<query>\"");
                }
                println!("🔍 Searching knowledge base: {q}");
                let results = kb.search(&q, 10)?;
                crate::knowledge::KnowledgeBase::display_results(&results);
            }
            _ => anyhow::bail!("Unknown action: {action}. Use: build | search"),
        }
        Ok(())
    }

    async fn generate_tests(&self, file: &PathBuf, function: Option<&str>, _dir: &PathBuf) -> Result<()> {
        let content = std::fs::read_to_string(file)
            .map_err(|e| anyhow::anyhow!("Cannot read {}: {e}", file.display()))?;

        println!("🧪 Generating tests for {}...", file.display());

        let provider = crate::repl::get_provider_from_config();

        let tests = crate::test_gen::generate_test(
            &provider, file, &content, function,
        ).await?;

        // Show generated tests
        println!("\n📝 Generated Tests:\n");
        println!("{}", tests);
        println!();

        // Ask to append
        print!("   Append to file? [Y/n] ");
        use std::io::{stdout, stdin, Write};
        std::io::stdout().flush().ok();
        let mut input = String::new();
        stdin().read_line(&mut input).ok();
        if input.trim().to_lowercase() != "n" {
            crate::test_gen::append_tests_to_file(file, &tests)?;
            println!("   ✅ Tests appended to {}", file.display());
        }

        Ok(())
    }

    fn handle_auth(&self, action: &str, provider: Option<&str>) -> Result<()> {
        match action {
            "login" => {
                let creds = crate::auth::interactive_login(provider.map(|s| s.to_string()))?;
                println!();
                println!("   ✅ Logged in as: {}", creds.provider);
                println!("   🔑 Key:        {}", creds.masked_api_key());
                println!("   🌐 Endpoint:   {}", creds.base_url);
                println!("   📦 Model:      {}", creds.default_model);
            }
            "status" => {
                // Check env vars first
                let env_key = std::env::var("HYPER_LLM_API_KEY").ok();
                if let Some(_key) = env_key {
                    println!("   🔑 Using HYPER_LLM_API_KEY environment variable");
                    println!("   🌐 Endpoint: {}", std::env::var("HYPER_LLM_BASE_URL").unwrap_or_else(|_| "default".to_string()));
                } else if let Ok(Some(creds)) = crate::auth::load_credentials() {
                    if creds.is_valid() {
                        println!("   ✅ Authenticated with {}", creds.provider);
                        println!("   🔑 Key:   {}", creds.masked_api_key());
                        println!("   🌐 URL:   {}", creds.base_url);
                        println!("   📦 Model: {}", creds.default_model);
                        println!("   🕐 Since: {}", creds.created_at.format("%Y-%m-%d %H:%M UTC"));
                    } else {
                        println!("   ⚠️  Stored credentials appear invalid (key too short)");
                        println!("   Run 'hyper auth login' to re-authenticate.");
                    }
                } else {
                    println!("   ❌ Not authenticated");
                    println!("   Run 'hyper auth login' to set up API key.");
                    println!();
                    println!("   Or set environment variables:");
                    println!("     export HYPER_LLM_API_KEY=\"sk-....");
                    println!("     export HYPER_LLM_BASE_URL=\"https://api.deepseek.com/v1\"");
                }
            }
            "logout" => {
                crate::auth::clear_credentials()?;
                println!("   ✅ Credentials cleared.");
            }
            "token" => {
                if let Some(key) = crate::auth::get_active_api_key() {
                    if key.len() > 8 {
                        println!("{}...{}", &key[..8], &key[key.len() - 4..]);
                    } else {
                        println!("{key}");
                    }
                } else {
                    println!("   ❌ No active authentication found.");
                    println!("   Run 'hyper auth login' or set HYPER_LLM_API_KEY.");
                }
            }
            _ => {
                anyhow::bail!("Unknown auth action: '{action}'. Use: login, status, logout, token");
            }
        }
        Ok(())
    }
}

/// Generate shell completions for the hyper command
fn generate_completions(shell_name: &str) {
    let shell = match shell_name.to_lowercase().as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "powershell" | "ps" => Shell::PowerShell,
        "elvish" => Shell::Elvish,
        other => {
            eprintln!("Unsupported shell: {other}");
            eprintln!("Supported: bash, zsh, fish, powershell, elvish");
            return;
        }
    };

    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();

    clap_complete::generate(shell, &mut cmd, &name, &mut std::io::stdout());

    // Print install instructions
    eprintln!("\n---");
    match shell {
        Shell::Bash => eprintln!("Save and source:\n  hyper completions bash > /usr/local/etc/bash_completion.d/hyper"),
        Shell::Zsh => eprintln!("Save and source:\n  hyper completions zsh > /usr/local/share/zsh/site-functions/_hyper"),
        Shell::Fish => eprintln!("Save and source:\n  hyper completions fish > ~/.config/fish/completions/hyper.fish"),
        _ => {}
    }
}

/// Interactive setup wizard — guides first-time configuration
fn run_setup() {
    use std::io::{stdout, stdin, Write};

    println!();
    println!("╔══════════════════════════════════════════╗");
    println!("║      HyperAgent Setup Wizard             ║");
    println!("╚══════════════════════════════════════════╝");
    println!();

    // Step 1: Provider selection
    println!("{0:─^40}", " Step 1: LLM Provider ");
    println!("Choose your LLM provider:");
    println!("  1) DeepSeek (default, ~$0.15/1M tokens)");
    println!("  2) OpenAI   (~$2.50/1M tokens, GPT-4o)");
    println!("  3) Custom   (any OpenAI-compatible API)");
    println!("  4) Skip     (set up manually later)");
    print!("\nChoice [1-4]: ");
    stdout().flush().ok();

    let mut choice = String::new();
    stdin().read_line(&mut choice).ok();
    let provider_index = choice.trim().parse::<u32>().unwrap_or(1);

    // Step 2: API key
    let (provider_name, base_url, default_model, default_key) = match provider_index {
        2 => ("openai", "https://api.openai.com/v1", "gpt-4o", "OPENAI_API_KEY"),
        3 => ("custom", "", "", "HYPER_LLM_API_KEY"),
        _ => ("deepseek", "https://api.deepseek.com/v1", "deepseek-v4-flash", "DEEPSEEK_API_KEY"),
    };

    println!();
    println!("{0:─^40}", " Step 2: API Key ");
    if provider_index == 3 {
        print!("Base URL: ");
        stdout().flush().ok();
        let mut url = String::new();
        stdin().read_line(&mut url).ok();
        let _custom_url = url.trim().to_string();

        print!("Model name: ");
        stdout().flush().ok();
        let mut model = String::new();
        stdin().read_line(&mut model).ok();
        let _custom_model = model.trim().to_string();
    }

    // Try env var first, then ask
    let api_key = std::env::var(default_key).ok();
    if let Some(ref key) = api_key {
        println!("   Using {} from environment", default_key);
    } else {
        println!("   Enter your {} API key (or leave empty to use env var later):", provider_name);
        print!("   API Key: ");
        stdout().flush().ok();
        let mut key = String::new();
        stdin().read_line(&mut key).ok();
        let _key = key.trim().to_string();
    }

    // Step 3: Config generation
    println!();
    println!("{0:─^40}", " Step 3: Configuration ");
    let config_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("hyper")
        .join("config.toml");

    if config_path.exists() {
        println!("   Config already exists at: {}", config_path.display());
        print!("   Overwrite? [y/N]: ");
        stdout().flush().ok();
        let mut overwrite = String::new();
        stdin().read_line(&mut overwrite).ok();
        if !overwrite.trim().to_lowercase().starts_with('y') {
            println!("   Keeping existing config.");
            println!();
            println!("   {0:─^40}", " Setup Complete ");
            println!("   Run 'hyper doctor' to verify your setup.");
            println!("   Run 'hyper run \"hello\" --mode ask' to test.");
            return;
        }
    }

    // Write config
    let api_key_val = api_key.unwrap_or_default();
    let config_content = format!(
        r#"[[providers]]
name = "{name}"
api_key = "{key}"
base_url = "{url}"
default_model = "{model}"
models = ["{model}"]

[[agents]]
name = "build"
mode = "Primary"
model = "{model}"
temperature = 0.1
description = "Execute code modifications"
[agents.permissions]
edit = "Allow"
bash = "Allow"
read = "Allow"
network = "Deny"
"#,
        name = provider_name,
        key = api_key_val,
        url = base_url,
        model = default_model,
    );

    std::fs::create_dir_all(config_path.parent().unwrap()).ok();
    match std::fs::write(&config_path, &config_content) {
        Ok(_) => println!("   ✅ Config created: {}", config_path.display()),
        Err(e) => eprintln!("   ⚠️  Failed to write config: {e}"),
    }

    // Step 4: Shell completions
    println!();
    println!("{0:─^40}", " Step 4: Shell Completions (optional) ");
    let current_shell = std::env::var("SHELL").unwrap_or_default();
    if current_shell.contains("zsh") {
        println!("   Detected: zsh");
        print!("   Install completions? [Y/n]: ");
        stdout().flush().ok();
        let mut install = String::new();
        stdin().read_line(&mut install).ok();
        if !install.trim().to_lowercase().starts_with('n') {
            println!("   Run: hyper completions zsh > /usr/local/share/zsh/site-functions/_hyper");
        }
    } else if current_shell.contains("bash") {
        println!("   Detected: bash");
        print!("   Install completions? [Y/n]: ");
        stdout().flush().ok();
        let mut install = String::new();
        stdin().read_line(&mut install).ok();
        if !install.trim().to_lowercase().starts_with('n') {
            println!("   Run: hyper completions bash > /usr/local/etc/bash_completion.d/hyper");
        }
    }

    println!();
    println!("{0:─^40}", " Setup Complete ");
    println!("   ✅ HyperAgent is ready to use!");
    println!();
    println!("   Next steps:");
    println!("     cd your-project");
    println!("     hyper init              # Build code index");
    println!("     hyper doctor            # Verify setup");
    println!("     hyper run \"explain this\" --mode ask");
    println!();
}
