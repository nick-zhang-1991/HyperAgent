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
use crate::llm::{LlmProvider, Message};
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
        }
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
        println!("   🎭 Mode: {}\n", self.mode);

        // Phase 1: Get relevant files from PageRank index
        println!("🔍 Scanning codebase with PageRank...");
        let relevant_files = self.index.get_relevant_files(prompt, 15, 4000);
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

        // Plan with retry (up to 2 attempts)
        let plan = self.create_plan_with_retry(&augmented_prompt, &relevant_files, 2).await?;
        println!("   Plan: {}", plan.summary);
        if let Some(ref steps) = plan.steps {
            for (i, step) in steps.iter().enumerate() {
                println!("   {}. {}", i + 1, step);
            }
        }
        println!();

        // Record plan to memory
        let steps_count = plan.steps.as_ref().map(|s| s.len()).unwrap_or(0);
        self.record_memory(
            &format!("Plan for '{}': {} — {} steps", prompt, plan.summary, steps_count),
            MemoryType::Decision,
        ).await;
        total_memories += 1;

        self.fire_hook(HookEvent::PostPlan, &plan.summary).await;

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

        // Phase 5: Apply
        self.fire_hook(HookEvent::PreApply, prompt).await;
        println!("✏️  Applying changes...");
        let apply_agent = ApplyAgent::new(&self.root, self.confirm);
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
            let fix_attempts = 3;
            for fix_round in 1..=fix_attempts {
                let linter = if has_cargo { "cargo check" } else { "tsc --noEmit" };
                println!("   🔧 Lint check ({linter})...");

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
                        println!("   ✅ Lint passed ({})", linter);
                        break;
                    }
                    Some(out) => {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        let error_snippet: Vec<&str> = stderr.lines()
                            .filter(|l| l.contains("error[") || l.contains("error:"))
                            .take(5)
                            .collect();

                        if error_snippet.is_empty() {
                            println!("   ✅ Lint passed ({})", linter);
                            break;
                        }

                        let errors = error_snippet.join("\n");
                        println!("   ⚠️  Lint errors detected (round {fix_round}/{fix_attempts}):");
                        for e in &error_snippet {
                            println!("      {e}");
                        }

                        if fix_round >= fix_attempts {
                            println!("   ❌ Max fix attempts reached — manual intervention needed");
                            break;
                        }

                        // Ask LLM to fix the errors
                        println!("   🔄 Requesting auto-fix from LLM...");

                        // Read the current file content for each approved change
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
                            "Fix compile errors in the changed files", &fix_files, &errors
                        );

                        let fix_response = self.provider.chat(vec![
                            crate::llm::Message {
                                role: "system".to_string(),
                                content: format!(
                                    "You are a code fixer. The following compile errors were found after applying changes.\n\
                                     Output ONLY the corrected file content in JSON format:\n\
                                     {{\"file\": \"relative/path\", \"content\": \"COMPLETE corrected file content\"}}\n\n\
                                     RULES:\n\
                                     - Output one JSON object per file that needs fixing\n\
                                     - Do NOT change anything beyond what's needed to fix the errors\n\
                                     - Keep the existing code structure intact\n\
                                     - Each JSON must be on its own line"
                                ),
                            },
                            crate::llm::Message {
                                role: "user".to_string(),
                                content: review_input,
                            },
                        ]).await;

                        match fix_response {
                            Ok(response) => {
                                // Parse the fix output and apply corrections
                                let fixed = crate::agent::code_agent::CodeAgent::parse_fix_response(
                                    &response, &self.root
                                );
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

                                if fixed.is_empty() {
                                    println!("   ⚠️  LLM couldn't generate fixes — stopping auto-fix");
                                    break;
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
            model_name: self.provider.model.clone(),
            memories_recorded: total_memories,
            response_text: String::new(),
            cost_estimate: (tokens_used as f64 / 1_000_000.0) * 0.15f64.max(self.provider.input_price_per_1m),
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
        &self,
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
            - If the answer requires code changes, say so but DO NOT propose edits\n\
            - Format code blocks with ```language\n\
            - Answer in the same language as the question",
            file_context,
            if mem_context.is_empty() { "None".to_string() } else { mem_context }
        );

        // Add MCP tools context if available (can be called via JSON function format)
        let mcp_tools_available = if let Some(ref mcp) = self.mcp {
            let tools = mcp.get_all_tools().await;
            if !tools.is_empty() {
                system_prompt.push_str("\n\n--- MCP Tools Available ---\n");
                system_prompt.push_str("You can call MCP tools by outputting a JSON block with:\n");
                system_prompt.push_str("  {\"tool_call\": {\"name\": \"tool_name\", \"arguments\": {...}}}\n");
                system_prompt.push_str("When you receive the result, incorporate it into your response.\n\n");
                for t in &tools {
                    let desc = if t.description.len() > 80 {
                        format!("{}...", &t.description[..77])
                    } else {
                        t.description.clone()
                    };
                    system_prompt.push_str(&format!("  - {}.{}: {desc}\n", t.server, t.name));
                }
                tools
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

        // Max 3 tool call rounds
        let max_rounds = if mcp_tools_available.is_empty() { 1 } else { 3 };
        let mut response_text = String::new();

        for round in 0..max_rounds {
            print!("   📝 (round {}/{}) ", round + 1, max_rounds);
            std::io::Write::flush(&mut std::io::stdout()).ok();

            let result = match self.provider.chat(messages.clone()).await {
                Ok(r) => {
                    print!("{r}");
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                    r
                },
                Err(e) => {
                    if round == 0 {
                        // Try streaming on first round
                        if let Ok(stream) = self.provider.chat_stream(messages.clone()).await {
                            let mut rx = stream.into_receiver();
                            let mut full = String::new();
                            while let Some(chunk) = rx.recv().await {
                                print!("{chunk}");
                                std::io::Write::flush(&mut std::io::stdout()).ok();
                                full.push_str(&chunk);
                            }
                            println!();
                            full
                        } else {
                            eprintln!("\n   Error: {e}");
                            String::new()
                        }
                    } else {
                        eprintln!("\n   Error: {e}");
                        String::new()
                    }
                }
            };

            if result.is_empty() {
                break;
            }

            // Check if the LLM wants to call an MCP tool
            if let Some(tool_call) = self.parse_mcp_tool_call(&result) {
                let tool_result = match &self.mcp {
                    Some(mcp) => mcp.call_tool(&tool_call.0, tool_call.1).await,
                    None => Err(anyhow::anyhow!("MCP not available")),
                };

                match tool_result {
                    Ok(value) => {
                        let result_str = serde_json::to_string_pretty(&value)
                            .unwrap_or_else(|_| "{}".to_string());
                        messages.push(Message { 
                            role: "user".to_string(),
                            content: format!(
                                "Tool '{}' returned:\n```json\n{}\n```\nPlease incorporate this into your response.",
                                tool_call.0, result_str
                            ),
                        });
                        println!("   🔧 Called MCP tool '{}'", tool_call.0);
                    }
                    Err(e) => {
                        messages.push(Message { 
                            role: "user".to_string(),
                            content: format!("Tool '{}' failed: {e}", tool_call.0),
                        });
                        eprintln!("   ⚠️  MCP tool '{}' failed: {e}", tool_call.0);
                    }
                }
            } else {
                // No tool call — this is the final response
                response_text = result;
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
            model_name: self.provider.model.clone(),
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
        let mut _last_error = String::new();
        for attempt in 1..=max_attempts {
            if attempt > 1 {
                println!("   🔄 Retrying plan creation (attempt {attempt}/{max_attempts})...");
            }
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
    /// Optimized: starts review as soon as any agent completes (pipeline overlap)
    async fn execute_with_retry(
        &self,
        augmented_prompt: &str,
        plan: &crate::agent::plan_agent::Plan,
        relevant_files: &[FileContext],
        _original_prompt: &str,
        _start: Instant,
        _total_memories: usize,
    ) -> Result<Vec<FileChange>> {
        let mut all_changes = Vec::new();
        let max_attempts = 2;

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

                tokio::spawn(async move {
                    let agent = crate::agent::code_agent::CodeAgent::new(&provider, &root);
                    let changes = agent.execute(&p, &chunk_steps, &files).await;
                    let _ = tx.send((i, changes)).await;
                });
            }
            drop(tx); // Close sender so rx can terminate

            // Collect results progressively — show live progress
            let mut completed = 0usize;
            use std::io::{Write, stdout};
            while let Some((i, changes)) = rx.recv().await {
                completed += 1;
                let elapsed = code_start.elapsed();
                let file_count = changes.len();

                if file_count > 0 || attempt == max_attempts {
                    print!("\r   ✅ Agent '{}' — {} changes [{completed}/{total_agents} done, {:.1}s]    \n",
                        chunks[i].name, file_count, elapsed.as_secs_f64());
                    stdout().flush().ok();
                } else {
                    // Show progress even for empty agents
                    print!("\r   ⏳ Agent '{}' — no changes [{completed}/{total_agents} done, {:.1}s]    \n",
                        chunks[i].name, elapsed.as_secs_f64());
                    stdout().flush().ok();
                }

                all_changes.extend(changes);
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
}

pub struct WorkChunk {
    pub name: String,
    pub steps: Vec<String>,
}
