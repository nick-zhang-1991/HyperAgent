//! Skills System — reusable procedural knowledge
//!
//! Skills are markdown documents with YAML frontmatter that capture
//! verified workflows, code patterns, and troubleshooting guides.
//!
//! Inspired by Hermes Agent's skill system.
//!
//! Storage: ~/.hyper/skills/<name>/SKILL.md
//!
//! Structure:
//! ```yaml
//! ---
//! name: my-skill
//! description: "Brief description of what this skill does"
//! category: software-development
//! tags: [rust, testing]
//! ---
//!
//! # Skill Title
//!
//! ## Prerequisites
//! ...
//!
//! ## Steps
//! 1. ...
//! 2. ...
//!
//! ## Pitfalls
//! - ...
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A loaded skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub content: String,
    pub path: PathBuf,
}

impl Skill {
    /// Load a skill from a SKILL.md file
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;

        // Parse YAML frontmatter (between --- markers)
        let (frontmatter, body) = if let Some(rest) = content.trim_start().strip_prefix("---") {
            if let Some(end) = rest.find("\n---") {
                let yaml_str = &rest[..end];
                let body_start = end + 5; // skip "\n---\n"
                (Some(yaml_str.trim()), Some(&content[body_start..]))
            } else {
                (None, Some(content.as_str()))
            }
        } else {
            (None, Some(content.as_str()))
        };

        let name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut skill = Skill {
            name,
            description: String::new(),
            category: None,
            tags: Vec::new(),
            content: body.unwrap_or(&content).to_string(),
            path: path.to_path_buf(),
        };

        // Parse frontmatter fields manually (avoid serde_yaml dependency)
        if let Some(fm) = frontmatter {
            for line in fm.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name: ") {
                    skill.name = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = line.strip_prefix("description: ") {
                    skill.description = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = line.strip_prefix("category: ") {
                    skill.category = Some(val.trim().trim_matches('"').to_string());
                } else if line.starts_with("tags:") {
                    // Parse inline array: [tag1, tag2, tag3]
                    if let Some(bracket) = line.find('[') {
                        let tags_str = &line[bracket..];
                        let tags_str = tags_str.trim_start_matches('[').trim_end_matches(']');
                        for tag in tags_str.split(',') {
                            let tag = tag.trim().trim_matches('"');
                            if !tag.is_empty() {
                                skill.tags.push(tag.to_string());
                            }
                        }
                    }
                }
            }
        }

        Ok(skill)
    }

    /// Render a short summary line
    pub fn summary_line(&self) -> String {
        let tags_str = if self.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", self.tags.join(", "))
        };
        format!("{} — {}{}", self.name, self.description, tags_str)
    }
}

/// Registry of all available skills
pub struct SkillsRegistry {
    skills_dir: PathBuf,
    skills: Vec<Skill>,
}

impl SkillsRegistry {
    /// Create registry and scan for skills
    pub fn new(base_dir: &Path) -> Self {
        let skills_dir = base_dir.join(".hyper").join("skills");
        let skills = if skills_dir.exists() {
            Self::discover(&skills_dir)
        } else {
            Vec::new()
        };
        Self { skills_dir, skills }
    }

    /// Scan the skills directory for SKILL.md files
    fn discover(skills_dir: &Path) -> Vec<Skill> {
        let mut skills = Vec::new();
        if !skills_dir.exists() {
            return skills;
        }

        // Walk all SKILL.md files (one level deep or nested)
        if let Ok(entries) = std::fs::read_dir(skills_dir) {
            for entry in entries.flatten() {
                let skill_path = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    entry.path().join("SKILL.md")
                } else {
                    entry.path()
                };

                if skill_path.exists() && skill_path.extension().is_some_and(|e| e == "md") {
                    if let Ok(skill) = Skill::load(&skill_path) {
                        skills.push(skill);
                    }
                }
            }
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills
    }

    /// Find a skill by name
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// List all skills
    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    /// Save a new skill (creates SKILL.md in skills directory)
    pub fn save(&self, name: &str, description: &str, category: &str, tags: &[String], body: &str) -> anyhow::Result<PathBuf> {
        let skill_dir = self.skills_dir.join(name);
        std::fs::create_dir_all(&skill_dir)?;

        let tags_str = if tags.is_empty() {
            String::new()
        } else {
            format!("\ntags: [{}]", tags.iter().map(|t| format!("\"{}\"", t)).collect::<Vec<_>>().join(", "))
        };

        let category_str = if category.is_empty() {
            String::new()
        } else {
            format!("\ncategory: \"{category}\"")
        };

        let content = format!(
            "---\nname: \"{name}\"\ndescription: \"{description}\"{category_str}{tags_str}\n---\n\n{}",
            body
        );

        let skill_path = skill_dir.join("SKILL.md");
        std::fs::write(&skill_path, content)?;
        Ok(skill_path)
    }

    /// Render all skills as display string
    pub fn render(&self) -> String {
        if self.skills.is_empty() {
            return "   No skills installed.".to_string();
        }

        let mut output = format!("   📚 Skills ({} total)\n", self.skills.len());
        for skill in &self.skills {
            let cat = skill.category.as_deref().unwrap_or("uncategorized");
            output.push_str(&format!("   • {} ({}) — {}\n", skill.name, cat, skill.description));
        }
        output
    }

    /// Delete a skill
    pub fn delete(&self, name: &str) -> anyhow::Result<()> {
        let skill_dir = self.skills_dir.join(name);
        let skill_file = skill_dir.join("SKILL.md");
        if skill_file.exists() {
            std::fs::remove_file(&skill_file)?;
            // Remove directory if empty
            let _ = std::fs::remove_dir(&skill_dir);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Skill '{name}' not found"))
        }
    }

    /// Count
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}
