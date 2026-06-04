use crate::index::FileContext;
use crate::llm::{LlmProvider, Message};
use anyhow::Result;

/// The result of planning — structured for parallel execution
///
/// Design inspired by writing-plans (superpowers) + subagent-driven-development:
/// - Bite-sized tasks (2-5 min each), independently executable
/// - Exact file paths for every task
/// - Verification steps for each task
/// - DRY, YAGNI, TDD principles baked in
#[derive(Debug)]
pub struct Plan {
    pub summary: String,
    pub steps: Option<Vec<String>>,
    #[allow(dead_code)]
    pub reasoning: String,
}

/// PlanAgent: Analyzes the task and creates a structured, actionable execution plan
///
/// Outputs plans that follow the writing-plans methodology:
/// - Each step is 2-5 minutes of focused work
/// - Steps are independently executable (parallel-safe)
/// - Steps reference exact file paths
pub struct PlanAgent<'a> {
    provider: &'a LlmProvider,
}

impl<'a> PlanAgent<'a> {
    pub fn new(provider: &'a LlmProvider) -> Self {
        Self { provider }
    }

    pub async fn create_plan(
        &self,
        prompt: &str,
        relevant_files: &[FileContext],
    ) -> Result<Plan> {
        let (messages, _file_context) = self.build_plan_messages(prompt, relevant_files);
        let response = self.provider.chat(messages).await?;
        let plan = self.parse_plan_response(&response);
        Ok(plan)
    }

    /// Create a plan with streaming output — shows dynamic progress inline.
    /// Falls back to batch if streaming fails.
    pub async fn create_plan_stream(
        &self,
        prompt: &str,
        relevant_files: &[FileContext],
    ) -> Result<Plan> {
        let (messages, _file_context) = self.build_plan_messages(prompt, relevant_files);

        match self.provider.chat_stream(messages.clone()).await {
            Ok(stream) => {
                let mut rx = stream.into_receiver();
                let mut full_response = String::new();
                let start = std::time::Instant::now();

                // Show streaming progress
                print!("\r   📋 Planning... ");
                let _ = std::io::Write::flush(&mut std::io::stdout()).ok();

                while let Some(chunk) = rx.recv().await {
                    full_response.push_str(&chunk);
                    let elapsed = start.elapsed();
                    // Show character count progress every so often
                    if full_response.len() % 50 < chunk.len() {
                        print!("\r   📋 Planning... ({} chars, {:.0}s)",
                            full_response.len(), elapsed.as_secs_f64());
                        let _ = std::io::Write::flush(&mut std::io::stdout()).ok();
                    }
                }

                let plan = self.parse_plan_response(&full_response);
                let elapsed = start.elapsed();
                println!("\r   📋 Plan ready ({:.1}s)", elapsed.as_secs_f64());
                Ok(plan)
            }
            Err(_) => {
                // Fallback to batch
                print!("   📋 Planning (batch)... ");
                let _ = std::io::Write::flush(&mut std::io::stdout()).ok();
                let response = self.provider.chat(messages).await?;
                let plan = self.parse_plan_response(&response);
                println!("done");
                Ok(plan)
            }
        }
    }

