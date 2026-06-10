/// Integration tests for i18n and skills
/// These use `hyperagent::` (the library crate) instead of `crate::` (which
/// refers to the integration test binary itself).

/// Test Chinese locale detection
#[test]
fn test_locale_detect_zh_cn() {
    // HYPER_LANG=zh-CN should activate Chinese
    std::env::set_var("HYPER_LANG", "zh-CN");
    let locale = hyperagent::i18n::Locale::detect();
    assert_eq!(locale, hyperagent::i18n::Locale::ZhCN);
    std::env::remove_var("HYPER_LANG");
}

/// Test English locale (default)
#[test]
fn test_locale_detect_default_en() {
    std::env::remove_var("HYPER_LANG");
    std::env::remove_var("LANG");
    let locale = hyperagent::i18n::Locale::detect();
    // Default should be English
    assert!(matches!(locale, hyperagent::i18n::Locale::En));
}

/// Test skill YAML parsing
#[test]
fn test_skill_parse_valid() {
    let skill_md = r#"---
name: Test Skill
version: 1.0.0
author: test
description: A test skill
tags: rust, test
---

# System prompt
Do this test thing.
"#;
    let skill = hyperagent::skill_market::parse_skill_md(skill_md);
    assert!(skill.is_some());
    let s = skill.unwrap();
    assert_eq!(s.name, "Test Skill");
    assert_eq!(s.author, "test");
    assert_eq!(s.tags, vec!["rust", "test"]);
}

/// Test skill parsing with missing fields
#[test]
fn test_skill_parse_missing_name() {
    let skill_md = r#"---
version: 1.0.0
author: test
---

No name here.
"#;
    let skill = hyperagent::skill_market::parse_skill_md(skill_md);
    assert!(skill.is_none());
}

/// Test skill template generation
#[test]
fn test_skill_create_template() {
    let template = hyperagent::skill_market::create_template("rust-linter");
    assert!(template.contains("name: rust-linter"));
    assert!(template.contains("version: 1.0.0"));
    assert!(template.contains("---"));
}

/// Test skill injection into prompt
#[test]
fn test_skill_inject_prompt() {
    use hyperagent::skill_market::Skill;
    let skills = vec![
        Skill {
            name: "Test".into(),
            version: "1.0".into(),
            author: "me".into(),
            description: "A test".into(),
            tags: vec!["test".into()],
            system_prompt: "Be helpful.".into(),
            install_steps: None,
            tools: None,
        }
    ];
    let prompt = hyperagent::skill_market::inject_skills_prompt(&skills);
    assert!(prompt.contains("## Active Skills"));
    assert!(prompt.contains("Test"));
    assert!(prompt.contains("Be helpful"));
}

/// Test empty skill list
#[test]
fn test_skill_inject_empty() {
    let prompt = hyperagent::skill_market::inject_skills_prompt(&[]);
    assert!(prompt.is_empty());
}