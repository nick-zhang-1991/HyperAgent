//! Agent Modes — role-based behavior (inspired by Roo-Code)
//!
//! Each mode defines:
//! - System prompt (behavior)
//! - Allowed tools (permissions)
//! - Model preference
//! - Temperature
//!
//! Built-in modes: Code (default), Architect, Ask, Debug
//! Custom modes can be defined in config.toml

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Built-in and custom agent modes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum ModeKind {
    /// Default — write production code, run tests, edit files
    Code,
    /// Plan architecture, design systems, draw diagrams
    Architect,
    /// Answer questions about the codebase (read-only)
    Ask,
    /// Root-cause analysis, add tracing, fix bugs
    Debug,
    /// User-defined custom mode
    Custom(String),
}

impl std::fmt::Display for ModeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModeKind::Code => write!(f, "code"),
            ModeKind::Architect => write!(f, "architect"),
            ModeKind::Ask => write!(f, "ask"),
            ModeKind::Debug => write!(f, "debug"),
            ModeKind::Custom(name) => write!(f, "custom:{name}"),
        }
    }
}

/// Tool permission level per mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ToolAccess {
    /// Tool not available
    Denied,
    /// Can read but not write
    ReadOnly,
    /// Full read/write access
    Allowed,
}

/// Tool group permissions for a mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModePermissions {
    pub edit_files: ToolAccess,
    pub read_files: ToolAccess,
    pub run_commands: ToolAccess,
    pub network: ToolAccess,
    pub git: ToolAccess,
    pub search: ToolAccess,
}

impl Default for ModePermissions {
    fn default() -> Self {
        Self {
            edit_files: ToolAccess::Allowed,
            read_files: ToolAccess::Allowed,
            run_commands: ToolAccess::Allowed,
            network: ToolAccess::Allowed,
            git: ToolAccess::Allowed,
            search: ToolAccess::Allowed,
        }
    }
}

impl ModePermissions {
    /// Permissions for ask mode — read-only, no command execution
    pub fn ask() -> Self {
        Self {
            edit_files: ToolAccess::Denied,
            read_files: ToolAccess::Allowed,
            run_commands: ToolAccess::Denied,
            network: ToolAccess::ReadOnly,
            git: ToolAccess::ReadOnly,
            search: ToolAccess::Allowed,
        }
    }

    /// Permissions for architect mode — can read/search/git, plan but no edits
    pub fn architect() -> Self {
        Self {
            edit_files: ToolAccess::ReadOnly,
            read_files: ToolAccess::Allowed,
            run_commands: ToolAccess::ReadOnly,
            network: ToolAccess::Allowed,
            git: ToolAccess::Allowed,
            search: ToolAccess::Allowed,
        }
    }

    /// Permissions for debug mode — can read files and run commands, cautious edits
    pub fn debug() -> Self {
        Self {
            edit_files: ToolAccess::ReadOnly,
            read_files: ToolAccess::Allowed,
            run_commands: ToolAccess::Allowed,
            network: ToolAccess::ReadOnly,
            git: ToolAccess::ReadOnly,
            search: ToolAccess::Allowed,
        }
    }
}

/// A complete mode definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeConfig {
    /// Display name
    pub name: String,
    /// Short description
    pub description: String,
    /// System prompt that defines behavior
    pub system_prompt: String,
    /// Tool access permissions
    pub permissions: ModePermissions,
    /// Default model (None = use default from config)
    pub model: Option<String>,
    /// Temperature (None = use default)
    pub temperature: Option<f32>,
}

/// Registry of all available modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeRegistry {
    pub modes: HashMap<String, ModeConfig>,
    pub default_mode: String,
}

impl Default for ModeRegistry {
    fn default() -> Self {
        let mut modes = HashMap::new();

        modes.insert("code".to_string(), ModeConfig {
            name: "Code".into(),
            description: "Everyday coding — edit files, run commands, write production code".into(),
            system_prompt: CODE_MODE_PROMPT.into(),
            permissions: ModePermissions::default(),
            model: None,
            temperature: None,
        });

        modes.insert("architect".to_string(), ModeConfig {
            name: "Architect".into(),
            description: "Plan systems, design architectures, write specs and migration plans".into(),
            system_prompt: ARCHITECT_MODE_PROMPT.into(),
            permissions: ModePermissions::architect(),
            model: None,
            temperature: Some(0.3),
        });

        modes.insert("ask".to_string(), ModeConfig {
            name: "Ask".into(),
            description: "Quick answers, explanations, documentation queries (read-only)".into(),
            system_prompt: ASK_MODE_PROMPT.into(),
            permissions: ModePermissions::ask(),
            model: None,
            temperature: Some(0.2),
        });

        modes.insert("debug".to_string(), ModeConfig {
            name: "Debug".into(),
            description: "Root-cause analysis, add trace logs, isolate bugs, fix issues".into(),
            system_prompt: DEBUG_MODE_PROMPT.into(),
            permissions: ModePermissions::debug(),
            model: None,
            temperature: Some(0.1),
        });

        Self {
            modes,
            default_mode: "code".into(),
        }
    }
}

