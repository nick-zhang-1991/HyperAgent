//! Auto test generation — uses LLM to generate #[test] functions

use crate::llm::{LlmProvider, Message};
use anyhow::Result;
use std::path::Path;

/// Generate a test for a specific function/module
pub async fn generate_test(
    provider: &LlmProvider,
    _file_path: &Path,
    file_content: &str,
    target_function: Option<&str>,
) -> Result<String> {
    let function_context = match target_function {
        Some(f) => format!(" for the function `{f}`"),
        None => " for this file".to_string(),
    };

    let system_prompt = format!(
        r#"You are a test generation assistant. Generate Rust #[test] functions{context}.

Rules:
- Output ONLY valid Rust code (no markdown, no explanation)
- Use #[cfg(test)] module and #[test] attributes
- Cover normal cases and edge cases
- Use assert! and assert_eq! for verification
- Follow existing test patterns in the codebase
- Keep tests focused and simple
- DO NOT include any text outside the code block

Input code:
```rust
{content}
```

Generate the test functions:"#,
        context = function_context,
        content = file_content
    );

    let response = provider
        .chat(vec![
            Message::text("system", system_prompt),
            Message::text("user", format!("Generate tests{}:", function_context)),
        ])
        .await?;

    // Extract code from potential markdown
    let code = if let Some(start) = response.find("```") {
        let after = &response[start + 3..];
        if let Some(end) = after.find("```") {
            // Skip language tag if present
            let code_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
            &after[code_start..end]
        } else {
            &response[start + 3..]
        }
    } else {
        &response
    };

    Ok(code.trim().to_string())
}

/// Add generated tests to a file
pub fn append_tests_to_file(path: &Path, tests: &str) -> Result<()> {
    let mut content = std::fs::read_to_string(path)?;

    // Check if tests already exist
    if content.contains("#[cfg(test)]") {
        anyhow::bail!("Tests already exist in {}. Remove them first or use `--force`.", path.display());
    }

    content.push_str("\n\n");
    content.push_str(tests);
    std::fs::write(path, content)?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Extract test code from LLM response. The pure-function extraction logic
    /// inside generate_test is duplicated here as `extract_code` so we can test it.
    /// (generate_test itself requires a real LLMProvider.)
    fn extract_code(response: &str) -> &str {
        if let Some(start) = response.find("```") {
            let after = &response[start + 3..];
            if let Some(end) = after.find("```") {
                let code_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
                return after[code_start..end].trim();
            }
            return &response[start + 3..];
        }
        response
    }

    #[test]
    fn test_extract_code_plain() {
        let r = "fn foo() {}";
        assert_eq!(extract_code(r), "fn foo() {}");
    }

    #[test]
    fn test_extract_code_with_rust_fence() {
        let r = "```rust\nfn foo() {}\n```";
        let extracted = extract_code(r);
        assert!(extracted.contains("fn foo()"));
        assert!(!extracted.contains("```"));
    }

    #[test]
    fn test_extract_code_with_plain_fence() {
        let r = "```\nfn bar() {}\n```";
        let extracted = extract_code(r);
        assert!(extracted.contains("fn bar()"));
    }

    #[test]
    fn test_extract_code_with_prose() {
        let r = "Here is the test:\n```rust\n#[test]\nfn x() {}\n```\nDone.";
        let extracted = extract_code(r);
        assert!(extracted.contains("#[test]"));
        assert!(extracted.contains("fn x()"));
        assert!(!extracted.contains("Here is"));
    }

    #[test]
    fn test_extract_code_no_close_fence() {
        let r = "```rust\nfn x() {}\nno close";
        let extracted = extract_code(r);
        assert!(extracted.contains("fn x()"));
    }

    #[test]
    fn test_extract_code_empty() {
        assert_eq!(extract_code(""), "");
    }

    #[test]
    fn test_append_tests_to_file_creates_with_cfgs() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hyperagent_tg_{}.rs", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "fn existing() {}\n").unwrap();

        append_tests_to_file(&path, "#[test]\nfn new_one() {}\n").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("fn existing()"));
        assert!(content.contains("#[test]"));
        assert!(content.contains("fn new_one()"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_append_tests_to_file_errors_if_cfg_test_exists() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hyperagent_tg2_{}.rs", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "fn foo() {}\n#[cfg(test)]\nmod tests {}\n").unwrap();

        let result = append_tests_to_file(&path, "#[test]\nfn new() {}");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Tests already exist"));
        assert!(msg.contains("--force"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_append_tests_to_file_adds_blank_lines() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hyperagent_tg3_{}.rs", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "fn x() {}").unwrap();

        append_tests_to_file(&path, "#[test]\nfn t() {}").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        // Should have proper separation
        assert!(content.contains("\n\n#[test]"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_append_tests_to_file_nonexistent_path_errors() {
        let result = append_tests_to_file(std::path::Path::new("/nonexistent/12345/foo.rs"), "x");
        assert!(result.is_err());
    }

    #[test]
    fn test_append_tests_to_file_empty_tests_string() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hyperagent_tg4_{}.rs", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "fn x() {}").unwrap();

        append_tests_to_file(&path, "").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("fn x()"));
        std::fs::remove_file(&path).ok();
    }
}
