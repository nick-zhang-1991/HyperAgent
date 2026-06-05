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
    pub depends: Vec<String>, // Skills that this skill depends on
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
            depends: Vec::new(),
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
                } else if let Some(val) = line.strip_prefix("depends: ") {
                    let depends_str = val.trim().trim_matches('"');
                    for d in depends_str.split(',') {
                        let d = d.trim().trim_matches('"');
                        if !d.is_empty() {
                            skill.depends.push(d.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn setup() -> (TempDir, SkillsRegistry) {
        let dir = TempDir::new().unwrap();
        let registry = SkillsRegistry::new(dir.path());
        (dir, registry)
    }

    #[test]
    fn test_empty_registry() {
        let (_dir, registry) = setup();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_save_and_list_skill() {
        let (dir, _registry) = setup();
        let registry = SkillsRegistry::new(dir.path());
        let path = registry.save("test-skill", "A test skill", "testing", &["rust".to_string(), "cli".to_string()], "# Test\n\nSome body").unwrap();
        assert!(path.exists());
        assert!(path.to_string_lossy().contains("test-skill"));

        // Re-discover after save
        let registry = SkillsRegistry::new(dir.path());
        assert_eq!(registry.len(), 1);
        let skill = registry.get("test-skill").unwrap();
        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill");
        assert_eq!(skill.category.as_deref(), Some("testing"));
        assert_eq!(skill.tags, vec!["rust", "cli"]);
        assert!(skill.content.contains("Test"));
    }

    #[test]
    fn test_get_nonexistent() {
        let (_dir, registry) = setup();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_save_duplicate_name_overwrites() {
        let (dir, _registry) = setup();
        let registry = SkillsRegistry::new(dir.path());
        registry.save("dup", "first", "cat1", &[], "body1").unwrap();
        registry.save("dup", "second", "cat2", &["tag1".to_string()], "body2").unwrap();
        // SkillsRegistry::save overwrites — test that last write wins
        let registry = SkillsRegistry::new(dir.path());
        let skill = registry.get("dup").unwrap();
        assert_eq!(skill.description, "second");
        assert_eq!(skill.tags, vec!["tag1"]);
    }

    #[test]
    fn test_delete_skill() {
        let (dir, _registry) = setup();
        let registry = SkillsRegistry::new(dir.path());
        registry.save("to-delete", "desc", "cat", &[], "body").unwrap();
        let registry = SkillsRegistry::new(dir.path());
        assert_eq!(registry.len(), 1);
        registry.delete("to-delete").unwrap();
        let registry = SkillsRegistry::new(dir.path());
        assert!(registry.is_empty());
    }

    #[test]
    fn test_delete_nonexistent() {
        let (_dir, registry) = setup();
        let result = registry.delete("ghost");
        assert!(result.is_err());
    }

    #[test]
    fn test_render_empty() {
        let (_dir, registry) = setup();
        let output = registry.render();
        assert!(output.contains("No skills"));
    }

    #[test]
    fn test_render_with_skills() {
        let (dir, _registry) = setup();
        let registry = SkillsRegistry::new(dir.path());
        registry.save("alpha", "First skill", "tools", &["go".to_string()], "body alpha").unwrap();
        registry.save("beta", "Second skill", "tools", &["rust".to_string()], "body beta").unwrap();
        let registry = SkillsRegistry::new(dir.path());
        let output = registry.render();
        assert!(output.contains("alpha"));
        assert!(output.contains("First skill"));
        assert!(output.contains("beta"));
        assert!(output.contains("Second skill"));
    }

    #[test]
    fn test_skill_loads_from_file() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".hyper").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        let skill_dir = skills_dir.join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let content = "---\nname: \"my-skill\"\ndescription: \"My custom skill\"\ncategory: \"devops\"\ntags: [\"deploy\", \"docker\"]\n---\n\n# My Skill\n\n## Steps\n1. Do X\n2. Do Y\n";
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();

        let registry = SkillsRegistry::new(dir.path());
        assert_eq!(registry.len(), 1);
        let skill = registry.get("my-skill").unwrap();
        assert_eq!(skill.description, "My custom skill");
        assert_eq!(skill.category.as_deref(), Some("devops"));
        assert!(skill.tags.contains(&"deploy".to_string()));
        assert!(skill.content.contains("Do X"));
    }

    #[test]
    fn test_skill_without_frontmatter() {
        let dir = TempDir::new().unwrap();
        let skills_dir = dir.path().join(".hyper").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        let path = skills_dir.join("plain.md");
        std::fs::write(&path, "# Plain Skill\n\nJust content").unwrap();

        let registry = SkillsRegistry::new(dir.path());
        assert_eq!(registry.len(), 1);
        let skill = registry.get("plain").unwrap();
        assert_eq!(skill.name, "plain");
        assert!(skill.content.contains("Plain Skill"));
    }
}
