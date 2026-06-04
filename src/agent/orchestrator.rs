//! HyperAgent Orchestrator — the engine that makes everything work end-to-end
//!
//! Full pipeline:
//! 1. Load memory context from past sessions
//! 2. Get relevant files from PageRank index
//! 3. PlanAgent: decompose task into parallel-safe steps
//! 4. CodeAgents (N-way parallel): execute steps concurrently
//! 5. MemoryAgent: auto-record key decisions and findings
//! 6. ReviewAgent: review all changes
//! 7. ApplyAgent: apply approved changes
//! 8. Record learnings to memory
//! 9. Fire hooks at each phase

use anyhow::Result;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::diff::FileChange;
use crate::hooks::{HookEvent, HookRegistry};
use crate::index::{FileContext, HyperIndex};
use crate::llm::{LlmProvider, Message, ProviderPool};
use crate::memory::{MemoryManager, MemoryType};

use super::apply_agent::ApplyAgent;
use super::review_agent::ReviewAgent;

/// Result of an orchestrator run
#[derive(Debug, Default)]
pub struct RunResult {
    pub files_modified: usize,
    pub tokens_used: usize,
    pub elapsed: Duration,
    #[allow(dead_code)]
    pub changes: Vec<FileChange>,
    pub model_name: String,
    pub memories_recorded: usize,
    pub response_text: String,  // The LLM's text response (for conversation history)
    #[allow(dead_code)]
    pub cost_estimate: f64,     // Estimated USD cost
}

/// The main orchestrator
pub struct Orchestrator {
    index: HyperIndex,
    root: PathBuf,
    provider: LlmProvider,
    provider_pool: Option<ProviderPool>,
    plan_provider: Option<LlmProvider>,
    review_provider: Option<LlmProvider>,
    parallel_agents: usize,
    confirm: bool,
    mode: String,
    project_context: Option<String>,
    conversation_history: Vec<(String, String)>,
    memory: Option<MemoryManager>,
    hooks: Option<HookRegistry>,
    mcp: Option<crate::mcp::McpRegistry>,
    mode_registry: Option<crate::modes::ModeRegistry>,
    budget_tracker: Option<crate::budget_tracker::BudgetTracker>,
    file_cache: crate::file_cache::FileContextCache,
    use_worktree: bool,
}

impl Orchestrator {
    pub fn new(
        index: HyperIndex,
        provider: LlmProvider,
        root: PathBuf,
        parallel_agents: usize,
        confirm: bool,
    ) -> Self {
        Self {
            index,
            provider,
            provider_pool: None,
            root,
            parallel_agents,
            confirm,
            mode: "code".into(),
            project_context: None,
            conversation_history: Vec::new(),
            memory: None,
            hooks: None,
            mcp: None,
            mode_registry: None,
            plan_provider: None,
            review_provider: None,
            budget_tracker: None,
            file_cache: crate::file_cache::FileContextCache::default(),
            use_worktree: false,
        }
    }

    pub fn with_worktree(mut self) -> Self {
        self.use_worktree = true;
        self
    }

    pub fn with_conversation_history(mut self, history: Vec<(String, String)>) -> Self {
        self.conversation_history = history;
        self
    }

    pub fn with_mode(mut self, mode: &str) -> Self {
        self.mode = mode.to_string();
        self
    }

    pub fn with_project_context(mut self, ctx: String) -> Self {
        self.project_context = Some(ctx);
        self
    }

    pub fn with_memory(mut self, memory: MemoryManager) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_hooks(mut self, hooks: HookRegistry) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn with_mcp(mut self, mcp: crate::mcp::McpRegistry) -> Self {
        self.mcp = Some(mcp);
        self
    }

    pub fn with_mode_registry(mut self, registry: crate::modes::ModeRegistry) -> Self {
        self.mode_registry = Some(registry);
        self
    }

    /// Configure a failover provider pool
    pub fn with_provider_pool(mut self, pool: ProviderPool) -> Self {
        self.provider_pool = Some(pool);
        self
    }

    /// Chat with automatic failover (pool → individual provider)
    async fn chat_with_failover(&mut self, messages: Vec<Message>) -> Result<String> {
        if let Some(pool) = &mut self.provider_pool {
            pool.chat(messages).await
        } else {
            self.provider.chat(messages).await
        }
    }

    /// Chat stream with automatic failover
    async fn chat_stream_with_failover(&mut self, messages: Vec<Message>) -> Result<crate::llm::streaming::StreamingResponse> {
        if let Some(pool) = &mut self.provider_pool {
            pool.chat_stream(messages).await
        } else {
            self.provider.chat_stream(messages).await
        }
    }

    /// Get active model name (from pool or direct provider)
    fn active_model_name(&self) -> String {
        self.provider_pool.as_ref()
            .map(|p| p.active_model().to_string())
            .unwrap_or_else(|| self.provider.model.clone())
    }

    /// Get active provider name (for display)
    fn active_provider_display(&self) -> String {
        self.provider_pool.as_ref()
            .map(|p| format!("{} ({})", p.active_provider_name(), p.active_model()))
            .unwrap_or_else(|| self.provider.model.clone())
    }

    #[allow(dead_code)]
    pub fn with_plan_provider(mut self, provider: LlmProvider) -> Self {
        self.plan_provider = Some(provider);
        self
    }

    #[allow(dead_code)]
    pub fn with_review_provider(mut self, provider: LlmProvider) -> Self {
        self.review_provider = Some(provider);
        self
    }

