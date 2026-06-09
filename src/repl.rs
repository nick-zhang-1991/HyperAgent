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
use base64::{engine::general_purpose::STANDARD, Engine as _};
use crate::router::ModelRouter;
use std::path::Path;
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
            "/mode", "/clear", "/cls",
            "/help", "/stats", "/memory",
            "/reindex", "/refresh",
            "/sessions", "/session",
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

    // IMPORTANT: we do NOT eagerly build the project index here.
    //
    // The REPL default mode is "ask" (Q&A). For pure Q&A the LLM should
    // answer directly from its own knowledge — no need to walk the project
    // tree, no PageRank lookup, no file scanning. Building the index is
    // expensive on large repos and very annoying for someone who just
    // wants to ask a quick question.
    //
    // The index is now loaded lazily:
    //   * ask / general modes → never load (direct LLM chat path)
    //   * code / debug / architect modes → load on first use (cache hit is
    //     instant, cache miss builds once and persists)
    //
    // `/reindex` (or `hyper init` outside the REPL) is the explicit way to
    // pre-warm the cache.
    let mut index: Option<HyperIndex> = None;

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

    // Orchestrator state across turns (for /image attachment)
    let mut orchestrator: Option<crate::agent::orchestrator::Orchestrator> = None;

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
    println!("  Mode:      {} (use /mode to change)", current_mode);
    println!("  Provider:  {} / {}", provider.model, provider.base_url);
    if let Some(cnt) = memory_count {
        println!("  Memory:    {} past learnings", cnt);
    }
    println!("  Index:     lazy (built on first code/debug/architect prompt)");
    println!("  Commands:  /exit  /mode <ask|code|debug|architect|general>  /image <path>  /help  /clear");
    println!();

    // REPL loop
    let mut conversation_history: Vec<(String, String)> = Vec::new();
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

        let trimmed = line.trim().to_string();

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
                "/help" | "/h" => {
                    println!();
                    println!("  Commands:");
                    println!("  ───────────────────────────────────────");
                    println!("  /exit, /quit       Exit the REPL");
                    println!("  /mode <mode>       Switch mode (ask/code/debug/architect/general)");
                    println!("  /mode              Show current mode");
                    println!("  /clear, /cls       Clear screen");
                    println!("  /help              Show this help");
                    println!("  /stats             Show project index stats");
                    println!("  /memory            Show memory stats");
                    println!("  /reindex           Force-rebuild the code index");
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
                        "ask" | "code" | "debug" | "architect" | "general" => {
                            current_mode = new_mode;
                            println!("  ✅ Mode switched to: {}", current_mode);
                        }
                        _ => {
                            println!(
                                "  ⚠️  Unknown mode: {}. Use: ask, code, debug, architect, general",
                                new_mode
                            );
                        }
                    }
                }
                cmd if cmd.starts_with("/image ") => {
                    match orchestrator.as_mut() {
                        Some(orch) => {
                            let path = cmd[7..].trim();
                            if path.is_empty() {
                                println!("  Usage: /image <path-to-image>");
                                println!("  Example: /image screenshot.png");
                                println!("  Supported formats: PNG, JPG, JPEG, GIF, WEBP");
                                continue;
                            }
                            match std::fs::read(path) {
                                Ok(bytes) => {
                                    let encoded = STANDARD.encode(&bytes);
                                    let mime = match std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
                                        Some("jpg") | Some("jpeg") => "image/jpeg",
                                        Some("gif") => "image/gif",
                                        Some("webp") => "image/webp",
                                        _ => "image/png",
                                    };
                                    let data_url = format!("data:{mime};base64,{encoded}");
                                    let _ = orch.with_image(data_url);
                                    println!("  📷 Image attached: {path} ({} bytes, base64-encoded)", bytes.len());
                                    println!("  It will be sent with your next message.");
                                }
                                Err(e) => {
                                    eprintln!("  ❌ Failed to read image: {e}");
                                }
                            }
                        }
                        None => {
                            eprintln!("  ⚠️  Agent not initialized yet. Type a message first.");
                        }
                    }
                }
                "/image" => {
                    println!("  Usage: /image <path-to-image>");
                    println!("  Example: /image screenshot.png");
                    println!("  Supported formats: PNG, JPG, JPEG, GIF, WEBP");
                }
                "/reindex" | "/refresh" => {
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
                "/sessions" => {
                    if let Ok(sm) = crate::session::SessionManager::new() {
                        match sm.list_by_tag("repl") {
                            Ok(sessions) => {
                                if sessions.is_empty() {
                                    println!("  No REPL sessions found.");
                                } else {
                                    println!("  Recent REPL sessions (last {}):", sessions.len().min(10));
                                    for s in sessions.iter().take(10) {
                                        println!("  {} | {}", s.id, s.summary.chars().take(70).collect::<String>());
                                    }
                                    println!();
                                    println!("  Use /session <id> to view details");
                                }
                            }
                            Err(e) => println!("  ⚠️  {e}"),
                        }
                    }
                }
                cmd if cmd.starts_with("/session ") => {
                    let id = cmd[9..].trim();
                    if let Ok(sm) = crate::session::SessionManager::new() {
                        match sm.load(id) {
                            Ok(s) => {
                                println!("{}", s.display());
                            }
                            Err(_e) => {
                                // Try fuzzy search
                                match sm.search(id) {
                                    Ok(matches) if !matches.is_empty() => {
                                        println!("  Session '{}' not found. Did you mean:", id);
                                        for m in matches.iter().take(5) {
                                            println!("    {} — {}", m.id, m.summary.chars().take(60).collect::<String>());
                                        }
                                    }
                                    _ => println!("  ❌ Session not found: {id}"),
                                }
                            }
                        }
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
            &current_mode,
            &provider,
            &memory_path,
            &memory_store,
            &mut index,
            &conversation_history,
        ).await {
            conversation_history.push((trimmed.clone(), response_text));
            if conversation_history.len() > 10 {
                conversation_history.remove(0);
            }
        }

        // Auto-save session after each turn
        if let Ok(sm) = crate::session::SessionManager::new() {
            let mut session = crate::session::Session::new(
                dir.to_string_lossy().as_ref(),
                &trimmed,
                &provider.model,
            );
            let hist_summary: Vec<String> = conversation_history.iter()
                .map(|(u, a)| format!("U: {} | A: {}", u.chars().take(50).collect::<String>(), a.chars().take(50).collect::<String>()))
                .collect();
            session.summary = format!("REPL {} | {} turns | {}", current_mode, conversation_history.len(), hist_summary.last().unwrap_or(&"".to_string()));
            session.messages = conversation_history.iter().map(|(u, _a)| {
                crate::session::ChatMessage { role: "user".into(), content: u.clone() }
            }).collect();
            session.tags = vec![current_mode.clone(), "repl".to_string()];
            let _ = sm.save(&session);
        }
    }

    // Save rustyline history for next session
    let _ = rl.save_history(&history_path);

    Ok(())
}

