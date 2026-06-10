//! Agent swarm — parallel multi-agent task execution.
//!
//! ```
//! hyper swarm "build REST API with auth + rate limiting + tests"
//! ```
//!
//! Master agent decomposes the task, spawns N sub-agents in parallel,
//! waits for completion, and merges results.

use crate::i18n;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// Run a swarm of agents on a complex task
pub async fn run(prompt: &str, agents: usize, dir: &PathBuf) -> Result<()> {
    println!("🐝 {}", i18n::t_with("swarm_title", &[&agents.to_string()]));
    println!("   Task: {}\n", prompt);

    // Step 1: Master decomposes the task
    println!("🧠 {}", i18n::t("swarm_decomposing"));
    let sub_tasks = decompose_task(prompt, agents).await?;

    if sub_tasks.is_empty() {
        anyhow::bail!("Could not decompose task into sub-tasks");
    }

    println!("   {}", i18n::t_with("swarm_decomposed", &[&sub_tasks.len().to_string()]));
    for (i, t) in sub_tasks.iter().enumerate() {
        println!("   {}. {}", i + 1, t.title);
    }
    println!();

    // Step 2: Execute all sub-tasks in parallel
    println!("🚀 Launching agents...\n");
    let mut handles = Vec::new();

    for (i, task) in sub_tasks.iter().cloned().enumerate() {
        let prompt_clone = task.prompt.clone();
        let dir_clone = dir.clone();
        let idx = i + 1;
        let title_clone = task.title.clone();

        let handle = tokio::spawn(async move {
            println!("   {}", i18n::t_with("swarm_agent_start", &[&idx.to_string(), &title_clone]));
            let result = run_agent(&prompt_clone, &dir_clone, idx).await;
            match &result {
                Ok(()) => println!("   [Agent {}] ✅ Complete", idx),
                Err(e) => println!("   [Agent {}] ❌ Failed: {}", idx, e),
            }
            result
        });

        handles.push((idx, task.title.clone(), handle));
    }

    // Step 3: Wait for all agents
    let mut failed = Vec::new();
    let mut succeeded = 0;

    for (idx, title, handle) in handles {
        match handle.await {
            Ok(Ok(())) => succeeded += 1,
            Ok(Err(e)) => failed.push((idx, title, e.to_string())),
            Err(e) => failed.push((idx, title, e.to_string())),
        }
    }

    // Step 4: Report
    println!("\n   ┌─ Swarm Report ────────────────────────────────────────┐");
    println!("   │ ✅ Succeeded: {:>3}/{}                                    │", succeeded, sub_tasks.len());
    println!("   │ ❌ Failed:    {:>3}                                      │", failed.len());
    println!("   └────────────────────────────────────────────────────────┘");

    if !failed.is_empty() {
        println!("\n   Failed tasks:");
        for (idx, title, err) in &failed {
            println!("   [Agent {}] {}: {}", idx, title, err);
        }
    } else {
        println!("\n   🎉 All agents completed successfully!");
        println!("   Run `git diff` to see changes.");
    }

    Ok(())
}

/// Decompose a complex task into sub-tasks using the LLM
async fn decompose_task(prompt: &str, num_agents: usize) -> Result<Vec<SubTask>> {
    let config = crate::config::AppConfig::load();

    let provider = crate::llm::LlmProvider::new(
        config
            .providers
            .first()
            .map(|c| c.models.first().map(|s| s.as_str()).unwrap_or("gpt-4o"))
            .unwrap_or("gpt-4o"),
        config
            .providers
            .first()
            .map(|c| c.base_url.as_str())
            .unwrap_or("https://api.openai.com/v1"),
        config
            .providers
            .first()
            .map(|c| c.api_key.as_str())
            .unwrap_or(""),
    )
    .context("Failed to create provider")?;

    let mut provider = provider;

    let decomposition_prompt = format!(
        "You are a task decomposition agent. Break down the following complex programming task into {} independent sub-tasks. Each sub-task should be:\n\
         1. Self-contained (no dependencies on other sub-tasks)\n\
         2. Clearly scoped (one file or module)\n\
         3. Testable independently\n\n\
         Task: {}\n\n\
         Output format (JSON array):\n\
         ```json\n\
         [\n   {{\"title\": \"Add auth middleware\", \"prompt\": \"Generate Rust auth middleware using JWT\"}},\n   ...\n\
         ]\n\
         ```\n\n\
         Output ONLY the JSON array, no other text.",
        num_agents, prompt
    );

    let response = provider.chat(vec![
        crate::llm::Message::text("system", "You are a task decomposition agent. Output ONLY valid JSON."),
        crate::llm::Message::text("user", &decomposition_prompt),
    ]).await
    .context("Failed to decompose task")?;

    // Parse JSON response
    let json_str = if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            &response[start..=end]
        } else {
            &response
        }
    } else {
        &response
    };

    let sub_tasks: Vec<SubTask> = serde_json::from_str(json_str)
        .context(format!("Failed to parse decomposition. Raw: {}", &json_str[..200.min(json_str.len())]))?;

    Ok(sub_tasks)
}

/// Run a single agent for a sub-task
async fn run_agent(prompt: &str, dir: &PathBuf, agent_idx: usize) -> Result<()> {
    let hyper_bin = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("hyperagent"));

    let output = Command::new(&hyper_bin)
        .args(["run", "--mode", "code", prompt])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .context("Failed to run hyper sub-agent")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Agent {} failed: {}", agent_idx, stderr.lines().last().unwrap_or("unknown"));
    }

    Ok(())
}

#[derive(serde::Deserialize, Clone)]
struct SubTask {
    title: String,
    prompt: String,
}
