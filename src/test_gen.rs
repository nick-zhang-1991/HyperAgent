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
            Message { 
                role: "system".to_string(),
                content: system_prompt,
            },
            Message { 
                role: "user".to_string(),
                content: format!("Generate tests{}:", function_context),
            },
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
