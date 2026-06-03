//! Project rules engine — `.hyperrules` files for project-specific guidelines
//!
//! Similar to Cline's `.clinerules`, HyperAgent reads `.hyperrules` from the project root
//! and injects them as structured context into the agent pipeline.
//!
//! # Format
//! Rules are defined in `.hyperrules` and/or `.hyperrules/*.md` files.
//! The format is plain markdown with optional section headers.
//!
//! ```markdown
//! # Coding Standards
//! - Use Result instead of panic everywhere
//! - All public functions must have doc comments
//! - Max line length: 100 characters
//!
//! # Architecture
//! - Routes go in src/routes/
//! - Services go in src/services/
//! - All DB queries use the repository pattern
//!
//! # Testing
//! - Every function must have a #[test]
//! - Integration tests in tests/ directory
//! - Mocks in tests/mocks/
//! ```

use anyhow::Result;
use std::path::Path;

/// Parsed project rules
#[derive(Debug, Clone, Default)]
pub struct ProjectRules {
    /// All rules concatenated into a single formatted string
    pub full_text: String,
    /// Sections extracted from headers
    pub sections: Vec<RuleSection>,
}

/// A named section of rules
#[derive(Debug, Clone)]
pub struct RuleSection {
    pub title: String,
    pub rules: Vec<String>,
}

impl ProjectRules {
    /// Load rules from `.hyperrules` file or `.hyperrules/` directory
    pub fn load(project_root: &Path) -> Result<Option<Self>> {
        // Try `.hyperrules` file first
        let rules_file = project_root.join(".hyperrules");
        if rules_file.exists() {
            let content = std::fs::read_to_string(&rules_file)?;
            return Ok(Some(Self::parse(&content)));
        }

        // Try `.hyperrules/` directory
        let rules_dir = project_root.join(".hyperrules");
        if rules_dir.exists() && rules_dir.is_dir() {
            let mut combined = String::new();
            let mut entries: Vec<_> = std::fs::read_dir(&rules_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path().extension().map(|ext| ext == "md").unwrap_or(false)
                })
                .collect();
            entries.sort_by_key(|e| e.file_name());

            for entry in &entries {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    combined.push_str(&format!(
                        "\n---\n# From: {}\n---\n",
                        entry.file_name().to_string_lossy()
                    ));
                    combined.push_str(&content);
                    combined.push('\n');
                }
            }

            if !combined.is_empty() {
                return Ok(Some(Self::parse(&combined)));
            }
        }

        // Check AGENTS.md integration: rules section
        let agents_md = project_root.join("AGENTS.md");
        if agents_md.exists() {
            if let Ok(content) = std::fs::read_to_string(&agents_md) {
                // Extract ## Rules section from AGENTS.md
                if let Some(rules_section) = extract_rules_section(&content) {
                    return Ok(Some(Self::parse(&rules_section)));
                }
            }
        }

        Ok(None)
    }

    /// Parse raw text into structured rules
    pub fn parse(text: &str) -> Self {
        let mut sections = Vec::new();
        let mut current_section = RuleSection {
            title: "General".to_string(),
            rules: Vec::new(),
        };

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("# ") || trimmed.starts_with("## ") {
                // New section header
                let title = trimmed
                    .trim_start_matches('#')
                    .trim()
                    .trim_start_matches('#')
                    .trim()
                    .to_string();
                if !current_section.rules.is_empty() || !sections.is_empty() {
                    sections.push(current_section);
                }
                current_section = RuleSection {
                    title,
                    rules: Vec::new(),
                };
            } else if trimmed.starts_with('-') || trimmed.starts_with('*') {
                let rule = trimmed.trim_start_matches('-').trim_start_matches('*').trim().to_string();
                if !rule.is_empty() {
                    current_section.rules.push(rule);
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with("---") {
                // Plain text line — treat as a rule
                current_section.rules.push(trimmed.to_string());
            }
        }

        // Push the last section
        if !current_section.rules.is_empty() || sections.is_empty() {
            sections.push(current_section);
        }

        let full_text = format_rules_for_prompt(&sections);

        Self { full_text, sections }
    }
}

/// Format sections as a structured prompt injection
fn format_rules_for_prompt(sections: &[RuleSection]) -> String {
    if sections.is_empty() {
        return String::new();
    }

    let mut output = String::from("\n\n## Project Rules\n\n");
    output.push_str("Follow these project-specific rules for ALL code you generate:\n\n");

    for section in sections {
        output.push_str(&format!("### {}\n", section.title));
        for rule in &section.rules {
            output.push_str(&format!("- {}\n", rule));
        }
        output.push('\n');
    }

    output.push_str("---\n");
    output
}

/// Extract a ## Rules section from AGENTS.md
fn extract_rules_section(content: &str) -> Option<String> {
    let mut in_rules = false;
    let mut rules_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("## Rules") || trimmed.starts_with("## Rules ") {
            in_rules = true;
            continue;
        }

        if in_rules {
            // Stop at the next ## section or end
            if trimmed.starts_with("## ") && !trimmed.starts_with("## Rules") {
                break;
            }
            rules_lines.push(line);
        }
    }

    if rules_lines.is_empty() {
        return None;
    }

    Some(rules_lines.join("\n"))
}

/// Get a brief summary of rules for display
pub fn rules_summary(rules_text: &str) -> String {
    let rule_count = rules_text.matches("- ").count();
    let section_count = rules_text.matches("### ").count();
    format!("{} rules in {} sections", rule_count, section_count.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_rules() {
        let text = "# Code Style\n- Use Result<T>\n- No unwrap()\n\n# Architecture\n- Routes in routes/\n";
        let rules = ProjectRules::parse(text);
        assert_eq!(rules.sections.len(), 2);
        assert_eq!(rules.sections[0].title, "Code Style");
        assert_eq!(rules.sections[0].rules.len(), 2);
        assert_eq!(rules.sections[1].rules.len(), 1);
    }

    #[test]
    fn test_parse_single_section() {
        let text = "- Rule 1\n- Rule 2\n- Rule 3\n";
        let rules = ProjectRules::parse(text);
        assert_eq!(rules.sections.len(), 1);
        assert_eq!(rules.sections[0].title, "General");
        assert_eq!(rules.sections[0].rules.len(), 3);
    }

    #[test]
    fn test_format_includes_header() {
        let rules = ProjectRules::parse("- use Result\n- no panic");
        assert!(rules.full_text.contains("Project Rules"));
        assert!(rules.full_text.contains("use Result"));
        assert!(rules.full_text.contains("no panic"));
    }

    #[test]
    fn test_empty_rules() {
        let text = "";
        let rules = ProjectRules::parse(text);
        assert_eq!(rules.sections.len(), 1);
        assert_eq!(rules.sections[0].title, "General");
        assert!(rules.sections[0].rules.is_empty());
    }

    #[test]
    fn test_extract_from_agents_md() {
        let content = "# Project\n\nSome text\n\n## Rules\n- Rule 1\n- Rule 2\n\n## Other\nblah";
        let extracted = extract_rules_section(content);
        assert!(extracted.is_some());
        assert!(extracted.unwrap().contains("Rule 1"));
    }
}
