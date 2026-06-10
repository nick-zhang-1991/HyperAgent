//! Skill marketplace — community-driven agent skills.
//!
//! ```
//! hyper skill install <url>     # Install from GitHub Gist/URL
//! hyper skill list              # List installed skills
//! hyper skill search <term>     # Search for skills
//! hyper skill create            # Create a new skill interactively
//! ```

use crate::i18n;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A skill definition (SKILL.md format)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Skill {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub tags: Vec<String>,
    #[serde(rename = "system-prompt")]
    pub system_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_steps: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
}

/// Skill registry (community index)
const COMMUNITY_INDEX_URL: &str = "https://raw.githubusercontent.com/nick-zhang-1991/HyperAgent-skills/main/index.json";

/// Skill storage directory
fn skills_dir() -> PathBuf {
    dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hyperagent")
        .join("skills")
}

/// List installed skills
pub fn list_installed() -> Result<Vec<Skill>> {
    let dir = skills_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "json" || e == "md") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Some(skill) = parse_skill_md(&content) {
                    skills.push(skill);
                }
            }
        }
    }
    Ok(skills)
}

/// Install a skill from a URL (GitHub Gist, raw URL, etc.)
pub async fn install(url: &str) -> Result<Skill> {
    println!("📦 {}", i18n::t_with("skill_installing", &[url]));
    
    let client = reqwest::Client::new();
    let resp = client.get(url)
        .header("Accept", "text/plain,text/markdown,*/*")
        .send()
        .await
        .context("Failed to download skill")?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} while downloading skill", resp.status());
    }

    let content = resp.text().await.context("Failed to read skill content")?;
    let skill = parse_skill_md(&content)
        .ok_or_else(|| anyhow::anyhow!("Invalid skill format — missing YAML frontmatter"))?;

    // Save to skills directory
    let dir = skills_dir();
    std::fs::create_dir_all(&dir)?;
    let filename = format!("{}.json", skill.name.replace(' ', "-").to_lowercase());
    let path = dir.join(filename);
    std::fs::write(&path, serde_json::to_string_pretty(&skill)?)?;

    println!("   ✅ {}", i18n::t_with("skill_installed", &[&skill.name, &skill.version]));
    println!("   📝 {}", skill.description);
    println!("   🏷️  Tags: {}", skill.tags.join(", "));

    Ok(skill)
}

/// Search community skill index
pub async fn search(term: &str) -> Result<Vec<Skill>> {
    let client = reqwest::Client::new();
    let resp = client.get(COMMUNITY_INDEX_URL)
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let all_skills: Vec<Skill> = r.json().await
                .context("Failed to parse community index")?;
            let term_lower = term.to_lowercase();
            let results: Vec<Skill> = all_skills.into_iter()
                .filter(|s| {
                    s.name.to_lowercase().contains(&term_lower)
                        || s.description.to_lowercase().contains(&term_lower)
                        || s.tags.iter().any(|t| t.to_lowercase().contains(&term_lower))
                })
                .collect();
            Ok(results)
        }
        Ok(r) => {
            println!("   ⚠️  Community index not available (HTTP {})", r.status());
            println!("   Browse: https://github.com/nick-zhang-1991/HyperAgent-skills");
            Ok(Vec::new())
        }
        Err(_) => {
            println!("   ⚠️  Community index not reachable");
            println!("   Browse: https://github.com/nick-zhang-1991/HyperAgent-skills");
            Ok(Vec::new())
        }
    }
}

