//! HyperAgent Interactive REPL — like Hermes' chat interface
//!
//! Enter with: hyper  (no subcommand — opens interactive shell)
//!
//! Commands:
//!   /exit, /quit     Exit REPL
//!   /mode <mode>     Switch agent mode (ask/code/debug/architect)
//!   /help            Show help
//!   /mode            Show current mode
//!   /clear           Clear screen
//!   /stats           Show project index stats
//!   /memory          Show memory stats
//!   /reindex         Force reindex the codebase

use crate::hooks::HookRegistry;
use crate::index::HyperIndex;
use crate::llm::LlmProvider;
use crate::memory::{MemoryManager, SqliteMemoryStore};
use crate::router::ModelRouter;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use rustyline::Editor;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::completion::{Completer, Pair};
use rustyline::Context;
use rustyline::Helper;

/// REPL command completer — provides tab completion for /commands
struct ReplCompleter;

impl Helper for ReplCompleter {}

impl Highlighter for ReplCompleter {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> std::borrow::Cow<'l, str> {
        std::borrow::Cow::Borrowed(line)
    }
    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        false
    }
}

impl Hinter for ReplCompleter {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}

impl Validator for ReplCompleter {
    fn validate(&self, _ctx: &mut rustyline::validate::ValidationContext) -> rustyline::Result<rustyline::validate::ValidationResult> {
        Ok(rustyline::validate::ValidationResult::Valid(None))
    }
    fn validate_while_typing(&self) -> bool {
        false
    }
}

impl Completer for ReplCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];
        let commands = vec![
            "/exit", "/quit",
            "/clear", "/cls",
            "/help", "/stats", "/memory",
            "/reindex", "/refresh", "/budget", "/telemetry", "/plugins", "/health", "/org", "/repo", "/edit", "/search", "/bg",
        ];

        let candidates: Vec<Pair> = if prefix.starts_with('/') {
            commands
                .into_iter()
                .filter(|cmd| cmd.starts_with(prefix))
                .map(|cmd| Pair {
                    display: cmd.to_string(),
                    replacement: cmd.to_string(),
                })
                .collect()
        } else {
            vec![]
        };

        Ok((pos, candidates))
    }
}

