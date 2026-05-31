use std::path::Path;

use crate::diff::{text_to_hunks, FileChange};
use crate::index::FileContext;
use crate::llm::{LlmProvider, Message};

/// CodeAgent: Executes code changes in parallel
///
/// Context optimization: code_agent sends only file structure + symbol info
/// to the LLM (not full file content), and the LLM outputs surgical diffs
/// instead of full-file rewrites. This reduces token usage by ~80%.
pub struct CodeAgent<'a> {
    provider: &'a LlmProvider,
    root: &'a Path,
}

impl<'a> CodeAgent<'a> {
    pub fn new(provider: &'a LlmProvider, root: &'a Path) -> Self {
        Self { provider, root }
    }

    pub async fn execute(
        &self,
        _prompt: &str,
        steps: &[String],
        files: &[FileContext],
    ) -> Vec<FileChange> {
        let (messages, _) = self.build_messages(steps, files);
        match self.provider.chat(messages).await {
            Ok(response) => self.parse_changes(&response),
            Err(e) => {
                eprintln!("   CodeAgent error: {e}");
                vec![]
            }
        }
    }

    /// Execute with streaming output — prints LLM response as it arrives
    pub async fn execute_stream(
        &self,
        agent_name: &str,
        _prompt: &str,
        steps: &[String],
        files: &[FileContext],
    ) -> Vec<FileChange> {
        let (messages, _) = self.build_messages(steps, files);

        match self.provider.chat_stream(messages.clone()).await {
            Ok(stream) => {
                let mut rx = stream.into_receiver();
                let mut full_response = String::new();
                print!("   💻 {}: ", agent_name);
                use std::io::{Write, stdout};
                stdout().flush().ok();

                while let Some(chunk) = rx.recv().await {
                    print!("{chunk}");
                    stdout().flush().ok();
                    full_response.push_str(&chunk);
                }
                println!();
                self.parse_changes(&full_response)
            }
            Err(_) => {
                match self.provider.chat(messages).await {
                    Ok(response) => self.parse_changes(&response),
                    Err(e) => {
                        eprintln!("   CodeAgent error: {e}");
                        vec![]
                    }
                }
            }
        }
    }

    /// Build messages for the LLM call
    fn build_messages(&self, steps: &[String], files: &[FileContext]) -> (Vec<Message>, String) {
        if steps.is_empty() {
            return (vec![], String::new());
        }

        let system_prompt = r#"You are HyperAgent's CodeAgent. You write precise, surgical code changes.

=== KARPATHY GUIDELINES — YOU MUST FOLLOW THESE ===

1. THINK BEFORE CODING
2. SIMPLICITY FIRST
3. SURGICAL CHANGES
4. REMOVE ORPHANS
5. VERIFY YOUR OUTPUT
=== END KARPATHY GUIDELINES ===

Output format — JSON objects:

FULL-FILE (create|delete): {"file": "path", "change_type": "create|delete", "content": "..."}
DIFF (edit — PREFERRED):   {"file": "path", "change_type": "edit", "diff": "@@ -1,3 +1,4 @@..."}

Rules:
- For EDIT use DIFF mode — output ONLY the changed lines
- For CREATE/DELETE use FULL-FILE mode
- Output ONLY valid JSON, one per line
- No extra text outside JSON
"#;

        let file_context = self.build_file_context(files);
        let user_message = format!(
            "Plan steps:\n{}\n\nRelevant files:\n{}\n\nGenerate code changes with DIFF format for edits.",
            steps.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n"),
            file_context,
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

    /// Build condensed file context: file info, symbols, and first 3 lines as style preview.
    fn build_file_context(&self, files: &[FileContext]) -> String {
        let mut ctx = String::new();
        for file in files {
            ctx.push_str(&format!(
                "\n--- {} ({} lines, score: {:.2}) ---\n",
                file.path.display(),
                file.total_lines,
                file.score
            ));

            let preview: Vec<&str> = file.content.lines().take(3).collect();
            if !preview.is_empty() {
                ctx.push_str("  Style preview:\n  ```\n");
                for line in &preview {
                    ctx.push_str(&format!("  {line}\n"));
                }
                ctx.push_str("  ```\n");
            }

            ctx.push('\n');
        }
        ctx
    }

    fn parse_changes(&self, response: &str) -> Vec<FileChange> {
        let mut changes = Vec::new();

        let mut start = 0;
        while let Some(json_start) = response[start..].find('{') {
            let actual_start = start + json_start;
            if let Some(json_end) = response[actual_start..].find('}') {
                let candidate = &response[actual_start..=actual_start + json_end];

                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(candidate) {
                    if let (Some(file), Some(change_type)) = (
                        parsed["file"].as_str(),
                        parsed["change_type"].as_str(),
                    ) {
                        let path = self.root.join(file);

                        match change_type {
                            "edit" => {
                                let old_content = if path.exists() {
                                    std::fs::read_to_string(&path).ok()
                                } else {
                                    None
                                };

                                if let Some(diff_text) = parsed["diff"].as_str() {
                                    let hunks = text_to_hunks(diff_text);
                                    changes.push(FileChange {
                                        file: path,
                                        change_type: "edit".to_string(),
                                        old_content,
                                        new_content: None,
                                        hunks,
                                    });
                                } else if let Some(content) = parsed["content"].as_str() {
                                    changes.push(FileChange {
                                        file: path,
                                        change_type: "edit".to_string(),
                                        old_content,
                                        new_content: Some(content.to_string()),
                                        hunks: vec![],
                                    });
                                }
                            }
                            "create" | "delete" => {
                                let content = parsed["content"].as_str().unwrap_or("");
                                let old_content = if path.exists() {
                                    std::fs::read_to_string(&path).ok()
                                } else {
                                    None
                                };
                                changes.push(FileChange {
                                    file: path,
                                    change_type: change_type.to_string(),
                                    old_content,
                                    new_content: Some(content.to_string()),
                                    hunks: vec![],
                                });
                            }
                            _ => {}
                        }
                    }
                }
                start = actual_start + json_end + 1;
            } else {
                break;
            }
        }
        changes
    }
}

/// Parse LLM fix response — returns (path, content) pairs
impl<'a> CodeAgent<'a> {
    pub fn parse_fix_response(response: &str, root: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
        let mut fixed = Vec::new();
        let mut start = 0;
        while let Some(json_start) = response[start..].find('{') {
            let actual_start = start + json_start;
            if let Some(json_end) = response[actual_start..].find('}') {
                let candidate = &response[actual_start..=actual_start + json_end];
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(candidate) {
                    let file = parsed["file"].as_str().unwrap_or("");
                    let content = parsed["content"].as_str().unwrap_or("");
                    if !file.is_empty() && !content.is_empty() {
                        fixed.push((root.join(file), content.to_string()));
                    }
                }
                start = actual_start + json_end + 1;
            } else {
                break;
            }
        }
        fixed
    }
}