/// Parse SKILL.md format with YAML frontmatter
pub fn parse_skill_md(content: &str) -> Option<Skill> {
    // YAML frontmatter: --- ... ---
    if !content.starts_with("---") {
        return None;
    }

    let end = content[3..].find("---")?;
    let yaml = &content[3..3 + end];
    let markdown = content[3 + end + 3..].trim();

    let mut name = String::new();
    let mut version = "1.0.0".to_string();
    let mut author = "unknown".to_string();
    let mut description = String::new();
    let mut tags = Vec::new();
    let mut system_prompt = String::new();

    for line in yaml.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("version:") {
            version = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("author:") {
            author = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("tags:") {
            tags = val.split(',').map(|t| t.trim().to_string()).collect();
        }
    }

    // The rest of the document is the system prompt
    system_prompt = markdown.to_string();

    if name.is_empty() {
        None
    } else {
        Some(Skill {
            name,
            version,
            author,
            description,
            tags,
            system_prompt,
            install_steps: None,
            tools: None,
        })
    }
}

/// Generate a SKILL.md template
pub fn create_template(name: &str) -> String {
    format!(
        r#"---
name: {name}
version: 1.0.0
author: your-name
description: What this skill does
tags: rust, code-review, best-practice
---

# {name}

You are an expert in [domain]. When activated:

1. Follow these conventions:
   - [convention 1]
   - [convention 2]

2. Tools you can use:
   - read_file, write_file, cargo check

3. Output format:
   - [describe expected output format]

## Example

User: "review this code for Rust best practices"
You: [example response]
"#
    )
}

/// Embed skills into agent system prompt
pub fn inject_skills_prompt(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut prompt = String::from("\n## Active Skills\n");
    prompt.push_str("The following expert skills are active. When relevant, apply their knowledge:\n\n");

    for skill in skills {
        prompt.push_str(&format!(
            "### {} (v{} by {})\n{}\n",
            skill.name, skill.version, skill.author, skill.description
        ));
        if !skill.system_prompt.is_empty() {
            prompt.push_str(&format!("\n{}\n", skill.system_prompt));
        }
    }

    prompt
}


#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_skill_md ────────────────────────────────────────

    #[test]
    fn test_parse_skill_md_full() {
        let md = r#"---
name: rust-linter
version: 2.0.0
author: alice
description: Lints Rust code
tags: rust, lint, code
---

You are a Rust linting expert.
"#;
        let s = parse_skill_md(md).expect("should parse");
        assert_eq!(s.name, "rust-linter");
        assert_eq!(s.version, "2.0.0");
        assert_eq!(s.author, "alice");
        assert_eq!(s.description, "Lints Rust code");
        assert_eq!(s.tags, vec!["rust", "lint", "code"]);
        assert!(s.system_prompt.contains("Rust linting expert"));
    }

    #[test]
    fn test_parse_skill_md_missing_name_returns_none() {
        let md = "---
version: 1.0
---
body";
        assert!(parse_skill_md(md).is_none());
    }

    #[test]
    fn test_parse_skill_md_no_frontmatter_returns_none() {
        assert!(parse_skill_md("just plain text").is_none());
        assert!(parse_skill_md("name: foo
---
body").is_none());
    }

    #[test]
    fn test_parse_skill_md_unclosed_frontmatter() {
        let md = "---
name: foo
body without closing";
        assert!(parse_skill_md(md).is_none());
    }

    #[test]
    fn test_parse_skill_md_defaults() {
        let md = "---
name: minimal
---
body";
        let s = parse_skill_md(md).unwrap();
        assert_eq!(s.version, "1.0.0");   // default
        assert_eq!(s.author, "unknown");   // default
        assert_eq!(s.description, "");
        assert!(s.tags.is_empty());
    }

    #[test]
    fn test_parse_skill_md_with_empty_frontmatter() {
        // no --- close but starts with ---
        let md = "---
---body";
        // should fail because find finds the second --- and yaml is empty
        // but name will be empty so it returns None
        let result = parse_skill_md(md);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_skill_md_extra_yaml_keys_ignored() {
        let md = r#"---
name: with-extras
version: 3.1.4
author: bob
description: test
tags: a, b
unknown-key: some-value
another: 42
---
body here
"#;
        let s = parse_skill_md(md).unwrap();
        assert_eq!(s.name, "with-extras");
        assert_eq!(s.tags.len(), 2);
    }

    #[test]
    fn test_parse_skill_md_tags_with_spaces() {
        let md = "---
name: t
tags: a, b , c
---
body";
        let s = parse_skill_md(md).unwrap();
        assert_eq!(s.tags, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_skill_md_system_prompt_is_body() {
        let md = "---
name: t
---

The quick brown fox.

Second paragraph.";
        let s = parse_skill_md(md).unwrap();
        assert!(s.system_prompt.contains("quick brown fox"));
        assert!(s.system_prompt.contains("Second paragraph"));
    }

    // ── create_template ───────────────────────────────────────

    #[test]
    fn test_create_template_basic() {
        let t = create_template("my-skill");
        assert!(t.contains("name: my-skill"));
        assert!(t.contains("# my-skill"));
        assert!(t.contains("---"));
        assert!(t.contains("version: 1.0.0"));
    }

    #[test]
    fn test_create_template_different_names() {
        let t1 = create_template("foo");
        let t2 = create_template("bar");
        assert!(t1.contains("foo"));
        assert!(!t1.contains("bar"));
        assert!(t2.contains("bar"));
        assert!(!t2.contains("foo"));
    }

    #[test]
    fn test_create_template_parses_back() {
        let t = create_template("roundtrip");
        let parsed = parse_skill_md(&t).expect("template should parse");
        assert_eq!(parsed.name, "roundtrip");
    }

    // ── inject_skills_prompt ──────────────────────────────────

    #[test]
    fn test_inject_skills_empty() {
        let prompt = inject_skills_prompt(&[]);
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_inject_skills_single() {
        let skill = Skill {
            name: "lint".into(),
            version: "1.0".into(),
            author: "me".into(),
            description: "Lints code".into(),
            tags: vec!["rust".into()],
            system_prompt: "Be strict.".into(),
            install_steps: None,
            tools: None,
        };
        let prompt = inject_skills_prompt(&[skill]);
        assert!(prompt.contains("## Active Skills"));
        assert!(prompt.contains("lint"));
        assert!(prompt.contains("Lints code"));
        assert!(prompt.contains("Be strict."));
    }

    #[test]
    fn test_inject_skills_multiple() {
        let skills = vec![
            Skill {
                name: "alpha".into(), version: "1".into(), author: "a".into(),
                description: "First".into(), tags: vec![],
                system_prompt: "A prompt".into(),
                install_steps: None, tools: None,
            },
            Skill {
                name: "beta".into(), version: "2".into(), author: "b".into(),
                description: "Second".into(), tags: vec![],
                system_prompt: "B prompt".into(),
                install_steps: None, tools: None,
            },
        ];
        let prompt = inject_skills_prompt(&skills);
        assert!(prompt.contains("alpha"));
        assert!(prompt.contains("beta"));
        assert!(prompt.contains("First"));
        assert!(prompt.contains("Second"));
        assert!(prompt.contains("A prompt"));
        assert!(prompt.contains("B prompt"));
    }

    #[test]
    fn test_inject_skills_empty_system_prompt_omitted() {
        let skill = Skill {
            name: "no-prompt".into(), version: "1".into(), author: "a".into(),
            description: "d".into(), tags: vec![],
            system_prompt: "".into(),
            install_steps: None, tools: None,
        };
        let prompt = inject_skills_prompt(&[skill]);
        // Should still contain name and description but not an empty system prompt section
        assert!(prompt.contains("no-prompt"));
        assert!(prompt.contains("d"));
    }

    // ── Skill struct serialization ────────────────────────────

    #[test]
    fn test_skill_serde_rename_system_prompt() {
        let s = Skill {
            name: "x".into(), version: "1".into(), author: "y".into(),
            description: "z".into(), tags: vec![],
            system_prompt: "w".into(),
            install_steps: None, tools: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        // The serde rename should produce "system-prompt"
        assert!(json.contains("system-prompt"));
        assert!(!json.contains("system_prompt"));
    }

    #[test]
    fn test_skill_serde_optional_fields_skipped() {
        let s = Skill {
            name: "x".into(), version: "1".into(), author: "y".into(),
            description: "z".into(), tags: vec![],
            system_prompt: "w".into(),
            install_steps: None, tools: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("install_steps"));
        assert!(!json.contains("tools"));
    }

    #[test]
    fn test_skill_serde_optional_fields_included() {
        let s = Skill {
            name: "x".into(), version: "1".into(), author: "y".into(),
            description: "z".into(), tags: vec![],
            system_prompt: "w".into(),
            install_steps: Some("cargo install foo".into()),
            tools: Some(vec!["cargo".into(), "rustc".into()]),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("install_steps"));
        assert!(json.contains("cargo install foo"));
        assert!(json.contains("\"tools\""));
    }

    #[test]
    fn test_skill_deserialize_from_yaml_format() {
        // The parse_skill_md path doesn't use serde directly, but
        // installation stores via JSON, so test JSON round-trip
        let original = r#"{"name":"x","version":"1","author":"y","description":"z","tags":[],"system-prompt":"w"}"#;
        let s: Skill = serde_json::from_str(original).unwrap();
        assert_eq!(s.system_prompt, "w");
    }

    // ── list_installed (filesystem) ───────────────────────────

    #[test]
    fn test_list_installed_empty_when_no_dir() {
        // The skills_dir is platform-specific; if it doesn't exist, returns empty vec
        // We can't easily mock dirs_next, so this is a smoke test
        let _ = list_installed(); // should not panic
    }
}
