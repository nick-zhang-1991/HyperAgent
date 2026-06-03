//! Tree-sitter based code parser (optional, behind `ts-parser` feature)
//!
//! Replaces the regex-based parser with AST-level symbol extraction.
//! Much more accurate: handles generics, lifetimes, nested types,
//! and all edge cases that regex misses.
//!
//! Use:
//! ```bash
//! cargo build --features ts-parser --release
//! ```

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use super::{Symbol, SymbolKind};

/// Language configuration mapping file extensions to tree-sitter languages
fn lang_for_file(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" | "mjs" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        "go" => Some("go"),
        "java" => Some("java"),
        _ => None,
    }
}

/// Get the tree-sitter language for a file extension
fn get_language(lang: &str) -> Option<tree_sitter::Language> {
    match lang {
        #[cfg(feature = "ts-parser")]
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        #[cfg(feature = "ts-parser")]
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        #[cfg(feature = "ts-parser")]
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        #[cfg(feature = "ts-parser")]
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        #[cfg(feature = "ts-parser")]
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        #[cfg(feature = "ts-parser")]
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        _ => None,
    }
}

/// Tree-sitter based parser
pub struct TsParser;

impl TsParser {
    /// Parse symbols from a file using tree-sitter
    pub fn parse_file(path: &Path, content: &str) -> Result<Vec<Symbol>> {
        let lang_name = lang_for_file(path)
            .ok_or_else(|| anyhow::anyhow!("Unsupported language: {}", path.display()))?;

        let language = get_language(lang_name)
            .ok_or_else(|| anyhow::anyhow!("Tree-sitter language not available: {lang_name}"))?;

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language)
            .map_err(|e| anyhow::anyhow!("Failed to set language: {e}"))?;

        let tree = parser.parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse: {}", path.display()))?;

        let root = tree.root_node();
        let mut symbols = Vec::new();
        Self::extract_symbols(&root, content, &mut symbols, 0);
        Ok(symbols)
    }

    /// Recursively extract symbols from the AST
    fn extract_symbols(
        node: &tree_sitter::Node,
        source: &str,
        symbols: &mut Vec<Symbol>,
        depth: usize,
    ) {
        if depth > 50 {
            return; // Safety limit
        }

        let kind = node.kind();
        let start = node.start_position();
        let end = node.end_position();

        // Check for symbol declarations based on node kind
        if let Some(symbol) = Self::node_to_symbol(node, kind, source) {
            symbols.push(symbol);
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::extract_symbols(&child, source, symbols, depth + 1);
        }
    }

    /// Convert a tree-sitter node to a Symbol, if applicable
    fn node_to_symbol(node: &tree_sitter::Node, kind: &str, source: &str) -> Option<Symbol> {
        let name = Self::node_name(node, source)?;
        let (start, end) = byte_span(node);

        match kind {
            // Rust
            "function_item" | "function_signature" => {
                Some(Symbol::new(&name, SymbolKind::Function, start, end))
            }
            "struct_item" => Some(Symbol::new(&name, SymbolKind::Struct, start, end)),
            "trait_item" => Some(Symbol::new(&name, SymbolKind::Trait, start, end)),
            "enum_item" => Some(Symbol::new(&name, SymbolKind::Enum, start, end)),
            "mod_item" | "module" => Some(Symbol::new(&name, SymbolKind::Module, start, end)),
            "type_item" | "type_alias" => Some(Symbol::new(&name, SymbolKind::TypeAlias, start, end)),
            "macro_definition" => Some(Symbol::new(&name, SymbolKind::Macro, start, end)),
            "use_declaration" => {
                let path = Self::node_text(node, source).unwrap_or(&name);
                // Extract the last segment
                let short_name = path.split("::").last().unwrap_or(path);
                Some(Symbol::new(short_name, SymbolKind::Import, start, end))
            }

            // Python
            "function_definition" => Some(Symbol::new(&name, SymbolKind::Function, start, end)),
            "class_definition" => Some(Symbol::new(&name, SymbolKind::Class, start, end)),
            "import_statement" | "import_from_statement" => {
                Some(Symbol::new(&name, SymbolKind::Import, start, end))
            }

            // JavaScript/TypeScript
            "function_declaration" => Some(Symbol::new(&name, SymbolKind::Function, start, end)),
            "class_declaration" => Some(Symbol::new(&name, SymbolKind::Class, start, end)),
            "method_definition" => Some(Symbol::new(&name, SymbolKind::Function, start, end)),
            "interface_declaration" => Some(Symbol::new(&name, SymbolKind::Interface, start, end)),
            "enum_declaration" => Some(Symbol::new(&name, SymbolKind::Enum, start, end)),
            "type_alias_declaration" => Some(Symbol::new(&name, SymbolKind::TypeAlias, start, end)),
            "variable_declaration" | "lexical_declaration" => {
                Some(Symbol::new(&name, SymbolKind::Variable, start, end))
            }
            "arrow_function" => {
                if let Some(parent) = node.parent() {
                    if parent.kind() == "variable_declarator" {
                        let var_name = Self::node_name(&parent, source)?;
                        return Some(Symbol::new(&var_name, SymbolKind::Function, start, end));
                    }
                }
                None
            }

            // Go
            "function_declaration" | "method_declaration" => {
                Some(Symbol::new(&name, SymbolKind::Function, start, end))
            }
            "struct_type" | "type_spec" => Some(Symbol::new(&name, SymbolKind::Struct, start, end)),
            "interface_type" => Some(Symbol::new(&name, SymbolKind::Interface, start, end)),
            "import_declaration" => Some(Symbol::new(&name, SymbolKind::Import, start, end)),

            // Java
            "method_declaration" => Some(Symbol::new(&name, SymbolKind::Function, start, end)),
            "class_declaration" => Some(Symbol::new(&name, SymbolKind::Class, start, end)),
            "interface_declaration" => Some(Symbol::new(&name, SymbolKind::Interface, start, end)),
            "import_declaration" => Some(Symbol::new(&name, SymbolKind::Import, start, end)),

            _ => None,
        }
    }

    /// Get the name of a node (first child named "name")
    fn node_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
        // Try `name` child first
        if let Some(name_node) = node.child_by_field_name("name") {
            return Self::node_text(&name_node, source).map(|s| s.to_string());
        }

        // Fallback: first identifier-like token
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() {
                if let Some(text) = Self::node_text(&child, source) {
                    if text.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        return Some(text.to_string());
                    }
                }
            }
        }

        None
    }

    /// Extract text content of a node
    fn node_text<'a>(node: &tree_sitter::Node, source: &'a str) -> Option<&'a str> {
        let start = node.start_byte();
        let end = node.end_byte();
        source.get(start..end)
    }
}

