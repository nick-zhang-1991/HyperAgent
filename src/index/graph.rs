use anyhow::Result;
use petgraph::graph::{NodeIndex, UnGraph};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::{FileSymbols, Symbol, SymbolKind};

/// The symbol graph for PageRank-based file relevance
///
/// Builds a graph where:
/// - Nodes = files
/// - Edges = symbol references (imports, function calls, type usage)
/// - PageRank score = file relevance
pub struct SymbolGraph {
    pub(crate) files: Vec<FileEntry>,
    // File index by path
    file_indices: HashMap<PathBuf, usize>,
    // The graph itself
    graph: UnGraph<usize, f64>,
    // PageRank scores
    pagerank_scores: Vec<f64>,
    // Symbol name → file indices that define it
    symbol_definitions: HashMap<String, HashSet<usize>>,
    // Symbol name → file indices that reference it
    symbol_references: HashMap<String, HashSet<usize>>,
    // All symbols for fast lookup
    all_symbols: Vec<(String, String)>, // (symbol_name, file_path)
}

#[derive(Debug, Clone)]
pub(crate) struct FileEntry {
    pub(crate) path: PathBuf,
    pub(crate) rel_path: String,
    pub(crate) language: String,
    pub(crate) symbols: Vec<Symbol>,
    #[allow(dead_code)]
    size: usize,
}

