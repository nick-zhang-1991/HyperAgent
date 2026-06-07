#![allow(unused)]
pub mod cache;
pub mod graph;
pub mod parser;
pub mod watcher;

use anyhow::{Context, Result};
use cache::IndexCache;
use graph::SymbolGraph;
use parser::CodeParser;
use std::path::{Path, PathBuf};
use watcher::FileWatcher;

/// File-level symbol information
#[derive(Debug, Clone)]
pub struct FileSymbols {
    pub file_path: PathBuf,
    pub rel_path: String,
    pub language: String,
    pub symbols: Vec<Symbol>,
}

/// A code symbol (function, class, struct, trait, etc.)
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
}

/// Types of symbols we track
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Class,
    Struct,
    Trait,
    Enum,
    Interface,
    Method,
    Variable,
    Import,
    Macro,
    Module,
    TypeAlias,
    Other(String),
}

impl SymbolKind {
    pub fn from_str(s: &str) -> Self {
        match s {
            "function" => SymbolKind::Function,
            "class" => SymbolKind::Class,
            "struct" => SymbolKind::Struct,
            "trait" => SymbolKind::Trait,
            "enum" => SymbolKind::Enum,
            "interface" => SymbolKind::Interface,
            "method" => SymbolKind::Method,
            "variable" => SymbolKind::Variable,
            "import" => SymbolKind::Import,
            "macro" => SymbolKind::Macro,
            "module" => SymbolKind::Module,
            "type_alias" | "type" => SymbolKind::TypeAlias,
            _ => SymbolKind::Other(s.to_string()),
        }
    }
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Function => write!(f, "function"),
            SymbolKind::Class => write!(f, "class"),
            SymbolKind::Struct => write!(f, "struct"),
            SymbolKind::Trait => write!(f, "trait"),
            SymbolKind::Enum => write!(f, "enum"),
            SymbolKind::Interface => write!(f, "interface"),
            SymbolKind::Method => write!(f, "method"),
            SymbolKind::Variable => write!(f, "variable"),
            SymbolKind::Import => write!(f, "import"),
            SymbolKind::Macro => write!(f, "macro"),
            SymbolKind::Module => write!(f, "module"),
            SymbolKind::TypeAlias => write!(f, "type_alias"),
            SymbolKind::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Index statistics
#[derive(Debug, Clone)]
pub struct IndexStats {
    pub files: usize,
    pub symbols: usize,
    pub references: usize,
    pub languages: usize,
    pub cache_size: String,
}

/// The HyperIndex - a global code understanding system
///
/// Combines tree-sitter parsing, SQLite caching, and PageRank-based
/// relevance ranking to provide instant code intelligence.
pub struct HyperIndex {
    root: PathBuf,
    parser: CodeParser,
    graph: SymbolGraph,
    cache: IndexCache,
    use_cache: bool,
}

impl HyperIndex {
    /// Create a new HyperIndex (loads cache if exists)
    pub fn new(root: &Path) -> Result<Self> {
        let root = root.canonicalize().context("Cannot find project root")?;
        let cache = IndexCache::new(&root)?;
        let parser = CodeParser::new();
        let graph = SymbolGraph::new();

        Ok(Self {
            root,
            parser,
            graph,
            cache,
            use_cache: true,
        })
    }

    /// Create or load existing index
    pub fn new_or_load(root: &Path) -> Result<Self> {
        let mut index = Self::new(root)?;
        if index.cache.has_data() {
            index.load_from_cache()?;
        } else {
            index.build()?;
        }
        Ok(index)
    }

    /// Check if cache exists and has data
    pub fn has_cache(&self) -> bool {
        self.cache.has_data()
    }

    /// Build the full index from scratch
    pub fn build(&mut self) -> Result<IndexStats> {
        println!("🔍 Scanning project: {}", self.root.display());

        let file_types = self.collect_source_files()?;
        let total = file_types.len();
        let mut processed = 0usize;

        for (rel_path, full_path, lang) in &file_types {
            processed += 1;
            if processed % 10 == 0 || processed == total {
                print!("\r   Parsing files: {processed}/{total}");
                use std::io::{Write, stdout};
                stdout().flush().ok();
            }

            match self.parser.parse_file(full_path, lang) {
                Ok(symbols) => {
                    let file_sym = FileSymbols {
                        file_path: full_path.clone(),
                        rel_path: rel_path.clone(),
                        language: lang.clone(),
                        symbols,
                    };
                    self.graph.add_file(file_sym)?;
                }
                Err(e) => {
                    tracing::debug!("Skipping {}: {e}", rel_path);
                }
            }
        }
        println!();

        // Build reference graph
        self.graph.build_reference_graph()?;

        // Run PageRank
        self.graph.compute_pagerank()?;

        // Save to cache
        if self.use_cache {
            self.save_to_cache()?;
        }

        let stats = IndexStats {
            files: self.graph.file_count(),
            symbols: self.graph.symbol_count(),
            references: self.graph.reference_count(),
            languages: self.graph.language_count(),
            cache_size: self.cache.size_str(),
        };

        Ok(stats)
    }