/// Run the interactive REPL session
pub async fn run_repl() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let dir = cwd.clone();

    // Memory path — create once, reuse across turns
    let memory_path = dir.join(".hyper").join("memory.db");
    std::fs::create_dir_all(dir.join(".hyper")).ok();
    let memory_count = get_memory_count(&memory_path);

    // Also keep memory store alive to avoid SQLite connection churn
    let memory_store = if memory_path.exists() {
        SqliteMemoryStore::new(&memory_path).ok()
    } else {
        None
    };

    // Get provider from config
    let provider = get_provider_from_config();

    // Index is lazily built on first prompt or via /reindex
    let mut index: Option<HyperIndex> = None;

    // Setup rustyline with persistent history
    let history_path = dir.join(".hyper").join("history.txt");
    let mut rl = Editor::<ReplCompleter, rustyline::history::FileHistory>::new()?;
    rl.set_helper(Some(ReplCompleter));
    if history_path.exists() {
        let _ = rl.load_history(&history_path);
    }

    // Welcome banner
    println!();
    println!("╔══════════════════════════════════════════════╗");
    println!("║        HyperAgent Interactive Shell         ║");
    println!("║    Type prompts directly, like chatting     ║");
    println!("║    ↑↓ arrow keys to browse history          ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();
    println!("  Directory: {}", dir.display());
    println!("  Provider:  {} / {}", provider.model, provider.base_url);
    if let Some(cnt) = memory_count {
        println!("  Memory:    {} past learnings", cnt);
    }
    println!("  Commands:  /exit  /help  /clear  /stats  /memory  /reindex  /budget  /telemetry  /plugins  /health  /repo  /org  /edit  /search  /bg");
    println!();

    // REPL loop
    let mut conversation_history: Vec<(String, String)> = Vec::new();
    let bg_manager = crate::process::ProcessManager::new();
    let mut browser_mgr = crate::browser::BrowserManager::new();
    loop {
        // Read line with rustyline (supports ↑↓ history, line editing)
        let line = match rl.readline("hyper> ") {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C — print newline, continue
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D — exit
                println!();
                break;
            }
            Err(e) => {
                eprintln!("Read error: {e}");
                break;
            }
        };

        let mut trimmed = line.trim().to_string();

        if trimmed.is_empty() {
            continue;
        }

        // Add to rustyline history (for ↑↓ navigation)
        rl.add_history_entry(&trimmed)?;

        // Handle slash commands
        if trimmed.starts_with('/') {
            match trimmed.as_str() {
                "/exit" | "/quit" | "/q" => {
                    println!("👋 Goodbye!");
                    break;
                }
                cmd if cmd.starts_with("/bg") => {
                    use crate::process::BgStatus;
                    let args: Vec<&str> = cmd[3..].trim().split_whitespace().collect();
                    match args.first() {
                        Some(&"list") | None => {
                            let procs = bg_manager.list();
                            if procs.is_empty() {
                                println!("  No background processes.");
                            } else {
                                println!("  📋 Background Processes:");
                                for (id, cmd_str, status, elapsed) in &procs {
                                    let status_icon = match status {
                                        BgStatus::Running => "🟢",
                                        BgStatus::Done(0) => "✅",
                                        BgStatus::Done(_) => "❌",
                                        BgStatus::Killed => "🛑",
                                        BgStatus::Failed(_) => "💥",
                                    };
                                    println!("  {status_icon} {id}: {cmd_str} [{elapsed}] — {status:?}");
                                }
                            }
                        }
                        Some(&"log" | &"output") if args.len() >= 2 => {
                            let id = args[1];
                            let lines = bg_manager.read_output(id);
                            if lines.is_empty() {
                                println!("  No new output from {id}.");
                            } else {
                                println!("  📄 Output from {id}:");
                                for line in &lines {
                                    println!("    {line}");
                                }
                            }
                        }
                        Some(&"all") if args.len() >= 2 => {
                            let id = args[1];
                            let lines = bg_manager.all_output(id);
                            if lines.is_empty() {
                                println!("  No output from {id}.");
                            } else {
                                println!("  📄 All output from {id}:");
                                for line in &lines {
                                    println!("    {line}");
                                }
                            }
                        }
                        Some(&"kill") if args.len() >= 2 => {
                            let id = args[1];
                            match bg_manager.kill(id) {
                                Ok(()) => println!("  🛑 Killed {id}"),
                                Err(e) => println!("  ⚠️  {e}"),
                            }
                        }
                        Some(&"input") if args.len() >= 3 => {
                            let id = args[1];
                            let text = args[2..].join(" ");
                            match bg_manager.send_input(id, &text) {
                                Ok(()) => println!("  📝 Sent input to {id}"),
                                Err(e) => println!("  ⚠️  {e}"),
                            }
                        }
                        Some(&"wait") if args.len() >= 2 => {
                            let id = args[1];
                            let timeout = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60u64);
                            println!("  ⏳ Waiting up to {timeout}s for {id}...");
                            match bg_manager.wait(id, timeout) {
                                Ok(status) => println!("  {status:?}"),
                                Err(e) => println!("  ⚠️  {e}"),
                            }
                        }
                        Some(&"cleanup") => {
                            let removed = bg_manager.cleanup();
                            println!("  🧹 Removed {removed} completed process(es)");
                        }
                        _ => {
                            // Default: run a command in the background
                            let rest = cmd[3..].trim();
                            if rest.is_empty() {
                                println!("  Usage: /bg <command> [args...]");
                                println!("  Commands: /bg list, /bg log <id>, /bg kill <id>, /bg input <id> <text>, /bg wait <id> [timeout], /bg cleanup");
                            } else {
                                let parts: Vec<String> = rest.split_whitespace().map(String::from).collect();
                                if parts.is_empty() {
                                    continue;
                                }
                                let command = parts[0].clone();
                                let args: Vec<String> = parts[1..].to_vec();
                                match bg_manager.spawn(&command, &args, &dir.to_string_lossy()) {
                                    Ok(id) => println!("  🚀 Spawned {id}: {command} {}", args.join(" ")),
                                    Err(e) => println!("  ⚠️  {e}"),
                                }
                            }
                        }
                    }
                }
                cmd if cmd.starts_with("/browser") => {
                    let args: Vec<&str> = cmd[8..].trim().split_whitespace().collect();
                    match args.first() {
                        Some(&"open") if args.len() >= 2 => {
                            let url = args[1];
                            match browser_mgr.get_or_launch().await {
                                Ok(browser) => {
                                    match browser.navigate(url).await {
                                        Ok(_) => {
                                            // Take screenshot after navigation
                                            let shot_path = format!("/tmp/hyper-shot-{}.png", std::process::id());
                                            match browser.screenshot_file(&shot_path).await {
                                                Ok(path) => {
                                                    if let Ok(title) = browser.page_title().await {
                                                        println!("   📄 Title: {}", title);
                                                    }
                                                    println!("   📷 Screenshot saved: {}", path.display());
                                                }
                                                Err(e) => println!("   ⚠️  Screenshot failed: {e}"),
                                            }
                                        }
                                        Err(e) => println!("   ⚠️  Navigation failed: {e}"),
                                    }
                                }
                                Err(e) => println!("   ⚠️  Browser launch failed: {e}"),
                            }
                        }
                        Some(&"screenshot") | Some(&"shot") => {
                            let path = args.get(1).map(|s| *s).unwrap_or("/tmp/hyper-shot.png");
                            match browser_mgr.get_or_launch().await {
                                Ok(browser) => {
                                    let save_path = PathBuf::from(path);
                                    match browser.screenshot_file(path).await {
                                        Ok(p) => println!("   📷 Screenshot saved: {}", p.display()),
                                        Err(e) => println!("   ⚠️  Screenshot failed: {e}"),
                                    }
                                }
                                Err(e) => println!("   ⚠️  {e}"),
                            }
                        }
                        Some(&"source") | Some(&"text") => {
                            match browser_mgr.get_or_launch().await {
                                Ok(browser) => {
                                    match browser.page_text().await {
                                        Ok(text) => {
                                            // Truncate to avoid flooding terminal
                                            let preview = if text.len() > 2000 {
                                                format!("{}...\n   (truncated, {} chars total)", &text[..2000], text.len())
                                            } else {
                                                text
                                            };
                                            println!("   📄 Page content:\n{}", preview);
                                        }
                                        Err(e) => println!("   ⚠️  {e}"),
                                    }
                                }
                                Err(e) => println!("   ⚠️  {e}"),
                            }
                        }
                        Some(&"html") => {
                            match browser_mgr.get_or_launch().await {
                                Ok(browser) => {
                                    match browser.page_html().await {
                                        Ok(html) => {
                                            let preview = if html.len() > 2000 {
                                                format!("{}...\n   (truncated, {} chars total)", &html[..2000], html.len())
                                            } else {
                                                html
                                            };
                                            println!("   📄 Page HTML:\n{}", preview);
                                        }
                                        Err(e) => println!("   ⚠️  {e}"),
                                    }
                                }
                                Err(e) => println!("   ⚠️  {e}"),
                            }
                        }
                        Some(&"click") if args.len() >= 2 => {
                            let selector = args[1..].join(" ");
                            match browser_mgr.get_or_launch().await {
                                Ok(browser) => {
                                    match browser.click(&selector).await {
                                        Ok(()) => {
                                            println!("   🖱️  Clicked: {}", selector);
                                            // Auto-screenshot after click
                                            let shot_path = format!("/tmp/hyper-click-{}.png", std::process::id());
                                            if let Ok(p) = browser.screenshot_file(&shot_path).await {
                                                println!("   📷 Screenshot: {}", p.display());
                                            }
                                        }
                                        Err(e) => println!("   ⚠️  Click failed: {e}"),
                                    }
                                }
                                Err(e) => println!("   ⚠️  {e}"),
                            }
                        }
                        Some(&"eval") if args.len() >= 2 => {
                            let js = args[1..].join(" ");
                            match browser_mgr.get_or_launch().await {
                                Ok(browser) => {
                                    match browser.evaluate_js(&js).await {
                                        Ok(value) => {
                                            let display = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string());
                                            println!("   🖥️  JS result:\n{}", display);
                                        }
                                        Err(e) => println!("   ⚠️  JS eval failed: {e}"),
                                    }
                                }
                                Err(e) => println!("   ⚠️  {e}"),
                            }
                        }
                        Some(&"close") => {
                            match browser_mgr.close().await {
                                Ok(()) => {}
                                Err(e) => println!("   ⚠️  {e}"),
                            }
                        }
                        Some(&"status") => {
                            print!("{}", browser_mgr.status());
                        }
                        _ => {
                            println!("  Browser commands:");
                            println!("  /browser open <url>       Open URL and screenshot");
                            println!("  /browser screenshot [path] Take screenshot");
                            println!("  /browser click <selector> Click element by CSS selector");
                            println!("  /browser source            Get page text content");
                            println!("  /browser html              Get page HTML");
                            println!("  /browser eval <js>         Execute JavaScript");
                            println!("  /browser close             Close browser");
                            println!("  /browser status            Show connection info");
                        }
                    }
                }
                cmd if cmd.starts_with("/edit ") || cmd.starts_with("/e ") => {
                    let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
                    if parts.len() < 3 {
                        println!("  Usage: /edit N \"new prompt\" — replace message N and re-execute");
                        continue;
                    }
                    let idx: usize = match parts[1].parse::<usize>() {
                        Ok(n) if n > 0 && n <= conversation_history.len() => n - 1,
                        Ok(_) => {
                            println!("  ⚠️  Index out of range (1..{})", conversation_history.len());
                            continue;
                        }
                        Err(_) => {
                            println!("  ⚠️  Invalid index: {}", parts[1]);
                            continue;
                        }
                    };
                    let new_prompt = parts[2].trim_matches('"').to_string();
                    let old_prompt = conversation_history[idx].0.clone();
                    println!("  ✏️  Edit [{}/{}]: \"{}\" → \"{}\"", idx + 1, conversation_history.len(), old_prompt, new_prompt);
                    conversation_history[idx].0 = new_prompt.clone();
                    trimmed = new_prompt;
                    // Fall through to execute the edited prompt
                }
                "/help" | "/h" => {
                    println!();
                    println!("  Commands:");
                    println!("  ───────────────────────────────────────");
                    println!("  /exit, /quit       Exit the REPL");
                    println!("  /clear, /cls       Clear screen");
                    println!("  /help              Show this help");
                    println!("  /stats             Show project index stats");
                    println!("  /memory            Show memory stats");
                    println!("  /reindex           Force-rebuild the code index");
                    println!("  /budget            Show session budget status");
                    println!("  /telemetry         Show usage telemetry");
                    println!("  /plugins           List installed plugins");
                    println!("  /health            Run codebase health check");
                    println!("  /repo              Multi-repository management");
                    println!("  /org               Organization management");
                    println!("  /edit N \"msg\"      Edit message N and re-execute");
                    println!("  /bg <cmd>           Run command in background");
                    println!("  /bg list            List background processes");
                    println!("  /bg log <id>        Read output from bg process");
                    println!("  /bg kill <id>       Kill a background process");
                    println!("  /bg input <id> t    Send input to bg process");
                    println!("  /search \"query\"    Semantic code search (RAG)");
                    println!("  ───────────────────────────────────────");
                    println!("  ↑↓ arrow keys      Browse command history");
                    println!("  Ctrl+C             Cancel current input");
                    println!("  Ctrl+D             Exit REPL");
                    println!("  Just type anything to ask the agent!");
                    println!();
                }
                "/clear" | "/cls" => {
                    print!("\x1B[2J\x1B[H");
                    std::io::Write::flush(&mut std::io::stdout())?;
                }
                "/stats" => {
                    if let Some(ref idx) = index {
                        match idx.stats() {
                            Ok(s) => {
                                println!("  Index stats:");
                                println!("    Files:      {}", s.files);
                                println!("    Symbols:    {}", s.symbols);
                                println!("    References: {}", s.references);
                                println!("    Languages:  {}", s.languages);
                                println!("    Cache:      {}", s.cache_size);
                            }
                            Err(e) => println!("  ⚠️  {e}"),
                        }
                    }
                }
                "/memory" => {
                    let cnt = get_memory_count(&memory_path).unwrap_or(0);
                    println!("  Memory entries: {cnt}");
                    if cnt > 0 {
                        if let Ok(conn) = rusqlite::Connection::open(&memory_path) {
                            if let Ok(mut stmt) =
                                conn.prepare("SELECT content, created_at FROM memories ORDER BY created_at DESC LIMIT 5")
                            {
                                let rows = stmt
                                    .query_map([], |row| {
                                        let content: String = row.get(0)?;
                                        let created: String = row.get(1)?;
                                        Ok((content, created))
                                    })
                                    .ok();
                                if let Some(rows) = rows {
                                    for row in rows.flatten() {
                                        let preview = if row.0.len() > 80 {
                                            format!("{}...", &row.0[..80])
                                        } else {
                                            row.0.clone()
                                        };
                                        println!("    [{:.8}] {}", row.1, preview);
                                    }
                                }
                            }
                        }
                    }
                }
                "/reindex" => {
                    println!("  Rebuilding index...");
                    match HyperIndex::new(&dir) {
                        Ok(mut new_idx) => match new_idx.build() {
                            Ok(s) => {
                                index = Some(new_idx);
                                println!("  Done: {} files, {} symbols", s.files, s.symbols);
                            }
                            Err(e) => println!("  ⚠️  {e}"),
                        },
                        Err(e) => println!("  ⚠️  {e}"),
                    }
                }
                "/budget" => {
                    let budget_path = dir.join(".hyper").join("budget.json");
                    if let Some(tracker) = crate::budget_tracker::BudgetTracker::load(&budget_path) {
                        println!("  Budget status: {}", tracker.status_display());
                        println!("  Calls: {}", tracker.call_count);
                        println!("  Input:  {} tokens", tracker.total_input_tokens);
                        println!("  Output: {} tokens", tracker.total_output_tokens);
                    } else {
                        println!("  No budget tracker found. Budget is unlimited.");
                    }
                }
                "/telemetry" => {
                    let telemetry_path = dir.join(".hyper").join("telemetry.db");
                    let config = crate::telemetry::TelemetryConfig {
                        enabled: true,
                        storage_path: telemetry_path.to_string_lossy().to_string(),
                    };
                    let t = crate::telemetry::Telemetry::new(config);
                    println!("{}", t.daily_stats());
                }
                "/plugins" => {
                    let registry = crate::plugins::PluginRegistry::new(&dir);
                    println!("{}", registry.render());
                }
                "/health" => {
                    println!("   Running health check...");
                    let report = crate::health::run_health_check(&dir);
                    print!("{}", crate::health::render_report(&report));
                }
                cmd if cmd.starts_with("/search") => {
                    let query = cmd.trim_start_matches("/search").trim().trim_matches('"');
                    if query.is_empty() {
                        println!("  Usage: /search \"your query\" — semantic code search");
                        continue;
                    }
                    let embedder = crate::embed::create_provider("mock:768");
                    if let Some(e) = embedder {
                        let query_emb = match e.embed(&[query.to_string()]) {
                            Ok(v) => v.into_iter().next().unwrap_or_default(),
                            Err(err) => { println!("  ⚠️  Embedding error: {err}"); continue; }
                        };
                        if let Some(ref idx) = index {
                            let files = idx.get_relevant_files("", 10, 9999);
                            println!("  🔎 Semantic search: \"{}\"", query);
                            for file in &files {
                                let file_emb = e.embed(&[file.content.clone()]).unwrap_or_default().into_iter().next().unwrap_or_default();
                                let sim = crate::embed::cosine_similarity(&query_emb, &file_emb);
                                println!("     [{:.2}] {}", sim, file.path.display());
                            }
                        } else {
                            println!("  ⚠️  Index not available");
                        }
                    } else {
                        println!("  ⚠️  No embedding provider configured. Set HYPER_EMBED=ollama:model:url or use mock:768");
                    }
                }
                cmd if cmd.starts_with("/repo") => {
                    let mut repo_mgr = crate::multi_repo::MultiRepoManager::new(&dir);
                    let args: Vec<&str> = cmd[5..].trim().split_whitespace().collect();
                    match args.first() {
                        Some(&"add") if args.len() >= 2 => {
                            let path = std::path::Path::new(args[1]);
                            let canonical = if path.is_absolute() {
                                path.to_path_buf()
                            } else {
                                dir.join(path)
                            };
                            match repo_mgr.add(&canonical) {
                                Ok(msg) => println!("{msg}"),
                                Err(e) => println!("  ⚠️  {e}"),
                            }
                        }
                        Some(&"remove") | Some(&"rm") if args.len() >= 2 => {
                            match repo_mgr.remove(args[1]) {
                                Ok(msg) => println!("{msg}"),
                                Err(e) => println!("  ⚠️  {e}"),
                            }
                        }
                        Some(&"search") if args.len() >= 2 => {
                            let query = args[1..].join(" ");
                            print!("{}", repo_mgr.search(&query));
                        }
                        _ => {
                            print!("{}", repo_mgr.list());
                            if !repo_mgr.is_empty() {
                                println!("   Commands: /repo add <path>  /repo remove <name>  /repo search <query>");
                            }
                        }
                    }
                }
                cmd if cmd.starts_with("/org") => {
                    let mut org_mgr = crate::organization::OrgManager::new(&dir);
                    let args: Vec<&str> = cmd[4..].trim().split_whitespace().collect();
                    match args.first() {
                        Some(&"init") if args.len() >= 2 => {
                            match org_mgr.init(args[1], args.get(2).unwrap_or(&"admin@local")) {
                                Ok(msg) => println!("{msg}"),
                                Err(e) => println!("  ⚠️  {e}"),
                            }
                        }
                        Some(&"status") | Some(&"info") => {
                            print!("{}", org_mgr.info());
                        }
                        Some(&"team") if args.get(1) == Some(&"add") && args.len() >= 3 => {
                            let role = args.get(3).unwrap_or(&"member");
                            match org_mgr.add_member(args[2], role, None) {
                                Ok(msg) => println!("{msg}"),
                                Err(e) => println!("  ⚠️  {e}"),
                            }
                        }
                        Some(&"team") if args.get(1) == Some(&"remove") && args.len() >= 3 => {
                            match org_mgr.remove_member(args[2]) {
                                Ok(msg) => println!("{msg}"),
                                Err(e) => println!("  ⚠️  {e}"),
                            }
                        }
                        Some(&"team") if args.get(1) == Some(&"list") => {
                            print!("{}", org_mgr.list_members());
                        }
                        _ => crate::organization::print_org_help(),
                    }
                }
                _ => {
                    println!("  Unknown command: {trimmed}. Type /help");
                }
            }
            continue;
        }

        // Run the agent with the prompt
        if let Some(response_text) = run_prompt(
            &trimmed,
            &dir,
            &provider,
            &memory_path,
            &memory_store,
            &mut index,
            &conversation_history,
        ).await {
            // Display the agent's response to the user
            println!("{}", response_text);
            conversation_history.push((trimmed.clone(), response_text));
            if conversation_history.len() > 10 {
                conversation_history.remove(0);
            }
        }
    }

    // Save rustyline history for next session
    let _ = rl.save_history(&history_path);

    Ok(())
}

