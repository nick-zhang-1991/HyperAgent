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
use std::time::Instant;

/// Run the interactive REPL session
pub async fn run_repl() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let dir = cwd.clone();

    // Build or load index once — persists across turns in the REPL
    print!("📚 Indexing codebase... ");
    std::io::Write::flush(&mut std::io::stdout())?;
    let mut index = match HyperIndex::new_or_load(&dir) {
        Ok(mut idx) => {
            if !idx.has_cache() {
                idx.build()?;
            }
            let stats = idx.stats()?;
            println!("done ({} files, {} symbols)", stats.files, stats.symbols);
            Some(idx)
        }
        Err(e) => {
            println!("⚠️  could not build index: {e}");
            None
        }
    };

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

    // Current mode
    let mut current_mode = "ask".to_string();

    // Welcome banner
    println!();
    println!("╔══════════════════════════════════════════════╗");
    println!("║        HyperAgent Interactive Shell         ║");
    println!("║    Type prompts directly, like chatting     ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();
    println!("  Directory: {}", dir.display());
    println!("  Mode:      {} (use /mode to change)", current_mode);
    println!("  Provider:  {} / {}", provider.model, provider.base_url);
    if let Some(cnt) = memory_count {
        println!("  Memory:    {} past learnings", cnt);
    }
    println!("  Commands:  /exit  /mode <ask|code|debug|architect>  /help  /clear");
    println!();

    // REPL loop
    let mut conversation_history: Vec<(String, String)> = Vec::new();
    loop {
        // Print prompt
        print!("hyper> ");
        std::io::Write::flush(&mut std::io::stdout())?;

        // Read a line
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => {
                println!(); // EOF
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Read error: {e}");
                break;
            }
        }

        let trimmed = line.trim().to_string();

        if trimmed.is_empty() {
            continue;
        }

        // Handle slash commands
        if trimmed.starts_with('/') {
            match trimmed.as_str() {
                "/exit" | "/quit" | "/q" => {
                    println!("👋 Goodbye!");
                    break;
                }
                "/help" | "/h" => {
                    println!();
                    println!("  Commands:");
                    println!("  ───────────────────────────────────────");
                    println!("  /exit, /quit       Exit the REPL");
                    println!("  /mode <mode>       Switch mode (ask/code/debug/architect)");
                    println!("  /mode              Show current mode");
                    println!("  /clear, /cls       Clear screen");
                    println!("  /help              Show this help");
                    println!("  /stats             Show project index stats");
                    println!("  /memory            Show memory stats");
                    println!("  /reindex           Force-rebuild the code index");
                    println!("  ───────────────────────────────────────");
                    println!("  Just type anything to ask the agent!");
                    println!();
                }
                "/clear" | "/cls" => {
                    print!("\x1B[2J\x1B[H");
                    std::io::Write::flush(&mut std::io::stdout())?;
                }
                "/mode" => {
                    println!("  Current mode: {}", current_mode);
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
                    // Show last 5
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
                cmd if cmd.starts_with("/mode ") => {
                    let new_mode = cmd[6..].trim().to_lowercase();
                    match new_mode.as_str() {
                        "ask" | "code" | "debug" | "architect" => {
                            current_mode = new_mode;
                            println!("  ✅ Mode switched to: {}", current_mode);
                        }
                        _ => {
                            println!(
                                "  ⚠️  Unknown mode: {}. Use: ask, code, debug, architect",
                                new_mode
                            );
                        }
                    }
                }
                "/reindex" => {
                    println!("  Rebuilding index...");
                    match HyperIndex::new(&dir) {
                        Ok(mut new_idx) => match new_idx.build() {
                            Ok(s) => {
                                // Replace the in-memory index
                                index = Some(new_idx);
                                println!("  Done: {} files, {} symbols", s.files, s.symbols);
                            }
                            Err(e) => println!("  ⚠️  {e}"),
                        },
                        Err(e) => println!("  ⚠️  {e}"),
                    }
                }
                _ => {
                    println!("  Unknown command: {trimmed}. Type /help");
                }
            }
            continue;
        }

        // Run the agent with the prompt — reuse the persisted index and memory
        match run_prompt(
            &trimmed,
            &dir,
            &current_mode,
            &provider,
            &memory_path,
            &memory_store,
            &mut index,  // pass mutable reference so index can be replaced if needed
            &conversation_history,
        ).await {
            Some(response_text) => {
                // Add current turn to history (for next iteration)
                // Keep only last 10 turns to bound token usage
                conversation_history.push((trimmed.clone(), response_text));
                if conversation_history.len() > 10 {
                    conversation_history.remove(0);
                }
            }
            None => {
                // Error — don't record to history
            }
        }
    }

    Ok(())
}

/// Execute a single prompt through the agent orchestrator
/// Reuses the persisted index and memory store across turns.
#[allow(clippy::too_many_arguments)]
async fn run_prompt(
    prompt: &str,
    dir: &Path,
    mode: &str,
    provider: &LlmProvider,
    _memory_path: &Path,
    _memory_store: &Option<SqliteMemoryStore>,
    index: &mut Option<HyperIndex>,
    conversation_history: &[(String, String)],
) -> Option<String> {
    let start = Instant::now();

    // Ensure index is available (build on first use if needed)
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

    // Build memory manager — reuse the persisted store connection
    let memory = _memory_store.as_ref().map(|store| {
        let wrapped_store: Box<dyn crate::memory::MemoryStore> = Box::new(store.clone());
        MemoryManager::new(wrapped_store, "hyperagent")
    });

    // Fresh hooks
    let hooks = HookRegistry::new(dir);

    // Build provider clone
    let provider = provider.clone();

    println!();

    // Build orchestrator — take ownership of index, then give it back
    if let Some(idx) = index.take() {
        let mut orchestrator = crate::agent::orchestrator::Orchestrator::new(
            idx,
            provider,
            dir.to_path_buf(),
            3,
            false,
        )
        .with_mode(mode)
        .with_conversation_history(conversation_history.to_vec());

        if let Some(mem) = memory {
            orchestrator = orchestrator.with_memory(mem);
        }
        orchestrator = orchestrator.with_hooks(hooks);

        // Connect MCP servers (REPL mode)
        let mcp_registry = crate::mcp::McpRegistry::new(dir);
        let mcp_servers = crate::mcp::McpRegistry::discover_servers(&[]);
        if !mcp_servers.is_empty() {
            mcp_registry.connect_all(&mcp_servers).await;
            orchestrator = orchestrator.with_mcp(mcp_registry);
        }

        // Run
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
    // Hot-reload config
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