    /// Build messages for the LLM plan call
    fn build_plan_messages(&self, prompt: &str, relevant_files: &[FileContext]) -> (Vec<Message>, String) {
        let file_context = self.build_file_context(relevant_files);

        let system_prompt = r#"You are HyperAgent's PlanAgent. Create precise, actionable execution plans.

=== INTELLIGENT INTENT DETECTION ===

First, determine what the user wants. COMMANDS (fix, add, create, implement, update, refactor, change, modify, remove, delete, optimize) are ALWAYS actions with steps. QUESTIONS (what, how, why, analyze, explain, suggestions ending with ?) are answers with empty steps [].

=== FOR ACTION PLANS (WRITING-PLANS METHODOLOGY) ===

Each plan must have:
1. A summary (1 sentence) of what needs to be done
2. Bite-sized steps — each 2-5 minutes of focused work
3. Each step must touch DIFFERENT files or concerns (parallel-safe)
4. Each step must reference exact file paths

Bite-Sized Step Examples:
  ✓ GOOD: "Add email field to User struct in src/models/user.rs"
  ✓ GOOD: "Create password hashing utility in src/auth/hash.rs"  
  ✓ GOOD: "Add login endpoint handler in src/routes/auth.rs"
  ✗ BAD: "Build authentication system" (too vague, too large)

Principles:
- YAGNI: Implement ONLY what's needed now. No speculative code.
- DRY: Don't copy-paste logic across steps. Note shared dependencies.
- Parallel-safe: Steps must not touch the same file (or orchestrator handles conflict)
- Order for dependencies: put setup steps before usage steps

Output format — return ONLY valid JSON:

For answers (no code changes needed):
{
  "summary": "Your full answer to the user's question here. Include analysis, reasoning, and specific code references.",
  "steps": [],
  "affected_files": []
}

For actions (code changes needed):
{
  "summary": "Brief summary of the plan (1-2 sentences)",
  "steps": [
    "Create EmailValidator struct in src/validation/email.rs with format check and domain validation",
    "Add login endpoint handler in src/routes/auth.rs that validates email via EmailValidator",
    "Register login route in src/main.rs router"
  ],
  "affected_files": ["src/validation/email.rs", "src/routes/auth.rs", "src/main.rs"]
}

Rules:
- steps should be 0-6 items maximum
- If steps is empty [] → no code changes, just answering
- If steps has items → each step is a SINGLE file operation (create/edit/delete one file)
- Each step should take 2-5 minutes
- Steps are ordered by dependency
- Do NOT include verification commands in steps (review agent handles that)
"#;

        let user_message = format!(
            "Task: {}\n\nRelevant files:\n{}\n\nCreate a bite-sized, parallel-safe plan for this task. Each step should modify exactly one file.",
            prompt, file_context
        );

        let messages = vec![
            Message { 
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            Message { 
                role: "user".to_string(),
                content: user_message,
            },
        ];
        (messages, file_context)
    }

    fn build_file_context(&self, files: &[FileContext]) -> String {
        let mut ctx = String::new();
        for (i, file) in files.iter().enumerate() {
            ctx.push_str(&format!(
                "[{}] {} ({} lines, relevance: {:.2})\n",
                i + 1,
                file.path.display(),
                file.total_lines,
                file.score
            ));

            let preview: Vec<&str> = file.content.lines().take(20).collect();
            if !preview.is_empty() {
                ctx.push_str("  ```\n");
                for line in preview {
                    ctx.push_str(&format!("  {line}\n"));
                }
                ctx.push_str("  ```\n");
            }
            ctx.push('\n');
        }
        ctx
    }

    fn parse_plan_response(&self, response: &str) -> Plan {
        // Strip markdown code fences (```json / ```) that reasoning models often add
        let cleaned = response
            .trim()
            .trim_start_matches(|c: char| c == '`' || c.is_whitespace() || c == '\n' || c == '\r')
            .trim_end_matches(|c: char| c == '`' || c.is_whitespace() || c == '\n' || c == '\r')
            .to_string();

        // Remove ```json prefix if present
        let cleaned = if let Some(rest) = cleaned.strip_prefix("```json") {
            rest.trim()
        } else if let Some(rest) = cleaned.strip_prefix("```") {
            rest.trim()
        } else {
            &cleaned
        };

        // Try to extract JSON between first { and last }
        let json_str = if let Some(start) = cleaned.find('{') {
            if let Some(end) = cleaned.rfind('}') {
                &cleaned[start..=end]
            } else {
                ""
            }
        } else {
            ""
        };

        if json_str.is_empty() {
            return Plan {
                summary: "No plan generated".to_string(),
                steps: None,
                reasoning: response.to_string(),
            };
        }

        // Try parsing the JSON — if it fails, try to fix common issues
        let parsed = serde_json::from_str::<serde_json::Value>(json_str)
            .or_else(|_| {
                // Try replacing single quotes with double quotes (common LLM mistake)
                let fixed = json_str
                    .replace('\'', "\"")
                    .replace("None", "null")
                    .replace("True", "true")
                    .replace("False", "false");
                serde_json::from_str(&fixed)
            })
            .ok();

        match parsed {
            Some(parsed) => {
                let summary = parsed["summary"]
                    .as_str()
                    .unwrap_or("Task execution")
                    .to_string();

                let steps = parsed["steps"].as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                });

                let affected_files = parsed["affected_files"].as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                });

                // If we have steps, make sure plan.steps is Some (even if empty vec)
                let plan = Plan {
                    summary,
                    steps: steps.or(Some(vec![])),
                    reasoning: response.to_string(),
                };

                // Log affected_files for debugging if present
                if let Some(files) = affected_files {
                    if !files.is_empty() {
                        tracing::debug!("Affected files: {:?}", files);
                    }
                }

                plan
            }
            None => Plan {
                summary: "No plan generated".to_string(),
                steps: None,
                reasoning: response.to_string(),
            },
        }
    }

    /// Build a plan prompt that explicitly tells the LLM to return JSON
    /// without reasoning/prelude text — optimized for DeepSeek reasoning models.
    fn build_action_plan_messages(&self, prompt: &str, relevant_files: &[FileContext]) -> (Vec<Message>, String) {
        let file_context = self.build_file_context(relevant_files);

        let system_prompt = r#"You are HyperAgent's PlanAgent. Return ONLY valid JSON, no preamble, no thinking.

IMPORTANT: Do NOT include any text before or after the JSON. No markdown, no backticks, no explanation.
Just raw JSON starting with { and ending with }.

=== FOR ANSWERS (no code changes needed, Q&A) ===
{"summary": "Your full answer here.", "steps": [], "affected_files": []}

=== FOR ACTIONS (code changes needed) ===
{"summary": "Brief summary (1-2 sentences)", "steps": ["Step 1: description with file path", "Step 2: description with file path"], "affected_files": ["path/to/file1.rs", "path/to/file2.rs"]}

Rules:
- steps should be 0-6 items maximum
- If steps is empty [] → just answering a question
- If steps has items → each step modifies ONE file
- Each step must reference exact file paths
- Steps are ordered by dependency
- Do NOT include verification commands in steps"#;

        let user_message = format!(
            "Task: {}\n\nRelevant files:\n{}\n\nReturn ONLY valid JSON — no thinking, no markdown, no code fences.",
            prompt, file_context
        );

        let messages = vec![
            Message { 
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            Message { 
                role: "user".to_string(),
                content: user_message,
            },
        ];
        (messages, file_context)
    }

    /// Create a plan with explicit action-focused prompt — no json-in-markdown, no thinking.
    /// Used as a retry strategy when the standard prompt fails.
    pub async fn create_action_plan(
        &self,
        prompt: &str,
        relevant_files: &[FileContext],
    ) -> Result<Plan> {
        let (messages, _file_context) = self.build_action_plan_messages(prompt, relevant_files);
        let response = self.provider.chat(messages).await?;
        let plan = self.parse_plan_response(&response);
        Ok(plan)
    }
}