    pub async fn run(&mut self, prompt: &str) -> Result<RunResult> {
        let start = Instant::now();
        let mut total_memories = 0usize;

        // Phase 0: Fire pre-run hooks
        self.fire_hook(HookEvent::PreRun, prompt).await;

        // Phase 0: Load memory context from past sessions
        let mem_context = self.load_memory_context(prompt).await;

        // Build the augmented prompt (project context + memory + mode + MCP tools)
        let augmented_prompt = self.build_augmented_prompt(prompt, &mem_context).await;

        println!("\n🚀 HyperAgent — Processing: {}", prompt);
        if !mem_context.is_empty() {
            println!("   🧠 Memory context: {} past learnings loaded",
                mem_context.matches('\n').count());
        }
        println!();
        // Phase 0: Scan codebase with PageRank
        println!("🔍 Scanning codebase with PageRank...");

        let mut relevant_files = self.index.get_relevant_files(prompt, 15, 4000);

        // Apply LRU file cache — summarize large files, evict old entries
        let max_full_tokens = (MAX_INPUT_TOKENS as f64 * 0.6) as usize; // 60% of budget for files
        let cached: Vec<crate::index::FileContext> = relevant_files
            .iter()
            .map(|f| self.file_cache.get_or_load(&f.path, f.score, max_full_tokens / relevant_files.len().max(1)))
            .collect();
        relevant_files = cached;

        println!("   Found {} relevant files (cache: {:.1}K tokens)",
            relevant_files.len(),
            self.file_cache.current_tokens() as f64 / 1000.0,
        );

        // Record file discovery to memory
        let file_paths: Vec<String> = relevant_files.iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();
        self.record_memory(
            &format!("Relevant files for '{}': {}", prompt, file_paths.join(", ")),
            MemoryType::CodebaseFact,
        ).await;
        total_memories += 1;

        // Phase 2: Planning
        self.fire_hook(HookEvent::PrePlan, prompt).await;
        print!("📋 Planning... ");
        std::io::Write::flush(&mut std::io::stdout()).ok();

        // Plan with retry (up to 2 attempts)
        let plan = self.create_plan_with_retry(&augmented_prompt, &relevant_files, 2).await?;

        // Intelligent intent detection: empty steps = Q&A, non-empty = action
        let has_actions = plan.steps.as_ref().map(|s| !s.is_empty()).unwrap_or(false);

        // Force action pipeline for command-style prompts even if plan agent returned empty steps
        let force_action = !has_actions && {
            let lower = prompt.to_lowercase();
            lower.starts_with("fix ") || lower.starts_with("add ") || lower.starts_with("implement ")
                || lower.starts_with("create ") || lower.starts_with("update ") || lower.starts_with("refactor ")
                || lower.starts_with("change ") || lower.starts_with("modify ") || lower.starts_with("remove ")
                || lower.starts_with("delete ") || lower.starts_with("optimize ") || lower.starts_with("rewrite ")
                || lower.starts_with("make ") || lower.starts_with("write ")
        };

        // Use prompt as plan summary when plan agent returned empty
        let plan = if force_action && plan.summary.is_empty() {
            crate::agent::plan_agent::Plan {
                summary: prompt.to_string(),
                steps: Some(vec![prompt.to_string()]),
                reasoning: plan.reasoning,
            }
        } else {
            plan
        };

        // Ensure steps exist for action requests
        let plan = if has_actions || force_action {
            let steps = plan.steps.clone().unwrap_or_else(|| {
                vec![format!("{}: {}", plan.summary, prompt)]
            });
            let steps = if steps.is_empty() {
                vec![prompt.to_string()]
            } else {
                steps
            };
            crate::agent::plan_agent::Plan {
                summary: plan.summary,
                steps: Some(steps),
                reasoning: plan.reasoning,
            }
        } else {
            plan
        };

        let has_actions = has_actions || force_action;

        if has_actions {
            // Show the plan for action requests
            println!("   Plan: {}", plan.summary);
            if let Some(ref steps) = plan.steps {
                for (i, step) in steps.iter().enumerate() {
                    println!("   {}. {}", i + 1, step);
                }
            }
            println!();
        }

        // Record plan to memory
        let steps_count = plan.steps.as_ref().map(|s| s.len()).unwrap_or(0);
        self.record_memory(
            &format!("Plan for '{}': {} — {} steps", prompt, plan.summary, steps_count),
            MemoryType::Decision,
        ).await;
        total_memories += 1;

        self.fire_hook(HookEvent::PostPlan, &plan.summary).await;

        if !has_actions {
            // Q&A mode: use run_ask_mode (with MCP tools) for direct answering
            println!(); // Finish "📋 Planning... " line
            let ask_result = self.run_ask_mode(prompt, &relevant_files, start, total_memories).await?;

            self.fire_hook(HookEvent::PostRun, &ask_result.response_text).await;
            return Ok(ask_result);
        }

        // Phase 2b: Apply context budget to fit token window
        let mut budget = ContextBudget::new(&augmented_prompt, &self.conversation_history, self.project_context.as_deref());
        budget.truncate_files(&mut relevant_files, 15);
        println!("   {}", budget.summary());

        // Phase 3: Parallel code execution
        self.fire_hook(HookEvent::PreCode, prompt).await;
        println!("👨‍💻 Executing ({} parallel agents)...", self.parallel_agents);

        // Execute with retry (up to 2 attempts if no changes)
        let all_changes = self.execute_with_retry(
            &augmented_prompt, &plan, &relevant_files, prompt, start, total_memories,
        ).await?;

        println!("   📦 Total: {} file changes from up to 2 attempts\n",
            all_changes.len());

        if all_changes.is_empty() {
            println!("⚠️  No changes generated. Code may already satisfy the task.");
            self.record_memory(
                &format!("No changes needed for '{}' — already satisfied", prompt),
                MemoryType::ActionOutcome,
            ).await;
            total_memories += 1;
            self.fire_hook(HookEvent::PostRun, "no changes").await;
            return Ok(RunResult {
                elapsed: start.elapsed(),
                memories_recorded: total_memories,
                ..Default::default()
            });
        }

        self.fire_hook(HookEvent::PostCode, &format!("{} changes", all_changes.len())).await;

        // Phase 4: Review
        self.fire_hook(HookEvent::PreReview, prompt).await;
        println!("🔎 Reviewing changes...");
        let review_provider = self.review_provider.as_ref().unwrap_or(&self.provider);
        let review_agent = ReviewAgent::new(review_provider);
        let approved: Vec<FileChange> = if self.confirm {
            review_agent.review(prompt, &all_changes).await?
        } else {
            all_changes.clone()
        };

        if approved.is_empty() {
            println!("⚠️  All changes rejected. Recording findings for future attempts.");
            self.record_memory(
                &format!("Changes rejected for '{}' — try different approach", prompt),
                MemoryType::ActionOutcome,
            ).await;
            total_memories += 1;
            self.fire_hook(HookEvent::PostRun, "all rejected").await;
            return Ok(RunResult {
                elapsed: start.elapsed(),
                memories_recorded: total_memories,
                ..Default::default()
            });
        }
        println!("   ✅ Approved {} changes\n", approved.len());
        self.fire_hook(HookEvent::PostReview, &format!("{} approved", approved.len())).await;

        // Phase 4b: Diff preview — show each change before applying
        println!("📋 Change preview:");
        for (i, change) in approved.iter().enumerate() {
            let diff_text = crate::diff_view::file_change_to_diff_text(change);
            let line_count = diff_text.lines().count();
            println!("\n   [{}/{}] {} ({} lines):", i + 1, approved.len(),
                change.file.display(), line_count);
            crate::diff_view::show_diff(&diff_text);
        }
        println!();

        // Phase 5: Apply (optionally in worktree sandbox)
        let (apply_root, mut _wt_manager) = self.prepare_apply_worktree().await?;

        self.fire_hook(HookEvent::PreApply, prompt).await;
        println!("✏️  Applying changes...");
        let apply_agent = ApplyAgent::new(&apply_root, self.confirm);
        let applied = apply_agent.apply(&approved).await?;

        for msg in &applied {
            println!("   {msg}");
        }
        self.fire_hook(HookEvent::PostApply, &applied.join(", ")).await;

        // Phase 5b: Lint-driven fix loop — check if applied changes compile
        // Quick cargo check / tsc check to verify changes compile
        let has_cargo = self.root.join("Cargo.toml").exists();
        let has_ts = self.root.join("tsconfig.json").exists();
        if has_cargo || has_ts {
            let linter = if has_cargo { "cargo check" } else { "tsc --noEmit" };
            let check_cmd = if has_cargo {
                std::process::Command::new("cargo").args(["check"]).current_dir(&self.root).output().ok()
            } else {
                std::process::Command::new("npx").args(["tsc", "--noEmit"]).current_dir(&self.root).output().ok()
            };
            match check_cmd {
                Some(out) if out.status.success() => {
                    println!("   ✅ Lint passed ({})", linter);
                }
                Some(_) => {
                    println!("   ⚠️  Lint check found issues — run `hyper run \"fix compile errors\"` to fix");
                }
                _ => {}
            }
        }

        // Sync worktree changes back to main repo and clean up
        if let Some(ref mut wt) = _wt_manager {
            Self::sync_worktree_and_cleanup(wt, &self.root).await;
        }

        // Record applied changes to memory
        let changed_files: Vec<String> = approved.iter()
            .map(|c| c.file.to_string_lossy().to_string())
            .collect();
        self.record_memory(
            &format!("Applied: modified {} files for '{}': {}",
                changed_files.len(), prompt, changed_files.join(", ")),
            MemoryType::ActionOutcome,
        ).await;

        // If this was a bug fix, record it specially
        let lprompt = prompt.to_lowercase();
        if lprompt.contains("bug") || lprompt.contains("fix") || lprompt.contains("error") {
            self.record_memory(
                &format!("Bug fix: '{}' — modified {}", prompt, changed_files.join(", ")),
                MemoryType::BugFix,
            ).await;
            total_memories += 1;
        }
        total_memories += 1;

        self.fire_hook(HookEvent::OnComplete,
            &format!("{} files modified", changed_files.len())).await;

        let elapsed = start.elapsed();
        let tokens_used = Self::estimate_tokens(prompt, &all_changes);

        // Build and render context dashboard
        let max_ctx = crate::agent::orchestrator::MAX_INPUT_TOKENS;
        let mut dashboard = crate::context_dashboard::ContextDashboard::new(max_ctx);
        dashboard.set_files(budget.full_files, budget.truncated_files, budget.total_files);
        dashboard.set_elapsed(elapsed);
        let input_price = if let Some(ref pool) = self.provider_pool {
            pool.input_price()
        } else {
            self.provider.input_price_per_1m
        };
        let cost = (tokens_used as f64 / 1_000_000.0) * input_price;
        dashboard.set_cost(cost);
        let dashboard_text = dashboard.render();
        for line in dashboard_text.lines() {
            println!("{line}");
        }

        // Auto-prune memory to max 100 entries
        if let Some(ref mem) = self.memory {
            if let Ok(pruned) = mem.prune(100) {
                if pruned > 0 {
                    tracing::debug!("Pruned {pruned} old memories");
                }
            }
            // Consolidate new memories
            if let Ok(uncon) = mem.store().get_unconsolidated(20) {
                if !uncon.is_empty() {
                    let ids: Vec<String> = uncon.iter().map(|e| e.id.clone()).collect();
                    let _ = mem.store().mark_consolidated(&ids);
                }
            }
        }

        Ok(RunResult {
            files_modified: changed_files.len(),
            tokens_used,
            elapsed,
            changes: all_changes,
            model_name: self.active_model_name(),
            memories_recorded: total_memories,
            response_text: String::new(),
            cost_estimate: (tokens_used as f64 / 1_000_000.0) * 0.15f64.max(if let Some(pool) = &self.provider_pool { pool.input_price() } else { self.provider.input_price_per_1m }),
        })
    }

