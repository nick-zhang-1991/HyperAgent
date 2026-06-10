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

    #[allow(dead_code)]
    /// Load hooks from config
    pub fn load_from_config(&mut self, hooks: Vec<Hook>) {
        self.hooks = hooks;
    }

    #[allow(dead_code)]
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
                // Security check before executing command
                let policy = crate::security::SecurityPolicy::default();
                let safety = crate::security::check_command_safety(command, &policy);
                if !crate::security::confirm_dangerous_action(&safety, false) {
                    let msg = format!("Hook '{}' blocked by security policy", hook.description.as_deref().unwrap_or(command));
                    if *required == Some(true) {
                        anyhow::bail!("{msg}");
                    } else {
                        eprintln!("  ⚠️  {msg}");
                        return Ok(());
                    }
                }
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_event_display_all_variants() {
        assert_eq!(HookEvent::PrePlan.to_string(), "pre_plan");
        assert_eq!(HookEvent::PostPlan.to_string(), "post_plan");
        assert_eq!(HookEvent::PreCode.to_string(), "pre_code");
        assert_eq!(HookEvent::PostCode.to_string(), "post_code");
        assert_eq!(HookEvent::PreApply.to_string(), "pre_apply");
        assert_eq!(HookEvent::PostApply.to_string(), "post_apply");
        assert_eq!(HookEvent::PreReview.to_string(), "pre_review");
        assert_eq!(HookEvent::PostReview.to_string(), "post_review");
        assert_eq!(HookEvent::PreRun.to_string(), "pre_run");
        assert_eq!(HookEvent::PostRun.to_string(), "post_run");
        assert_eq!(HookEvent::OnError.to_string(), "on_error");
        assert_eq!(HookEvent::OnComplete.to_string(), "on_complete");
    }

    #[test]
    fn test_hook_event_equality() {
        assert_eq!(HookEvent::PrePlan, HookEvent::PrePlan);
        assert_ne!(HookEvent::PrePlan, HookEvent::PostPlan);
        assert_ne!(HookEvent::OnError, HookEvent::OnComplete);
    }

    #[test]
    fn test_hook_event_serde() {
        let event = HookEvent::PrePlan;
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, "\"pre_plan\"");
        let back: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, HookEvent::PrePlan);
    }

    #[test]
    fn test_hook_event_all_serde_roundtrips() {
        for event in [
            HookEvent::PrePlan, HookEvent::PostPlan,
            HookEvent::PreCode, HookEvent::PostCode,
            HookEvent::PreApply, HookEvent::PostApply,
            HookEvent::PreReview, HookEvent::PostReview,
            HookEvent::PreRun, HookEvent::PostRun,
            HookEvent::OnError, HookEvent::OnComplete,
        ] {
            let json = serde_json::to_string(&event).unwrap();
            let back: HookEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, back);
        }
    }

    #[test]
    fn test_hook_action_command_serde() {
        let action = HookAction::Command {
            command: "cargo build".into(),
            required: Some(true),
            timeout: Some(60),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\""));
        assert!(json.contains("\"command\""));
        assert!(json.contains("\"required\":true"));
        let back: HookAction = serde_json::from_str(&json).unwrap();
        match back {
            HookAction::Command { command, required, timeout } => {
                assert_eq!(command, "cargo build");
                assert_eq!(required, Some(true));
                assert_eq!(timeout, Some(60));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_hook_action_script_serde() {
        let action = HookAction::Script {
            path: "./scripts/lint.sh".into(),
            args: Some(vec!["--strict".into()]),
            required: None,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"type\""));
        assert!(json.to_lowercase().contains("\"script\""));
        let back: HookAction = serde_json::from_str(&json).unwrap();
        match back {
            HookAction::Script { path, args, .. } => {
                assert_eq!(path, "./scripts/lint.sh");
                assert_eq!(args, Some(vec!["--strict".to_string()]));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_hook_action_webhook_serde() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Token".to_string(), "secret".to_string());
        let action = HookAction::Webhook {
            url: "https://example.com/hook".into(),
            headers: Some(headers),
            sync: Some(true),
        };
        let json = serde_json::to_string(&action).unwrap();
        let back: HookAction = serde_json::from_str(&json).unwrap();
        match back {
            HookAction::Webhook { url, sync, .. } => {
                assert_eq!(url, "https://example.com/hook");
                assert_eq!(sync, Some(true));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_hook_action_log_serde() {
        let action = HookAction::Log {
            message: "Build complete".into(),
            level: Some("info".into()),
        };
        let json = serde_json::to_string(&action).unwrap();
        let back: HookAction = serde_json::from_str(&json).unwrap();
        match back {
            HookAction::Log { message, level } => {
                assert_eq!(message, "Build complete");
                assert_eq!(level, Some("info".to_string()));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_hook_serde() {
        let hook = Hook {
            event: HookEvent::PreCode,
            action: HookAction::Log {
                message: "starting".into(),
                level: None,
            },
            description: Some("Pre-code notification".into()),
            modes: Some(vec!["code".into()]),
            if_condition: None,
        };
        let json = serde_json::to_string(&hook).unwrap();
        let back: Hook = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event, HookEvent::PreCode);
        assert_eq!(back.description.as_deref(), Some("Pre-code notification"));
        assert_eq!(back.modes, Some(vec!["code".to_string()]));
    }

    // ── HookRegistry ────────────────────────────────────────

    #[test]
    fn test_hook_registry_new_is_empty() {
        let reg = HookRegistry::new(std::path::Path::new("/tmp"));
        assert!(reg.list_hooks().is_empty());
    }

    #[test]
    fn test_hook_registry_register_adds() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "x".into(), level: None },
            description: None,
            modes: None,
            if_condition: None,
        });
        assert_eq!(reg.list_hooks().len(), 1);
    }

    #[test]
    fn test_hook_registry_dedup_on_event() {
        // register() removes existing hooks with same event
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "first".into(), level: None },
            description: Some("first".into()),
            modes: None,
            if_condition: None,
        });
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "second".into(), level: None },
            description: Some("second".into()),
            modes: None,
            if_condition: None,
        });
        // Should have only the second one
        assert_eq!(reg.list_hooks().len(), 1);
        assert_eq!(reg.list_hooks()[0].description.as_deref(), Some("second"));
    }

    #[test]
    fn test_hook_registry_different_events_coexist() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "a".into(), level: None },
            description: None,
            modes: None,
            if_condition: None,
        });
        reg.register(Hook {
            event: HookEvent::PostPlan,
            action: HookAction::Log { message: "b".into(), level: None },
            description: None,
            modes: None,
            if_condition: None,
        });
        assert_eq!(reg.list_hooks().len(), 2);
    }

    #[test]
    fn test_hook_registry_load_from_config() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        let hooks = vec![
            Hook {
                event: HookEvent::PreCode,
                action: HookAction::Log { message: "x".into(), level: None },
                description: None,
                modes: None,
                if_condition: None,
            },
            Hook {
                event: HookEvent::PostCode,
                action: HookAction::Log { message: "y".into(), level: None },
                description: None,
                modes: None,
                if_condition: None,
            },
        ];
        reg.load_from_config(hooks);
        assert_eq!(reg.list_hooks().len(), 2);
    }

    #[test]
    fn test_fire_with_no_hooks_is_ok() {
        let reg = HookRegistry::new(std::path::Path::new("/tmp"));
        let result = reg.fire(&HookEvent::PrePlan, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_fire_with_log_action_succeeds() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "hello".into(), level: Some("info".into()) },
            description: Some("test log".into()),
            modes: None,
            if_condition: None,
        });
        // Should not panic; just prints
        let result = reg.fire(&HookEvent::PrePlan, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_fire_with_mode_filter_excludes() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "x".into(), level: None },
            description: None,
            modes: Some(vec!["architect".into()]),
            if_condition: None,
        });
        // Mode filter doesn't match — hook should be skipped silently
        let result = reg.fire(&HookEvent::PrePlan, Some("code"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_fire_with_mode_filter_matches() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "x".into(), level: None },
            description: None,
            modes: Some(vec!["code".into(), "architect".into()]),
            if_condition: None,
        });
        let result = reg.fire(&HookEvent::PrePlan, Some("architect"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_fire_with_failing_condition_skipped() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "x".into(), level: None },
            description: None,
            modes: None,
            if_condition: Some("false".into()),
        });
        // condition is `false` which exits 1, so hook is skipped
        let result = reg.fire(&HookEvent::PrePlan, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_fire_with_passing_condition_runs() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "x".into(), level: None },
            description: None,
            modes: None,
            if_condition: Some("true".into()),
        });
        let result = reg.fire(&HookEvent::PrePlan, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_fire_unrelated_event_does_not_match() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "x".into(), level: None },
            description: None,
            modes: None,
            if_condition: None,
        });
        // Fire a different event
        let result = reg.fire(&HookEvent::PostRun, None);
        assert!(result.is_ok());
        // Hook should not have run (no output check but shouldn't crash)
    }

    #[test]
    fn test_evaluate_condition_via_fire() {
        let mut reg = HookRegistry::new(std::path::Path::new("/tmp"));
        reg.register(Hook {
            event: HookEvent::PrePlan,
            action: HookAction::Log { message: "ran".into(), level: None },
            description: None,
            modes: None,
            if_condition: Some("[ -d /tmp ]".into()),
        });
        // /tmp exists so condition passes
        let result = reg.fire(&HookEvent::PrePlan, None);
        assert!(result.is_ok());
    }
}
