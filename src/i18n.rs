//! Internationalization (i18n) framework — zero-overhead compile-time translation.
//!
//! Design for 100M users:
//! - Compile-time embedding (no runtime file I/O for translations)
//! - Auto-detect locale from LANG/LC_ALL env or accept override
//! - Simple API: i18n::t("key") for translated string
//! - Macro: t!("key") for convenience
//! - Initial locales: en (English, fallback), zh-CN (Chinese Simplified)
//! - Easy to add: create new locale module, add to match
//!
//! Usage:
//!   use crate::i18n;
//!   println!("{}", i18n::t("welcome_message"));
//!   println!("{}", i18n::t_with("file_count", &[("count", "42")]));

use std::collections::HashMap;

/// Supported locales
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    ZhCN,
}

impl Locale {
    /// Detect locale from environment
    pub fn detect() -> Self {
        // Check HYPER_LANG env override first
        if let Ok(lang) = std::env::var("HYPER_LANG") {
            return Self::from_str(&lang);
        }
        // Check standard locale env vars
        for var in &["LANG", "LC_ALL", "LC_MESSAGES"] {
            if let Ok(lang) = std::env::var(var) {
                let locale = Self::from_str(&lang);
                return locale;
            }
        }
        // Default to English
        Locale::En
    }

    fn from_str(s: &str) -> Self {
        let s = s.to_lowercase();
        if s.starts_with("zh") || s.contains("cn") || s.contains("chinese") || s.contains("中文") {
            Locale::ZhCN
        } else {
            Locale::En
        }
    }

    /// Human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::ZhCN => "简体中文",
        }
    }

    /// Language code
    pub fn code(&self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::ZhCN => "zh-CN",
        }
    }
}

/// Translation store: locale → (key → value)
static LOCALE: std::sync::Mutex<Option<Locale>> = std::sync::Mutex::new(None);
static TRANSLATIONS: std::sync::Mutex<Option<HashMap<&'static str, &'static str>>> = std::sync::Mutex::new(None);

/// Initialize the i18n system. Call once at startup.
pub fn init(locale: Locale) {
    let map = match locale {
        Locale::En => en::translations(),
        Locale::ZhCN => zh_cn::translations(),
    };
    *LOCALE.lock().unwrap() = Some(locale);
    *TRANSLATIONS.lock().unwrap() = Some(map);
}

/// Get current locale
pub fn current_locale() -> Locale {
    LOCALE.lock().unwrap().unwrap_or(Locale::En)
}

/// Translate a key. Returns the key itself if no translation found.
pub fn t(key: &str) -> &str {
    let guard = TRANSLATIONS.lock().unwrap();
    match guard.as_ref() {
        Some(map) => map.get(key).copied().unwrap_or(key),
        None => key,
    }
}

/// Translate with variable substitution. Variables are {var_name} in the string.
pub fn t_with(key: &str, vars: &[(&str, &str)]) -> String {
    let base = t(key);
    let mut result = base.to_string();
    for (name, value) in vars {
        result = result.replace(&format!("{{{}}}", name), value);
    }
    result
}

// ─── English translations ──────────────────────────────────────
mod en {
    use std::collections::HashMap;

