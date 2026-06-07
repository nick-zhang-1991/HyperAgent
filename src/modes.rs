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
    /// General-purpose assistant — coding, writing, analysis, translation, brainstorming
    General,
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
            ModeKind::General => write!(f, "general"),
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

        modes.insert("general".to_string(), ModeConfig {
            name: "General".into(),
            description: "General-purpose assistant — coding, writing, analysis, translation, brainstorming".into(),
            system_prompt: GENERAL_MODE_PROMPT.into(),
            permissions: ModePermissions::default(),
            model: None,
            temperature: Some(0.3),
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

const ASK_MODE_PROMPT: &str = r#"You are in **Ask Mode** — a knowledgeable assistant for questions and explanations.

Guidelines:
1. **Read first** — search the codebase before answering about code. Cite specific files/lines.
2. **Be accurate** — if you're not sure, say so. Don't make up APIs or facts.
3. **Explain why** — not just what, but why.
4. **Keep it concise** — answer directly, then offer to elaborate.
5. **No edits** — this mode is read-only. Don't modify files or run commands.
6. **General queries welcome** — answer non-coding questions (writing, analysis, translation, etc.) using your knowledge."#;

const GENERAL_MODE_PROMPT: &str = r#"You are in **General Mode** — a versatile assistant for any task.

Guidelines:
1. **Be helpful and accurate** — handle coding, writing, analysis, translation, brainstorming, research, and more.
2. **Use tools when available** — if MCP tools, web search, or knowledge base are available, use them proactively.
3. **Be concise but complete** — answer the question directly, offer depth when asked.
4. **Adapt to context** — if the user is in a project directory, reference the codebase. If not, just help.
5. **Format well** — use markdown, code blocks, lists, and tables as appropriate.
6. **Multimodal aware** — if the user provides images, analyze them carefully and reference visual details.
7. **Language** — answer in the same language as the user's question."#;

const DEBUG_MODE_PROMPT: &str = r#"You are in **Debug Mode** — find and fix bugs.

Guidelines:
1. **Reproduce first** — understand the expected vs actual behavior.
2. **Isolate** — narrow down to the smallest scope that exhibits the bug.
3. **Add trace logging** — insert temporary logging to observe state flow.
4. **Check assumptions** — verify types, null safety, edge cases, error paths.
5. **One fix at a time** — apply the minimal fix, verify it, then move on.
6. **Clean up** — remove any temporary debug logging after fixing.
7. **Document root cause** — explain what caused the bug in the fix PR."#;

#[cfg(test)]
mod tests {
    use super::*;

    // ── ModeKind Display ───────────────────────────────────────

    #[test]
    fn mode_kind_display_built_in_variants() {
        assert_eq!(ModeKind::Code.to_string(), "code");
        assert_eq!(ModeKind::Architect.to_string(), "architect");
        assert_eq!(ModeKind::Ask.to_string(), "ask");
        assert_eq!(ModeKind::General.to_string(), "general");
        assert_eq!(ModeKind::Debug.to_string(), "debug");
    }

    #[test]
    fn mode_kind_display_custom_includes_prefix() {
        let m = ModeKind::Custom("reviewer".into());
        assert_eq!(m.to_string(), "custom:reviewer");
    }

    // ── ModeKind serde (kebab-case) ────────────────────────────

    #[test]
    fn mode_kind_serializes_kebab_case() {
        let v = serde_json::to_value(ModeKind::Code).unwrap();
        assert_eq!(v, serde_json::json!("code"));
        let v = serde_json::to_value(ModeKind::Architect).unwrap();
        assert_eq!(v, serde_json::json!("architect"));
        let v = serde_json::to_value(ModeKind::Custom("reviewer".into())).unwrap();
        assert_eq!(v, serde_json::json!("reviewer"));
    }

    #[test]
    fn mode_kind_deserializes_kebab_case() {
        let m: ModeKind = serde_json::from_value(serde_json::json!("code")).unwrap();
        assert_eq!(m, ModeKind::Code);
        let m: ModeKind = serde_json::from_value(serde_json::json!("debug")).unwrap();
        assert_eq!(m, ModeKind::Debug);
        let m: ModeKind = serde_json::from_value(serde_json::json!("my-custom")).unwrap();
        assert_eq!(m, ModeKind::Custom("my-custom".into()));
    }

    #[test]
    fn mode_kind_round_trip() {
        for kind in [ModeKind::Code, ModeKind::Architect, ModeKind::Ask, ModeKind::General, ModeKind::Debug] {
            let v = serde_json::to_value(&kind).unwrap();
            let back: ModeKind = serde_json::from_value(v).unwrap();
            assert_eq!(kind, back);
        }
    }

    // ── ToolAccess serde ───────────────────────────────────────

    #[test]
    fn tool_access_serializes_kebab_case() {
        assert_eq!(serde_json::to_value(ToolAccess::Denied).unwrap(), serde_json::json!("denied"));
        assert_eq!(serde_json::to_value(ToolAccess::ReadOnly).unwrap(), serde_json::json!("read-only"));
        assert_eq!(serde_json::to_value(ToolAccess::Allowed).unwrap(), serde_json::json!("allowed"));
    }

    // ── Default permission matrix ──────────────────────────────

    #[test]
    fn default_permissions_all_allowed() {
        let p = ModePermissions::default();
        assert_eq!(p.edit_files, ToolAccess::Allowed);
        assert_eq!(p.read_files, ToolAccess::Allowed);
        assert_eq!(p.run_commands, ToolAccess::Allowed);
        assert_eq!(p.network, ToolAccess::Allowed);
        assert_eq!(p.git, ToolAccess::Allowed);
        assert_eq!(p.search, ToolAccess::Allowed);
    }

    #[test]
    fn ask_permissions_lock_down_writes_and_commands() {
        let p = ModePermissions::ask();
        assert_eq!(p.edit_files, ToolAccess::Denied);
        assert_eq!(p.run_commands, ToolAccess::Denied);
        assert_eq!(p.network, ToolAccess::ReadOnly);
        assert_eq!(p.git, ToolAccess::ReadOnly);
        assert_eq!(p.read_files, ToolAccess::Allowed);
        assert_eq!(p.search, ToolAccess::Allowed);
    }

    #[test]
    fn architect_permissions_can_read_design_but_not_edit() {
        let p = ModePermissions::architect();
        assert_eq!(p.edit_files, ToolAccess::ReadOnly);
        assert_eq!(p.run_commands, ToolAccess::ReadOnly);
        assert_eq!(p.read_files, ToolAccess::Allowed);
        assert_eq!(p.network, ToolAccess::Allowed);
        assert_eq!(p.git, ToolAccess::Allowed);
        assert_eq!(p.search, ToolAccess::Allowed);
    }

    #[test]
    fn debug_permissions_allow_commands_readonly_edits() {
        let p = ModePermissions::debug();
        assert_eq!(p.edit_files, ToolAccess::ReadOnly);
        assert_eq!(p.run_commands, ToolAccess::Allowed);
        assert_eq!(p.network, ToolAccess::ReadOnly);
        assert_eq!(p.git, ToolAccess::ReadOnly);
        assert_eq!(p.read_files, ToolAccess::Allowed);
        assert_eq!(p.search, ToolAccess::Allowed);
    }

    // ── Built-in registry ──────────────────────────────────────

    #[test]
    fn default_registry_contains_five_built_in_modes() {
        let r = ModeRegistry::default();
        assert_eq!(r.default_mode, "code");
        assert!(r.modes.contains_key("code"));
        assert!(r.modes.contains_key("architect"));
        assert!(r.modes.contains_key("ask"));
        assert!(r.modes.contains_key("general"));
        assert!(r.modes.contains_key("debug"));
        assert!(r.modes.contains_key("code")); // not 6+ extra
        assert_eq!(r.modes.len(), 5);
    }

    #[test]
    fn default_registry_get_returns_config() {
        let r = ModeRegistry::default();
        let code = r.get("code").unwrap();
        assert_eq!(code.name, "Code");
        assert!(!code.system_prompt.is_empty());
    }

    #[test]
    fn default_registry_get_missing_returns_none() {
        let r = ModeRegistry::default();
        assert!(r.get("nonexistent").is_none());
    }

    #[test]
    fn default_registry_list_returns_all() {
        let r = ModeRegistry::default();
        let v = r.list();
        assert_eq!(v.len(), 5);
    }

    #[test]
    fn default_modes_have_nonempty_prompts() {
        let r = ModeRegistry::default();
        for mode in r.list() {
            assert!(!mode.system_prompt.is_empty(), "{} prompt empty", mode.name);
            assert!(!mode.description.is_empty(), "{} desc empty", mode.name);
        }
    }

    #[test]
    fn code_mode_is_default_and_has_full_permissions() {
        let r = ModeRegistry::default();
        let code = r.get("code").unwrap();
        assert_eq!(code.permissions, ModePermissions::default());
    }

    #[test]
    fn ask_mode_uses_ask_permissions() {
        let r = ModeRegistry::default();
        let ask = r.get("ask").unwrap();
        assert_eq!(ask.permissions, ModePermissions::ask());
    }

    #[test]
    fn architect_mode_uses_architect_permissions() {
        let r = ModeRegistry::default();
        let arch = r.get("architect").unwrap();
        assert_eq!(arch.permissions, ModePermissions::architect());
    }

    #[test]
    fn debug_mode_uses_debug_permissions() {
        let r = ModeRegistry::default();
        let dbg = r.get("debug").unwrap();
        assert_eq!(dbg.permissions, ModePermissions::debug());
    }

    // ── Custom mode registration ───────────────────────────────

    #[test]
    fn register_custom_mode_adds_to_registry() {
        let mut r = ModeRegistry::default();
        let custom = ModeConfig {
            name: "Reviewer".into(),
            description: "Code review specialist".into(),
            system_prompt: "You review code carefully.".into(),
            permissions: ModePermissions::ask(),
            model: Some("gpt-4".into()),
            temperature: Some(0.0),
        };
        r.register("reviewer".into(), custom.clone());
        let fetched = r.get("reviewer").unwrap();
        assert_eq!(fetched.name, "Reviewer");
        assert_eq!(fetched.model, Some("gpt-4".into()));
    }

    #[test]
    fn register_can_override_built_in_mode() {
        let mut r = ModeRegistry::default();
        let mut custom = ModeConfig {
            name: "Code (custom)".into(),
            description: "Override".into(),
            system_prompt: "Custom code prompt".into(),
            permissions: ModePermissions::default(),
            model: None,
            temperature: None,
        };
        custom.permissions = ModePermissions::architect();
        r.register("code".into(), custom);
        let fetched = r.get("code").unwrap();
        assert_eq!(fetched.name, "Code (custom)");
        assert_eq!(fetched.permissions.edit_files, ToolAccess::ReadOnly);
    }

    #[test]
    fn load_from_config_bulk_inserts() {
        let mut r = ModeRegistry::default();
        let mut custom = HashMap::new();
        for name in ["a", "b", "c"] {
            custom.insert(name.into(), ModeConfig {
                name: name.to_uppercase(),
                description: format!("{name} mode"),
                system_prompt: format!("prompt for {name}"),
                permissions: ModePermissions::default(),
                model: None,
                temperature: None,
            });
        }
        r.load_from_config(custom);
        assert!(r.get("a").is_some());
        assert!(r.get("b").is_some());
        assert!(r.get("c").is_some());
        // Built-ins still present
        assert!(r.get("code").is_some());
        assert_eq!(r.modes.len(), 8);
    }

    // ── build_prompt ───────────────────────────────────────────

    #[test]
    fn build_prompt_includes_mode_name_and_permissions() {
        let r = ModeRegistry::default();
        let p = r.build_prompt("ask", None);
        assert!(p.contains("Ask"), "should include mode name in prompt: {p}");
        assert!(p.contains("edit_files: denied")
            || p.contains("Edit files: denied")
            || p.contains("Edit files: Denied"),
            "should include permission summary: {p}");
    }

    #[test]
    fn build_prompt_includes_base_instructions() {
        let r = ModeRegistry::default();
        let p = r.build_prompt("code", Some("PROJECT: hyperagent\n"));
        assert!(p.starts_with("PROJECT: hyperagent"), "base should lead: {p}");
    }

    #[test]
    fn build_prompt_falls_back_to_code_for_unknown_mode() {
        let r = ModeRegistry::default();
        let p = r.build_prompt("totally-fake-mode", None);
        // Should fall back to "code" mode's name in the prompt
        assert!(p.contains("Code"), "should fall back to code: {p}");
    }

    // ── ModeConfig serde round trip ────────────────────────────

    #[test]
    fn mode_config_round_trips_through_serde() {
        let cfg = ModeConfig {
            name: "X".into(),
            description: "d".into(),
            system_prompt: "p".into(),
            permissions: ModePermissions::debug(),
            model: Some("m".into()),
            temperature: Some(0.7),
        };
        let v = serde_json::to_value(&cfg).unwrap();
        let back: ModeConfig = serde_json::from_value(v).unwrap();
        assert_eq!(back, cfg);
    }
}