impl SymbolGraph {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            file_indices: HashMap::new(),
            graph: UnGraph::new_undirected(),
            pagerank_scores: Vec::new(),
            symbol_definitions: HashMap::new(),
            symbol_references: HashMap::new(),
            all_symbols: Vec::new(),
        }
    }

    /// Add a parsed file to the graph
    pub fn add_file(&mut self, file_sym: FileSymbols) -> Result<()> {
        let path = file_sym.file_path.clone();
        if self.file_indices.contains_key(&path) {
            return Ok(()); // Already indexed
        }

        let idx = self.files.len();
        let _node = self.graph.add_node(idx);

        let rel_path = file_sym.rel_path.clone();
        let language = file_sym.language.clone();

        // Track symbol definitions
        for sym in &file_sym.symbols {
            self.all_symbols
                .push((sym.name.clone(), rel_path.clone()));

            match sym.kind {
                SymbolKind::Import => {
                    // Import statements create edges to other files
                    self.symbol_references
                        .entry(sym.name.clone())
                        .or_default()
                        .insert(idx);
                }
                _ => {
                    // Definitions are tracked for reference resolution
                    self.symbol_definitions
                        .entry(sym.name.clone())
                        .or_default()
                        .insert(idx);
                }
            }
        }

        self.files.push(FileEntry {
            path: path.clone(),
            rel_path,
            language,
            symbols: file_sym.symbols,
            size: 0,
        });

        self.file_indices.insert(path, idx);

        Ok(())
    }

    /// Build the reference graph by connecting files that reference each other
    pub fn build_reference_graph(&mut self) -> Result<()> {
        // Connect files via import/reference relationships
        for file_idx in 0..self.files.len() {
            let file = &self.files[file_idx];

            // Find all import symbols with their signatures
            let imports: Vec<(String, String)> = file
                .symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Import)
                .map(|s| (s.name.clone(), s.signature.clone()))
                .collect();

            for (import_name, signature) in &imports {

                // Strategy 1: Try exact match against symbol definitions
                if let Some(targets) = self.symbol_definitions.get(import_name) {
                    for &target_idx in targets {
                        Self::add_edge_weighted(&mut self.graph, file_idx, target_idx, 1.0);
                    }
                }

                // Strategy 2: Try last-segment match (e.g., "crate::utils::helpers" → "helpers")
                let last_segment = import_name.split("::").last()
                    .or_else(|| import_name.split('/').last())
                    .unwrap_or(import_name);
                if last_segment != import_name {
                    if let Some(targets) = self.symbol_definitions.get(last_segment) {
                        for &target_idx in targets {
                            Self::add_edge_weighted(&mut self.graph, file_idx, target_idx, 0.8);
                        }
                    }
                }

                // Strategy 3: Match import path to file paths
                // "crate::utils::helpers" → look for files with "utils/helpers" in path
                let path_candidates: Vec<String> = import_name
                    .split("::")
                    .filter(|s| !s.is_empty() && *s != "crate" && *s != "self" && *s != "super")
                    .map(|s| s.to_string())
                    .collect();

                if !path_candidates.is_empty() {

                    for (j, other_file) in self.files.iter().enumerate() {
                        if j != file_idx {
                            let rel_lower = other_file.rel_path.to_lowercase();
                            let rel_no_ext = rel_lower
                                .trim_end_matches(".rs")
                                .trim_end_matches(".py")
                                .trim_end_matches(".ts")
                                .trim_end_matches(".js");
                            let rel_as_path = rel_no_ext.replace('/', "::").replace('\\', "::");

                            // Check if the import path appears in this file's module path
                            for seg in &path_candidates {
                                if rel_as_path.contains(&seg.to_lowercase()) {
                                    Self::add_edge_weighted(&mut self.graph, file_idx, j, 0.5);
                                    break;
                                }
                            }
                        }
                    }
                }

                // Strategy 4: Parse the full signature line for Rust-style `use` statements
                // e.g. "use crate::module::StructName" - try matching StructName
                if signature.starts_with("use ") {
                    for seg in import_name.split("::") {
                        if let Some(targets) = self.symbol_definitions.get(seg) {
                            for &target_idx in targets {
                                if file_idx != target_idx {
                                    Self::add_edge_weighted(&mut self.graph, file_idx, target_idx, 0.3);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Add an edge between two nodes, incrementing weight if edge already exists
    fn add_edge_weighted(graph: &mut UnGraph<usize, f64>, from: usize, to: usize, weight: f64) {
        let n1 = NodeIndex::new(from);
        let n2 = NodeIndex::new(to);
        if !graph.contains_edge(n1, n2) {
            graph.add_edge(n1, n2, weight);
        } else if let Some(edge) = graph.find_edge(n1, n2) {
            if let Some(w) = graph.edge_weight_mut(edge) {
                *w += weight;
            }
        }
    }

    /// Compute PageRank scores for all files
    pub fn compute_pagerank(&mut self) -> Result<()> {
        let n = self.files.len();
        if n == 0 {
            return Ok(());
        }

        // If graph has no edges, assign equal scores
        if self.graph.edge_count() == 0 {
            let score = 1.0 / n as f64;
            self.pagerank_scores = vec![score; n];
            return Ok(());
        }

        // Run PageRank
        let damping = 0.85;
        let max_iter = 100;
        let tolerance = 1e-6;

        let mut scores = vec![1.0 / n as f64; n];
        let mut new_scores = vec![0.0; n];

        // Precompute out-degree for each node
        let mut out_degree = vec![0usize; n];
        for edge in self.graph.edge_indices() {
            if let Some((start, end)) = self.graph.edge_endpoints(edge) {
                out_degree[start.index()] += 1;
                out_degree[end.index()] += 1;
            }
        }

        for _iter in 0..max_iter {
            let mut diff = 0.0;

            for i in 0..n {
                let node = NodeIndex::new(i);
                let mut sum = 0.0;

                for neighbor in self.graph.neighbors(node) {
                    let j = neighbor.index();
                    if out_degree[j] > 0 {
                        if let Some(edge) = self.graph.find_edge(node, neighbor) {
                            if let Some(w) = self.graph.edge_weight(edge) {
                                sum += scores[j] * w / out_degree[j] as f64;
                            }
                        }
                    }
                }

                new_scores[i] = (1.0 - damping) / n as f64 + damping * sum;
                diff += (new_scores[i] - scores[i]).abs();
            }

            std::mem::swap(&mut scores, &mut new_scores);

            if diff < tolerance {
                break;
            }
        }

        self.pagerank_scores = scores;
        Ok(())
    }

    /// Find symbols matching a query (fuzzy match)
    pub fn find_symbols(&self, query: &str) -> Vec<String> {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        if query_words.is_empty() {
            return vec![];
        }

        let mut matches: Vec<(f64, String)> = self
            .all_symbols
            .iter()
            .filter_map(|(name, path)| {
                let name_lower = name.to_lowercase();
                let path_lower = path.to_lowercase();

                // Score based on how many query words match
                let name_match_count = query_words
                    .iter()
                    .filter(|w| name_lower.contains(*w))
                    .count() as f64
                    * 2.0; // Name matches are weighted double

                let path_match_count = query_words
                    .iter()
                    .filter(|w| path_lower.contains(*w))
                    .count() as f64;

                let total = name_match_count + path_match_count;
                if total > 0.0 {
                    Some((total, name.clone()))
                } else {
                    None
                }
            })
            .collect();

        matches.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        matches.truncate(30);
        matches.into_iter().map(|(_, name)| name).collect()
    }

    /// Get top files by PageRank score, boosted by matched symbols and query path matches
    pub fn get_top_files(&self, matched_symbols: Vec<String>, max_files: usize, query: &str) -> Vec<(PathBuf, f64)> {
        let mut file_scores: HashMap<usize, f64> = HashMap::new();
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        // Start with PageRank scores
        for (i, score) in self.pagerank_scores.iter().enumerate() {
            file_scores.insert(i, *score);
        }

        // Boost files that contain matched symbols
        for sym_name in &matched_symbols {
            if let Some(def_indices) = self.symbol_definitions.get(sym_name) {
                for &idx in def_indices {
                    *file_scores.entry(idx).or_insert(0.0) += 0.5;
                }
            }
            if let Some(ref_indices) = self.symbol_references.get(sym_name) {
                for &idx in ref_indices {
                    *file_scores.entry(idx).or_insert(0.0) += 0.3;
                }
            }
        }

        // Direct path boost: if query words appear in file path, boost by 0.2 per word
        for (i, file_entry) in self.files.iter().enumerate() {
            let path_lower = file_entry.rel_path.to_lowercase();
            for word in &query_words {
                if path_lower.contains(word) {
                    *file_scores.entry(i).or_insert(0.0) += 0.2;
                }
            }
        }

        // Sort by score
        let mut sorted: Vec<(usize, f64)> = file_scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top files
        sorted
            .into_iter()
            .take(max_files)
            .filter_map(|(idx, score)| {
                self.files.get(idx).map(|f| (f.path.clone(), score))
            })
            .collect()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn symbol_count(&self) -> usize {
        self.all_symbols.len()
    }

    pub fn reference_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn language_count(&self) -> usize {
        let mut langs = HashSet::new();
        for f in &self.files {
            langs.insert(f.language.clone());
        }
        langs.len()
    }

    /// Iterate over edges: (from_file_index, to_file_index, weight)
    pub(crate) fn iter_edges(&self) -> Vec<(usize, usize, f64)> {
        self.graph
            .edge_references()
            .map(|e| {
                let (from, to) = (e.source().index(), e.target().index());
                (from, to, *e.weight())
            })
            .collect()
    }
}