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

    /// Create a plan with streaming output — prints LLM response as it arrives
    pub async fn create_plan_stream(
        &self,
        agent_name: &str,
        prompt: &str,
        relevant_files: &[FileContext],
    ) -> Result<Plan> {
        let (messages, _file_context) = self.build_plan_messages(prompt, relevant_files);

        match self.provider.chat_stream(messages.clone()).await {
            Ok(stream) => {
                let mut rx = stream.into_receiver();
                let mut full_response = String::new();
                if !agent_name.is_empty() {
                    print!("   📋 {}: ", agent_name);
                }
                use std::io::{Write, stdout};
                stdout().flush().ok();

                while let Some(chunk) = rx.recv().await {
                    print!("{chunk}");
                    stdout().flush().ok();
                    full_response.push_str(&chunk);
                }
                println!();
                let plan = self.parse_plan_response(&full_response);
                Ok(plan)
            }
            Err(_) => {
                // Fallback to batch
                let response = self.provider.chat(messages).await?;
                let plan = self.parse_plan_response(&response);
                Ok(plan)
            }
        }
    }

    /// Build messages for the LLM plan call
    fn build_plan_messages(&self, prompt: &str, relevant_files: &[FileContext]) -> (Vec<Message>, String) {
        let file_context = self.build_file_context(relevant_files);

        let system_prompt = r#"You are HyperAgent's PlanAgent. Create precise, actionable execution plans.

=== WRITING-PLANS METHODOLOGY (inspired by superpowers/supermaven) ===

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
- steps should be 1-6 items maximum (if more, merge related file changes)
- Each step is a SINGLE file operation (create/edit/delete one file)
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
        let json_str = if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                &response[start..=end]
            } else {
                ""
            }
        } else {
            ""
        };

        if json_str.is_empty() {
            return Plan {
                summary: response.lines().next().unwrap_or("No plan generated").to_string(),
                steps: None,
                reasoning: response.to_string(),
            };
        }

        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(parsed) => {
                let summary = parsed["summary"]
                    .as_str()
                    .unwrap_or("Task execution")
                    .to_string();

                let steps = parsed["steps"].as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                });

                Plan {
                    summary,
                    steps,
                    reasoning: response.to_string(),
                }
            }
            Err(_) => Plan {
                summary: "Task execution".to_string(),
                steps: None,
                reasoning: response.to_string(),
            },
        }
    }
}