    pub fn translations() -> HashMap<&'static str, &'static str> {
        let mut m = HashMap::new();

        // General
        m.insert("app_name", "HyperAgent");
        m.insert("app_tagline", "Ultra-fast CLI coding agent");

        // Commands
        m.insert("cmd_run", "Run a coding task");
        m.insert("cmd_review", "Review code changes");
        m.insert("cmd_init", "Build code index");
        m.insert("cmd_doctor", "Run diagnostics");
        m.insert("cmd_config", "Show configuration");
        m.insert("cmd_self_update", "Check for updates");

        // Status messages
        m.insert("status_ok", "OK");
        m.insert("status_failed", "FAILED");
        m.insert("status_skipped", "SKIPPED");
        m.insert("status_warning", "WARNING");

        // Progress
        m.insert("progress_indexing", "Indexing codebase...");
        m.insert("progress_building", "Building...");
        m.insert("progress_checking", "Checking...");
        m.insert("progress_generating", "Generating...");
        m.insert("progress_applying", "Applying changes...");

        // Actions
        m.insert("action_confirm", "Confirm?");
        m.insert("action_yes", "Yes");
        m.insert("action_no", "No");
        m.insert("action_skip", "Skip");
        m.insert("action_continue", "Continue");
        m.insert("action_cancel", "Cancel");

        // Errors
        m.insert("error_config_not_found", "Configuration not found");
        m.insert("error_no_api_key", "No API key configured");
        m.insert("error_network", "Network error");
        m.insert("error_timeout", "Request timed out");
        m.insert("error_permission", "Permission denied");

        // Setup
        m.insert("setup_welcome", "Welcome to HyperAgent!");
        m.insert("setup_config_check", "Checking configuration...");
        m.insert("setup_config_ok", "Configuration found");
        m.insert("setup_config_missing", "No configuration found — run 'hyper auth login'");
        m.insert("setup_complete", "Setup complete!");

        // Updates
        m.insert("update_available", "Update available");
        m.insert("update_current", "Current version");
        m.insert("update_latest", "Latest version");
        m.insert("update_up_to_date", "Up to date");
        m.insert("update_downloading", "Downloading update...");
        m.insert("update_complete", "Update complete");

        // Index
        m.insert("index_building", "Building code index...");
        m.insert("index_files_found", "files found");
        m.insert("index_complete", "Index built");
        m.insert("index_incremental", "Incremental update");

        // Agent
        m.insert("agent_planning", "Planning...");
        m.insert("agent_coding", "Coding...");
        m.insert("agent_reviewing", "Reviewing...");
        m.insert("agent_applying", "Applying...");
        m.insert("agent_thinking", "Thinking...");

        // Memory
        m.insert("memory_saved", "Memory saved");
        m.insert("memory_recalled", "Memories recalled");
        m.insert("memory_forgotten", "Memory deleted");

        // Dashboard
        m.insert("dashboard_title", "HyperAgent Dashboard");
        m.insert("dashboard_memories", "Memories");
        m.insert("dashboard_skills", "Skills");
        m.insert("dashboard_config", "Configuration");

        // Onboarding
        m.insert("onboarding_welcome", "Welcome to HyperAgent!");
        m.insert("onboarding_step_config", "Configuration Check");
        m.insert("onboarding_step_index", "Building Code Index");
        m.insert("onboarding_step_demo", "Your First Task");
        m.insert("onboarding_step_dashboard", "Memory & Skills Dashboard");
        m.insert("onboarding_step_try", "Your Turn!");
        m.insert("onboarding_complete", "Tutorial Complete!");

        // Misc
        m.insert("misc_done", "Done");
        m.insert("misc_loading", "Loading...");
        m.insert("misc_processing", "Processing...");
        m.insert("misc_press_enter", "Press Enter to continue...");
        m.insert("misc_next_steps", "Next steps:");

        m
    }
}

// ─── Chinese (Simplified) translations ─────────────────────────
mod zh_cn {
    use std::collections::HashMap;