/// Get byte span from a node
fn byte_span(node: &tree_sitter::Node) -> (usize, usize) {
    (node.start_byte(), node.end_byte())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_lang_for_file() {
        assert_eq!(lang_for_file(&PathBuf::from("main.rs")), Some("rust"));
        assert_eq!(lang_for_file(&PathBuf::from("app.py")), Some("python"));
        assert_eq!(lang_for_file(&PathBuf::from("index.ts")), Some("typescript"));
        assert_eq!(lang_for_file(&PathBuf::from("main.go")), Some("go"));
        assert_eq!(lang_for_file(&PathBuf::from("file.txt")), None);
    }

    #[cfg(feature = "ts-parser")]
    #[test]
    fn test_parse_rust_function() {
        let path = PathBuf::from("test.rs");
        let content = r#"
fn hello() -> String {
    "world".to_string()
}

pub async fn compute(x: i32) -> i32 {
    x * 2
}
"#;
        let symbols = TsParser::parse_file(&path, content).unwrap();
        assert!(symbols.iter().any(|s| s.name == "hello" && s.kind == SymbolKind::Function));
        assert!(symbols.iter().any(|s| s.name == "compute" && s.kind == SymbolKind::Function));
    }

    #[cfg(feature = "ts-parser")]
    #[test]
    fn test_parse_rust_struct_and_trait() {
        let path = PathBuf::from("test.rs");
        let content = r#"
pub struct User {
    name: String,
}

trait Runnable {
    fn run(&self);
}
"#;
        let symbols = TsParser::parse_file(&path, content).unwrap();
        assert!(symbols.iter().any(|s| s.name == "User" && s.kind == SymbolKind::Struct));
        assert!(symbols.iter().any(|s| s.name == "Runnable" && s.kind == SymbolKind::Trait));
    }
}
