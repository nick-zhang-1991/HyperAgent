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
    // The REPL doesn't pre-load the index. Whether to scan the project
    // is decided per-prompt by `looks_like_coding_task`:
    //   * Questions / chat / explanations → direct LLM call, no scan.
    //   * Coding tasks (imperative verbs, file refs, code blocks, dev
    //     commands) → lazy load (cache hit is instant, miss builds
    //     once and persists for the rest of the session).
    //
    // `/reindex` (or `hyper init` outside the REPL) is the explicit
    // way to pre-warm the cache. `/code` is no longer needed because
    // routing is automatic.
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
    println!("  Index:     auto (loaded only when prompt looks like a coding task)");
    println!("  Commands:  /exit  /mode <ask|code|debug|architect|general>  /image <path>  /help  /clear  /reindex");
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
                    println!("  /mode <mode>       Switch LLM tone (ask/code/debug/architect/general)");
                    println!("  /mode              Show current mode");
                    println!("  /clear, /cls       Clear screen");
                    println!("  /help              Show this help");
                    println!("  /stats             Show project index stats");
                    println!("  /memory            Show memory stats");
                    println!("  /reindex           Force-rebuild the code index");
                    println!("  ───────────────────────────────────────");
                    println!("  Routing is automatic per prompt:");
                    println!("    Q&A / chat       → direct LLM (no project scan)");
                    println!("    coding tasks     → lazy index + full pipeline");
                    println!("  Use /code <prompt> to force the code path.");
                    println!("  ↑↓ arrow keys      Browse command history");
                    println!("  Ctrl+C             Cancel current input");
                    println!("  Ctrl+D             Exit REPL");
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
                cmd if cmd.starts_with("/code ") => {
                    // Force the code path: bypass the classifier and always
                    // load the project index / run the full pipeline.
                    let force_prompt = cmd[6..].trim().to_string();
                    if force_prompt.is_empty() {
                        println!("  Usage: /code <prompt>");
                        println!("  Force the code path (skip auto-routing).");
                        continue;
                    }
                    rl.add_history_entry(&force_prompt).ok();
                    println!("  🧠 [forced code path]");
                    if let Some(response_text) = run_prompt(
                        &force_prompt,
                        &dir,
                        &current_mode,
                        &provider,
                        &memory_path,
                        &memory_store,
                        &mut index,
                        &conversation_history,
                        true, // force_code
                    ).await {
                        conversation_history.push((force_prompt.clone(), response_text));
                        if conversation_history.len() > 10 {
                            conversation_history.remove(0);
                        }
                    }
                    continue;
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

        // Run the agent with the prompt (auto-routed by looks_like_coding_task)
        if let Some(response_text) = run_prompt(
            &trimmed,
            &dir,
            &current_mode,
            &provider,
            &memory_path,
            &memory_store,
            &mut index,
            &conversation_history,
            false, // auto-route; use /code to force
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

/// Heuristic classifier: does this prompt need the project index, or can
/// the LLM answer it directly from its own knowledge?
///
/// Design goal: **default to Q&A** (the fast path). Only classify as a
/// coding task if the prompt contains strong signals that the user wants
/// the agent to actually work on the codebase — not merely talk about it.
///
/// Why: a user running `hyper` and typing "what is a closure?" should get
/// an instant answer, not a 30-second project tree walk. But typing
/// "refactor the UserService to use async/await" should still trigger
/// the full index + Orchestrator pipeline.
///
/// This is intentionally simple and conservative. If it's ambiguous,
/// treat it as Q&A — the user can prefix with `/code` to force the
/// code path, or `/reindex` to pre-warm the cache.
fn looks_like_coding_task(prompt: &str) -> bool {
    let p = prompt.trim();
    if p.is_empty() {
        return false;
    }
    let lower = p.to_lowercase();

    // Strong signal 1: code blocks (fenced ``` or inline backticks with
    // a function call / path / namespace inside).
    if p.contains("```") {
        return true;
    }
    if p.contains('`') && (p.contains('(') || p.contains("::") || p.contains('/')) {
        return true;
    }

    // Strong signal 2: explicit file path references to source files.
    const CODE_EXTS: &[&str] = &[
        ".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs",
        ".go", ".java", ".c", ".cc", ".cpp", ".cxx", ".h", ".hpp",
        ".cs", ".rb", ".php", ".swift", ".kt", ".kts", ".scala", ".rs",
        ".sh", ".bash", ".zsh", ".ps1",
        ".toml", ".yaml", ".yml", ".json", ".xml", ".html", ".css",
        ".scss", ".less", ".sql", ".lua", ".r", ".dart", ".vue", ".svelte",
        ".lock", ".proto", ".graphql", ".gql",
    ];
    for ext in CODE_EXTS {
        if lower.contains(ext) {
            return true;
        }
    }
    if lower.contains("cargo.toml") || lower.contains("package.json") || lower.contains("pyproject.toml") {
        return true;
    }

    // Strong signal 3: imperative verbs that mean "do something to the code".
    // Match as a word-starting prefix to avoid false positives like
    // "fixed" appearing in normal text.
    let coding_verbs: &[&str] = &[
        // Chinese
        "实现", "写一个", "写个", "写一段", "加上", "添加", "新增",
        "修改", "改成", "改为", "改为", "改成", "改成", "改成",
        "删除", "移除", "去掉", "删掉",
        "重构", "重写", "优化", "调整", "改造", "改成", "改写",
        "修复", "修一下", "修这个", "修好", "解决", "排查",
        "创建", "新建", "建一个", "建个",
        "导入", "引用", "引入", "封装", "抽象", "提取",
        "补全", "补上", "加上", "写完",
        "把 ", "将 ", "把代码", "把函数", "把这段",
        // English
        "implement", "refactor", "rewrite", "optimize", "fix",
        "add a", "add the", "add this", "add an",
        "remove the", "remove this", "remove a", "remove an",
        "create a", "create the", "create an",
        "update the", "update this", "update a",
        "patch the", "patch this",
        "make it", "make the", "turn it", "turn the",
        "rename", "extract", "inline", "wrap",
        "migrate", "port", "convert to", "upgrade",
        "write a", "write the", "write this", "write an",
        "edit the", "edit this", "edit a",
        "modify the", "modify this",
    ];
    // For each verb, check if it appears as a word-starting token
    // (preceded by start-of-string, whitespace, or punctuation).
    for verb in coding_verbs {
        if let Some(idx) = lower.find(verb) {
            let before_ok = idx == 0
                || !lower.as_bytes()[idx - 1].is_ascii_alphanumeric();
            let after_ok = idx + verb.len() >= lower.len()
                || !lower.as_bytes()[idx + verb.len()].is_ascii_alphanumeric()
                || lower.as_bytes()[idx + verb.len()] == b' ';
            if before_ok && after_ok {
                return true;
            }
        }
    }

    // Strong signal 4: dev commands / build tools mentioned as actions.
    let dev_cmds: &[&str] = &[
        "cargo build", "cargo test", "cargo run", "cargo check",
        "cargo install", "cargo add", "cargo fmt", "cargo clippy",
        "npm install", "npm run", "npm test", "npm i ", "pnpm install", "pnpm add",
        "yarn add", "yarn install", "yarn run",
        "go build", "go test", "go run", "go mod",
        "make ", "cmake ", "docker ", "kubectl ",
        "git commit", "git push", "git merge", "git rebase", "git checkout",
        "pytest", "jest ", "mocha", "rspec", "xcodebuild",
        "pip install", "pip3 install",
    ];
    for cmd in dev_cmds {
        if lower.contains(cmd) {
            return true;
        }
    }

    // Strong signal 5: "in <file>" / "在 <file> 中" patterns.
    let in_file_patterns: &[&str] = &[
        " in src/", " in tests/", " in lib/", " in app/",
        " in the file", " in the code", " in the repo", " in this file",
        " in this repo", " in the project",
        "在 src", "在 .", "在文件", "在代码", "在项目",
        "在 main.", "在 lib.", "在 index.", "在 app.",
    ];
    for pat in in_file_patterns {
        if lower.contains(pat) {
            return true;
        }
    }

    // Strong signal 6: code-construction keywords (function/class names in
    // dev context). Heavier weight than plain mentions.
    let dev_keywords: &[&str] = &[
        " fn ", " struct ", " enum ", " trait ", " impl ",
        " class ", " def ", " function ", " method ",
        " const ", " let ", " var ", " pub fn",
        " interface ", " type ", " module ",
    ];
    for kw in dev_keywords {
        if lower.contains(kw) {
            return true;
        }
    }

    false
}

/// Execute a single prompt through the agent orchestrator
/// Reuses the persisted index and memory store across turns.
///
/// Routing is **prompt-driven, not mode-driven**. The classifier
/// (`looks_like_coding_task`) decides whether to:
///   * Skip the index entirely and call the LLM directly — for any
///     prompt that's a question, explanation, or chat ("what is X?",
///     "explain Y", "how does Z work").
///   * Build/load the project index, create an Orchestrator, and run
///     the full plan/code/review/apply pipeline — for any prompt that
///     contains code, file references, or imperative verbs targeting
///     the codebase ("implement X", "refactor Y", "fix the bug in Z").
///
/// The user doesn't have to switch `/mode` for this to work — it's
/// automatic per-prompt. Mode still controls the system-prompt tone
/// in the chat path.
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
    force_code: bool,
) -> Option<String> {
    let start = Instant::now();

    // ── Classify the prompt ────────────────────────────────────
    // Default to Q&A. Only treat as a coding task if the prompt
    // contains strong signals (code blocks, file paths, imperative
    // verbs targeting code, dev commands, etc.). The user is free
    // to type "what is a closure?" or "explain Rust's borrow checker"
    // — those never need the project index.
    //
    // `force_code` (set by the /code command) bypasses the classifier
    // and goes straight to the code path.
    let is_coding = force_code || looks_like_coding_task(prompt);

    // ── Fast path: question / chat / general knowledge ─────────
    if !is_coding {
        return run_passthrough_chat(
            prompt, dir, mode, &provider, conversation_history, start,
        ).await;
    }

    if force_code {
        println!("  🧠 [code path forced by /code]");
    } else {
        println!("  🧠 [coding task detected — loading project index]");
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
            prompt, dir, mode, &provider, conversation_history, start,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── looks_like_coding_task ─────────────────────────────────

    #[test]
    fn test_looks_like_coding_task_empty() {
        assert!(!looks_like_coding_task(""));
        assert!(!looks_like_coding_task("   "));
    }

    #[test]
    fn test_looks_like_coding_task_code_fence() {
        assert!(looks_like_coding_task("Here's some code:\n```rust\nfn x() {}\n```"));
        assert!(looks_like_coding_task("```python\nprint('hi')\n```"));
    }

    #[test]
    fn test_looks_like_coding_task_inline_backticks() {
        assert!(looks_like_coding_task("Call `foo()` with `bar::baz`"));
        assert!(looks_like_coding_task("Use `path/to/file.rs`"));
    }

    #[test]
    fn test_looks_like_coding_task_file_extensions() {
        assert!(looks_like_coding_task("Look at main.rs"));
        assert!(looks_like_coding_task("Update index.tsx"));
        assert!(looks_like_coding_task("Edit config.toml"));
        assert!(looks_like_coding_task("Check package.json"));
    }

    #[test]
    fn test_looks_like_coding_task_english_verbs() {
        assert!(looks_like_coding_task("implement the function"));
        assert!(looks_like_coding_task("refactor the code"));
        assert!(looks_like_coding_task("fix the bug"));
        assert!(looks_like_coding_task("add a new feature"));
        assert!(looks_like_coding_task("remove the old method"));
        assert!(looks_like_coding_task("rename the variable"));
        assert!(looks_like_coding_task("extract the helper"));
    }

    #[test]
    fn test_looks_like_coding_task_chinese_verbs() {
        assert!(looks_like_coding_task("实现一个新的函数"));
        assert!(looks_like_coding_task("写一个测试"));
        assert!(looks_like_coding_task("修改这段代码"));
        assert!(looks_like_coding_task("删除旧的文件"));
        assert!(looks_like_coding_task("重构一下"));
        assert!(looks_like_coding_task("修复这个bug"));
    }

    #[test]
    fn test_looks_like_coding_task_dev_commands() {
        assert!(looks_like_coding_task("run cargo build"));
        assert!(looks_like_coding_task("execute cargo test"));
        assert!(looks_like_coding_task("run npm install"));
        assert!(looks_like_coding_task("try pytest"));
    }

    #[test]
    fn test_looks_like_coding_task_in_file_pattern() {
        assert!(looks_like_coding_task("change the function in src/main.rs"));
        assert!(looks_like_coding_task("edit the code in tests/foo.rs"));
    }

    #[test]
    fn test_looks_like_coding_task_code_keywords() {
        assert!(looks_like_coding_task("add a fn new_helper"));
        assert!(looks_like_coding_task("create struct User"));
        assert!(looks_like_coding_task("implement trait Display"));
    }

    #[test]
    fn test_looks_like_coding_task_plain_question() {
        assert!(!looks_like_coding_task("what is a closure"));
        assert!(!looks_like_coding_task("explain Rust's borrow checker"));
        assert!(!looks_like_coding_task("tell me a joke"));
        assert!(!looks_like_coding_task("hello"));
    }

    #[test]
    fn test_looks_like_coding_task_false_positive_avoidance() {
        // "fixed" alone should not be detected as a coding task
        // (since it's in the past tense without imperative form)
        // The function uses word-boundary detection
        let result = looks_like_coding_task("I fixed the bug yesterday");
        // "fixed" doesn't match "fix" exactly due to word-boundary
        // but "fix" is in coding_verbs - and "fix" doesn't appear in this string
        // but "fix" does match the verb list
        // Actually "fixed" contains "fix" as substring - need to check boundary logic
        // "fix" would match at index 2, before is space (good), after is "e" (alphanumeric, so NOT a word boundary)
        // So this should NOT be a coding task
        assert!(!result, "past-tense 'fixed' should not be detected");
    }

    #[test]
    fn test_looks_like_coding_task_cargo_toml_mention() {
        assert!(looks_like_coding_task("update Cargo.toml"));
        assert!(looks_like_coding_task("edit pyproject.toml"));
    }

    // ── get_memory_count ──────────────────────────────────────

    #[test]
    fn test_get_memory_count_nonexistent_path() {
        let p = Path::new("/nonexistent/path/that/does/not/exist/db.sqlite");
        assert_eq!(get_memory_count(p), Some(0));
    }

    #[test]
    fn test_get_memory_count_with_empty_db() {
        let dir = std::env::temp_dir().join(format!("hyperagent_repl_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("mem.sqlite");
        // Create an empty SQLite db
        let _ = rusqlite::Connection::open(&db_path).unwrap();
        // An empty db without memories table returns 0 from the COUNT query
        // Actually COUNT(*) on a non-existent table would fail and unwrap_or(0) would kick in
        let count = get_memory_count(&db_path);
        assert_eq!(count, Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_get_memory_count_with_memories_table() {
        let dir = std::env::temp_dir().join(format!("hyperagent_repl_mem_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("mem.sqlite");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE memories (id INTEGER PRIMARY KEY, content TEXT); INSERT INTO memories (content) VALUES ('a'), ('b'), ('c');").unwrap();
        drop(conn);
        let count = get_memory_count(&db_path);
        assert_eq!(count, Some(3));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
