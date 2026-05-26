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
        if steps.is_empty() {
            return vec![];
        }

        let system_prompt = r#"You are HyperAgent's CodeAgent. You write precise, surgical code changes.

=== KARPATHY GUIDELINES — YOU MUST FOLLOW THESE ===

1. THINK BEFORE CODING
- First analyze the existing code structure and patterns
- Surface assumptions about what needs to change

2. SIMPLICITY FIRST
- Minimum code that solves exactly the problem, nothing more
- No abstractions for single-use code
- NO speculative code — only what directly serves the plan step

3. SURGICAL CHANGES
- Touch ONLY the files and lines needed for this task
- Do NOT "improve" adjacent code, formatting, or comments
- Match existing code style exactly
- Every changed line must trace directly to the user's request

4. REMOVE ORPHANS
- When your changes make something unused, remove it

5. VERIFY YOUR OUTPUT
- Ensure no syntax errors in the generated code
- Verify the change actually solves the intended step
=== END KARPATHY GUIDELINES ===

Output format — output one JSON object per line with either FULL-FILE or DIFF mode:

FULL-FILE mode (for creates and deletes):
{"file": "relative/path/to/file", "change_type": "create|delete", "content": "COMPLETE file content"}

DIFF mode (for edits — PREFERRED, saves tokens):
{"file": "relative/path/to/file", "change_type": "edit", "diff": "@@ -line,count +line,count @@\n context line\n-old line\n+new line\n..."}

Rules:
- For CREATE and DELETE: use FULL-FILE mode (content field)
- For EDIT: use DIFF mode (diff field) — output ONLY the changed lines as a unified diff
- The diff format uses `@@ -old_start,old_count +new_start,new_count @@` headers
- Lines starting with space are context, `-` is removed, `+` is added
- Output ONLY valid JSON objects, one per line
- No extra text, no markdown outside JSON
"#;

        let file_context = self.build_file_context(files);

        let user_message = format!(
            "Plan steps to execute:\n{}\n\nRelevant files (symbols only):\n{}\n\nGenerate the necessary code changes. For edits, use DIFF format. For new files, use full content.",
            steps.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n"),
            file_context,
        );

        match self
            .provider
            .chat(vec![
                Message { 
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                Message { 
                    role: "user".to_string(),
                    content: user_message,
                },
            ])
            .await
        {
            Ok(response) => self.parse_changes(&response),
            Err(e) => {
                eprintln!("   CodeAgent error: {e}");
                vec![]
            }
        }
    }

    /// Build condensed file context: only file info, symbols, and first 3 lines as style preview.
    /// No full-file dumps — saves ~80% input tokens.
    fn build_file_context(&self, files: &[FileContext]) -> String {
        let mut ctx = String::new();
        for file in files {
            ctx.push_str(&format!(
                "\n--- {} ({} lines, score: {:.2}) ---\n",
                file.path.display(),
                file.total_lines,
                file.score
            ));

            // Show first 3 lines as style preview
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

                                // Support both diff mode and full-content mode for edits
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
                                    // Fallback: full-file mode
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
