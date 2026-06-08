use crate::cli::*;

impl Cli {
    pub(crate) async fn run_agent(
        &self,
        prompt: &str,
        dir: &Path,
        agents: usize,
        model: Option<String>,
        base_url: Option<String>,
        api_key: Option<String>,
        yes: bool,
        reindex: bool,
        mode: &str,
        _session_id: &Option<String>,
        image: Option<PathBuf>,
    ) -> Result<()> {
        // Check for AGENTS.md project context
        let agents_md = dir.join("AGENTS.md");
        let project_context = if agents_md.exists() {
            match std::fs::read_to_string(&agents_md) {
                Ok(content) => {
                    println!("📄 Loaded AGENTS.md project context");
                    Some(content)
                }
                Err(_) => None,
            }
        } else {
            None
        };

        // Build or load index
        println!("📚 Indexing codebase...");
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

        let augmented_prompt = match &project_context {
            Some(ctx) => {
                orchestrator = orchestrator.with_project_context(ctx.clone());
                format!("{prompt_with_image}\n\n--- Project Context (AGENTS.md) ---\n{ctx}")
            }
            None => prompt_with_image,
        };

        if let Some(mem) = memory {
            orchestrator = orchestrator.with_memory(mem);
        }
        if let Some(h) = hooks {
            orchestrator = orchestrator.with_hooks(h);
        }

        // Build the knowledge base (auto-create if doesn't exist)
        let knowledge_mgr = {
            let kb_db = dir.join(".hyper").join("knowledge_mem.db");
            use crate::memory::{MemoryManager, SqliteMemoryStore};
            let store = SqliteMemoryStore::new(&kb_db).unwrap_or_else(|_| {
                SqliteMemoryStore::new(&kb_db).expect("failed to open knowledge db")
            });
            MemoryManager::new(Box::new(store), "knowledge").with_container("_knowledge")
        };
        let kb = crate::knowledge::KnowledgeBase::new(dir, knowledge_mgr);
        if kb.build().is_ok() {
            orchestrator = orchestrator.with_knowledge_base(kb);
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
        let result = orchestrator.run(&augmented_prompt).await?;

        // Save session automatically
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

        // Print complete results
        println!();
        println!("─── Summary ────────────────────────────────────────");
        println!("  Files modified: {}", result.files_modified);
        println!("  Tokens:         ~{}", result.tokens_used);
        println!("  Wall time:      {:.1}s", result.elapsed.as_secs_f64());
        println!("  Memories saved: {}", result.memories_recorded);
        println!("  Model:          {}", result.model_name);
        println!("  Mode:           {mode}");
        println!("────────────────────────────────────────────────────");

        // Cost tracking
        let cost_per_1k = 0.15; // ~$0.15/M tokens for deepseek-v4-flash
        let cost = (result.tokens_used as f64 / 1000.0) * cost_per_1k;
        println!("  Cost:           ~${:.4}", cost);

        Ok(())
    }

    pub(crate) async fn review_diff(&self, against: &str, dir: &PathBuf, _model: Option<&str>) -> Result<()> {
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

    pub(crate) async fn analyze_diff(&self, diff: &str, _dir: &PathBuf) -> Result<()> {
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

    pub(crate) async fn build_index(&self, dir: &Path, force: bool) -> Result<()> {
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

    pub(crate) async fn show_stats(&self, dir: &Path) -> Result<()> {
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

    pub(crate) async fn handle_mode(&self, action: &ModeAction) -> Result<()> {
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

    pub(crate) async fn handle_memory(&self, action: &MemoryAction) -> Result<()> {
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
            MemoryAction::Prune {
                below,
                older_than_days,
                min_importance,
                container,
            } => {
                // Re-build the manager with the requested container tag
                // (the default CLI manager is bound to "_default").
                let scoped = if container != "_default" {
                    crate::memory::MemoryManager::new(
                        // Cheap trick: re-use the same on-disk store via a fresh
                        // SqliteMemoryStore. The handle above is consumed by
                        // the original manager; this opens the same path.
                        Box::new(crate::memory::SqliteMemoryStore::new(std::path::Path::new(
                            "~/.hyper/memory.db",
                        ))?),
                        "hyper",
                    )
                    .with_container(container)
                } else {
                    manager
                };
                let deleted = if let Some(days) = *older_than_days {
                    scoped.forget_older_than(days, *min_importance)?
                } else {
                    scoped.forget_below(*below)?
                };
                println!(
                    "🧹 Pruned {deleted} memories (container={container}, threshold={})",
                    older_than_days
                        .map(|d| format!("age>{d}d & imp<{min_importance}"))
                        .unwrap_or_else(|| format!("score<{below}"))
                );
            }
        }
        Ok(())
    }

    pub(crate) async fn handle_graph(&self, action: &GraphAction) -> Result<()> {
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

    pub(crate) async fn handle_mcp(&self, action: &McpAction) -> Result<()> {
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
            McpAction::Serve { container, db } => {
                return crate::mcp::serve_stdio(container, db.as_deref()).await;
            }
        }
        Ok(())
    }

    pub(crate) async fn handle_kanban(&self, action: &KanbanAction) -> Result<()> {
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

    pub(crate) async fn handle_hooks(&self, action: &HooksAction) -> Result<()> {
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

    pub(crate) async fn handle_session(&self, action: &SessionAction) -> Result<()> {
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
                        SessionAction::Share { id } => {
                let share_store = crate::session::ShareStore::new();
                let token = share_store.share(&id);
                let session_path = format!("{}/hyper/sessions/{}.json",
                    dirs_next::data_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).display(), id);
                println!("   🔗 Session shared!");
                println!("   Token: {}", token);
                println!("   Web:   http://127.0.0.1:3000/api/share/{}", token);
                println!("   CLI:   hyper session join {}", token);
                println!("   File:  {}", session_path);
                Ok(())
            }
            SessionAction::Join { token } => {
                let share_store = crate::session::ShareStore::new();
                match share_store.resolve(&token) {
                    Some(session_id) => {
                        let sessions = crate::session::SessionManager::new()?;
                        match sessions.load(&session_id) {
                            Ok(session) => {
                                session.display();
                                println!();
                                println!("   💡 Run `hyper session fork {}` to continue this session", session_id);
                            }
                            Err(e) => eprintln!("   ❌ Session not found: {e}"),
                        }
                        Ok(())
                    }
                    None => {
                        eprintln!("   ❌ Invalid or expired share token: {}", token);
                        Ok(())
                    }
                }
            }
            SessionAction::Export {SessionAction::Export { output } => {
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

    pub(crate) async fn run_doctor(&self) -> Result<()> {
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

    pub(crate) fn show_config(&self, verbose: bool) -> Result<()> {
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

    pub(crate) async fn web_search(&self, query: String, max: usize) -> Result<()> {
        println!("🔍 Searching for: {query}");
        let results = crate::web_search::search(&query, max).await?;
        crate::web_search::display_results(&results);
        Ok(())
    }

    pub(crate) async fn show_diff(&self, against: &str, staged: bool, dir: &Path) -> Result<()> {
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
    pub(crate) async fn show_diff_side_by_side(&self, against: &str, staged: bool, dir: &Path) -> Result<()> {
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

    pub(crate) async fn deploy_project(&self, tag: &str, dir: &PathBuf) -> Result<()> {
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

    pub(crate) async fn commit_changes(&self, message: Option<&str>, dir: &PathBuf) -> Result<()> {
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

    pub(crate) async fn list_agents(&self, name: &Option<String>, message: &[String], _dir: &PathBuf) -> Result<()> {
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

    pub(crate) async fn handle_team(&self, action: &TeamAction) -> Result<()> {
        match action {
            TeamAction::Init { name, email } => {
                crate::team::TeamConfig::init(name, email)?;
            }
            TeamAction::Members => {
                if let Some(team) = crate::team::TeamConfig::load()? {
                    team.print_members();
                } else {
                    println!("  No team initialized. Use 'hyper team init <name> --email <email>'");
                }
            }
            TeamAction::Invite { email, role } => {
                let mut team = crate::team::TeamConfig::load()?
                    .ok_or_else(|| anyhow::anyhow!("No team. Run 'hyper team init' first"))?;
                let role = match role.as_str() {
                    "admin" => crate::team::MemberRole::Admin,
                    "member" => crate::team::MemberRole::Member,
                    "viewer" => crate::team::MemberRole::Viewer,
                    _ => anyhow::bail!("Unknown role: {}. Use: admin, member, viewer", role),
                };
                team.add_member(email, role)?;
            }
            TeamAction::Remove { email } => {
                let mut team = crate::team::TeamConfig::load()?
                    .ok_or_else(|| anyhow::anyhow!("No team. Run 'hyper team init' first"))?;
                team.remove_member(email)?;
            }
        }
        Ok(())
    }

    pub(crate) async fn handle_sync(&self, action: &SyncAction) -> Result<()> {
        let config = crate::sync::SyncConfig::load()?;
        match action {
            SyncAction::Push => {
                println!("☁️  Pushing to cloud...");
                let report = crate::sync::push(&config).await?;
                report.print();
            }
            SyncAction::Pull => {
                println!("☁️  Pulling from cloud...");
                let report = crate::sync::pull(&config).await?;
                report.print();
            }
            SyncAction::Status => {
                println!();
                println!("  ☁️  Sync Status");
                println!("  {}", "─".repeat(40));
                println!("  Endpoint:    {}", config.endpoint);
                println!("  Configured:  {}", if config.is_configured() { "✅" } else { "❌" });
                println!("  Auto-sync:   {}", if config.auto_sync { "✅" } else { "❌" });
                println!();
                println!("  Syncing: memories={} skills={} config={} rules={}",
                    if config.sync_memories { "✅" } else { "❌" },
                    if config.sync_skills { "✅" } else { "❌" },
                    if config.sync_config { "✅" } else { "❌" },
                    if config.sync_rules { "✅" } else { "❌" },
                );
                if !config.is_configured() {
                    println!();
                    println!("  To configure: export HYPER_SYNC_KEY=<your-key>");
                }
                println!();
            }
        }
        Ok(())
    }

    pub(crate) async fn handle_billing(&self, action: &BillingAction) -> Result<()> {
        match action {
            BillingAction::Status => {
                let billing = crate::billing::BillingState::load_or_create()?;
                billing.print_status();
            }
            BillingAction::Upgrade { tier } => {
                let target_tier = match tier.to_lowercase().as_str() {
                    "pro" => crate::billing::Tier::Pro,
                    "team" => crate::billing::Tier::Team,
                    "enterprise" => crate::billing::Tier::Enterprise,
                    _ => {
                        eprintln!("Unknown tier: {}. Available: pro, team, enterprise", tier);
                        return Ok(());
                    }
                };
                let url = crate::billing::stripe_checkout_url(
                    target_tier,
                    "https://hyperagent.dev/success",
                    "https://hyperagent.dev/cancel",
                )?;
                println!();
                println!("  \x1b[1;33m💳 Upgrade to {}\x1b[0m", target_tier.name());
                println!();
                println!("  Open this URL to complete checkout:");
                println!("  \x1b[36m{}\x1b[0m", url);
                println!();
                println!("  In production, this opens your browser automatically.");
                if cfg!(target_os = "macos") {
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                }
            }
            BillingAction::License { key } => {
                let secret = std::env::var("HYPER_BILLING_SECRET")
                    .unwrap_or_else(|_| "hyperagent-default-secret".to_string());
                match crate::billing::parse_license(key, &secret) {
                    Ok(payload) => {
                        let mut billing = crate::billing::BillingState::load_or_create()?;
                        billing.activate_license(&payload)?;
                        println!();
                        println!("  \x1b[1;32m✅ License activated!\x1b[0m");
                        println!("  Plan: \x1b[1;33m{}\x1b[0m", payload.tier().name());
                        if let Some(days) = payload.days_remaining() {
                            println!("  Days remaining: {}", days);
                        } else {
                            println!("  Expiry: Never (perpetual license)");
                        }
                        println!();
                    }
                    Err(e) => {
                        eprintln!("\n  \x1b[1;31m❌ Invalid license: {}\x1b[0m\n", e);
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn handle_knowledge(&self, action: &str, query: &Option<Vec<String>>, dir: &Path) -> Result<()> {
        let knowledge_mgr = {
            let kb_db = dir.join(".hyper").join("knowledge_mem.db");
            use crate::memory::{MemoryManager, SqliteMemoryStore};
            let store = SqliteMemoryStore::new(&kb_db).unwrap_or_else(|_| {
                SqliteMemoryStore::new(&kb_db).expect("failed to open knowledge db")
            });
            MemoryManager::new(Box::new(store), "knowledge").with_container("_knowledge")
        };
        let kb = crate::knowledge::KnowledgeBase::new(dir, knowledge_mgr);
        match action {
            "build" => {
                println!("📚 Building knowledge base...");
                let count = kb.build()?;
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

    pub(crate) async fn generate_tests(&self, file: &PathBuf, function: Option<&str>, _dir: &PathBuf) -> Result<()> {
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
        use std::io::{stdin, Write};
        std::io::stdout().flush().ok();
        let mut input = String::new();
        stdin().read_line(&mut input).ok();
        if input.trim().to_lowercase() != "n" {
            crate::test_gen::append_tests_to_file(file, &tests)?;
            println!("   ✅ Tests appended to {}", file.display());
        }

        Ok(())
    }

    pub(crate) async fn handle_bench(&self, action: &BenchAction) -> Result<()> {
        match action {
            BenchAction::Memory {
                memories,
                chunks,
                queries,
                json,
            } => {
                let cfg = crate::bench::BenchConfig {
                    memories: *memories,
                    chunks: *chunks,
                    queries: *queries,
                    top_k: 10,
                };
                let report = crate::bench::run(&cfg)?;
                if *json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    crate::bench::print_report(&report);
                }
            }
        }
        Ok(())
    }
}