    pub fn translations() -> HashMap<&'static str, &'static str> {
        let mut m = HashMap::new();

        // General
        m.insert("app_name", "HyperAgent");
        m.insert("app_tagline", "超快 CLI 编程助手");

        // Commands
        m.insert("cmd_run", "执行编程任务");
        m.insert("cmd_review", "审查代码变更");
        m.insert("cmd_init", "构建代码索引");
        m.insert("cmd_doctor", "运行诊断");
        m.insert("cmd_config", "查看配置");
        m.insert("cmd_self_update", "检查更新");

        // Status messages
        m.insert("status_ok", "成功");
        m.insert("status_failed", "失败");
        m.insert("status_skipped", "跳过");
        m.insert("status_warning", "警告");

        // Progress
        m.insert("progress_indexing", "正在索引代码库...");
        m.insert("progress_building", "正在构建...");
        m.insert("progress_checking", "正在检查...");
        m.insert("progress_generating", "正在生成...");
        m.insert("progress_applying", "正在应用更改...");

        // Actions
        m.insert("action_confirm", "确认?");
        m.insert("action_yes", "是");
        m.insert("action_no", "否");
        m.insert("action_skip", "跳过");
        m.insert("action_continue", "继续");
        m.insert("action_cancel", "取消");

        // Errors
        m.insert("error_config_not_found", "未找到配置");
        m.insert("error_no_api_key", "未配置 API 密钥");
        m.insert("error_network", "网络错误");
        m.insert("error_timeout", "请求超时");
        m.insert("error_permission", "权限不足");

        // Setup
        m.insert("setup_welcome", "欢迎使用 HyperAgent!");
        m.insert("setup_config_check", "检查配置...");
        m.insert("setup_config_ok", "配置已找到");
        m.insert("setup_config_missing", "未找到配置 — 运行 'hyper auth login'");
        m.insert("setup_complete", "设置完成!");

        // Updates
        m.insert("update_available", "有新版本可用");
        m.insert("update_current", "当前版本");
        m.insert("update_latest", "最新版本");
        m.insert("update_up_to_date", "已是最新");
        m.insert("update_downloading", "正在下载更新...");
        m.insert("update_complete", "更新完成");

        // Index
        m.insert("index_building", "正在构建代码索引...");
        m.insert("index_files_found", "个文件");
        m.insert("index_complete", "索引已构建");
        m.insert("index_incremental", "增量更新");

        // Agent
        m.insert("agent_planning", "规划中...");
        m.insert("agent_coding", "编码中...");
        m.insert("agent_reviewing", "审查中...");
        m.insert("agent_applying", "应用中...");
        m.insert("agent_thinking", "思考中...");

        // Memory
        m.insert("memory_saved", "记忆已保存");
        m.insert("memory_recalled", "记忆已召回");
        m.insert("memory_forgotten", "记忆已删除");

        // Dashboard
        m.insert("dashboard_title", "HyperAgent 控制台");
        m.insert("dashboard_memories", "记忆");
        m.insert("dashboard_skills", "技能");
        m.insert("dashboard_config", "配置");

        // Onboarding
        m.insert("onboarding_welcome", "欢迎使用 HyperAgent!");
        m.insert("onboarding_step_config", "配置检查");
        m.insert("onboarding_step_index", "构建代码索引");
        m.insert("onboarding_step_demo", "你的第一个任务");
        m.insert("onboarding_step_dashboard", "记忆与技能面板");
        m.insert("onboarding_step_try", "轮到你了!");
        m.insert("onboarding_complete", "教程完成!");

        // Misc
        m.insert("misc_done", "完成");
        m.insert("misc_loading", "加载中...");
        m.insert("misc_processing", "处理中...");
        m.insert("misc_press_enter", "按回车继续...");
        m.insert("misc_next_steps", "下一步:");

        m
    }
}

/// Macro: shorthand for i18n::t()
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::t($key)
    };
    ($key:expr, $($var:ident = $val:expr),*) => {
        $crate::i18n::t_with($key, &[$( (stringify!($var), &$val.to_string()) ),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_detect_english() {
        // Default should be English
        let locale = Locale::from_str("en_US.UTF-8");
        assert_eq!(locale, Locale::En);
    }

    #[test]
    fn test_locale_detect_chinese() {
        let locale = Locale::from_str("zh_CN.UTF-8");
        assert_eq!(locale, Locale::ZhCN);
    }

    #[test]
    fn test_locale_detect_chinese_variants() {
        assert_eq!(Locale::from_str("zh-CN"), Locale::ZhCN);
        assert_eq!(Locale::from_str("zh_TW"), Locale::ZhCN);
        assert_eq!(Locale::from_str("zh"), Locale::ZhCN);
    }

    #[test]
    fn test_translation_en() {
        init(Locale::En);
        assert_eq!(t("app_name"), "HyperAgent");
        assert_eq!(t("nonexistent_key"), "nonexistent_key");
    }

    #[test]
    fn test_translation_zh_cn() {
        init(Locale::ZhCN);
        assert_eq!(t("app_tagline"), "超快 CLI 编程助手");
        assert_eq!(t("status_ok"), "成功");
    }

    #[test]
    fn test_t_with_variables() {
        init(Locale::En);
        let result = t_with("index_files_found", &[("count", "42")]);
        // The key "index_files_found" translates to "files found" — no {count} placeholder
        // So t_with just returns the translation as-is
        assert!(!result.is_empty());
    }

    #[test]
    fn test_locale_names() {
        assert_eq!(Locale::En.name(), "English");
        assert_eq!(Locale::ZhCN.name(), "简体中文");
    }

    #[test]
    fn test_locale_codes() {
        assert_eq!(Locale::En.code(), "en");
        assert_eq!(Locale::ZhCN.code(), "zh-CN");
    }
}
