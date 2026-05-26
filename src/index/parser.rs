use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

use super::{Symbol, SymbolKind};

/// Code parser using regex-based symbol extraction
pub struct CodeParser {
    patterns: HashMap<String, Vec<(Regex, SymbolKind)>>,
}

impl CodeParser {
    pub fn new() -> Self {
        Self {
            patterns: Self::build_patterns(),
        }
    }

    fn build_patterns() -> HashMap<String, Vec<(Regex, SymbolKind)>> {
        let mut m = HashMap::new();

        let insert = |m: &mut HashMap<_, _>, lang: &str, patterns: Vec<(&str, SymbolKind)>| {
            let compiled: Vec<_> = patterns
                .into_iter()
                .filter_map(|(p, k)| Regex::new(p).ok().map(|r| (r, k)))
                .collect();
            m.insert(lang.to_string(), compiled);
        };

        insert(
            &mut m,
            "rust",
            vec![
                (r"(?:pub\s+)?(?:async\s+)?fn\s+(\w+)", SymbolKind::Function),
                (r"(?:pub\s+)?unsafe\s+(?:async\s+)?fn\s+(\w+)", SymbolKind::Function),
                (r"(?:pub\s+)?struct\s+(\w+)", SymbolKind::Struct),
                (r"(?:pub\s+)?trait\s+(\w+)", SymbolKind::Trait),
                (r"(?:pub\s+)?enum\s+(\w+)", SymbolKind::Enum),
                (r"use\s+([\w:]+)", SymbolKind::Import),
                (r"(?:pub\s+)?mod\s+(\w+)", SymbolKind::Module),
                (r"(?:pub\s+)?type\s+(\w+)", SymbolKind::TypeAlias),
                (r"(?:pub\s+)?macro_rules!\s+(\w+)", SymbolKind::Macro),
            ],
        );

        insert(
            &mut m,
            "python",
            vec![
                (r"^(?:async\s+)?def\s+(\w+)\s*\(", SymbolKind::Function),
                (r"^class\s+(\w+)", SymbolKind::Class),
                (r"^import\s+(\S+)", SymbolKind::Import),
                (r"^from\s+(\S+)\s+import", SymbolKind::Import),
            ],
        );

        insert(
            &mut m,
            "javascript",
            vec![
                (r"(?:export\s+)?(?:async\s+)?function\s+(\w+)", SymbolKind::Function),
                (r"(?:export\s+)?class\s+(\w+)", SymbolKind::Class),
                (r"(?:export\s+)?const\s+(\w+)\s*=", SymbolKind::Variable),
                (r#"(?:import\s+.*from\s+["'])([^"']+)(?:["'])"#, SymbolKind::Import),
            ],
        );

        insert(
            &mut m,
            "typescript",
            vec![
                (r"(?:export\s+)?(?:async\s+)?function\s+(\w+)", SymbolKind::Function),
                (r"(?:export\s+)?class\s+(\w+)", SymbolKind::Class),
                (r"(?:export\s+)?interface\s+(\w+)", SymbolKind::Interface),
                (r"(?:export\s+)?type\s+(\w+)\s*=", SymbolKind::TypeAlias),
                (r"(?:export\s+)?enum\s+(\w+)", SymbolKind::Enum),
                (r#"(?:import\s+.*from\s+["'])([^"']+)(?:["'])"#, SymbolKind::Import),
            ],
        );

        insert(
            &mut m,
            "go",
            vec![
                (r"^func\s+(\w+)", SymbolKind::Function),
                (r"^type\s+(\w+)\s+struct", SymbolKind::Struct),
                (r"^type\s+(\w+)\s+interface", SymbolKind::Interface),
                (r#"^import\s+["']([^"']+)"#, SymbolKind::Import),
            ],
        );

        insert(
            &mut m,
            "java",
            vec![
                (r"(?:public|private|protected)?\s*(?:class)\s+(\w+)", SymbolKind::Class),
                (r"(?:public|private|protected)?\s*(?:interface)\s+(\w+)", SymbolKind::Interface),
                (r"import\s+([\w.]+);", SymbolKind::Import),
            ],
        );

        m
    }

    pub fn parse_file(&self, path: &Path, language: &str) -> Result<Vec<Symbol>> {
        let content = std::fs::read_to_string(path)?;

        if language == "config" {
            return Ok(vec![]);
        }

        let mut symbols = Vec::new();

        if let Some(patterns) = self.patterns.get(language) {
            for (i, line) in content.lines().enumerate() {
                for (re, kind) in patterns {
                    if let Some(caps) = re.captures(line) {
                        if let Some(name) = caps.get(1) {
                            let name = name.as_str().trim().to_string();
                            if !name.is_empty() && name.len() <= 100 && !name.starts_with('#') {
                                symbols.push(Symbol {
                                    name,
                                    kind: kind.clone(),
                                    start_line: i + 1,
                                    end_line: i + 1,
                                    signature: line.trim().to_string(),
                                });
                                break;
                            }
                        }
                    }
                }
            }
        }

        Ok(symbols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_rust_function() {
        let content = "pub fn hello() {}\nfn world() {}\nasync fn test() {}";
        let path = std::path::Path::new("/tmp/test.rs");
        std::fs::write(path, content).unwrap();
        let parser = CodeParser::new();
        let symbols = parser.parse_file(path, "rust").unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(symbols.len(), 3);
        assert!(symbols.iter().any(|s| s.name == "hello"));
        assert!(symbols.iter().any(|s| s.name == "world"));
    }

    #[test]
    fn test_detect_rust_struct() {
        let content = "pub struct Config {}\nstruct Inner {}\n";
        let path = std::path::Path::new("/tmp/test_struct.rs");
        std::fs::write(path, content).unwrap();
        let parser = CodeParser::new();
        let symbols = parser.parse_file(path, "rust").unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(symbols.len(), 2);
        assert!(symbols.iter().any(|s| s.name == "Config"));
    }

    #[test]
    fn test_detect_python_function() {
        let content = "def hello():\n    pass\nclass MyClass:\n    pass\n";
        let path = std::path::Path::new("/tmp/test.py");
        std::fs::write(path, content).unwrap();
        let parser = CodeParser::new();
        let symbols = parser.parse_file(path, "python").unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(symbols.len(), 2);
        assert!(symbols.iter().any(|s| s.name == "hello"));
        assert!(symbols.iter().any(|s| s.name == "MyClass"));
    }

    #[test]
    fn test_detect_config_returns_empty() {
        let content = "key = value\n[section]\nname = \"test\"\n";
        let path = std::path::Path::new("/tmp/test.toml");
        std::fs::write(path, content).unwrap();
        let parser = CodeParser::new();
        let symbols = parser.parse_file(path, "config").unwrap();
        let _ = std::fs::remove_file(path);
        assert!(symbols.is_empty());
    }
}