    /// Estimate cost for a run based on token count
    pub fn estimate_cost(tokens: usize, input_price: f64) -> f64 {
        // Rough estimate: assume 60% input tokens, 40% output tokens
        let input_tokens = tokens as f64 * 0.6;
        let output_tokens = tokens as f64 * 0.4;
        let output_price = input_price * 4.0; // Output is typically 4x input price
        (input_tokens / 1_000_000.0 * input_price) + (output_tokens / 1_000_000.0 * output_price)
    }

    /// Check if the estimated cost would exceed the budget
    pub fn would_exceed_budget(tokens: usize, input_price: f64, max_budget: f64) -> bool {
        if max_budget <= 0.0 {
            return false; // No budget limit
        }
        Self::estimate_cost(tokens, input_price) > max_budget
    }

    /// Run in ASK mode: direct Q&A with code context, no plan/code/review pipeline
    /// Supports MCP tool calling: if MCP tools are available, the LLM can call them
    async fn run_ask_mode(
        &mut self,
        prompt: &str,
        relevant_files: &[FileContext],
        start: Instant,
        total_memories: usize,
    ) -> Result<RunResult> {
        println!("   🤔 Answering question...");

        let file_context = self.build_ask_file_context(relevant_files);
        let mem_context = self.load_memory_context(prompt).await;

        let mut system_prompt = format!(
            "You are HyperAgent's ASK mode — a helpful coding assistant.\n\
            Answer the user's question about the codebase concisely and accurately.\n\n\
            Relevant files from the project:\n{}\n\n\
            Past context about this project:\n{}\n\n\
            Rules:\n\
            - Be concise but complete\n\
            - Reference specific file paths and function names when relevant\n\
            - If you find a bug or improvement, fix it using available tools\n\
            - Format code blocks with ```language\n\
            - Answer in the same language as the question",
            file_context,
            if mem_context.is_empty() { "None".to_string() } else { mem_context }
        );

        // Add MCP tools context if available — use native OpenAI function calling
        let mcp_tool_defs: Vec<crate::llm::provider::ToolDefinition> = if let Some(ref mcp) = self.mcp {
            let defs = mcp.to_tool_definitions().await;
            if !defs.is_empty() {
                system_prompt.push_str("\n\nYou have access to MCP tools listed below. Use them when needed to gather information or perform actions.");
                for def in &defs {
                    let desc = if def.function.description.len() > 80 {
                        format!("{}...", &def.function.description[..77])
                    } else {
                        def.function.description.clone()
                    };
                    system_prompt.push_str(&format!("\n  - {}: {desc}", def.function.name));
                }
                defs
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut messages = vec![
            Message { 
                role: "system".to_string(),
                content: system_prompt,
            },
        ];

        // Inject conversation history so the model knows what was discussed
        for (prev_user, prev_assistant) in &self.conversation_history {
            messages.push(Message { 
                role: "user".to_string(),
                content: prev_user.clone(),
            });
            messages.push(Message { 
                role: "assistant".to_string(),
                content: prev_assistant.clone(),
            });
        }

        // Add the user prompt
        messages.push(Message { 
            role: "user".to_string(),
            content: prompt.to_string(),
        });

        // Max 3 tool call rounds — use native OpenAI function calling when tools exist
        let max_rounds = if mcp_tool_defs.is_empty() { 1 } else { 3 };
        let has_tools = !mcp_tool_defs.is_empty();
        let mut response_text = String::new();

        for round in 0..max_rounds {
            print!("   📝 (round {}/{}) ", round + 1, max_rounds);
            std::io::Write::flush(&mut std::io::stdout()).ok();

            // Use native function calling with tools, or fallback to plain chat
            let response_msg = if has_tools {
                match self.provider.chat_with_tools(
                    messages.clone(),
                    Some(mcp_tool_defs.clone()),
                ).await {
                    Ok(msg) => msg,
                    Err(e) => {
                        // Fallback: try plain chat
                        match self.chat_with_failover(messages.clone()).await {
                            Ok(r) => {
                                print!("{r}");
                                std::io::Write::flush(&mut std::io::stdout()).ok();
                                crate::llm::provider::ChatResponseMessage {
                                    content: Some(r),
                                    tool_calls: vec![],
                                }
                            }
                            Err(e2) => {
                                if round == 0 {
                                    // Try streaming as last resort
                                    if let Ok(stream) = self.chat_stream_with_failover(messages.clone()).await {
                                        let mut rx = stream.into_receiver();
                                        let mut full = String::new();
                                        while let Some(chunk) = rx.recv().await {
                                            print!("{chunk}");
                                            std::io::Write::flush(&mut std::io::stdout()).ok();
                                            full.push_str(&chunk);
                                        }
                                        println!();
                                        crate::llm::provider::ChatResponseMessage {
                                            content: Some(full),
                                            tool_calls: vec![],
                                        }
                                    } else {
                                        eprintln!("\n   Error: {e2}");
                                        crate::llm::provider::ChatResponseMessage {
                                            content: None,
                                            tool_calls: vec![],
                                        }
                                    }
                                } else {
                                    eprintln!("\n   Error: {e2}");
                                    crate::llm::provider::ChatResponseMessage {
                                        content: None,
                                        tool_calls: vec![],
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                match self.chat_with_failover(messages.clone()).await {
                    Ok(r) => {
                        print!("{r}");
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                        crate::llm::provider::ChatResponseMessage {
                            content: Some(r),
                            tool_calls: vec![],
                        }
                    },
                    Err(e) => {
                        if round == 0 {
                            if let Ok(stream) = self.chat_stream_with_failover(messages.clone()).await {
                                let mut rx = stream.into_receiver();
                                let mut full = String::new();
                                while let Some(chunk) = rx.recv().await {
                                    print!("{chunk}");
                                    std::io::Write::flush(&mut std::io::stdout()).ok();
                                    full.push_str(&chunk);
                                }
                                println!();
                                crate::llm::provider::ChatResponseMessage {
                                    content: Some(full),
                                    tool_calls: vec![],
                                }
                            } else {
                                eprintln!("\n   Error: {e}");
                                crate::llm::provider::ChatResponseMessage {
                                    content: None,
                                    tool_calls: vec![],
                                }
                            }
                        } else {
                            eprintln!("\n   Error: {e}");
                            crate::llm::provider::ChatResponseMessage {
                                content: None,
                                tool_calls: vec![],
                            }
                        }
                    }
                }
            };

            // Process native tool calls
            if !response_msg.tool_calls.is_empty() {
                for tool_call in &response_msg.tool_calls {
                    // Parse the tool name: "server.tool_name" → tool_name
                    let tool_name = tool_call.function.name.clone();
                    let args: serde_json::Value = match serde_json::from_str(&tool_call.function.arguments) {
                        Ok(v) => v,
                        Err(_) => serde_json::json!({}),
                    };

                    let result_value = match &self.mcp {
                        Some(mcp) => mcp.call_tool(&tool_name, args).await,
                        None => Err(anyhow::anyhow!("MCP not available")),
                    };

                    match result_value {
                        Ok(value) => {
                            let result_str = serde_json::to_string_pretty(&value)
                                .unwrap_or_else(|_| "{}".to_string());
                            messages.push(Message {
                                role: "tool".to_string(),
                                content: result_str,
                            });
                            println!("   🔧 Called MCP tool '{}'", tool_name);
                        }
                        Err(e) => {
                            messages.push(Message {
                                role: "tool".to_string(),
                                content: format!("Error: {e}"),
                            });
                            eprintln!("   ⚠️  MCP tool '{tool_name}' failed: {e}");
                        }
                    }
                }
                // Continue loop for next round with tool results
            } else {
                // No tool calls — this is the final response
                response_text = response_msg.content.unwrap_or_default();
                break;
            }
        }

        if !response_text.is_empty() {
            println!();
        }

        self.record_memory(
            &format!("Ask response for '{}': {}", &prompt[..prompt.char_indices().nth(100).map(|(i,_)|i).unwrap_or(prompt.len())], 
                     &response_text[..response_text.char_indices().nth(200).map(|(i,_)|i).unwrap_or(response_text.len())]),
            MemoryType::Decision,
        ).await;

        self.fire_hook(HookEvent::PostRun, "ask complete").await;

        // Consolidate memories
        if let Some(ref mem) = self.memory {
            if let Ok(uncon) = mem.store().get_unconsolidated(20) {
                if !uncon.is_empty() {
                    let ids: Vec<String> = uncon.iter().map(|e| e.id.clone()).collect();
                    let _ = mem.store().mark_consolidated(&ids);
                }
            }
        }

        Ok(RunResult {
            elapsed: start.elapsed(),
            memories_recorded: total_memories + 1,
            model_name: self.active_model_name(),
            response_text,
            ..Default::default()
        })
    }

    /// Simplified Q&A — single streaming round for when no code changes are needed.
    /// Unlike run_ask_mode, this has no tool-calling loop and no "DO NOT propose edits" restriction.
    async fn run_ask_mode_direct(
        &mut self,
        prompt: &str,
        relevant_files: &[FileContext],
    ) -> Result<String> {
        let file_context = self.build_ask_file_context(relevant_files);
        let mem_context = self.load_memory_context(prompt).await;

        let system_prompt = format!(
            "You are HyperAgent — a helpful coding assistant. Answer the user's question concisely and accurately.\n\n\
            Relevant files from the project:\n{}\n\n\
            Past context about this project:\n{}\n\n\
            Rules:\n\
            - Be concise but complete\n\
            - Reference specific file paths and function names when relevant\n\
            - If the user wants code changes, suggest the approach with code blocks\n\
            - Format code blocks with ```language\n\
            - Answer in the same language as the question",
            file_context,
            if mem_context.is_empty() { "None".to_string() } else { mem_context }
        );

        let mut messages = vec![Message {
            role: "system".to_string(),
            content: system_prompt,
        }];

        // Inject conversation history
        for (prev_user, prev_assistant) in &self.conversation_history {
            messages.push(Message {
                role: "user".to_string(),
                content: prev_user.clone(),
            });
            messages.push(Message {
                role: "assistant".to_string(),
                content: prev_assistant.clone(),
            });
        }

        messages.push(Message {
            role: "user".to_string(),
            content: prompt.to_string(),
        });

        // Stream a single response
        print!("   💬 ");
        std::io::Write::flush(&mut std::io::stdout()).ok();

        match self.chat_stream_with_failover(messages).await {
            Ok(stream) => {
                let mut rx = stream.into_receiver();
                let mut response_text = String::new();
                while let Some(chunk) = rx.recv().await {
                    print!("{chunk}");
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                    response_text.push_str(&chunk);
                }
                println!();
                Ok(response_text)
            }
            Err(e) => {
                // Fallback to batch
                match self.chat_with_failover(
                    vec![Message {
                        role: "user".to_string(),
                        content: format!("{prompt}\n\nRelevant files:\n{file_context}"),
                    }]
                ).await {
                    Ok(r) => Ok(r),
                    Err(e2) => {
                        eprintln!("   Error: {e2}");
                        Ok(String::new())
                    }
                }
            }
        }
    }

    /// Check if LLM response contains an MCP tool call
    fn parse_mcp_tool_call(&self, response: &str) -> Option<(String, serde_json::Value)> {
        // Look for {"tool_call": {"name": "...", "arguments": {...}}}
        if let Some(start) = response.find("{\"tool_call\"") {
            if let Some(end) = response[start..].find('}') {
                let candidate = &response[start..=start + end];
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(candidate) {
                    if let Some(tc) = parsed["tool_call"].as_object() {
                        if let Some(name) = tc["name"].as_str() {
                            let args = tc.get("arguments").cloned().unwrap_or(serde_json::Value::Null);
                            return Some((name.to_string(), args));
                        }
                    }
                }
            }
        }
        None
    }

    /// Build concise file context for ASK mode (summary only, no code dumps)
    fn build_ask_file_context(&self, files: &[FileContext]) -> String {
        let mut ctx = String::new();
        for file in files {
            ctx.push_str(&format!(
                "  [{}] {} ({} lines)\n",
                file.path.display(),
                file.path.display(),
                file.total_lines
            ));
            // Show first 5 lines as preview only
            let preview: Vec<&str> = file.content.lines().take(5).collect();
            if !preview.is_empty() {
                ctx.push_str("  ```\n");
                for line in preview {
                    ctx.push_str(&format!("  {line}\n"));
                }
                ctx.push_str("  ```\n");
            }
        }
        ctx
    }

    /// Build the full prompt with memory context + project context + mode + conversation history + MCP tools
    async fn build_augmented_prompt(&self, prompt: &str, mem_context: &str) -> String {
        let mut parts = vec![prompt.to_string()];

        // Inject conversation history so the model knows what was just discussed
        if !self.conversation_history.is_empty() {
            parts.push("\n\n--- Recent Conversation ---\n".to_string());
            for (i, (usr, asst)) in self.conversation_history.iter().enumerate() {
                let turn = i + 1;
                parts.push(format!("\n[Turn {turn}]\nUser: {usr}\nAssistant: {asst}\n"));
            }
        }

        if let Some(ref ctx) = self.project_context {
            parts.push("\n\n--- Project Context (AGENTS.md) ---\n".to_string());
            parts.push(ctx.clone());
        }

        if !mem_context.is_empty() {
            // Insert memory context as a "what we learned from past sessions" section
            parts.push("\n\n--- Past Memory Context ---\n".to_string());
            parts.push(mem_context.to_string());
        }

        // Add MCP tools context if available
        if let Some(ref mcp) = self.mcp {
            let tools_str = mcp.tools_to_llm_format().await;
            if !tools_str.is_empty() {
                parts.push(tools_str);
            }
        }

        // Add mode-specific instructions from ModeRegistry
        let mode_prompt = match &self.mode_registry {
            Some(registry) => registry.build_prompt(&self.mode, None),
            None => match self.mode.as_str() {
                "architect" => "\n\nMODE: ARCHITECT — design systems, output plans. Do NOT implement code.".to_string(),
                "ask" => "\n\nMODE: ASK — answer questions only. Do NOT modify files.".to_string(),
                "debug" => "\n\nMODE: DEBUG — focus on root-cause analysis. Add minimal logging. Verify fixes.".to_string(),
                _ => String::new(),
            },
        };
        if !mode_prompt.is_empty() {
            parts.push(mode_prompt);
        }

        parts.join("")
    }

    /// Load memory context relevant to the task
    async fn load_memory_context(&self, prompt: &str) -> String {
        match &self.memory {
            Some(mem) => mem.build_context(prompt, 8).unwrap_or_default(),
            None => String::new(),
        }
    }

    /// Record a memory silently
    async fn record_memory(&self, content: &str, mem_type: MemoryType) {
        if let Some(ref mem) = self.memory {
            let _ = mem.remember(content, mem_type);
        }
    }

    /// Fire a hook event
    async fn fire_hook(&self, event: HookEvent, _data: &str) {
        if let Some(ref hooks) = self.hooks {
            let _ = hooks.fire(&event, Some(&self.mode));
        }
    }

    /// Create plan with retry on empty/failed plan
    /// Uses progressive prompting: standard → action-focused → raw retry
    async fn create_plan_with_retry(
        &self,
        prompt: &str,
        files: &[FileContext],
        max_attempts: usize,
    ) -> Result<crate::agent::plan_agent::Plan> {
        let plan_provider = self.plan_provider.as_ref().unwrap_or(&self.provider);
        let plan_agent = crate::agent::plan_agent::PlanAgent::new(plan_provider);

        // First attempt: batch (reliable JSON parsing)
        match plan_agent.create_plan(prompt, files).await {
            Ok(plan) if !plan.summary.is_empty() && plan.summary != "No plan generated" => {
                return Ok(plan);
            }
            _ => {}
        }

        // Fallback retries with batch
        let mut _last_error = String::new();
        let plan_provider = self.plan_provider.as_ref().unwrap_or(&self.provider);
        let plan_agent = crate::agent::plan_agent::PlanAgent::new(plan_provider);

        for attempt in 1..=max_attempts.saturating_sub(1) {
            println!("   🔄 Retrying plan creation (attempt {attempt}/{max_attempts})...");
            match plan_agent.create_plan(prompt, files).await {
                Ok(plan) if !plan.summary.is_empty() => {
                    return Ok(plan);
                }
                Ok(_) => { _last_error = "Empty plan".to_string(); }
                Err(e) => { _last_error = format!("{e}"); }
            }
        }
        // Last attempt: return whatever we got
        let plan_agent = crate::agent::plan_agent::PlanAgent::new(&self.provider);
        plan_agent.create_plan(prompt, files).await
    }

    /// Execute code agents with retry if no changes generated
    /// Progressive: review each agent's changes as they complete (pipeline overlap)
    async fn execute_with_retry(
        &self,
        augmented_prompt: &str,
        plan: &crate::agent::plan_agent::Plan,
        relevant_files: &[FileContext],
        original_prompt: &str,
        _start: Instant,
        _total_memories: usize,
    ) -> Result<Vec<FileChange>> {
        let mut all_changes: Vec<FileChange> = Vec::new();
        let max_attempts = 2;
        let _task_prompt = original_prompt.to_string();
        let _review_provider = self.review_provider.as_ref().unwrap_or(&self.provider).clone();

        for attempt in 1..=max_attempts {
            if attempt > 1 && all_changes.is_empty() {
                println!("   🔄 Retrying code execution (attempt {attempt}/{max_attempts})...");
            }

            let steps = plan.steps.clone().unwrap_or_default();
            if steps.is_empty() {
                break;
            }

            let chunks = self.smart_split(&steps, relevant_files, self.parallel_agents);
            let total_agents = chunks.len();

            // Use channel to collect results progressively
            let (tx, mut rx) = tokio::sync::mpsc::channel::<(usize, Vec<FileChange>)>(total_agents);

            let code_start = Instant::now();
            for (i, chunk) in chunks.iter().enumerate() {
                let tx = tx.clone();
                let root = self.root.clone();
                let provider = self.provider.clone();
                let p = augmented_prompt.to_string();
                let files = relevant_files.to_vec();
                let chunk_steps = chunk.steps.clone();
                let agent_name = chunk.name.clone();

                tokio::spawn(async move {
                    let agent = crate::agent::code_agent::CodeAgent::new(&provider, &root);
                    let changes = agent.execute_stream(&agent_name, &p, &chunk_steps, &files).await;
                    let _ = tx.send((i, changes)).await;
                });
            }
            drop(tx); // Close sender so rx can terminate

            // Collect results progressively — review each agent's changes as they arrive
            let mut completed = 0usize;
            use std::io::{Write, stdout};

            // Use per-agent change tracking for progressive review
            let mut agent_changes: Vec<Vec<FileChange>> = vec![Vec::new(); total_agents];

            while let Some((i, changes)) = rx.recv().await {
                completed += 1;
                let elapsed = code_start.elapsed();
                let file_count = changes.len();
                let agent_name = chunks[i].name.clone();

                if file_count > 0 || attempt == max_attempts {
                    print!("\r   ✅ Agent '{}' — {} changes [{completed}/{total_agents} done, {:.1}s]    \n",
                        agent_name, file_count, elapsed.as_secs_f64());
                    stdout().flush().ok();
                } else {
                    print!("\r   ⏳ Agent '{}' — no changes [{completed}/{total_agents} done, {:.1}s]    \n",
                        agent_name, elapsed.as_secs_f64());
                    stdout().flush().ok();
                }

                agent_changes[i] = changes;
            }

            // Merge all approved changes
            for mut changes in agent_changes {
                all_changes.append(&mut changes);
            }

            // Print summary line after all agents complete
            println!("   📦 {total_agents} agents completed in {:.1}s", code_start.elapsed().as_secs_f64());

            if !all_changes.is_empty() {
                break; // Got changes, no retry needed
            }
        }

        Ok(all_changes)
    }

    /// Smart work splitter — groups related files together per agent
    fn smart_split(
        &self,
        steps: &[String],
        _files: &[FileContext],
        n: usize,
    ) -> Vec<WorkChunk> {
        if steps.is_empty() {
            return vec![WorkChunk {
                name: "default".into(),
                steps: vec![],
            }];
        }

        if n <= 1 || steps.len() <= 1 {
            return vec![WorkChunk {
                name: "agent-1".into(),
                steps: steps.to_vec(),
            }];
        }

        // Distribute steps evenly across N parallel agents
        let chunk_size = steps.len().div_ceil(n);
        let mut chunks = Vec::new();
        for i in 0..n {
            let start = i * chunk_size;
            if start >= steps.len() {
                break;
            }
            let end = (start + chunk_size).min(steps.len());
            chunks.push(WorkChunk {
                name: format!("agent-{}", i + 1),
                steps: steps[start..end].to_vec(),
            });
        }

        chunks
    }

    fn estimate_tokens(prompt: &str, changes: &[FileChange]) -> usize {
        let mut total = prompt.len() / 4;
        for change in changes {
            if let Some(ref content) = change.old_content {
                total += content.len() / 4;
            }
            if let Some(ref content) = change.new_content {
                total += content.len() / 4;
            }
        }
        total
    }

    /// Prepare worktree for isolated apply+lint (if enabled), or return self.root.
    async fn prepare_apply_worktree(&mut self) -> Result<(std::path::PathBuf, Option<crate::git::worktree::WorktreeManager>)> {
        if !self.use_worktree {
            return Ok((self.root.clone(), None));
        }
        let is_git = self.root.join(".git").exists();
        if !is_git {
            println!("   ⚠️  Not a git repo — worktree isolation skipped");
            return Ok((self.root.clone(), None));
        }
        let status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.root).output().ok();
        if let Some(out) = status {
            if !String::from_utf8_lossy(&out.stdout).trim().is_empty() {
                println!("   ⚠️  Uncommitted changes — worktree isolation skipped");
                return Ok((self.root.clone(), None));
            }
        }
        let mut wt_manager = crate::git::worktree::WorktreeManager::new(&self.root, "hyper-apply");
        match wt_manager.create_worktrees(1).await {
            Ok(paths) => {
                let root = paths[0].clone();
                println!("   🌳 Apply sandbox: {:?}", root);
                Ok((root, Some(wt_manager)))
            }
            Err(e) => {
                println!("   ⚠️  Worktree failed: {e} — falling back");
                Ok((self.root.clone(), None))
            }
        }
    }

    /// Sync worktree changes back to main repo via git diff + apply, then clean up
    async fn sync_worktree_and_cleanup(wt_manager: &mut crate::git::worktree::WorktreeManager, main_root: &std::path::Path) {
        let diff = match wt_manager.diff_worktree(0) {
            Ok(d) => d,
            Err(e) => { println!("   ⚠️  Worktree diff: {e}"); let _ = wt_manager.cleanup().await; return; }
        };
        if diff.is_empty() { let _ = wt_manager.cleanup().await; return; }
        let patch_file = std::env::temp_dir().join(format!("hyper-patch-{}", std::process::id()));
        if std::fs::write(&patch_file, &diff).is_err() { let _ = wt_manager.cleanup().await; return; }
        let check = std::process::Command::new("git")
            .args(["apply", "--check", patch_file.to_str().unwrap()])
            .current_dir(main_root).output();
        match check {
            Ok(o) if o.status.success() => {
                let _ = std::process::Command::new("git")
                    .args(["apply", patch_file.to_str().unwrap()])
                    .current_dir(main_root).output();
                println!("   ✅ Changes synced from worktree");
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                println!("   ⚠️  Sync conflict: {:?} — inspect {:?}", err.lines().next().unwrap_or("?"), patch_file);
                return;
            }
            Err(e) => println!("   ⚠️  git apply failed: {e}"),
        }
        let _ = wt_manager.cleanup().await;
    }
}

pub struct WorkChunk {
    pub name: String,
    pub steps: Vec<String>,
}

/// Token budget manager — keeps LLM context windows from overflowing
///
/// In a large codebase, 15 files × 4000 chars = 60K chars (~15K tokens)
/// can easily blow past the model's context limit. ContextBudget tracks
/// fixed overhead (system prompt, history, project context) and allocates
/// remaining budget to files by relevance.
const MAX_INPUT_TOKENS: usize = 128_000;
const MAX_OUTPUT_TOKENS: usize = 16_384;
const TOKEN_RATIO: usize = 4;

fn _estimate_chars_to_tokens(s: &str) -> usize {
    s.len() / TOKEN_RATIO
}

pub struct ContextBudget {
    pub fixed_tokens: usize,
    pub available_for_files: usize,
    pub full_files: usize,
    pub truncated_files: usize,
    pub total_files: usize,
}

impl ContextBudget {
    /// Calculate budget given fixed overhead (system prompt, history, project ctx)
    pub fn new(fixed_overhead: &str, conversation: &[(String, String)], project_context: Option<&str>) -> Self {
        let mut fixed = _estimate_chars_to_tokens(fixed_overhead);

        // Conversation history overhead
        for (user, asst) in conversation {
            fixed += _estimate_chars_to_tokens(user) + _estimate_chars_to_tokens(asst) + 10;
        }

        // Project context (AGENTS.md etc.)
        if let Some(ctx) = project_context {
            fixed += _estimate_chars_to_tokens(ctx);
        }

        let available = if MAX_INPUT_TOKENS > fixed {
            MAX_INPUT_TOKENS - fixed
        } else {
            4_000
        };

        Self {
            fixed_tokens: fixed,
            available_for_files: available,
            full_files: 0,
            truncated_files: 0,
            total_files: 0,
        }
    }

    /// Truncate file contents to fit within the token budget.
    /// Keeps high-relevance files fully, truncates lower-relevance to preview.
    pub fn truncate_files(&mut self, files: &mut [crate::index::FileContext], max_files: usize) {
        let total = files.len().min(max_files);
        self.total_files = total;

        // Sort by score descending
        files.sort_by(|a, b| {
            b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
        });

        if total == 0 || self.available_for_files == 0 {
            for file in files.iter_mut().take(total) {
                file.content.clear();
            }
            return;
        }

        let per_file_budget = (self.available_for_files / total).min(2_000).max(100);

        for (i, file) in files.iter_mut().enumerate().take(total) {
            let file_tokens = _estimate_chars_to_tokens(&file.content);

            if file_tokens > per_file_budget {
                let truncate_len = per_file_budget * TOKEN_RATIO;
                let truncated: String = file.content.chars().take(truncate_len).collect();
                file.content = format!(
                    "{}...\n// [truncated: {file_tokens}→{per_file_budget}t, score:{:.2}]",
                    truncated, file.score
                );
                self.truncated_files += 1;
            } else {
                self.full_files += 1;
            }
        }

        // Clear files beyond the limit
        for file in files.iter_mut().skip(total) {
            file.content.clear();
        }
    }

    /// Display a summary of the budget utilization
    pub fn summary(&self) -> String {
        format!(
            "Budget: {:.1}K fixed + {:.1}K files = {:.1}K / {:.1}K tokens ({} full, {} truncated)",
            self.fixed_tokens as f64 / 1000.0,
            (MAX_INPUT_TOKENS - self.available_for_files) as f64 / 1000.0,
            (MAX_INPUT_TOKENS - self.available_for_files + self.fixed_tokens) as f64 / 1000.0,
            (self.fixed_tokens + MAX_INPUT_TOKENS - self.available_for_files) as f64 / 1000.0,
            self.full_files,
            self.truncated_files,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::HyperIndex;
    use std::path::Path;

    /// Create a minimal Rust project for testing
    fn create_test_project(dir: &Path) {
        std::fs::create_dir_all(dir.join("src")).ok();
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "hyperagent-test"
version = "0.1.0"
edition = "2021"
"#,
        ).ok();
        std::fs::write(
            dir.join("src").join("lib.rs"),
            r#"pub fn greet() -> &'static str { "hello" }
"#,
        ).ok();
    }

    #[tokio::test]
    async fn test_orchestrator_new() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        create_test_project(&root);

        let mut index = HyperIndex::new(&root).unwrap();
        index.build().unwrap();

        let provider = LlmProvider::new(
            "test-model".to_string(),
            "http://localhost:9999/v1".to_string(),
            "test-key".to_string(),
        ).unwrap();

        let orch = Orchestrator::new(index, provider, root, 2, true);
        assert_eq!(orch.parallel_agents, 2);
        assert!(orch.confirm);
    }

    #[tokio::test]
    async fn test_orchestrator_chunking() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        create_test_project(&root);

        let mut index = HyperIndex::new(&root).unwrap();
        index.build().unwrap();

        let provider = LlmProvider::new(
            "test-model".to_string(),
            "http://localhost:9999/v1".to_string(),
            "test-key".to_string(),
        ).unwrap();

        let mut orch = Orchestrator::new(index, provider, root, 3, true);
        let steps = vec![
            "Step 1".to_string(),
            "Step 2".to_string(),
            "Step 3".to_string(),
            "Step 4".to_string(),
            "Step 5".to_string(),
        ];

        let chunks = orch.smart_split(&steps, &[], 3);
        assert_eq!(chunks.len(), 3, "5 steps with 3 agents = 3 chunks");
        assert_eq!(chunks[0].steps.len(), 2, "first chunk should have 2 steps");
        assert_eq!(chunks[1].steps.len(), 2, "second chunk should have 2 steps");
    }

    #[tokio::test]
    async fn test_orchestrator_chunking_single() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        create_test_project(&root);

        let mut index = HyperIndex::new(&root).unwrap();
        index.build().unwrap();

        let provider = LlmProvider::new(
            "test-model".to_string(),
            "http://localhost:9999/v1".to_string(),
            "test-key".to_string(),
        ).unwrap();

        let mut orch = Orchestrator::new(index, provider, root, 1, true);
        let steps = vec!["Only step".to_string()];
        let chunks = orch.smart_split(&steps, &[], 1);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].name, "agent-1");
    }

    #[tokio::test]
    async fn test_orchestrator_cost_estimate() {
        let cost = Orchestrator::estimate_cost(100_000, 0.15);
        // 60k input tokens * $0.15/1M + 40k output tokens * $0.60/1M
        // = 0.009 + 0.024 = 0.033
        assert!((cost - 0.033).abs() < 0.001, "cost should be ~$0.033, got {cost}");
    }

    #[tokio::test]
    async fn test_orchestrator_budget_check() {
        assert!(!Orchestrator::would_exceed_budget(100_000, 0.15, 0.0), "no budget = always allowed");
        assert!(!Orchestrator::would_exceed_budget(100_000, 0.15, 1.0), "1.0 budget > 0.033 cost");
        assert!(Orchestrator::would_exceed_budget(100_000_000, 0.15, 1.0), "huge token count should exceed");
    }

    #[tokio::test]
    async fn test_orchestrator_mode_default() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        create_test_project(&root);

        let mut index = HyperIndex::new(&root).unwrap();
        index.build().unwrap();

        let provider = LlmProvider::new(
            "test-model".to_string(),
            "http://localhost:9999/v1".to_string(),
            "test-key".to_string(),
        ).unwrap();

        let orch = Orchestrator::new(index, provider, root.clone(), 2, true)
            .with_mode("ask");
        assert_eq!(orch.mode, "ask");

        let orch2 = Orchestrator::new(
            HyperIndex::new(&root).unwrap(),
            LlmProvider::new("test".to_string(), "http://localhost:9999/v1".to_string(), "test-key".to_string()).unwrap(),
            root, 2, true,
        ).with_mode("code");
        assert_eq!(orch2.mode, "code");
    }

    #[tokio::test]
    async fn test_orchestrator_builder_methods() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        create_test_project(&root);

        let mut index = HyperIndex::new(&root).unwrap();
        index.build().unwrap();

        let provider = LlmProvider::new(
            "test-model".to_string(),
            "http://localhost:9999/v1".to_string(),
            "test-key".to_string(),
        ).unwrap();

        let orch = Orchestrator::new(index, provider, root, 2, true)
            .with_project_context("test context".to_string())
            .with_conversation_history(vec![("hello".into(), "hi".into())]);

        assert!(orch.project_context.is_some());
        assert_eq!(orch.conversation_history.len(), 1);
    }
}
