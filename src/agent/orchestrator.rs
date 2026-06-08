#![allow(unused)]
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
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use crate::diff::FileChange;
use crate::hooks::{HookEvent, HookRegistry};
use crate::index::{FileContext, HyperIndex};
use crate::llm::{ContentPart, LlmProvider, Message, ProviderPool};
use crate::llm::provider::{ToolDefinition, ToolFunction};
use crate::memory::{MemoryManager, MemoryType};

use super::apply_agent::ApplyAgent;
use super::review_agent::ReviewAgent;
use super::plan_agent::Intent;

/// Maximum depth for sub-agent task tool to prevent infinite recursion.
pub const MAX_TASK_DEPTH: u32 = 3;

/// Result of an orchestrator run
#[derive(Debug, Default, Clone)]
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
    use_worktree: bool,
    pending_image: Option<String>,
    knowledge_base: Option<crate::knowledge::KnowledgeBase>,
    /// When true, shell/Python commands run in Docker sandbox
    sandbox_enabled: bool,
    /// Loaded plugin tools from .hyper/tools/
    plugin_manager: Option<crate::plugin::PluginManager>,
    /// Current sub-agent task depth (incremented by task tool)
    task_depth: AtomicU32,
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
            use_worktree: false,
            pending_image: None,
            knowledge_base: None,
            sandbox_enabled: false,
            plugin_manager: None,
            task_depth: AtomicU32::new(0),
        }
    }

    pub fn with_sandbox(mut self) -> Self {
        self.sandbox_enabled = true;
        self
    }

    pub fn with_worktree(mut self) -> Self {
        self.use_worktree = true;
        self
    }
    pub fn with_image(&mut self, path: impl Into<String>) -> &mut Self {
        self.pending_image = Some(path.into());
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

    pub fn with_knowledge_base(mut self, kb: crate::knowledge::KnowledgeBase) -> Self {
        self.knowledge_base = Some(kb);
        self
    }

    pub fn with_plugins(mut self, pm: crate::plugin::PluginManager) -> Self {
        if pm.count() > 0 {
            println!("   🔌 Loaded {} plugin tool(s): {}", pm.count(), pm.list_tools().join(", "));
        }
        self.plugin_manager = Some(pm);
        self
    }

    /// Paginate large tool output: show first N chars, save the rest to a temp file
    /// Returns a message telling the LLM how to read the rest
    fn paginate_output(&self, output: &str, max_chars: usize, tool_name: &str) -> String {
        if output.len() <= max_chars {
            return output.to_string();
        }

        let overflow_dir = self.root.join(".hyper").join("tool_output");
        let _ = std::fs::create_dir_all(&overflow_dir);
        let filename = format!("{}_{}.txt", tool_name, std::process::id());
        let overflow_path = overflow_dir.join(&filename);

        let first_part = &output[..max_chars];
        let remaining = &output[max_chars..];

        let _ = std::fs::write(&overflow_path, remaining);

        format!(
            "{}\n\n--- [Output truncated: {} chars total, showing first {}. The full remaining output is saved to: {}] ---\n\
             To see the rest, use: `read_file` with path \"{}\"",
            first_part,
            output.len(),
            max_chars,
            overflow_path.display(),
            overflow_path.display()
        )
    }

    /// Compress old conversation history into a summary to keep context bounded
    /// Keeps the most recent K turns intact, summarizes everything older
    /// Uses the LLM to generate a concise summary
    /// Enhanced with token-aware triggering and adaptive thresholds
    async fn compress_conversation(&mut self) {
        const MAX_HISTORY_PAIRS: usize = 16;
        const KEEP_RECENT: usize = 6;
        const MAX_ESTIMATED_TOKENS: usize = 32000; // ~24k token soft limit for compression trigger

        if self.conversation_history.is_empty() {
            return;
        }

        // Count trigger: either too many pairs OR estimated tokens too high
        let estimated_tokens: usize = self.conversation_history.iter()
            .map(|(u, a)| u.len() / 4 + a.len() / 4 + 10)
            .sum();
        let too_many_pairs = self.conversation_history.len() > MAX_HISTORY_PAIRS;
        let too_many_tokens = estimated_tokens > MAX_ESTIMATED_TOKENS;

        if !too_many_pairs && !too_many_tokens {
            return;
        }

        // Adaptive: if token-heavy, keep fewer recent turns
        let keep_recent = if too_many_tokens && estimated_tokens > 48000 {
            3 // Aggressive compression for very long conversations
        } else {
            KEEP_RECENT
        };

        let old_turns = self.conversation_history.len().saturating_sub(keep_recent);
        if old_turns == 0 {
            return;
        }
        let old: Vec<(String, String)> = self.conversation_history.drain(..old_turns).collect();

        println!("   📐 Context: ~{:.1}K tokens, {} turns → compressing (keep {} recent, summarize {})",
            estimated_tokens as f64 / 1000.0,
            self.conversation_history.len() + old.len(),
            keep_recent,
            old.len());

        // Build a summary of old turns
        let turns_text: String = old.iter().enumerate()
            .map(|(i, (u, a))| format!("Turn {}:\nUser: {}\nAssistant: {}\n", i + 1, u, a))
            .collect();

        let system_prompt = "You are a conversation summarizer. Summarize the key information, \
            decisions, and context from these conversation turns in 2-3 sentences. \
            Focus on facts that are still relevant, not the conversation flow itself. \
            Output ONLY the summary, no preamble.";

        match self.provider.chat(vec![
            Message::text("system", system_prompt),
            Message::text("user", format!("Summarize these conversation turns:\n\n{}", turns_text)),
        ]).await {
            Ok(summary) => {
                let compressed = format!("[Previous conversation summary: {}]", summary.trim());
                // Insert the summary at the beginning
                self.conversation_history.insert(0, (
                    "[system: conversation compressed]".to_string(),
                    compressed,
                ));
            }
            Err(_) => {
                // If compression fails, keep old turns — safer to keep context than lose it
                self.conversation_history.splice(0..0, old);
            }
        }
    }

    /// Process user feedback/corrections from conversation history.
    /// Detects correction patterns (e.g., "don't use X", "instead use Y", "that's wrong")
    /// and records them as high-importance UserPreference memories.
    async fn process_feedback(&self, current_prompt: &str) {
        if self.memory.is_none() || self.conversation_history.is_empty() {
            return;
        }

        // Check if the current prompt contains correction keywords
        let correction_keywords = [
            "don't", "dont", "not", "wrong", "incorrect", "instead",
            "actually", "try using", "should use", "prefer", "never",
            "stop", "avoid", "always use", "better to",
        ];
        let lower = current_prompt.to_lowercase();
        let has_correction = correction_keywords.iter().any(|k| lower.contains(k));

        // Also check the last user turn in history for corrections
        let last_user_turn = self.conversation_history.last()
            .map(|(u, _)| u.clone())
            .unwrap_or_default();
        let last_lower = last_user_turn.to_lowercase();
        let recent_correction = correction_keywords.iter().any(|k| last_lower.contains(k));

        if has_correction || recent_correction {
            let text = if has_correction { current_prompt } else { &last_user_turn };
            self.record_memory(
                &format!("User correction/preference: {}", text.chars().take(200).collect::<String>()),
                MemoryType::UserPreference,
            ).await;
        }
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

        // Phase 0: Compress conversation history if too long
        self.compress_conversation().await;

        // Phase 0b: Extract feedback/corrections from conversation history
        self.process_feedback(prompt).await;

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
        println!("   🎭 Mode: {}\n", self.mode);

        // Phase 1: Get relevant files from PageRank index
        println!("🔍 Scanning codebase with PageRank...");
        let mut relevant_files = self.index.get_relevant_files(prompt, 15, 4000);
        println!("   Found {} relevant files\n", relevant_files.len());

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
        println!("📋 Planning...");

        // ASK mode: skip plan/code/review pipeline, do direct Q&A
        if self.mode == "ask" {
            return self.run_ask_mode(prompt, &relevant_files, start, total_memories).await;
        }

        // Plan with retry (up to 2 attempts) — fallback to general mode on failure
        let plan = match self.create_plan_with_retry(&augmented_prompt, &relevant_files, 2).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("   ⚠️  Planning failed: {e}");
                eprintln!("   ℹ️  Falling back to general-purpose mode...");
                return self.run_general_mode(prompt, &relevant_files, start, total_memories).await;
            }
        };
        println!("   Plan: {}", plan.summary);
        println!("   Intent: {:?}", plan.intent);
        if let Some(ref steps) = plan.steps {
            for (i, step) in steps.iter().enumerate() {
                println!("   {}. {}", i + 1, step);
            }
        }
        println!();

        // Route based on LLM-classified intent
        match plan.intent {
            Intent::Ask => {
                println!("   💬 Answering question directly...");
                return self.run_ask_mode(prompt, &relevant_files, start, total_memories).await;
            }
            Intent::General => {
                // If plan agent naturally decomposed into sub-steps, execute them in sequence
                if let Some(ref steps) = plan.steps {
                    if steps.len() > 1 {
                        println!("   🔄 Multi-step task: {} steps", steps.len());
                        return self.run_task_mode(prompt, steps, start, total_memories).await;
                    }
                }
                // Single-step or no steps → direct general mode
                return self.run_general_mode(prompt, &relevant_files, start, total_memories).await;
            }
            Intent::Code => {
                // Continue to code execution pipeline below
            }
        }

        // Record plan to memory
        let steps_count = plan.steps.as_ref().map(|s| s.len()).unwrap_or(0);
        self.record_memory(
            &format!("Plan for '{}': {} — {} steps", prompt, plan.summary, steps_count),
            MemoryType::Decision,
        ).await;
        total_memories += 1;

        self.fire_hook(HookEvent::PostPlan, &plan.summary).await;

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

        // Phase 3b: Inline diff preview — show changes before review
        if !all_changes.is_empty() {
            let diff_preview = crate::diff::render_diff_preview(&all_changes, 5);
            println!("{}", diff_preview);
        }

        if all_changes.is_empty() {
            // If no code changes generated but intent was "code", still try general mode
            // This handles cases where the code agent couldn't figure out file changes
            // for a general task that the LLM misclassified as "code"
            if plan.intent != Intent::Code {
                println!("   ℹ️ No code changes needed — switching to general-purpose mode...");
                return self.run_general_mode(prompt, &relevant_files, start, total_memories).await;
            }
            // Only use keyword fallback when intent was already "code"
            if !Self::is_coding_task(prompt) {
                println!("   ℹ️ No code changes needed — switching to general-purpose mode...");
                return self.run_general_mode(prompt, &relevant_files, start, total_memories).await;
            }
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
        let approved = review_agent.review(prompt, &all_changes).await?;

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

        // Phase 4b: Self-reflection — assess quality of approved changes
        self.fire_hook(HookEvent::PreCode, &format!("reflection on {} changes", approved.len())).await;
        if let Err(e) = self.self_reflect(prompt, &approved).await {
            eprintln!("   ⚠️  Self-reflection failed (non-fatal): {e}");
        }
        self.fire_hook(HookEvent::PostCode, &format!("reflection {} items", approved.len())).await;

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
        // If cargo check fails, feed errors back to LLM for auto-fix
        let has_cargo = self.root.join("Cargo.toml").exists();
        let has_ts = self.root.join("tsconfig.json").exists();
        if has_cargo || has_ts {
            let max_attempts = 5;
            let mut seen_errors: std::collections::HashSet<String> = std::collections::HashSet::new();
            for fix_round in 1..=max_attempts {
                let linter = if has_cargo { "cargo check" } else { "tsc --noEmit" };
                print!("   🔧 Lint check ({linter}, round {fix_round}/{max_attempts})...");
                let _ = std::io::Write::flush(&mut std::io::stdout());

                let check_result = if has_cargo {
                    std::process::Command::new("cargo")
                        .args(["check"])
                        .current_dir(&self.root)
                        .output()
                        .ok()
                } else {
                    std::process::Command::new("npx")
                        .args(["tsc", "--noEmit"])
                        .current_dir(&self.root)
                        .output()
                        .ok()
                };

                match check_result {
                    Some(out) if out.status.success() => {
                        println!(" ✅");
                        break;
                    }
                    Some(out) => {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        // Extract all error lines with file:line
                        let mut new_errors: Vec<String> = Vec::new();
                        for line in stderr.lines() {
                            let line = line.trim();
                            // Match: "error[E0308]" or "error: ..." or "....rs:line:col: error["
                            let is_error = line.contains("error[")
                                || line.starts_with("error:")
                                || (line.ends_with(".rs") && line.contains("error"))
                                || line.contains("aborting due to");
                            if is_error {
                                if seen_errors.insert(line.to_string()) {
                                    new_errors.push(line.to_string());
                                }
                            }
                        }

                        if new_errors.is_empty() {
                            // Check if there are any real errors at all
                            let has_errors = stderr.lines().any(|l| l.contains("error["));
                            if !has_errors {
                                println!(" ✅ (no actionable errors)");
                                break;
                            }
                        }

                        println!(); // newline after the "checking..." message
                        println!(
                            "   ⚠️  {} new lint errors (round {fix_round}/{max_attempts}, {} total seen):",
                            new_errors.len(),
                            seen_errors.len()
                        );
                        for e in new_errors.iter().take(10) {
                            println!("      {e}");
                        }
                        if new_errors.len() > 10 {
                            println!("      ... and {} more", new_errors.len() - 10);
                        }

                        if fix_round >= max_attempts {
                            println!("   ❌ Max fix attempts ({max_attempts}) reached — {}
      Errors remaining: {} unique, {} new this round",
                                if seen_errors.len() <= 3 { "likely a deep issue" } else { "manual intervention needed" },
                                seen_errors.len(), new_errors.len());
                            break;
                        }

                        // Build error context (full stderr, trimmed to relevant parts)
                        let error_context: String = stderr.lines()
                            .filter(|l| {
                                l.contains("error[") || l.starts_with("error:")
                                    || l.starts_with("  --> ") || l.starts_with("   = ")
                                    || l.starts_with("help:") || l.starts_with("note:")
                                    || l.trim().starts_with("|")
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        // Read the current file content for each approved change
                        println!("   🔄 Requesting auto-fix from LLM (round {fix_round}/{max_attempts})...");
                        let fix_files: Vec<crate::diff::FileChange> = approved.iter()
                            .filter_map(|c| {
                                let path = &c.file;
                                if path.exists() {
                                    let content = std::fs::read_to_string(path).ok()?;
                                    Some(crate::diff::FileChange {
                                        file: path.clone(),
                                        change_type: "edit".to_string(),
                                        old_content: None,
                                        new_content: Some(content),
                                        hunks: vec![],
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect();

                        let review_input = crate::agent::review_agent::build_review_context_for_lint(
                            &format!("Fix compile errors in the changed files (attempt {fix_round})"), &fix_files, &error_context
                        );

                        let fix_system_prompt = &format!(
                            "You are a code fixer. The following compile errors were found after applying changes.\
                             Output ONLY the corrected file content in JSON format:\\n\
                             {}\"file\": \"relative/path\", \"content\": \"COMPLETE corrected file content\"{}\\n\
                             RULES:\\n\
                             - Output one JSON object per file that needs fixing\\n\
                             - Do NOT change anything beyond what is needed to fix the errors\\n\
                             - Keep the existing code structure intact\\n\
                             - Each JSON must be on its own line",
                            '{', '}'
                        );

                        let fix_response = self.provider.chat(vec![
                            Message::text("system", fix_system_prompt),
                            Message::text("user", review_input),
                        ]).await;

                        match fix_response {
                            Ok(response) => {
                                let fixed = crate::agent::code_agent::CodeAgent::parse_fix_response(
                                    &response, &self.root
                                );
                                if fixed.is_empty() {
                                    println!("   ⚠️  LLM couldn't generate fixes — stopping auto-fix");
                                    let remaining: Vec<&String> = seen_errors.iter().take(3).collect();
                                    let remaining_sample: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();
                    println!("      Remaining errors (sample): {}", remaining_sample.join("; "));
                                    break;
                                }
                                for (path, content) in &fixed {
                                    if let Some(parent) = path.parent() {
                                        let _ = std::fs::create_dir_all(parent);
                                    }
                                    if let Err(e) = std::fs::write(path, content) {
                                        eprintln!("   ⚠️  Failed to write fix for {}: {e}", path.display());
                                    } else {
                                        println!("   ✅ Fixed: {}", path.display());
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("   ⚠️  Fix generation failed: {e}");
                                break;
                            }
                        }
                    }
                    None => {
                        // linter not available, skip
                        break;
                    }
                }
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

        println!("\n✅ Done — {} files in {:.1}s",
            changed_files.len(),
            elapsed.as_secs_f64());

        // Auto-prune memory to max 100 entries
        if let Some(ref mem) = self.memory {
            if let Ok(pruned) = mem.prune(100) {
                if pruned > 0 {
                    tracing::debug!("Pruned {pruned} old memories");
                }
            }
            // Consolidate new memories
            if let Ok(uncon) = mem.store_ref().get_unconsolidated(20) {
                if !uncon.is_empty() {
                    let ids: Vec<String> = uncon.iter().map(|e| e.id.clone()).collect();
                    let _ = mem.store_ref().mark_consolidated(&ids);
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

        let is_general = self.mode == "general";
        let file_context = if is_general {
            String::new()
        } else {
            self.build_ask_file_context(relevant_files)
        };
        let mem_context = self.load_memory_context(prompt).await;

        let system_prompt = if is_general {
            format!(
                "You are HyperAgent's General mode — a knowledgeable, versatile assistant.\n\
                Help with coding, writing, analysis, translation, brainstorming, research, and more.\n\
                Be concise but thorough. Use markdown formatting when helpful.\n\
                Answer in the same language as the question.\n\
                If the user attaches an image, analyze it carefully and reference what you see.\n\
                Past context:\n{}\n\
                Rules:\n\
                - Be concise but complete\n\
                - Format code blocks with ```language\n\
                - Answer in the same language as the question",
                if mem_context.is_empty() { "None".to_string() } else { mem_context }
            )
        } else {
            format!(
                "You are HyperAgent's ASK mode — a helpful coding assistant.\n\
                Answer the user's question about the codebase concisely and accurately.\n\
                Relevant files from the project:\n{}\n\
                Past context about this project:\n{}\n\
                Rules:\n\
                - Be concise but complete\n\
                - Reference specific file paths and function names when relevant\n\
                - If the answer requires code changes, say so but DO NOT propose edits\n\
                - Format code blocks with ```language\n\
                - Answer in the same language as the question",
                file_context,
                if mem_context.is_empty() { "None".to_string() } else { mem_context }
            )
        };

        // Add MCP tools context if available
        let mcp_tool_defs: Vec<crate::llm::provider::ToolDefinition> = if let Some(ref mcp) = self.mcp {
            let defs = mcp.to_tool_definitions().await;
            if !defs.is_empty() {
                let mut tools_note = String::from("\n\nYou have access to MCP tools listed below. Use them when needed.");
                for def in &defs {
                    let desc = if def.function.description.len() > 80 {
                        format!("{}...", &def.function.description[..77])
                    } else {
                        def.function.description.clone()
                    };
                    tools_note.push_str(&format!("\n  - {}: {desc}", def.function.name));
                }
                let mut sp = system_prompt.clone();
                sp.push_str(&tools_note);
                defs
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let system_prompt_final = if mcp_tool_defs.is_empty() {
            system_prompt
        } else {
            let mut sp = system_prompt;
            sp.push_str("\n\nYou have access to MCP tools listed below. Use them when needed to gather information or perform actions.");
            for def in &mcp_tool_defs {
                let desc = if def.function.description.len() > 80 {
                    format!("{}...", &def.function.description[..77])
                } else {
                    def.function.description.clone()
                };
                sp.push_str(&format!("\n  - {}: {desc}", def.function.name));
            }
            sp
        };

        let mut messages = vec![
            Message::text("system", system_prompt_final),
        ];

        // Inject conversation history
        for (prev_user, prev_assistant) in &self.conversation_history {
            messages.push(Message::text("user", prev_user.clone()));
            messages.push(Message::text("assistant", prev_assistant.clone()));
        }

        // Build user message with optional image attachment
        let user_msg = if let Some(ref img_path) = self.pending_image {
            // Resolve path relative to project root or cwd
            let full_path = if std::path::Path::new(img_path).is_absolute() {
                std::path::PathBuf::from(img_path)
            } else {
                self.root.join(img_path)
            };

            let image_data = match std::fs::read(&full_path) {
                Ok(d) => d,
                Err(e) => return Ok(RunResult {
                    response_text: format!("Failed to load image '{}': {e}", full_path.display()),
                    ..Default::default()
                }),
            };

            // Detect MIME type from magic bytes
            let mime = if image_data.len() > 8 {
                let header = &image_data[..image_data.len().min(12)];
                if header.starts_with(b"\x89PNG") { "image/png" }
                else if header.starts_with(b"\xff\xd8\xff") { "image/jpeg" }
                else if header.starts_with(b"GIF8") { "image/gif" }
                else if header.starts_with(b"RIFF") && header.len() > 8
                    && &header[8..12] == b"WEBP" { "image/webp" }
                else { "image/png" }
            } else { "image/png" };

            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&image_data);
            let image_url = format!("data:{};base64,{}", mime, b64);

            println!("   🖼️  Image loaded: {} ({:.1} KB, {})", full_path.display(), image_data.len() as f64 / 1024.0, mime);

            Message {
                role: "user".to_string(),
                parts: vec![
                    ContentPart::Text {
                        r#type: "text".to_string(),
                        text: prompt.to_string(),
                    },
                    ContentPart::ImageUrl {
                        r#type: "image_url".to_string(),
                        image_url: crate::llm::provider::ImageUrl { url: image_url },
                    },
                ],
            }
        } else {
            Message::text("user", prompt.to_string())
        };
        messages.push(user_msg);

        // Max 3 tool call rounds
        let max_rounds = if mcp_tool_defs.is_empty() { 1 } else { 3 };
        let has_tools = !mcp_tool_defs.is_empty();
        let mut response_text = String::new();

        for round in 0..max_rounds {
            print!("   📝 (round {}/{}) ", round + 1, max_rounds);
            std::io::Write::flush(&mut std::io::stdout()).ok();

            let response_msg = if has_tools {
                match self.provider.chat_with_tools(
                    messages.clone(),
                    Some(mcp_tool_defs.clone()),
                ).await {
                    Ok(msg) => msg,
                    Err(_e) => {
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
                    }
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
                    let tool_name = tool_call.function.name.clone();
                    let args: serde_json::Value = match serde_json::from_str(&tool_call.function.arguments) {
                        Ok(v) => v,
                        Err(_) => serde_json::json!({}),
                    };

                    let result_value = match &self.mcp {
                        Some(mcp) => {
                            // Safety gate: check tool before executing
                            let safety = crate::security::check_tool_safety(&tool_name, &args);
                            if !crate::security::confirm_dangerous_action(&safety, true) {
                                continue; // Skip blocked tool, go to next
                            }
                            mcp.call_tool(&tool_name, args).await
                        }
                        None => Err(anyhow::anyhow!("MCP not available")),
                    };

                    match result_value {
                        Ok(value) => {
                            let result_str = serde_json::to_string_pretty(&value)
                                .unwrap_or_else(|_| "{}".to_string());
                            messages.push(Message::text("tool", result_str));
                            println!("   🔧 Called MCP tool '{}'", tool_name);
                        }
                        Err(e) => {
                            messages.push(Message::text("tool", format!("Error: {e}")));
                            eprintln!("   ⚠️  MCP tool '{}' failed: {e}", tool_name);
                        }
                    }
                }
            } else {
                response_text = response_msg.text_content();
                break;
            }
        }

        // Scrub any leaked memory context from the response
        let mem_ctx = self.load_memory_context(&response_text).await;
        response_text = crate::memory::scrub_response(&response_text, &mem_ctx);

        if !response_text.is_empty() {
            println!();
        }

        // Reset pending image so it doesn't leak to next turn
        self.pending_image = None;

        self.record_memory(
            &format!("Ask response for '{}': {}", &prompt[..prompt.char_indices().nth(100).map(|(i,_)|i).unwrap_or(prompt.len())],
                     &response_text[..response_text.char_indices().nth(200).map(|(i,_)|i).unwrap_or(response_text.len())]),
            MemoryType::Decision,
        ).await;

        self.fire_hook(HookEvent::PostRun, "ask complete").await;

        if let Some(ref mem) = self.memory {
            if let Ok(uncon) = mem.store_ref().get_unconsolidated(20) {
                if !uncon.is_empty() {
                    let ids: Vec<String> = uncon.iter().map(|e| e.id.clone()).collect();
                    let _ = mem.store_ref().mark_consolidated(&ids);
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



    /// Execute a built-in tool and return the result as a string
    async fn execute_builtin_tool(&self, name: &str, args: &serde_json::Value) -> String {
        match name {
            "web_search" => {
                let query = args["query"].as_str().unwrap_or("");
                if query.is_empty() {
                    return "Error: 'query' parameter is required".to_string();
                }
                match crate::web_search::search(query, 6).await {
                    Ok(results) => {
                        let mut output = String::new();
                        for (i, r) in results.iter().enumerate() {
                            output.push_str(&format!("{}. **{}**\n   URL: {}\n   {}\n\n", i + 1, r.title, r.url, r.snippet));
                        }
                        if output.is_empty() {
                            output = format!("No search results found for: {query}");
                        }
                        output
                    }
                    Err(e) => format!("Error: web search failed: {e}"),
                }
            }
            "read_file" => {
                let file_path = args["path"].as_str().unwrap_or("");
                if file_path.is_empty() {
                    return "Error: 'path' parameter is required".to_string();
                }
                let full_path = self.root.join(file_path);
                match std::fs::read_to_string(&full_path) {
                    Ok(content) => {
                        let lines: Vec<&str> = content.lines().collect();
                        let total = lines.len();
                        let show = lines.iter().take(80).copied().collect::<Vec<_>>();
                        let mut output = format!("📄 `{}` ({} lines):\n```\n", full_path.display(), total);
                        for (i, line) in show.iter().enumerate() {
                            output.push_str(&format!("{:>4}| {}\n", i + 1, line));
                        }
                        if total > 80 {
                            output.push_str(&format!("... ({} more lines)\n", total - 80));
                        }
                        output.push_str("```\n");
                        output
                    }
                    Err(e) => format!("Error: cannot read '{}': {e}", full_path.display()),
                }
            }
            "run_bash" => {
                let command = args["command"].as_str().unwrap_or("");
                if command.is_empty() {
                    return "Error: 'command' parameter is required".to_string();
                }
                if self.sandbox_enabled {
                    let config = crate::sandbox::SandboxConfig {
                        project_root: Some(self.root.to_string_lossy().to_string()),
                        network_enabled: false,
                        timeout_secs: 60,
                        ..Default::default()
                    };
                    let sandbox = crate::sandbox::Sandbox::new(config);
                    if !sandbox.is_available() {
                        return "⚠️ Sandbox mode enabled but Docker is not available. Install Docker Desktop or disable sandbox mode with `--no-sandbox`.".to_string();
                    }
                    match sandbox.run(command, 60).await {
                        Ok(result) => {
                            let mut output = String::new();
                            if !result.stdout.is_empty() {
                                output.push_str(result.stdout.trim());
                            }
                            if !result.stderr.is_empty() {
                                if !output.is_empty() { output.push('\n'); }
                                output.push_str(&format!("[stderr]\n{}", result.stderr.trim()));
                            }
                            if result.exit_code != 0 {
                                output.push_str(&format!("\n[exit code: {}]", result.exit_code));
                            }
                            if output.is_empty() {
                                output = "(no output)".to_string();
                            }
                            if output.len() > 8000 {
                                output = self.paginate_output(&output, 8000, "run_bash");
                            }
                            output
                        }
                        Err(e) => format!("⚠️ Sandbox execution failed: {e}"),
                    }
                } else {
                    match std::process::Command::new("sh")
                        .args(["-c", command])
                        .current_dir(&self.root)
                        .output()
                {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let mut result = String::new();
                        if !stdout.is_empty() {
                            result.push_str(stdout.trim());
                        }
                        if !stderr.is_empty() {
                            if !result.is_empty() { result.push('\n'); }
                            result.push_str(&format!("[stderr]\n{}", stderr.trim()));
                        }
                        if !output.status.success() {
                            result.push_str(&format!("\n[exit code: {}]", output.status.code().unwrap_or(-1)));
                        }
                        if result.is_empty() {
                            result = "(no output)".to_string();
                        }
                        if result.len() > 8000 {
                            result = self.paginate_output(&result, 8000, "run_bash");
                        }
                        result
                    }
                    Err(e) => format!("Error: command execution failed: {e}"),
                }
            }
            }
            "knowledge_search" => {
                let query = args["query"].as_str().unwrap_or("");
                if query.is_empty() {
                    return "Error: 'query' parameter is required".to_string();
                }
                match &self.knowledge_base {
                    Some(kb) => match kb.search(query, 6) {
                        Ok(results) => {
                            if results.is_empty() {
                                format!("No knowledge base results for: {query}")
                            } else {
                                let mut output = String::new();
                                for chunk in &results {
                                    let preview = if chunk.content.len() > 300 {
                                        format!("{}...", &chunk.content[..297])
                                    } else {
                                        chunk.content.clone()
                                    };
                                    output.push_str(&format!("📄 `{}` (score: {:.2})\n{}\n\n",
                                        chunk.file.display(), chunk.score, preview));
                                }
                                output
                            }
                        }
                        Err(e) => format!("Error: knowledge search failed: {e}"),
                    },
                    None => "Knowledge base not available. Use 'hyper knowledge build' to index project documentation.".to_string(),
                }
            }
            "memory_search" => {
                let query = args["query"].as_str().unwrap_or("");
                if query.is_empty() {
                    return "Error: 'query' parameter is required".to_string();
                }
                let limit = args["limit"].as_u64().unwrap_or(5) as usize;
                match &self.memory {
                    Some(mem) => match mem.recall_fused(query, limit) {
                        Ok(results) => {
                            if results.is_empty() {
                                format!("No memories found for: {query}")
                            } else {
                                let mut output = String::from("📍 Memory search results:\n");
                                for sm in &results {
                                    let ago = chrono::Utc::now()
                                        .signed_duration_since(sm.entry.created_at);
                                    let ago_str = if ago.num_minutes() < 60 {
                                        format!("{}m ago", ago.num_minutes())
                                    } else if ago.num_hours() < 24 {
                                        format!("{}h ago", ago.num_hours())
                                    } else {
                                        format!("{}d ago", ago.num_days())
                                    };
                                    let pct = (sm.total_score.min(10.0) / 10.0 * 100.0) as u32;
                                    output.push_str(&format!(
                                        "  [{ago_str}] ({track}/{layer}) [{pct}%] {content}\n",
                                        track = sm.entry.track,
                                        layer = sm.entry.layer,
                                        content = sm.entry.content,
                                    ));
                                }
                                output
                            }
                        }
                        Err(e) => format!("Error: memory search failed: {e}"),
                    },
                    None => "Memory system is not available in this session.".to_string(),
                }
            }
            "memory_add" => {
                let content = args["content"].as_str().unwrap_or("");
                if content.is_empty() {
                    return "Error: 'content' parameter is required".to_string();
                }
                let mem_type = args["memory_type"].as_str().unwrap_or("codebase_fact");
                let mem_type_parsed = match mem_type {
                    "user_preference" => MemoryType::UserPreference,
                    "codebase_fact" => MemoryType::CodebaseFact,
                    "decision" => MemoryType::Decision,
                    "bug_fix" => MemoryType::BugFix,
                    "learned" => MemoryType::Learned,
                    "ephemeral" => MemoryType::Ephemeral,
                    _ => MemoryType::CodebaseFact,
                };
                match &self.memory {
                    Some(mem) => match mem.remember(content, mem_type_parsed) {
                        Ok(id) => {
                            format!("✅ Memory saved (id={}, type={})", id, mem_type)
                        }
                        Err(e) => format!("Error: failed to save memory: {e}"),
                    },
                    None => "Memory system is not available in this session.".to_string(),
                }
            }
            "memory_remove" => {
                let id = args["id"].as_str().unwrap_or("");
                if id.is_empty() {
                    return "Error: 'id' parameter is required".to_string();
                }
                match &self.memory {
                    Some(mem) => match mem.store_ref().delete(id) {
                        Ok(_) => format!("🗑️ Memory '{}' removed", id),
                        Err(e) => format!("Error: failed to remove memory: {e}"),
                    },
                    None => "Memory system is not available in this session.".to_string(),
                }
            }
            "python_repl" => {
                let code = args["code"].as_str().unwrap_or("");
                if code.is_empty() {
                    return "Error: 'code' parameter is required".to_string();
                }
                // Find the REPL Python script relative to binary or config
                let script_dir = if let Some(home) = dirs_next::home_dir() {
                    home.join(".hyper").join("scripts")
                } else {
                    self.root.join("scripts")
                };
                let repl_script = script_dir.join("repl_python.py");
                let fallback_script = self.root.join("scripts").join("repl_python.py");

                let script_path = if repl_script.exists() { repl_script } else { fallback_script };

                if !script_path.exists() {
                    return format!("Python REPL script not found at {}. Create scripts/repl_python.py or install ~/.hyper/scripts/repl_python.py", script_path.display());
                }

                match std::process::Command::new("python3")
                    .arg(script_path.to_str().unwrap_or(""))
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                {
                    Ok(mut child) => {
                        use std::io::Write;
                        if let Some(ref mut stdin) = child.stdin {
                            let _ = stdin.write_all(code.as_bytes());
                        }
                        match child.wait_with_output() {
                            Ok(output) => {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let stderr = String::from_utf8_lossy(&output.stderr);

                                // Try to parse JSON result from the REPL script
                                let mut result = String::new();
                                if let Ok(json_result) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                                    if let Some(out) = json_result["stdout"].as_str() {
                                        if !out.is_empty() {
                                            result.push_str(out.trim());
                                        }
                                    }
                                    if let Some(err) = json_result["stderr"].as_str() {
                                        if !err.is_empty() {
                                            if !result.is_empty() { result.push('\n'); }
                                            result.push_str(&format!("[stderr]\n{}", err.trim()));
                                        }
                                    }
                                } else {
                                    // Raw output fallback
                                    if !stdout.trim().is_empty() {
                                        result.push_str(stdout.trim());
                                    }
                                    if !stderr.trim().is_empty() {
                                        if !result.is_empty() { result.push('\n'); }
                                        result.push_str(&format!("[stderr]\n{}", stderr.trim()));
                                    }
                                }

                                if !output.status.success() && result.is_empty() {
                                    result = format!("[exit code: {}]", output.status.code().unwrap_or(-1));
                                }
                                if result.is_empty() {
                                    result = "(no output)".to_string();
                                }
                                if result.len() > 8000 {
                                    result = self.paginate_output(&result, 8000, "python_repl");
                                }
                                result
                            }
                            Err(e) => format!("Error: failed to read Python output: {e}"),
                        }
                    }
                    Err(e) => format!("Error: failed to launch Python3: {e}. Is Python3 installed?"),
                }
            }
            "read_document" => {
                let file_path = args["path"].as_str().unwrap_or("");
                if file_path.is_empty() {
                    return "Error: 'path' parameter is required".to_string();
                }
                let full_path = if std::path::Path::new(file_path).is_absolute() {
                    std::path::PathBuf::from(file_path)
                } else {
                    self.root.join(file_path)
                };

                if !full_path.exists() {
                    return format!("Error: file not found: {}", full_path.display());
                }

                // Find the parse_document script
                let script_dir = if let Some(home) = dirs_next::home_dir() {
                    home.join(".hyper").join("scripts")
                } else {
                    self.root.join("scripts")
                };
                let doc_script = script_dir.join("parse_document.py");
                let fallback_script = self.root.join("scripts").join("parse_document.py");
                let script_path = if doc_script.exists() { doc_script } else { fallback_script };

                if !script_path.exists() {
                    return format!("Document parser script not found at {}. Create scripts/parse_document.py", script_path.display());
                }

                match std::process::Command::new("python3")
                    .arg(script_path.to_str().unwrap_or(""))
                    .arg(full_path.to_str().unwrap_or(""))
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output()
                {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);

                        // Parse JSON result
                        if let Ok(json_result) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                            if let Some(content) = json_result["content"].as_str() {
                                let engine = json_result["engine"].as_str().unwrap_or("unknown");
                                let fmt = json_result["format"].as_str().unwrap_or("unknown");
                                let pages = json_result["pages"].as_i64().unwrap_or(0);
                                let truncated = json_result["truncated"].as_bool().unwrap_or(false);
                                let mut result = format!(
                                    "📄 Parsed as {} (engine: {}, ~{} lines{})\n\n{}",
                                    fmt, engine, pages,
                                    if truncated { ", truncated" } else { "" },
                                    content
                                );
                                if result.len() > 16000 {
                                    result = self.paginate_output(&result, 16000, "read_document");
                                }
                                result
                            } else if let Some(error) = json_result["error"].as_str() {
                                format!("Error: {error}")
                            } else {
                                format!("Unexpected result from document parser: {}", stdout.trim())
                            }
                        } else if !stderr.trim().is_empty() {
                            format!("Error: {}\n{}", stderr.trim(), stdout.trim())
                        } else {
                            format!("Document content:\n{}", stdout.trim())
                        }
                    }
                    Err(e) => format!("Error: failed to launch document parser: {e}"),
                }
            }
            "browser" => {
                let command = args["command"].as_str().unwrap_or("");
                if command.is_empty() {
                    return "Error: 'command' parameter is required".to_string();
                }

                // Find the browser script
                let script_dir = if let Some(home) = dirs_next::home_dir() {
                    home.join(".hyper").join("scripts")
                } else {
                    self.root.join("scripts")
                };
                let browser_script = script_dir.join("browser_tool.py");
                let fallback_script = self.root.join("scripts").join("browser_tool.py");
                let script_path = if browser_script.exists() { browser_script } else { fallback_script };

                if !script_path.exists() {
                    return format!("Browser script not found at {}. Create scripts/browser_tool.py", script_path.display());
                }

                let mut args_vec = vec![
                    script_path.to_str().unwrap_or(""),
                    command,
                ];

                match command {
                    "open" => {
                        let url = args["url"].as_str().unwrap_or("about:blank");
                        args_vec.push(url);
                    }
                    "screenshot" => {
                        // No extra args needed
                    }
                    "source" => {
                        // No extra args needed
                    }
                    "eval" => {
                        let js = args["js"].as_str().unwrap_or("");
                        args_vec.push(js);
                    }
                    "close" => {}
                    _ => return format!("Error: unknown browser command '{}'. Use: open, screenshot, source, eval, close", command),
                }

                match std::process::Command::new("python3")
                    .args(&args_vec)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output()
                {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);

                        if let Ok(json_result) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                            if let Some(error) = json_result["error"].as_str() {
                                format!("Error: {error}")
                            } else if let Some(content) = json_result["content"].as_str() {
                                let mut result = format!("📄 Page content:\n\n{}", content);
                                if result.len() > 16000 {
                                    result = self.paginate_output(&result, 16000, "browser");
                                }
                                result
                            } else if let Some(result_val) = json_result["result"].as_str() {
                                format!("Result: {}", result_val)
                            } else if command == "screenshot" {
                                format!("📸 Screenshot saved. Use vision model to analyze it. Path: {}", json_result["path"].as_str().unwrap_or("unknown"))
                            } else if command == "close" {
                                "🛑 Browser closed.".to_string()
                            } else if let Some(html) = json_result["html"].as_str() {
                                let mut result = format!("🌐 Page HTML:\n\n{}", html);
                                if result.len() > 16000 {
                                    result = self.paginate_output(&result, 16000, "browser");
                                }
                                result
                            } else {
                                format!("Result: {}", stdout.trim())
                            }
                        } else if !stderr.trim().is_empty() {
                            format!("Error: {}\n{}", stderr.trim(), stdout.trim())
                        } else {
                            stdout.trim().to_string()
                        }
                    }
                    Err(e) => format!("Error: failed to launch browser tool: {e}. Is Python3 installed?"),
                }
            }
            "platform_setup" => {
                let action = args["action"].as_str().unwrap_or("check");

                // Find the platform check script
                let script_dir = if let Some(home) = dirs_next::home_dir() {
                    home.join(".hyper").join("scripts")
                } else {
                    self.root.join("scripts")
                };
                let check_script = script_dir.join("platform_check.py");
                let fallback_script = self.root.join("scripts").join("platform_check.py");
                let script_path = if check_script.exists() { check_script } else { fallback_script };

                match action {
                    "check" => {
                        if !script_path.exists() {
                            return format!("Platform check script not found at {}", script_path.display());
                        }
                        match std::process::Command::new("python3")
                            .arg(script_path.to_str().unwrap_or(""))
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .output()
                        {
                            Ok(output) => {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                if let Ok(data) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                                    let os = data["os"].as_str().unwrap_or("unknown");
                                    let mut report = format!("🖥️  Platform: {} ({})\n", os, data["arch"].as_str().unwrap_or(""));
                                    report.push_str(&format!("   Python: {} ({})\n",
                                        if data["python"]["available"].as_bool().unwrap_or(false) { "✅" } else { "❌" },
                                        data["python"]["version"].as_str().unwrap_or("not found")));
                                    report.push_str(&format!("   Chrome: {}\n",
                                        if data["chrome"]["available"].as_bool().unwrap_or(false) { "✅" } else { "❌" }));
                                    report.push_str(&format!("   pdftotext: {}\n",
                                        if data["pdftotext"]["available"].as_bool().unwrap_or(false) { "✅" } else { "❌" }));

                                    let pkgs = &data["python"]["packages"];
                                    report.push_str("\n   Python packages:\n");
                                    for (pkg, ok) in pkgs.as_object().unwrap_or(&serde_json::Map::new()) {
                                        report.push_str(&format!("     {}: {}\n", if *ok == serde_json::Value::Bool(true) { "✅" } else { "❌" }, pkg));
                                    }

                                    if let Some(missing) = data["tools_missing"].as_array() {
                                        if !missing.is_empty() {
                                            report.push_str(&format!("\n   Missing ({})", missing.len()));
                                        }
                                    }

                                    if let Some(guide) = data["install_guide"].as_str() {
                                        report.push_str(&format!("\n\n📦 Install guide:\n{}", guide));
                                    }
                                    report
                                } else {
                                    format!("Platform info:\n{}", stdout.trim())
                                }
                            }
                            Err(e) => format!("Error: failed to check platform: {e}"),
                        }
                    }
                    "guide" => {
                        if !script_path.exists() {
                            return format!("Platform check script not found at {}", script_path.display());
                        }
                        match std::process::Command::new("python3")
                            .arg(script_path.to_str().unwrap_or(""))
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .output()
                        {
                            Ok(output) => {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                if let Ok(data) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                                    if let Some(guide) = data["install_guide"].as_str() {
                                        format!("📦 Install guide:\n{}", guide)
                                    } else {
                                        "No install guide available.".to_string()
                                    }
                                } else {
                                    stdout.trim().to_string()
                                }
                            }
                            Err(e) => format!("Error: {e}"),
                        }
                    }
                    "install_python_pkg" => {
                        let pkg = args["package"].as_str().unwrap_or("");
                        if pkg.is_empty() {
                            return "Error: 'package' parameter required for install_python_pkg".to_string();
                        }
                        match std::process::Command::new("pip3")
                            .args(["install", pkg])
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .output()
                        {
                            Ok(output) => {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                let success = output.status.success();
                                if success {
                                    format!("✅ Installed {}.", pkg)
                                } else {
                                    // Try pip (no 3) as fallback on some systems
                                    match std::process::Command::new("pip")
                                        .args(["install", pkg])
                                        .stdout(std::process::Stdio::piped())
                                        .stderr(std::process::Stdio::piped())
                                        .output()
                                    {
                                        Ok(retry) => {
                                            if retry.status.success() {
                                                format!("✅ Installed {}.", pkg)
                                            } else {
                                                let err = String::from_utf8_lossy(&retry.stderr);
                                                format!("Error: failed to install {}: {}\nTry: pip install {}", pkg, err.lines().next().unwrap_or("unknown"), pkg)
                                            }
                                        }
                                        Err(e) => format!("Error: pip not found: {e}"),
                                    }
                                }
                            }
                            Err(e) => format!("Error: failed to run pip3: {e}"),
                        }
                    }
                    _ => format!("Error: unknown action '{}'. Use: check, guide, install_python_pkg", action),
                }
            }
            "analyze_image" => {
                let path = args["path"].as_str().unwrap_or("");
                if path.is_empty() {
                    return "Error: 'path' parameter is required".to_string();
                }
                let full_path = if std::path::Path::new(path).is_absolute() {
                    std::path::PathBuf::from(path)
                } else {
                    self.root.join(path)
                };

                if !full_path.exists() {
                    return format!("Error: image not found: {}", full_path.display());
                }

                let img_data = match std::fs::read(&full_path) {
                    Ok(d) => d,
                    Err(e) => return format!("Error: failed to read image: {e}"),
                };

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
                    else { "image/png" }
                } else { "image/png" };

                let image_url = format!("data:{};base64,{}", mime, b64);
                let question = args["prompt"].as_str().unwrap_or("Describe this image in detail. What do you see? Read any text visible in the image.");

                let messages = vec![
                    Message::text("system", "You are HyperAgent's vision analysis tool. Analyze the provided image carefully and describe what you see in detail. If there is text, read it. Identify objects, people, scenes, colors, and any notable elements."),
                    Message {
                        role: "user".to_string(),
                        parts: vec![
                            ContentPart::Text { r#type: "text".to_string(), text: question.to_string() },
                            ContentPart::ImageUrl {
                                r#type: "image_url".to_string(),
                                image_url: crate::llm::provider::ImageUrl { url: image_url.clone() },
                            },
                        ],
                    },
                ];

                match self.provider.chat(messages).await {
                    Ok(analysis) => {
                        format!("🔍 Image analysis ({:.1} KB):\n\n{}", img_data.len() as f64 / 1024.0, analysis.trim())
                    }
                    Err(e) => {
                        format!("Error: vision analysis failed: {e}\n\nThe provider may not support vision. Try using a vision-capable model (e.g. GPT-4o, Claude Sonnet).")
                    }
                }
            }
            "desktop" => {
                let action = args["action"].as_str().unwrap_or("");
                match action {
                    "screenshot" => {
                        let tmp = std::env::temp_dir().join(format!("hyper_screenshot_{}.png", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
                        match crate::computer_use_cross::ComputerUse::screenshot(tmp.to_str().unwrap_or("/tmp/hyper_screenshot.png")) {
                            Ok(_) => format!("Screenshot saved. The desktop is visible. Path: {}", tmp.display()),
                            Err(e) => format!("Error: screenshot failed: {e}"),
                        }
                    }
                    "click" => {
                        let x = args["x"].as_i64().unwrap_or(0) as i32;
                        let y = args["y"].as_i64().unwrap_or(0) as i32;
                        match crate::computer_use_cross::ComputerUse::click(x, y) {
                            Ok(_) => format!("Clicked at ({x}, {y})"),
                            Err(e) => format!("Error: click failed: {e}"),
                        }
                    }
                    "type" => {
                        let text = args["text"].as_str().unwrap_or("");
                        if text.is_empty() { return "Error: 'text' parameter required for type action".to_string(); }
                        match crate::computer_use_cross::ComputerUse::type_text(text) {
                            Ok(_) => format!("Typed text at cursor"),
                            Err(e) => format!("Error: type failed: {e}"),
                        }
                    }
                    "key" => {
                        let name = args["name"].as_str().unwrap_or("");
                        if name.is_empty() { return "Error: 'name' parameter required for key action".to_string(); }
                        match crate::computer_use_cross::ComputerUse::key_press(name) {
                            Ok(_) => format!("Pressed key: {name}"),
                            Err(e) => format!("Error: key press failed: {e}"),
                        }
                    }
                    _ => format!("Error: unknown desktop action '{action}'. Use: screenshot, click, type, key"),
                }
            }
            "task" => {
                let prompt = args["prompt"].as_str().unwrap_or("");
                if prompt.is_empty() {
                    return "Error: 'prompt' parameter is required".to_string();
                }
                let current = self.task_depth.fetch_add(1, Ordering::Relaxed);
                if current >= MAX_TASK_DEPTH - 1 {
                    self.task_depth.fetch_sub(1, Ordering::Relaxed);
                    return format!("Error: sub-agent task depth exceeded (max {MAX_TASK_DEPTH}). Cannot delegate further. Complete the task yourself.");
                }
                let messages = vec![
                    Message::text("system", "You are a helpful sub-agent. Complete the task given below. Be concise and thorough. Do NOT call the task tool yourself — you are already a sub-agent."),
                    Message::text("user", prompt),
                ];
                match self.provider.chat(messages).await {
                    Ok(response) => {
                        self.task_depth.fetch_sub(1, Ordering::Relaxed);
                        response
                    }
                    Err(e) => {
                        self.task_depth.fetch_sub(1, Ordering::Relaxed);
                        format!("Error: sub-agent task failed: {e}")
                    }
                }
            }
            _ => format!("Error: unknown tool '{name}'"),
        }
    }

    /// Check if a prompt looks like a coding task that needs the code pipeline
    fn is_coding_task(prompt: &str) -> bool {
        let coding_keywords = [
            "fix ", "bug", "implement", "add ", "create ", "refactor",
            "edit ", "update ", "change ", "delete ", "remove ", "modify",
            "write a function", "write code", "test ", "compile", "build ",
            "cargo", "npm ", " yarn", "pip ", "src/", ".rs", ".py", ".ts",
            ".js", ".tsx", ".jsx", "fn ", "def ", "class ", "struct ",
            "trait ", "impl ", "pub ", "let ", "const ", "import ",
            "error[E", "clippy", "linter", "lint ",
        ];
        let lower = prompt.to_lowercase();
        coding_keywords.iter().any(|kw| lower.contains(kw))
    }

    /// Run a general-purpose tool-use loop for non-coding tasks
    /// Supports: web_search, read_file, run_bash, knowledge_search, plus MCP tools
    async fn run_general_mode(
        &mut self,
        prompt: &str,
        _relevant_files: &[FileContext],
        start: Instant,
        total_memories: usize,
    ) -> Result<RunResult> {
        println!("   🌐 Handling with general-purpose tools...");

        let mem_context = self.load_memory_context(prompt).await;

        // System prompt with tool descriptions
        let system_prompt = format!(
            "You are HyperAgent — a versatile AI assistant capable of handling any task.\n\n\
            You have access to built-in tools:\n\
            - **web_search(query)**: Search the web for current information (news, docs, facts, research)\n\
            - **read_file(path)**: Read a file from the project directory (relative path)\n\
            - **run_bash(command)**: Execute a bash command in the project root\n\
            - **memory_search(query, limit?)**: Search persistent memory for relevant information\\n\\\n            - **memory_add(content, memory_type?)**: Save a fact to persistent memory\\n\
            General guidelines:\n\
            - Use tools proactively when you need more information\n\
            - Be concise but thorough in your answers\n\
            - Format code blocks with ```language\n\
            - Answer in the same language as the user's question\n\
            - If the user asks for code changes, first read the relevant files, understand them, then implement\n\n\
            Past context:\n{}\n\
            Project root: {}",
            if mem_context.is_empty() { "None".to_string() } else { mem_context },
            self.root.display()
        );

        // Build tool definitions (built-in + MCP if available)
        let mut tool_defs = crate::agent::tools::builtin_tool_definitions(&self.mode, self.memory.is_some());

        // Add MCP tools if available
        let mcp_tool_defs: Vec<ToolDefinition> = if let Some(ref mcp) = self.mcp {
            let defs = mcp.to_tool_definitions().await;
            if !defs.is_empty() {
                for def in &defs {
                    println!("   🔌 MCP tool available: {}", def.function.name);
                }
            }
            defs
        } else {
            Vec::new()
        };
        tool_defs.extend(mcp_tool_defs.clone());

        let mut messages = vec![
            Message::text("system", system_prompt),
        ];

        // Inject conversation history
        for (prev_user, prev_assistant) in &self.conversation_history {
            messages.push(Message::text("user", prev_user.clone()));
            messages.push(Message::text("assistant", prev_assistant.clone()));
        }

        // Build user message (with optional image)
        let user_msg = if let Some(ref img_path) = self.pending_image {
            Message {
                role: "user".to_string(),
                parts: vec![
                    ContentPart::Text { r#type: "text".to_string(), text: prompt.to_string() },
                    ContentPart::ImageUrl {
                        r#type: "image_url".to_string(),
                        image_url: crate::llm::provider::ImageUrl {
                            url: format!("data:image/png;base64,{}", img_path),
                        },
                    },
                ],
            }
        } else {
            Message::text("user", prompt.to_string())
        };
        messages.push(user_msg);

        let max_rounds = if mcp_tool_defs.is_empty() { 5 } else { 8 };
        let has_tools = !tool_defs.is_empty();
        let mut response_text = String::new();

        for round in 0..max_rounds {
            use std::io::{Write, stdout};

            if round > 0 {
                print!("   🔄 Round {}/{}\n", round + 1, max_rounds);
                stdout().flush().ok();
            }

            // First round: try streaming for fast Q&A; subsequent rounds: batch with tools
            let response_msg = if round == 0 && !has_tools {
                // Simple Q&A — try streaming
                match self.chat_stream_with_failover(messages.clone()).await {
                    Ok(stream) => {
                        let mut rx = stream.into_receiver();
                        let mut full = String::new();
                        while let Some(chunk) = rx.recv().await {
                            print!("{chunk}");
                            stdout().flush().ok();
                            full.push_str(&chunk);
                        }
                        println!();
                        crate::llm::provider::ChatResponseMessage {
                            content: Some(full),
                            tool_calls: vec![],
                        }
                    }
                    Err(_) => {
                        // Fallback to batch
                        match self.chat_with_failover(messages.clone()).await {
                            Ok(r) => {
                                print!("{r}");
                                stdout().flush().ok();
                                println!();
                                crate::llm::provider::ChatResponseMessage {
                                    content: Some(r),
                                    tool_calls: vec![],
                                }
                            }
                            Err(e) => {
                                eprintln!("\n   Error: {e}");
                                crate::llm::provider::ChatResponseMessage {
                                    content: None,
                                    tool_calls: vec![],
                                }
                            }
                        }
                    }
                }
            } else {
                // Batch with tools (supports tool calling)
                let provider = self.provider.clone();

                match provider.chat_with_tools(messages.clone(), Some(tool_defs.clone())).await {
                    Ok(msg) => {
                        // Print text content if present
                        if let Some(ref text) = msg.content {
                            if msg.tool_calls.is_empty() {
                                print!("{text}");
                                stdout().flush().ok();
                            }
                        }
                        msg
                    }
                    Err(e) => {
                        if round == 0 {
                            match self.chat_with_failover(messages.clone()).await {
                                Ok(r) => {
                                    print!("{r}");
                                    stdout().flush().ok();
                                    println!();
                                    crate::llm::provider::ChatResponseMessage {
                                        content: Some(r),
                                        tool_calls: vec![],
                                    }
                                }
                                Err(e2) => {
                                    eprintln!("\n   Error: {e2}");
                                    crate::llm::provider::ChatResponseMessage {
                                        content: None,
                                        tool_calls: vec![],
                                    }
                                }
                            }
                        } else {
                            eprintln!("\n   Error: {e}");
                            break;
                        }
                    }
                }
            };

            // Process tool calls
            if !response_msg.tool_calls.is_empty() {
                for tool_call in &response_msg.tool_calls {
                    let tool_name = tool_call.function.name.clone();
                    let args: serde_json::Value = match serde_json::from_str(&tool_call.function.arguments) {
                        Ok(v) => v,
                        Err(_) => serde_json::json!({}),
                    };

                    // Check if it's a plugin tool, MCP tool, or built-in tool
                    let is_plugin = self.plugin_manager.as_ref().map_or(false, |pm| pm.list_tools().contains(&tool_name));
                    let is_mcp = !is_plugin && mcp_tool_defs.iter().any(|d| d.function.name == tool_name);
                    let result_str = if is_plugin {
                        // Dispatch to plugin tool
                        match &self.plugin_manager {
                            Some(pm) => match pm.call_tool(&tool_name, args).await {
                                Ok(value) => serde_json::to_string_pretty(&value)
                                    .unwrap_or_else(|_| "{}".to_string()),
                                Err(e) => format!("⚠️ Plugin '{tool_name}' error: {e}"),
                            },
                            None => format!("⚠️ Plugin '{tool_name}' not loaded"),
                        }
                    } else if is_mcp {
                        // Safety gate: check tool before executing
                        let safety = crate::security::check_tool_safety(&tool_name, &args);
                        if !crate::security::confirm_dangerous_action(&safety, self.mode == "task") {
                            format!("🛑 Tool '{}' blocked by safety policy", tool_name)
                        } else {
                            match &self.mcp {
                                Some(mcp) => match mcp.call_tool(&tool_name, args).await {
                                    Ok(value) => serde_json::to_string_pretty(&value)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                    Err(e) => format!("Error: {e}"),
                                },
                                None => "Error: MCP not available".to_string(),
                            }
                        }
                    } else {
                        self.execute_builtin_tool(&tool_name, &args).await
                    };

                    // Paginate very long results — save to temp file, tell LLM how to continue
                    let truncated = if result_str.len() > 4000 {
                        self.paginate_output(&result_str, 4000, &tool_name)
                    } else {
                        result_str.clone()
                    };

                    messages.push(Message::text("tool", truncated));
                    println!("   🔧 Called '{}'", tool_name);
                }
                // Continue to next round
                continue;
            }

            // No tool calls — final response
            response_text = response_msg.text_content();
            break;
        }

        // Scrub any leaked memory context from the response (general mode)
        if !response_text.is_empty() {
            let mem_ctx = self.load_memory_context(&response_text).await;
            response_text = crate::memory::scrub_response(&response_text, &mem_ctx);
        }

        // Print a newline after the response if it ended without one
        if !response_text.is_empty() {
            println!();
        }

        self.pending_image = None;

        self.record_memory(
            &format!("General response for '{}': {}",
                &prompt[..prompt.char_indices().nth(100).map(|(i,_)|i).unwrap_or(prompt.len())],
                &response_text[..response_text.char_indices().nth(200).map(|(i,_)|i).unwrap_or(response_text.len())]),
            MemoryType::ActionOutcome,
        ).await;

        self.fire_hook(HookEvent::PostRun, "general mode complete").await;

        // Consolidate new memories
        if let Some(ref mem) = self.memory {
            if let Ok(uncon) = mem.store_ref().get_unconsolidated(20) {
                if !uncon.is_empty() {
                    let ids: Vec<String> = uncon.iter().map(|e| e.id.clone()).collect();
                    let _ = mem.store_ref().mark_consolidated(&ids);
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

    /// Run a multi-step task by executing sub-tasks sequentially with accumulated context
    /// Each sub-task has access to built-in tools (web_search, read_file, run_bash, etc.)
    /// Results from earlier steps are fed as context to later steps
    async fn run_task_mode(
        &mut self,
        prompt: &str,
        steps: &[String],
        start: std::time::Instant,
        total_memories: usize,
    ) -> Result<RunResult> {
        println!("   🧩 Executing {} sub-tasks sequentially...", steps.len());
        for (i, step) in steps.iter().enumerate() {
            println!("\n   ── Step {}/{}: {} ──", i + 1, steps.len(), step);
        }
        println!();

        let mem_context = self.load_memory_context(prompt).await;
        let mut accumulated_context = String::new();
        let mut final_response = String::new();
        let mut all_tool_defs = crate::agent::tools::builtin_tool_definitions(&self.mode, self.memory.is_some());

        // Add MCP tools if available
        if let Some(ref mcp) = self.mcp {
            let defs = mcp.to_tool_definitions().await;
            all_tool_defs.extend(defs);
        }

        for (i, step) in steps.iter().enumerate() {
            println!("   🔄 Step {}/{}: {}", i + 1, steps.len(), step);

            let step_prompt = format!(
                "Task: {}\n\nCurrent step: {}\n\nAccumulated context from previous steps:\n{}\n\n\
                 Focus ONLY on this step. Use the available tools to complete it.\n\
                 When done, provide a concise summary of what you found/did.",
                prompt,
                step,
                if accumulated_context.is_empty() { "None yet".to_string() } else { accumulated_context.clone() }
            );

            let system_prompt = format!(
                "You are HyperAgent — a versatile AI assistant completing a multi-step task.\n\n\
                You have access to tools:\n\
                - **web_search(query)**: Search the web for current information\n\
                - **read_file(path)**: Read a file from the project directory\n\
                - **run_bash(command)**: Execute bash commands\n\
                - **python_repl(code)**: Execute Python code (state persists)\n\
                - **read_document(path)**: Read PDF/Word/Excel documents\n\
                - **browser(command, url)**: Control headless Chrome browser\n\
                - **platform_setup(action)**: Check/setup tools\n\n\
                Step {}/{} of the plan. Complete this step, then provide a summary of what was accomplished.\n\
                Past context:\n{}",
                i + 1, steps.len(),
                if mem_context.is_empty() { "None".to_string() } else { mem_context.clone() }
                );
    
            let mut messages = vec![
                Message::text("system", &system_prompt),
                Message::text("user", &step_prompt),
            ];

            // Tool loop for this step (max 5 rounds)
            let max_rounds = 5;
            let mut step_output = String::new();

            for round in 0..max_rounds {
                use std::io::{Write, stdout};

                match self.provider.chat_with_tools(messages.clone(), Some(all_tool_defs.clone())).await {
                    Ok(response_msg) => {
                        // Process tool calls
                        if !response_msg.tool_calls.is_empty() {
                            for tool_call in &response_msg.tool_calls {
                                let tool_name = tool_call.function.name.clone();
                                let args: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
                                    .unwrap_or(serde_json::json!({}));

                                let is_mcp = all_tool_defs.iter().any(|d| d.function.name == tool_name);
                                let result_str = if is_mcp {
                                    match &self.mcp {
                                        Some(mcp) => match mcp.call_tool(&tool_name, args).await {
                                            Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_default(),
                                            Err(e) => format!("Error: {e}"),
                                        },
                                        None => "Error: MCP not available".to_string(),
                                    }
                                } else {
                                    self.execute_builtin_tool(&tool_name, &args).await
                                };

                                let truncated = if result_str.len() > 4000 {
                                    self.paginate_output(&result_str, 4000, &tool_name)
                                } else {
                                    result_str
                                };

                                messages.push(Message::text("tool", truncated));
                                print!("      🔧 Called '{}'\n", tool_name);
                                stdout().flush().ok();
                            }
                            continue; // More tool calls
                        }

                        // Text response — this step is done
                        step_output = response_msg.text_content();
                        break;
                    }
                    Err(e) => {
                        if round == 0 {
                            match self.chat_with_failover(messages.clone()).await {
                                Ok(r) => {
                                    step_output = r;
                                    break;
                                }
                                Err(e2) => {
                                    eprintln!("\n   Error on step: {e2}");
                                    step_output = format!("[Error: {e2}]");
                                    break;
                                }
                            }
                        } else {
                            eprintln!("\n   Error on step: {e}");
                            step_output = format!("[Error: {e}]");
                            break;
                        }
                    }
                }
            }

            // Accumulate results
            if !step_output.is_empty() {
                let step_summary = format!("--- Step {}/{}: {} ---\n{}\n", i + 1, steps.len(), step, step_output);
                accumulated_context.push_str(&format!("\n\nStep {} output:\n{}\n", i + 1, step_output));
                final_response.push_str(&step_summary);
                println!("   ✅ Step {}/{} complete\n", i + 1, steps.len());
            }
        }

        // Final summary
        if !final_response.is_empty() {
            println!("\n📋 Final Result:\n{}", final_response);
        }

        self.fire_hook(HookEvent::PostRun, "task mode complete").await;

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
            response_text: final_response,
            ..Default::default()
        })
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
    async fn create_plan_with_retry(
        &self,
        prompt: &str,
        files: &[FileContext],
        max_attempts: usize,
    ) -> Result<crate::agent::plan_agent::Plan> {
        let plan_provider = self.plan_provider.as_ref().unwrap_or(&self.provider);
        let plan_agent = crate::agent::plan_agent::PlanAgent::new(plan_provider);

        // First attempt with streaming
        match plan_agent.create_plan_stream("plan", prompt, files).await {
            Ok(plan) if plan.steps.as_ref().map(|s| !s.is_empty()).unwrap_or(false) => {
                return Ok(plan);
            }
            _ => {}
        }

        // Retries with batch
        let mut _last_error = String::new();
        for attempt in 1..=max_attempts.saturating_sub(1) {
            println!("   🔄 Retrying plan creation (attempt {attempt}/{max_attempts})...");
            let plan_provider = self.plan_provider.as_ref().unwrap_or(&self.provider);
            let plan_agent = crate::agent::plan_agent::PlanAgent::new(plan_provider);
            match plan_agent.create_plan(prompt, files).await {
                Ok(plan) => {
                    if plan.steps.as_ref().map(|s| !s.is_empty()).unwrap_or(false) {
                        return Ok(plan);
                    }
                    _last_error = "Empty plan (no steps)".to_string();
                }
                Err(e) => {
                    _last_error = format!("{e}");
                }
            }
        }
        // Last attempt: return whatever we got, even if empty
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

    /// Self-reflection: assess quality of approved changes.
    /// Uses the LLM to evaluate if changes are correct, have issues, or missed the goal.
    /// Records findings to memory but doesn't block the pipeline — this is advisory.
    async fn self_reflect(&self, prompt: &str, changes: &[FileChange]) -> anyhow::Result<()> {
        if changes.is_empty() {
            return Ok(());
        }

        // Build a compact summary of changes for the reflection prompt
        let mut summary = String::new();
        for (i, change) in changes.iter().enumerate() {
            let old_len = change.old_content.as_ref().map(|c| c.len()).unwrap_or(0);
            let new_len = change.new_content.as_ref().map(|c| c.len()).unwrap_or(0);
            let delta = if new_len > old_len { new_len - old_len } else { 0 };
            summary.push_str(&format!(
                "  {}. {} ({}): {delta} bytes added\n",
                i + 1,
                change.file.display(),
                change.change_type,
            ));
        }

        let reflection_prompt = format!(
            "You are a code quality reviewer. Review the following changes made for the task:\n\
             TASK: {prompt}\n\n\
             CHANGES:\n{summary}\n\n\
             Evaluate:\n\
             1. Do these changes correctly implement the task?\n\
             2. Are there any bugs, edge cases missed, or quality issues?\n\
             3. Are there missing pieces (tests, error handling, documentation)?\n\
             4. Could there be unintended side effects?\n\n\
             Respond with either:\n\
             - \"✅ ALL GOOD\" (if changes look correct and complete)\n\
             - \"⚠️ ISSUES: <brief description>\" (if there are minor issues)\n\
             - \"❌ PROBLEMS: <brief description>\" (if changes are wrong or incomplete)\n\n\
             Keep your analysis brief — 2-3 sentences maximum."
        );

        let response = self.provider.chat(vec![
            Message::text("system", &reflection_prompt),
        ]).await;

        match response {
            Ok(analysis) => {
                // Record reflection to memory
                let trimmed = analysis.trim();
                let mem_content = format!("Self-reflection for '{}': {}", 
                    &prompt[..prompt.len().min(80)],
                    &trimmed[..trimmed.len().min(200)]
                );
                self.record_memory(&mem_content, MemoryType::Learned).await;

                if trimmed.starts_with("❌") || trimmed.contains("PROBLEMS") {
                    eprintln!("\n   🔄 Self-reflection found issues:");
                    eprintln!("   {}\n", trimmed);
                    // Record the issue as a separate memory for future reference
                    self.record_memory(
                        &format!("Issues in '{}': {}", &prompt[..prompt.len().min(80)], trimmed),
                        MemoryType::BugFix,
                    ).await;
                } else if trimmed.starts_with("⚠️") || trimmed.contains("ISSUES") {
                    println!("   📝 Self-reflection notes: {}", trimmed);
                } else {
                    // All good — confirm but be quiet about it
                    debug_assert!(true, "Reflection passed for: {}", prompt);
                }
                Ok(())
            }
            Err(e) => {
                // Reflection failure is non-fatal
                eprintln!("   ⚠️  Self-reflection LLM call failed: {e}");
                Ok(())
            }
        }
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
const MAX_INPUT_TOKENS: usize = 32_000;
const MAX_OUTPUT_TOKENS: usize = 8_192;
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

        let per_file_budget = (self.available_for_files / total).clamp(100, 2_000);

        for (_i, file) in files.iter_mut().enumerate().take(total) {
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

    #[tokio::test]
    async fn test_tool_definitions_ask_mode() {
        // Ask mode should only have read/search tools
        let tools = crate::agent::tools::builtin_tool_definitions("ask", false);
        for t in &tools {
            let name = t.function.name.as_str();
            assert!(
                matches!(name,
                    "web_search" | "read_file" | "knowledge_search"
                    | "memory_search" | "read_document" | "browser"
                    | "analyze_image" | "platform_setup" | "desktop"
                ),
                "Unexpected tool '{name}' in ask mode"
            );
        }
    }

    #[tokio::test]
    async fn test_tool_definitions_task_mode() {
        // Task/code mode should have execution tools
        let tools = crate::agent::tools::builtin_tool_definitions("task", false);
        let names: std::collections::HashSet<&str> =
            tools.iter().map(|t| t.function.name.as_str()).collect();

        assert!(names.contains("run_bash"), "task mode should have run_bash");
        assert!(names.contains("web_search"), "task mode should have web_search");
        assert!(names.contains("memory_add"), "task mode should have memory_add");
        assert!(names.contains("python_repl"), "task mode should have python_repl");
    }

    #[tokio::test]
    async fn test_tool_definitions_with_memory() {
        // With memory=true, should include memory_remove
        let tools = crate::agent::tools::builtin_tool_definitions("general", true);
        let names: std::collections::HashSet<&str> =
            tools.iter().map(|t| t.function.name.as_str()).collect();

        assert!(names.contains("memory_remove"), "with_memory should include memory_remove");
        assert!(names.contains("memory_add"), "should always have memory_add");
    }

    #[tokio::test]
    async fn test_tool_definitions_without_memory() {
        // Without memory, should NOT include memory_remove
        let tools = crate::agent::tools::builtin_tool_definitions("general", false);
        let names: std::collections::HashSet<&str> =
            tools.iter().map(|t| t.function.name.as_str()).collect();

        assert!(!names.contains("memory_remove"), "no memory_remove without memory");
    }

    #[tokio::test]
    async fn test_orchestrator_mode_selection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        create_test_project(&root);

        let mut index = HyperIndex::new(&root).unwrap();
        index.build().unwrap();

        let provider = LlmProvider::new(
            "test".to_string(),
            "http://localhost:9999/v1".to_string(),
            "test-key".to_string(),
        ).unwrap();

        // Default mode check
        let orch = Orchestrator::new(index, provider, root, 2, true);
        assert_eq!(orch.mode, "code", "default mode should be 'code'");
    }

    #[tokio::test]
    async fn test_tool_definitions_unique_names() {
        // All tool definitions should have unique names
        let tools = crate::agent::tools::builtin_tool_definitions("general", true);
        let mut seen = std::collections::HashSet::new();
        for t in &tools {
            let name = t.function.name.as_str();
            assert!(seen.insert(name), "duplicate tool name: '{name}'");
        }
    }
}