impl ModeRegistry {
    pub fn get(&self, name: &str) -> Option<&ModeConfig> {
        self.modes.get(name)
    }

    pub fn list(&self) -> Vec<&ModeConfig> {
        self.modes.values().collect()
    }

    #[allow(dead_code)]
    /// Add or override a custom mode
    pub fn register(&mut self, name: String, config: ModeConfig) {
        self.modes.insert(name, config);
    }

    #[allow(dead_code)]
    /// Load custom modes from config.toml `[modes.*]` sections
    pub fn load_from_config(&mut self, config_modes: HashMap<String, ModeConfig>) {
        for (name, config) in config_modes {
            self.modes.insert(name, config);
        }
    }

    #[allow(dead_code)]
    /// Get the system prompt for a mode, combined with base context
    pub fn build_prompt(&self, mode: &str, base_instructions: Option<&str>) -> String {
        let mode_cfg = self.modes.get(mode)
            .unwrap_or_else(|| self.modes.get("code").unwrap());

        let base = base_instructions.unwrap_or("");
        format!(
            "{}\n\n--- Agent Mode: {} ---\n{}\n\n--- Permissions ---\n\
             - Edit files: {:?}\n- Run commands: {:?}\n- Network: {:?}\n- Git: {:?}\n- Search: {:?}",
            base,
            mode_cfg.name,
            mode_cfg.system_prompt,
            mode_cfg.permissions.edit_files,
            mode_cfg.permissions.run_commands,
            mode_cfg.permissions.network,
            mode_cfg.permissions.git,
            mode_cfg.permissions.search,
        )
    }
}

// ═══════════════════════════════════════════════
// Built-in mode system prompts
// ═══════════════════════════════════════════════

const CODE_MODE_PROMPT: &str = r#"You are in **Code Mode** — write production-quality code.

Guidelines:
1. **Think before coding** — understand the problem fully before writing solutions.
2. **Be surgical** — make minimal, focused changes. Don't refactor unrelated code.
3. **Remove orphans** — when replacing old code, remove the old version.
4. **Verify** — after changes, verify correctness. Run tests if possible.
5. **Simplicity first** — prefer straightforward solutions over clever ones.
6. **Respect existing style** — match the project's code conventions.
7. **Handle errors** — never leave unwrap() in production code."#;

const ARCHITECT_MODE_PROMPT: &str = r#"You are in **Architect Mode** — design systems, not code.

Guidelines:
1. **Understand the big picture** — read the project structure first.
2. **Design before details** — propose architecture, data flow, and component boundaries.
3. **YAGNI** — don't add complexity that isn't needed yet.
4. **Document decisions** — explain trade-offs and why you chose this approach.
5. **Output plans** — produce actionable implementation plans, not code.
6. **Consider migrations** — if changing existing systems, plan the migration path.
7. **Use Mermaid/ASCII diagrams** — communicate structure visually."#;

const ASK_MODE_PROMPT: &str = r#"You are in **Ask Mode** — answer questions about the codebase.

Guidelines:
1. **Read first** — search the codebase before answering. Cite specific files/lines.
2. **Be accurate** — if you're not sure, say so. Don't make up APIs.
3. **Explain why** — not just what the code does, but why it was done that way.
4. **Keep it concise** — answer the question directly, then offer to elaborate.
5. **No edits** — this mode is read-only. Don't modify files or run commands."#;

const DEBUG_MODE_PROMPT: &str = r#"You are in **Debug Mode** — find and fix bugs.

Guidelines:
1. **Reproduce first** — understand the expected vs actual behavior.
2. **Isolate** — narrow down to the smallest scope that exhibits the bug.
3. **Add trace logging** — insert temporary logging to observe state flow.
4. **Check assumptions** — verify types, null safety, edge cases, error paths.
5. **One fix at a time** — apply the minimal fix, verify it, then move on.
6. **Clean up** — remove any temporary debug logging after fixing.
7. **Document root cause** — explain what caused the bug in the fix PR."#;