/// Execute a single prompt through the agent orchestrator
/// Reuses the persisted index and memory store across turns.
#[allow(clippy::too_many_arguments)]
async fn run_prompt(
    prompt: &str,
    dir: &Path,
    provider: &LlmProvider,
    _memory_path: &Path,
    _memory_store: &Option<SqliteMemoryStore>,
    index: &mut Option<HyperIndex>,
    conversation_history: &[(String, String)],
) -> Option<String> {
    let start = Instant::now();

    if index.is_none() {
        match HyperIndex::new_or_load(dir) {
            Ok(mut idx) => {
                if !idx.has_cache() {
                    idx.build().ok();
                }
                *index = Some(idx);
            }
            Err(_) => {
                println!("  ⚠️  Index not available");
                return None;
            }
        }
    }

    let memory = _memory_store.as_ref().map(|store| {
        let wrapped_store: Box<dyn crate::memory::MemoryStore> = Box::new(store.clone());
        MemoryManager::new(wrapped_store, "hyperagent")
    });

    let hooks = HookRegistry::new(dir);
    let provider = provider.clone();

    println!();

    if let Some(idx) = index.take() {
        let mut orchestrator = crate::agent::orchestrator::Orchestrator::new(
            idx,
            provider,
            dir.to_path_buf(),
            3,
            false,
        )
        .with_conversation_history(conversation_history.to_vec());

        if let Some(mem) = memory {
            orchestrator = orchestrator.with_memory(mem);
        }
        orchestrator = orchestrator.with_hooks(hooks);

        let mut mcp_registry = crate::mcp::McpRegistry::new(dir);
        let mcp_servers = crate::mcp::McpRegistry::discover_servers(&[]);
        if !mcp_servers.is_empty() {
            mcp_registry.connect_all(&mcp_servers).await;
            orchestrator = orchestrator.with_mcp(mcp_registry);
        }

        let result = orchestrator.run(prompt).await;
        println!();
        let elapsed = start.elapsed();
        tracing::debug!("REPL round-trip: {:.2}s", elapsed.as_secs_f64());

        match result {
            Ok(result) => {
                println!(
                    "  ⏱️  {:.1}s | {} | {} memories | {} files modified",
                    result.elapsed.as_secs_f64(),
                    result.model_name,
                    result.memories_recorded,
                    result.files_modified,
                );
                Some(result.response_text.clone())
            }
            Err(e) => {
                println!("  ⚠️  Error: {e}");
                None
            }
        }
    } else {
        println!("  ⚠️  Index not available");
        None
    }
}

/// Get provider from config or env (with hot-reload check)
pub fn get_provider_from_config() -> LlmProvider {
    let mut router = match ModelRouter::new() {
        Ok(r) => r,
        Err(_) => return LlmProvider::from_env_or(None, None, None).unwrap(),
    };
    let _ = router.refresh_if_changed();
    let agent_config = router
        .get_agent("build")
        .or_else(|| router.get_agent("general"));
    let model_name = agent_config
        .map(|a| a.model.as_str())
        .unwrap_or("deepseek-v4-flash");
    match router.select_provider(model_name) {
        Ok(p) => LlmProvider::new(
            if model_name != p.default_model {
                model_name
            } else {
                &p.default_model
            },
            &p.base_url,
            &p.api_key,
        )
        .unwrap_or_else(|_| LlmProvider::from_env_or(None, None, None).unwrap()),
        Err(_) => LlmProvider::from_env_or(None, None, None).unwrap(),
    }
}

fn get_memory_count(path: &Path) -> Option<usize> {
    if !path.exists() {
        return Some(0);
    }
    match rusqlite::Connection::open(path) {
        Ok(conn) => {
            let count: usize = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .unwrap_or(0);
            Some(count)
        }
        Err(_) => None,
    }
}