/// Modes that answer the user directly from the LLM without scanning
/// the project. Kept in sync with the gating in `run_prompt`.
const PASSTHROUGH_MODES: &[&str] = &["ask", "general"];

fn is_passthrough_mode(mode: &str) -> bool {
    PASSTHROUGH_MODES.contains(&mode)
}

/// Execute a single prompt through the agent orchestrator
/// Reuses the persisted index and memory store across turns.
///
/// Routing:
///   * ask / general → direct LLM chat (no project scan, no index, no
///     orchestrator — just answer the question from the model's knowledge
///     and the running conversation history).
///   * code / debug / architect → full pipeline: lazy index, plan/code/
///     review, then apply. The index is loaded on first use, cached for
///     the rest of the REPL session.
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

    // ── Fast path: ask / general ───────────────────────────────
    // Skip the index entirely. Build a small chat context, call the LLM,
    // print the answer. This is what makes the REPL feel responsive when
    // the user just wants to ask a question.
    if is_passthrough_mode(mode) {
        return run_passthrough_chat(
            prompt, dir, mode, provider, conversation_history, start,
        ).await;
    }

    // ── Code / debug / architect: lazy index + full pipeline ───
    if index.is_none() {
        print!("  📚 Indexing codebase... ");
        std::io::Write::flush(&mut std::io::stdout()).ok();
        match HyperIndex::new_or_load(dir) {
            Ok(mut idx) => {
                if !idx.has_cache() {
                    idx.build().ok();
                }
                let stats = idx.stats().ok();
                if let Some(s) = stats {
                    println!("done ({} files, {} symbols)", s.files, s.symbols);
                } else {
                    println!("done");
                }
                *index = Some(idx);
            }
            Err(e) => {
                println!("⚠️  could not build index: {e}");
                println!("  ℹ️  Continuing without code context (Orchestrator may not find files)");
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
        .with_mode(mode)
        .with_conversation_history(conversation_history.to_vec());

        if let Some(mem) = memory {
            orchestrator = orchestrator.with_memory(mem);
        }
        orchestrator = orchestrator.with_hooks(hooks);

        let mcp_registry = crate::mcp::McpRegistry::new(dir);
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
        // Index is unavailable and the mode requires it — fall back to
        // the passthrough chat so the user still gets an answer.
        eprintln!("  ℹ️  Falling back to direct LLM chat (no code index)");
        run_passthrough_chat(
            prompt, dir, mode, provider, conversation_history, start,
        ).await
    }
}

/// Direct LLM chat for ask / general — no project scan, no orchestrator.
///
/// We keep the system prompt intentionally short and mode-aware. The LLM
/// is told it's answering from general knowledge (and the running
/// conversation) — NOT from any code index.
async fn run_passthrough_chat(
    prompt: &str,
    _dir: &Path,
    mode: &str,
    provider: &LlmProvider,
    conversation_history: &[(String, String)],
    start: Instant,
) -> Option<String> {
    let system_prompt = match mode {
        "ask" => "You are HyperAgent's ask mode — a helpful assistant.\n\
                  Answer the user's question concisely and accurately.\n\
                  Use the conversation history for context.\n\
                  Format code with ```language```.\n\
                  Answer in the same language as the question.\n\
                  Rules:\n\
                  - Be concise but complete\n\
                  - Reference prior turns when relevant\n\
                  - Don't propose file edits — this is a read-only chat",
        "general" => "You are HyperAgent in general mode — a versatile AI assistant.\n\
                      Handle any task: coding, writing, analysis, translation,\n\
                      brainstorming, research, math, general knowledge.\n\
                      Use the conversation history for context.\n\
                      Format code with ```language```.\n\
                      Answer in the same language as the question.\n\
                      Rules:\n\
                      - Be helpful, concise, and accurate\n\
                      - For code questions, give runnable examples\n\
                      - If the user asks for file edits, suggest commands but\n\
                        do not claim to have modified anything",
        _ => "You are HyperAgent. Answer concisely in the same language as the question.",
    }
    .to_string();

    let mut messages: Vec<crate::llm::Message> = Vec::new();
    messages.push(crate::llm::Message::text("system", system_prompt));

    for (prev_user, prev_assistant) in conversation_history {
        messages.push(crate::llm::Message::text("user", prev_user.clone()));
        messages.push(crate::llm::Message::text("assistant", prev_assistant.clone()));
    }
    messages.push(crate::llm::Message::text("user", prompt));

    print!("\n  💬 ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    match provider.chat(messages).await {
        Ok(response) => {
            println!("{}", response);
            let elapsed = start.elapsed();
            println!(
                "  ⏱️  {:.1}s | direct chat (no code scan)",
                elapsed.as_secs_f64()
            );
            Some(response)
        }
        Err(e) => {
            eprintln!("\n  ⚠️  Error: {e}");
            None
        }
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
