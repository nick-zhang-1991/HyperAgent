//! Hooks System — pre/post event hooks for agent lifecycle
//!
//! Inspired by codex's hooks system.
//!
//! Events that can be hooked:
//! - pre_plan, post_plan
//! - pre_code, post_code
//! - pre_apply, post_apply
//! - pre_review, post_review
//! - pre_run, post_run
//! - on_error
//! - on_complete
//!
//! Hook actions:
//! - Run a shell script
//! - Run a command
//! - Execute a Python/JS script
//! - Send a webhook notification

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Event names in the agent lifecycle
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PrePlan,
    PostPlan,
    PreCode,
    PostCode,
    PreApply,
    PostApply,
    PreReview,
    PostReview,
    PreRun,
    PostRun,
    OnError,
    OnComplete,
}

impl std::fmt::Display for HookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookEvent::PrePlan => write!(f, "pre_plan"),
            HookEvent::PostPlan => write!(f, "post_plan"),
            HookEvent::PreCode => write!(f, "pre_code"),
            HookEvent::PostCode => write!(f, "post_code"),
            HookEvent::PreApply => write!(f, "pre_apply"),
            HookEvent::PostApply => write!(f, "post_apply"),
            HookEvent::PreReview => write!(f, "pre_review"),
            HookEvent::PostReview => write!(f, "post_review"),
            HookEvent::PreRun => write!(f, "pre_run"),
            HookEvent::PostRun => write!(f, "post_run"),
            HookEvent::OnError => write!(f, "on_error"),
            HookEvent::OnComplete => write!(f, "on_complete"),
        }
    }
}

/// What to do when a hook fires
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HookAction {
    /// Run a shell command
    Command {
        command: String,
        /// If true, hook failure fails the agent
        required: Option<bool>,
        /// Timeout in seconds
        timeout: Option<u64>,
    },
    /// Run a script file
    Script {
        path: String,
        args: Option<Vec<String>>,
        required: Option<bool>,
    },
    /// Send a webhook notification (POST with JSON body)
    Webhook {
        url: String,
        headers: Option<HashMap<String, String>>,
        /// If true, wait for response before continuing
        sync: Option<bool>,
    },
    /// Log a message (no-op, but visible in tracing)
    Log {
        message: String,
        level: Option<String>,
    },
}

/// A complete hook definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub event: HookEvent,
    pub action: HookAction,
    pub description: Option<String>,
    /// Run this hook only on matching agent modes (code/architect/ask/debug)
    pub modes: Option<Vec<String>>,
    /// Run this hook only if the condition script exits 0
    pub if_condition: Option<String>,
}

/// Registry of all hooks
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookRegistry {
    /// Hooks indexed by event
    #[serde(skip)]
    hooks: Vec<Hook>,
    /// Project root for relative paths
    #[serde(skip)]
    project_root: PathBuf,
}

impl HookRegistry {
    pub fn new(project_root: &Path) -> Self {
        Self {
            hooks: Vec::new(),
            project_root: project_root.to_path_buf(),
        }
    }

    /// Load hooks from config
    pub fn load_from_config(&mut self, hooks: Vec<Hook>) {
        self.hooks = hooks;
    }

    /// Register a single hook
    pub fn register(&mut self, hook: Hook) {
        // Remove existing hooks with same event+action combination (dedup)
        self.hooks.retain(|h| !(h.event == hook.event));
        self.hooks.push(hook);
    }

    /// Fire a hook event — runs all matching hooks
    pub fn fire(&self, event: &HookEvent, mode: Option<&str>) -> anyhow::Result<()> {
        let matching: Vec<&Hook> = self.hooks.iter()
            .filter(|h| h.event == *event)
            .filter(|h| {
                // If modes filter is set, check if current mode matches
                h.modes.as_ref().map_or(true, |modes| {
                    mode.map_or(true, |m| modes.contains(&m.to_string()))
                })
            })
            .filter(|h| {
                // Check condition
                h.if_condition.as_ref().map_or(true, |cond| {
                    self.evaluate_condition(cond)
                })
            })
            .collect();

        if matching.is_empty() {
            return Ok(());
        }

        for hook in &matching {
            if let Some(desc) = &hook.description {
                println!("  🪝 Hook: {desc}");
            }
            self.execute_hook(hook, event)?;
        }

        Ok(())
    }

    fn execute_hook(&self, hook: &Hook, event: &HookEvent) -> anyhow::Result<()> {
        match &hook.action {
            HookAction::Command { command, required, timeout: _ } => {
                let output = Command::new("sh")
                    .args(["-c", command])
                    .current_dir(&self.project_root)
                    .output()?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let msg = format!("Hook '{}' failed: {stderr}", hook.description.as_deref().unwrap_or(command));
                    if *required == Some(true) {
                        anyhow::bail!(msg);
                    } else {
                        eprintln!("  ⚠️  {msg}");
                    }
                }
            }
            HookAction::Script { path, args, required } => {
                let script_path = if Path::new(path).is_absolute() {
                    PathBuf::from(path)
                } else {
                    self.project_root.join(path)
                };

                let mut cmd = Command::new(&script_path);
                if let Some(ref extra_args) = args {
                    cmd.args(extra_args);
                }
                let output = cmd.current_dir(&self.project_root).output()?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let msg = format!("Hook script '{path}' failed: {stderr}");
                    if *required == Some(true) {
                        anyhow::bail!(msg);
                    } else {
                        eprintln!("  ⚠️  {msg}");
                    }
                }
            }
            HookAction::Webhook { url, headers, sync } => {
                if *sync == Some(true) {
                    // Synchronous webhook — fire and forget would use async
                    let payload = serde_json::json!({
                        "event": event.to_string(),
                        "project": self.project_root.to_string_lossy(),
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });

                    let client = reqwest::blocking::Client::builder()
                        .timeout(std::time::Duration::from_secs(10))
                        .build()?;

                    let mut req = client.post(url).json(&payload);
                    if let Some(ref hdrs) = headers {
                        for (k, v) in hdrs {
                            req = req.header(k, v);
                        }
                    }

                    match req.send() {
                        Ok(resp) => {
                            if !resp.status().is_success() {
                                eprintln!("  ⚠️  Webhook returned {}", resp.status());
                            }
                        }
                        Err(e) => {
                            eprintln!("  ⚠️  Webhook failed: {e}");
                        }
                    }
                }
            }
            HookAction::Log { message, level } => {
                match level.as_deref().unwrap_or("info") {
                    "info" => println!("  📝 {}", message),
                    "warn" => eprintln!("  ⚠️  {message}"),
                    "error" => eprintln!("  ❌ {message}"),
                    _ => println!("  📝 {message}"),
                }
            }
        }
        Ok(())
    }

    fn evaluate_condition(&self, condition: &str) -> bool {
        Command::new("sh")
            .args(["-c", condition])
            .current_dir(&self.project_root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn list_hooks(&self) -> Vec<&Hook> {
        self.hooks.iter().collect()
    }
}
