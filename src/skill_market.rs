//! Skill marketplace — community-driven agent skills.
//!
//! ```
//! hyper skill install <url>     # Install from GitHub Gist/URL
//! hyper skill list              # List installed skills
//! hyper skill search <term>     # Search for skills
//! hyper skill create            # Create a new skill interactively
//! ```

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
    println!("📦 Installing skill from: {url}");
    
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

    println!("   ✅ Installed: {} v{}", skill.name, skill.version);
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
fn parse_skill_md(content: &str) -> Option<Skill> {
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
    }

    prompt
}
