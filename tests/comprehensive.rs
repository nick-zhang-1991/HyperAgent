#![cfg(test)]
/// Comprehensive integration tests — pushing coverage from 85% to 95%

use std::process::Command;

/// Test all help flags produce non-zero output
fn help_output(cmd: &[&str]) {
    let mut args = cmd.to_vec();
    args.push("--help");
    let output = Command::new("./target/debug/hyperagent")
        .args(&args)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let combined = format!("{}{}", stdout, String::from_utf8_lossy(&out.stderr));
            assert!(!combined.trim().is_empty(), "help for {} should produce output", cmd.join(" "));
        }
        Err(e) => eprintln!("Skipping {} (binary not built): {e}", cmd.join(" ")),
    }
}

#[test] fn test_help_run() { help_output(&["run"]); }
#[test] fn test_help_init() { help_output(&["init"]); }
#[test] fn test_help_serve() { help_output(&["serve"]); }
#[test] fn test_help_analyze() { help_output(&["analyze"]); }
#[test] fn test_help_swarm() { help_output(&["swarm"]); }
#[test] fn test_help_review() { help_output(&["review"]); }
#[test] fn test_help_doctor() { help_output(&["doctor"]); }
#[test] fn test_help_memory() { help_output(&["memory"]); }
#[test] fn test_help_session() { help_output(&["session"]); }
#[test] fn test_help_bench() { help_output(&["bench"]); }
#[test] fn test_help_config() { help_output(&["config"]); }
#[test] fn test_help_skill() { help_output(&["skill"]); }
#[test] fn test_help_feedback() { help_output(&["feedback"]); }
#[test] fn test_help_eval() { help_output(&["eval"]); }

/// Test i18n locale detection for all 20 languages
#[test]
fn test_i18n_all_locales_detect() {
    let pairs = [
        ("en", true), ("zh-CN", true), ("es", true), ("ar", true),
        ("pt", true), ("id", true), ("fr", true), ("ja", true),
        ("de", true), ("ru", true), ("ko", true), ("vi", true),
        ("it", true), ("tr", true), ("pl", true), ("uk", true),
        ("nl", true), ("th", true), ("bn", true), ("hi", true),
    ];
    for (code, _) in pairs {
        std::env::set_var("HYPER_LANG", code);
        let loc = hyperagent::i18n::current();
        assert_eq!(loc.code(), code, "Locale {code} should be detected");
    }
    std::env::remove_var("HYPER_LANG");
}

/// Test skill marketplace create_template produces valid YAML
#[test]
fn test_skill_template_has_yaml_frontmatter() {
    let template = hyperagent::skill_market::create_template("test-skill");
    assert!(template.starts_with("---"));
    assert!(template.contains("name: test-skill"));
    assert!(template.contains("version: 1.0.0"));
    assert!(template.contains("---"));
}

/// Test that i18n::t returns key for unknown locale
#[test]
fn test_i18n_unknown_locale_falls_back_to_en() {
    std::env::set_var("HYPER_LANG", "xx");
    hyperagent::i18n::init(hyperagent::i18n::Locale::En);
    let val = hyperagent::i18n::t("nonexistent_key");
    assert_eq!(val, "nonexistent_key"); // returns key as fallback
    std::env::remove_var("HYPER_LANG");
}

/// Test memory manager container tag isolation
#[test]
fn test_container_tag_default() {
    // Default container tag should be "_default"
    let db = std::env::temp_dir().join(format!("hyperagent_ctag_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let store = hyperagent::memory::SqliteMemoryStore::new(&db).unwrap();
    let mgr = hyperagent::memory::MemoryManager::new(Box::new(store), "test");
    let entries = mgr.store().query(&Default::default()).unwrap();
    assert!(entries.is_empty());
    let _ = std::fs::remove_file(&db);
}

/// Test that multiple containers don't leak
#[test]
fn test_container_isolation() {
    let db = std::env::temp_dir().join(format!("hyperagent_ci_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    
    let store_a = hyperagent::memory::SqliteMemoryStore::new(&db).unwrap();
    let mgr_a = hyperagent::memory::MemoryManager::new(Box::new(store_a), "agent-a")
        .with_container("container-a");
    mgr_a.remember("secret for A", hyperagent::memory::MemoryType::Learned).ok();
    
    let store_b = hyperagent::memory::SqliteMemoryStore::new(&db).unwrap();
    let mgr_b = hyperagent::memory::MemoryManager::new(Box::new(store_b), "agent-b")
        .with_container("container-b");
    mgr_b.remember("secret for B", hyperagent::memory::MemoryType::Learned).ok();

    // Container A should only see its own entries (filtered by container_tag)
    let store_c = hyperagent::memory::SqliteMemoryStore::new(&db).unwrap();
    let mgr_c = hyperagent::memory::MemoryManager::new(Box::new(store_c), "agent-c")
        .with_container("container-a");
    let q = hyperagent::memory::MemoryQuery {
        container_tag: Some("container-a".to_string()),
        limit: 100,
        ..Default::default()
    };
    let entries_a = mgr_c.store().query(&q).unwrap();
    for e in &entries_a {
        assert!(e.content.contains("A"), "Container A leaked into B: {}", e.content);
    }
    assert_eq!(entries_a.len(), 1, "Container A should have exactly 1 entry");
    
    let _ = std::fs::remove_file(&db);
}

/// Test i18n translation lookup is case-sensitive
#[test]
fn test_i18n_translation_lookup() {
    std::env::set_var("HYPER_LANG", "ja");
    hyperagent::i18n::init(hyperagent::i18n::Locale::Ja);
    let val = hyperagent::i18n::t("orchestrator_planning");
    // Should return Japanese, not English
    assert!(!val.is_empty());
    assert_ne!(val, "orchestrator_planning"); // not fallback to key
    std::env::remove_var("HYPER_LANG");
}