    /// Get ranked files relevant to a query
    pub fn get_relevant_files(&self, query: &str, max_files: usize, _max_tokens: u32) -> Vec<FileContext> {
        // Query relevance: match symbols by name
        let matched_symbols = self.graph.find_symbols(query);
        let ranked_files = self.graph.get_top_files(matched_symbols, max_files, query);

        ranked_files
            .into_iter()
            .map(|(path, score)| {
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let lines = content.lines().count();
                FileContext {
                    path,
                    score,
                    content,
                    total_lines: lines,
                    summary: String::new(),
                }
            })
            .collect()
    }

    /// Collect all source files in the project
    /// Uses ignore::WalkBuilder to respect .gitignore and .ignore files.
    /// Also supports .hyperignore for additional project-specific patterns.
    fn collect_source_files(&self) -> Result<Vec<(String, PathBuf, String)>> {
        let mut files = Vec::new();

        // Use ignore crate to respect .gitignore + .ignore automatically
        let walker = ignore::WalkBuilder::new(&self.root)
            .standard_filters(true)   // respect .gitignore, .ignore, hidden files
            .parents(true)
            .build();

        // Also load .hyperignore for additional patterns
        let ignore_file = self.root.join(".hyperignore");
        let hyperignore_patterns: Vec<String> = if ignore_file.exists() {
            std::fs::read_to_string(&ignore_file)
                .map(|content| {
                    content
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty() && !l.starts_with('#'))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![]
        };

        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = entry.path();

            // Skip if matches .hyperignore patterns
            if !hyperignore_patterns.is_empty() {
                let rel = path.strip_prefix(&self.root).unwrap_or(path);
                let rel_str = rel.to_string_lossy();
                if hyperignore_patterns.iter().any(|p| {
                    rel_str == *p || rel_str.starts_with(&format!("{p}/")) || rel_str.contains(p)
                }) {
                    continue;
                }
            }

            let rel_path = path
                .strip_prefix(&self.root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            // Detect language by extension
            if let Some(lang) = self.detect_language(path) {
                files.push((rel_path, path.to_path_buf(), lang));
            }
        }

        Ok(files)
    }

    /// Detect programming language from file extension
    fn detect_language(&self, path: &Path) -> Option<String> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        match ext.as_str() {
            "rs" => Some("rust".into()),
            "py" => Some("python".into()),
            "js" | "mjs" | "cjs" => Some("javascript".into()),
            "ts" | "tsx" => Some("typescript".into()),
            "jsx" => Some("jsx".into()),
            "go" => Some("go".into()),
            "java" => Some("java".into()),
            "c" | "h" => Some("c".into()),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" => Some("cpp".into()),
            "rb" => Some("ruby".into()),
            "php" => Some("php".into()),
            "swift" => Some("swift".into()),
            "kt" | "kts" => Some("kotlin".into()),
            "scala" => Some("scala".into()),
            "rsx" | "rsh" => Some("rust".into()),
            "ex" | "exs" => Some("elixir".into()),
            "clj" | "cljs" | "cljc" | "edn" => Some("clojure".into()),
            "hs" => Some("haskell".into()),
            "lua" => Some("lua".into()),
            "sh" | "bash" | "zsh" => Some("bash".into()),
            "sql" => Some("sql".into()),
            "r" | "R" => Some("r".into()),
            "dart" => Some("dart".into()),
            "zig" => Some("zig".into()),
            "toml" | "yaml" | "yml" | "json" | "xml" | "html" | "css" | "scss" => {
                Some("config".into())
            }
            _ => None,
        }
    }

    fn load_from_cache(&mut self) -> Result<()> {
        self.graph = self.cache.load_graph()?;
        Ok(())
    }

    fn save_to_cache(&self) -> Result<()> {
        self.cache.save_graph(&self.graph)?;
        Ok(())
    }

    pub fn stats(&self) -> Result<IndexStats> {
        Ok(IndexStats {
            files: self.graph.file_count(),
            symbols: self.graph.symbol_count(),
            references: self.graph.reference_count(),
            languages: self.graph.language_count(),
            cache_size: self.cache.size_str(),
        })
    }

    /// Start a file watcher to detect codebase changes
    /// Incrementally updates the index for changed files.
    /// Falls back to full rebuild for edge cases.
    pub fn watch(&self) -> Result<FileWatcher> {
        let root = self.root.clone();
        let root_for_watch = root.clone();
        let cache_db_path = self.cache.db_path();
        let parser = self.parser.clone();
        let callback = move |paths: Vec<String>| {
            tracing::info!("Files changed: {} file(s), re-indexing...", paths.len());
            let mut count = 0usize;
            let mut had_errors = false;

            // Open a separate DB connection for incremental updates
            let cache = match IndexCache::new(&root) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("Cannot open cache for incremental update: {e}");
                    let _ = std::fs::remove_file(&cache_db_path);
                    return;
                }
            };

