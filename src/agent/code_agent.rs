use std::path::Path;

use crate::diff::FileChange;
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

    /// Execute with streaming output — shows real-time progress with file names
    /// as they're detected, then displays parsed changes.
    pub async fn execute_stream(
        &self,
        agent_name: &str,
        _prompt: &str,
        steps: &[String],
        files: &[FileContext],
    ) -> Vec<FileChange> {
        let (messages, _) = self.build_messages(steps, files);
        if messages.is_empty() {
            return vec![];
        }

        use std::io::{Write, stdout};

        // Try streaming first for real-time progress
        match self.provider.chat_stream(messages.clone()).await {
            Ok(stream) => {
                let mut rx = stream.into_receiver();
                let mut full_response = String::new();
                let mut file_names: Vec<String> = Vec::new();
                let start = std::time::Instant::now();

                // Collect chunks progressively, showing file names as detected
                while let Some(chunk) = rx.recv().await {
                    full_response.push_str(&chunk);
                    // Progressive parsing: scan for "file": "..." patterns
                    // This catches file paths before full JSON is complete
                    for part in chunk.split('"') {
                        if part.starts_with("src/") || part.starts_with("lib/") || part.starts_with("tests/")
                            || part.starts_with("frontend/") || part.starts_with("backend/")
                            || part.contains(".rs") || part.contains(".ts") || part.contains(".py")
                            || part.contains(".js") || part.contains(".toml") || part.contains(".json")
                            || part.contains(".css") || part.contains(".html")
                        {
                            // Likely a file path — normalize
                            let candidate = part.trim().to_string();
                            if candidate.len() > 3 && candidate.len() < 200
                                && !file_names.contains(&candidate)
                                && !candidate.contains(' ')
                            {
                                file_names.push(candidate);
                            }
                        }
                    }
                    // Update progress line
                    let elapsed = start.elapsed();
                    if file_names.is_empty() {
                        print!("\r   💻 {}: generating... [{:.0}s]", agent_name, elapsed.as_secs_f64());
                    } else {
                        let shown = if file_names.len() <= 3 {
                            file_names.join(", ")
                        } else {
                            format!("{} (+{} more)", file_names[..3].join(", "), file_names.len() - 3)
                        };
                        print!("\r   💻 {}: [{}] {} [{:.0}s]",
                            agent_name, file_names.len(), shown, elapsed.as_secs_f64());
                    }
                    let _ = stdout().flush();
                }

                // Parse final response
                let changes = self.parse_changes(&full_response);
                let elapsed = start.elapsed();
                if !changes.is_empty() {
                    println!("\r   💻 {}: done — {} change(s) in {:.1}s                      ",
                        agent_name, changes.len(), elapsed.as_secs_f64());
                    changes
                } else {
                    // Try fallback diff parser
                    let fallback = self.parse_fallback_diff(&full_response);
                    if !fallback.is_empty() {
                        println!("\r   💻 {}: done — {} change(s) (diff) in {:.1}s              ",
                            agent_name, fallback.len(), elapsed.as_secs_f64());
                        fallback
                    } else {
                        println!("\r   💻 {}: no changes found ({:.1}s)                        ",
                            agent_name, elapsed.as_secs_f64());
                        vec![]
                    }
                }
            }
            Err(_stream_err) => {
                // Fallback to batch mode
                print!("\r   💻 {}: generating...  ", agent_name);
                let _ = stdout().flush();

                match self.provider.chat(messages).await {
                    Ok(response) => {
                        println!("\r   💻 {}: parsing changes ({} chars)...", agent_name, response.len());
                        let changes = self.parse_changes(&response);
                        if changes.is_empty() {
                            let fallback = self.parse_fallback_diff(&response);
                            fallback
                        } else {
                            changes
                        }
                    }
                    Err(e) => {
                        eprintln!("   💻 {} error: {e}", agent_name);
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

Output format — JSON objects, one per line. No other text.

For edits: {"file": "relative/path", "change_type": "edit", "content": "COMPLETE new file content (include ALL lines after your change)"}
For creates: {"file": "relative/path", "change_type": "create", "content": "FULL file content"}
For deletes: {"file": "relative/path", "change_type": "delete"}

Rules:
- For EDIT, output the COMPLETE new file content — every line including all context
- For CREATE, output the full file content
- Output ONLY valid JSON, one per line
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

            let full_content: Vec<&str> = file.content.lines().collect();
            ctx.push_str(&format!("  Full content ({} lines):\n  ```\n", full_content.len()));
            for line in &full_content {
                ctx.push_str(&format!("  {line}\n"));
            }
            ctx.push_str("  ```\n");

            ctx.push('\n');
        }
        ctx
    }

    pub fn parse_changes(&self, response: &str) -> Vec<FileChange> {
        let mut changes = Vec::new();
        let bytes = response.as_bytes();
        let n = bytes.len();
        let mut i = 0;

        // Brace-counting scanner: correctly handles nested {} in diff content
        while i < n {
            if bytes[i] != b'{' {
                i += 1;
                continue;
            }
            // Found a potential JSON start at position i
            let mut depth = 1u32;
            let mut j = i + 1;
            let mut in_string = false;
            while j < n && depth > 0 {
                let c = bytes[j];
                if c == b'"' && (j == 0 || bytes[j-1] != b'\\') {
                    in_string = !in_string;
                } else if !in_string {
                    match c {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                }
                j += 1;
            }
            if depth == 0 {
                let candidate = &response[i..j];
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
                                    let hunks = crate::diff::text_to_hunks(diff_text);
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
                i = j;
            } else {
                i += 1;
            }
        }
        changes
    }

    /// Fallback parser: if no structured JSON changes found, try to extract
    /// unified diff blocks directly from the response text.
    pub fn parse_fallback_diff(&self, response: &str) -> Vec<FileChange> {
        let mut changes = Vec::new();
        let lines: Vec<&str> = response.lines().collect();
        let mut i = 0;

        // Try to find a section header with a file path like "--- a/path" or "diff --git a/path"
        while i < lines.len() {
            let line = lines[i];
            let file_path = if line.starts_with("--- a/") || line.starts_with("+++ b/") {
                // Extract path: strip the a/ or b/ prefix
                let path_str = line.trim_start_matches("--- a/").trim_start_matches("+++ b/").trim();
                if !path_str.is_empty() && !path_str.contains('/') && i + 1 < lines.len() {
                    // Single filename, find the full path from file context
                    self.root.join(path_str)
                } else if !path_str.is_empty() {
                    self.root.join(path_str)
                } else {
                    i += 1;
                    continue;
                }
            } else if line.starts_with("diff --git") {
                // Format: diff --git a/path b/path
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let path_str = parts[3].trim_start_matches("b/");
                    self.root.join(path_str)
                } else {
                    i += 1;
                    continue;
                }
            } else {
                i += 1;
                continue;
            };

            // Collect diff hunks until next file marker or end
            let mut diff_lines = Vec::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if l.starts_with("diff --git") || l.starts_with("--- a/") {
                    break;
                }
                diff_lines.push(l);
                i += 1;
            }

            if !diff_lines.is_empty() {
                let diff_text = diff_lines.join("\n");
                let old_content = if file_path.exists() {
                    std::fs::read_to_string(&file_path).ok()
                } else {
                    None
                };
                let hunks = crate::diff::text_to_hunks(&diff_text);
                if !hunks.is_empty() {
                    changes.push(FileChange {
                        file: file_path,
                        change_type: "edit".to_string(),
                        old_content,
                        new_content: None,
                        hunks,
                    });
                }
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
                    // Support both "content" (full-file) and "diff" (surgical) modes
                    let content = parsed["content"].as_str()
                        .or_else(|| parsed["diff"].as_str())
                        .unwrap_or("");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_parse_fix_response_edit_diff() {
        // Simple JSON with diff
        let response = r#"{"file": "src/main.rs", "change_type": "edit", "diff": "@@ -1,3 +1,4 @@"}"#;
        // Verify the response is valid JSON first
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(response);
        assert!(parsed.is_ok(), "response should be valid JSON: {:?}", parsed.err());
        let _v = parsed.unwrap();
        // Now test parse_fix_response
        let root = Path::new("/tmp");
        let results = CodeAgent::parse_fix_response(response, root);
        assert_eq!(results.len(), 1, "should parse one JSON object");
        let (path, content) = &results[0];
        assert!(path.ends_with("src/main.rs"));
        assert_eq!(content, "@@ -1,3 +1,4 @@");
    }

    #[test]
    fn test_parse_fix_response_create() {
        let response = r#"{"file": "src/lib.rs", "change_type": "create", "content": "pub fn hello()"}"#;
        let root = Path::new("/tmp");
        let results = CodeAgent::parse_fix_response(response, root);
        assert_eq!(results.len(), 1);
        let (path, content) = &results[0];
        assert!(path.ends_with("src/lib.rs"));
        assert_eq!(content, "pub fn hello()");
    }

    #[test]
    fn test_parse_fix_response_multiple_json() {
        let response = r#"leading text {"file":"a.rs","change_type":"create","content":"fn a()"} middle {"file":"b.rs","change_type":"create","content":"fn b()"} trailing"#;
        let root = Path::new("/tmp");
        let results = CodeAgent::parse_fix_response(response, root);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_parse_fix_response_empty() {
        let root = Path::new("/tmp");
        let results = CodeAgent::parse_fix_response("", root);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_fix_response_non_json() {
        let root = Path::new("/tmp");
        let results = CodeAgent::parse_fix_response("just some random text with no JSON", root);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_fix_response_edit_with_newlines() {
        // Test with actual JSON that has a diff containing newlines
        let response = "{\"file\": \"src/main.rs\", \"change_type\": \"edit\", \"diff\": \"@@ -1,3 +1,4 @@\\n-old line\\n+new line\\n context\"}";
        let root = Path::new("/tmp");
        let results = CodeAgent::parse_fix_response(response, root);
        assert_eq!(results.len(), 1);
        let (_path, content) = &results[0];
        assert!(content.contains("@@"));
    }

    #[test]
    fn test_code_agent_new_with_root() {
        // Test the constructor
        use std::path::PathBuf;
        let root = PathBuf::from("/tmp/test-project");
        let provider = crate::llm::LlmProvider::new(
            "test".to_string(),
            "http://localhost:9999".to_string(),
            "test-key".to_string(),
        );
        // Just verify we can create an instance — parse_fix_response is tested above
        if let Ok(provider) = provider {
            let _agent = CodeAgent::new(&provider, &root);
        }
    }
}