            for path_str in &paths {
                let full_path = PathBuf::from(path_str);
                let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");

                // Only process source files
                let lang = match ext {
                    "rs" => "rust",
                    "py" => "python",
                    "ts" | "tsx" => "typescript",
                    "js" | "jsx" => "javascript",
                    "go" => "go",
                    "java" => "java",
                    _ => continue,
                };

                if !full_path.exists() {
                    // File deleted — remove from cache
                    let rel = full_path.strip_prefix(&root).unwrap_or(&full_path);
                    if cache.remove_file(&rel.to_string_lossy()).is_err() {
                        had_errors = true;
                    }
                    count += 1;
                    tracing::debug!("Removed from index: {}", full_path.display());
                    continue;
                }

                // Re-parse and update
                match parser.parse_file(&full_path, lang) {
                    Ok(symbols) => {
                        let rel = full_path.strip_prefix(&root).unwrap_or(&full_path);
                        let file_sym = FileSymbols {
                            file_path: full_path.clone(),
                            rel_path: rel.to_string_lossy().to_string(),
                            language: lang.to_string(),
                            symbols,
                        };
                        if cache.upsert_file(&file_sym).is_err() {
                            had_errors = true;
                        }
                        count += 1;
                        tracing::debug!("Re-indexed: {}", full_path.display());
                    }
                    Err(e) => {
                        tracing::debug!("Skipping {}: {e}", full_path.display());
                    }
                }
            }

            if count > 0 && !had_errors {
                tracing::info!("Incremental index: {count} files re-indexed");
            } else if had_errors {
                tracing::warn!("Incremental update had errors — invalidating cache for full rebuild");
                let _ = std::fs::remove_file(&cache_db_path);
            }
        };
        let mut watcher = FileWatcher::new(&self.root, callback)?;
        watcher.watch(&root_for_watch)?;
        println!("   👁️  Watching for file changes (incremental)...");
        Ok(watcher)
    }
}

/// A file with its relevance context
#[derive(Debug, Clone)]
pub struct FileContext {
    pub path: PathBuf,
    pub score: f64,
    pub content: String,
    pub total_lines: usize,
    pub summary: String,
}

impl FileContext {
    /// Generate a condensed summary: first 15 lines + symbol list
    pub fn generate_summary(&mut self) {
        if self.content.is_empty() {
            self.summary = String::new();
            return;
        }
        let lines: Vec<&str> = self.content.lines().collect();
        let mut summary = String::new();

        // First 15 lines as style preview
        let preview_lines = lines.len().min(15);
        for line in lines.iter().take(preview_lines) {
            summary.push_str(line);
            summary.push('\n');
        }
        if preview_lines < lines.len() {
            summary.push_str(&format!("// ... {}/{} lines shown\n", preview_lines, lines.len()));
        }

        // Extract key symbols: function names, structs, classes from the content
        let ext = self.path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = match ext {
            "rs" => "rust", "py" => "python", "js" | "mjs" => "javascript",
            "ts" | "tsx" => "typescript", "go" => "go", "java" => "java",
            _ => "",
        };
        if !lang.is_empty() {
            let parser = crate::index::parser::CodeParser::new();
            let tmp_dir = std::env::temp_dir().join("hyper-summary");
            let _ = std::fs::create_dir_all(&tmp_dir);
            let tmp_path = tmp_dir.join(format!("summary.{}", ext));
            let _ = std::fs::write(&tmp_path, &self.content);
            if let Ok(symbols) = parser.parse_file(&tmp_path, lang) {
                let names: Vec<String> = symbols.iter()
                    .filter(|s| matches!(s.kind, crate::index::SymbolKind::Function | crate::index::SymbolKind::Struct | crate::index::SymbolKind::Class | crate::index::SymbolKind::Trait | crate::index::SymbolKind::Interface))
                    .map(|s| s.name.clone())
                    .collect();
                if !names.is_empty() {
                    summary.push_str(&format!("// Key symbols: {}\n", names.join(", ")));
                }
            }
            let _ = std::fs::remove_file(&tmp_path);
        }

        self.summary = summary;
    }

    /// Replace content with condensed version for token efficiency
    pub fn condense(&mut self, max_chars: usize) {
        if self.content.len() <= max_chars {
            return;
        }
        // Keep first 20% and last 10% of the file
        let first_part = max_chars * 2 / 3;
        let last_part = max_chars / 3;
        let chars: Vec<char> = self.content.chars().collect();
        let mut condensed = String::with_capacity(max_chars + 50);
        condensed.extend(chars.iter().take(first_part));
        condensed.push_str("\n// ... [truncated] ...\n");
        if chars.len() > last_part {
            condensed.extend(chars.iter().skip(chars.len() - last_part).take(last_part));
        }
        self.content = condensed;
    }

    /// Return condensed representation for LLM prompts
    pub fn to_condensed_string(&self) -> String {
        if !self.summary.is_empty() {
            format!(
                "--- {} ({} lines, score: {:.2}) ---\n{}",
                self.path.display(),
                self.total_lines,
                self.score,
                if self.content.len() > 2000 {
                    &self.summary
                } else {
                    &self.content
                }
            )
        } else {
            format!(
                "--- {} ({} lines, score: {:.2}) ---\n{}",
                self.path.display(),
                self.total_lines,
                self.score,
                self.content
            )
        }
    }
}
