use anyhow::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::graph::SymbolGraph;

/// SQLite-based cache for the symbol index
///
/// Speeds up cold starts by persisting the parsed index.
/// Uses a simple schema:
/// - `symbols`: all symbol definitions
/// - `references`: inter-file references  
/// - `file_metadata`: file-level info
pub struct IndexCache {
    conn: Mutex<Connection>,
    db_path: PathBuf,
}

impl IndexCache {
    pub fn new(root: &Path) -> Result<Self> {
        let cache_dir = root.join(".hyper");
        std::fs::create_dir_all(&cache_dir)?;

        let db_path = cache_dir.join("index.db");
        let conn = Connection::open(&db_path)?;

        // Create tables
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                rel_path TEXT NOT NULL,
                language TEXT NOT NULL,
                pagerank REAL DEFAULT 0.0
            );
            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                start_line INTEGER,
                end_line INTEGER,
                signature TEXT,
                FOREIGN KEY (file_id) REFERENCES files(id)
            );
            CREATE TABLE IF NOT EXISTS ref_edges (
                from_file INTEGER NOT NULL,
                to_file INTEGER NOT NULL,
                weight REAL DEFAULT 1.0,
                PRIMARY KEY (from_file, to_file),
                FOREIGN KEY (from_file) REFERENCES files(id),
                FOREIGN KEY (to_file) REFERENCES files(id)
            );
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);
            CREATE INDEX IF NOT EXISTS idx_ref_edges_from ON ref_edges(from_file);
            CREATE TABLE IF NOT EXISTS file_snapshots (
                path TEXT PRIMARY KEY,
                mtime INTEGER NOT NULL,
                content_hash TEXT NOT NULL,
                size_bytes INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_snapshots_path ON file_snapshots(path);
            ",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path,
        })
    }

    pub fn has_data(&self) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM files", [], |row| {
            let count: i64 = row.get(0)?;
            Ok(count > 0)
        })
        .unwrap_or(false)
    }

    pub fn load_graph(&self) -> Result<SymbolGraph> {
        let conn = self.conn.lock().unwrap();
        let mut graph = SymbolGraph::new();

        // Load files (we'll rebuild symbols and edges)
        let mut stmt = conn.prepare("SELECT id, path, rel_path, language, pagerank FROM files")?;
        let file_rows: Vec<(i64, String, String, String, f64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Rebuild FileSymbols from cached data
        for (_id, path, rel_path, language, _pagerank) in &file_rows {
            let mut sym_stmt = conn.prepare(
                "SELECT name, kind, start_line, end_line, signature FROM symbols WHERE file_id = ?1",
            )?;
            let mut symbols: Vec<super::Symbol> = Vec::new();
            let rows = sym_stmt.query_map([_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, i32>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            for (name, kind, start, end, sig) in rows.flatten() {
                    symbols.push(super::Symbol {
                        name,
                        kind: super::SymbolKind::from_str(&kind),
                        start_line: start as usize,
                        end_line: end as usize,
                        signature: sig,
                    });
            }
            drop(sym_stmt);

            graph.add_file(super::FileSymbols {
                file_path: PathBuf::from(path),
                rel_path: rel_path.clone(),
                language: language.clone(),
                symbols,
            })?;
        }

        // Build reference graph and compute PageRank
        graph.build_reference_graph()?;
        graph.compute_pagerank()?;

        Ok(graph)
    }

    pub fn save_graph(&self, graph: &SymbolGraph) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Clear old data
        conn.execute("DELETE FROM ref_edges", [])?;
        conn.execute("DELETE FROM symbols", [])?;
        conn.execute("DELETE FROM files", [])?;

        // Save files and their symbols
        let mut file_id: i64 = 0;
        for file in &graph.files {
            file_id += 1;
            let rel_path = file.rel_path.as_str();
            let path = file.path.to_string_lossy();
            let lang = file.language.as_str();
            conn.execute(
                "INSERT INTO files (id, path, rel_path, language, pagerank) VALUES (?1, ?2, ?3, ?4, 0.0)",
                rusqlite::params![file_id, path, rel_path, lang],
            )?;

            // Save symbols
            for sym in &file.symbols {
                conn.execute(
                    "INSERT INTO symbols (file_id, name, kind, start_line, end_line, signature) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        file_id,
                        sym.name,
                        format!("{}", sym.kind),
                        sym.start_line as i32,
                        sym.end_line as i32,
                        sym.signature,
                    ],
                )?;
            }
        }

        // Save reference edges
        for (from, to, weight) in graph.iter_edges() {
            conn.execute(
                "INSERT INTO ref_edges (from_file, to_file, weight) VALUES (?1, ?2, ?3)",
                rusqlite::params![from, to, weight],
            )?;
        }

        Ok(())
    }

    pub fn size_str(&self) -> String {
        let len = std::fs::metadata(&self.db_path)
            .map(|m| m.len())
            .unwrap_or(0);
        if len < 1024 {
            format!("{len}B")
        } else if len < 1024 * 1024 {
            format!("{:.1}KB", len as f64 / 1024.0)
        } else {
            format!("{:.1}MB", len as f64 / (1024.0 * 1024.0))
        }
    }

    /// Get the path to the cache database file (for watcher invalidation)
    #[allow(dead_code)]
    pub fn db_path(&self) -> PathBuf {
        self.db_path.clone()
    }

    /// Remove a file and its symbols from the cache
    pub fn remove_file(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Get file ID
        let file_id: Option<i64> = conn.query_row(
            "SELECT id FROM files WHERE path = ?1",
            rusqlite::params![path],
            |row| row.get(0),
        ).ok();

        if let Some(fid) = file_id {
            conn.execute("DELETE FROM symbols WHERE file_id = ?1", rusqlite::params![fid])?;
            conn.execute("DELETE FROM ref_edges WHERE from_file = ?1 OR to_file = ?1", rusqlite::params![fid])?;
            conn.execute("DELETE FROM files WHERE id = ?1", rusqlite::params![fid])?;
        }
        Ok(())
    }

    /// Add or update a single file's symbols in the cache
    pub fn upsert_file(&self, file_sym: &super::FileSymbols) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let path = file_sym.file_path.to_string_lossy();
        let rel_path = file_sym.rel_path.as_str();
        let lang = file_sym.language.as_str();

        // Remove old entry if exists
        let file_id: Option<i64> = conn.query_row(
            "SELECT id FROM files WHERE path = ?1",
            rusqlite::params![path.as_ref()],
            |row| row.get(0),
        ).ok();

        if let Some(fid) = file_id {
            conn.execute("DELETE FROM symbols WHERE file_id = ?1", rusqlite::params![fid])?;
            conn.execute("UPDATE files SET rel_path = ?1, language = ?2 WHERE id = ?3",
                rusqlite::params![rel_path, lang, fid])?;
            // Insert new symbols
            for sym in &file_sym.symbols {
                conn.execute(
                    "INSERT INTO symbols (file_id, name, kind, start_line, end_line, signature) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![fid, sym.name, format!("{}", sym.kind), sym.start_line as i32, sym.end_line as i32, sym.signature],
                )?;
            }
        } else {
            // Insert new file + symbols
            let max_id: i64 = conn.query_row("SELECT COALESCE(MAX(id), 0) FROM files", [], |row| row.get(0)).unwrap_or(0);
            let new_id = max_id + 1;
            conn.execute(
                "INSERT INTO files (id, path, rel_path, language, pagerank) VALUES (?1, ?2, ?3, ?4, 0.0)",
                rusqlite::params![new_id, path.as_ref(), rel_path, lang],
            )?;
            for sym in &file_sym.symbols {
                conn.execute(
                    "INSERT INTO symbols (file_id, name, kind, start_line, end_line, signature) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![new_id, sym.name, format!("{}", sym.kind), sym.start_line as i32, sym.end_line as i32, sym.signature],
                )?;
            }
        }
        Ok(())
    }

    /// Record a file snapshot for change detection
    pub fn snapshot_file(&self, path: &str, mtime: i64, content_hash: &str, size: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO file_snapshots (path, mtime, content_hash, size_bytes) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![path, mtime, content_hash, size],
        )?;
        Ok(())
    }

    /// Detect changed, new, and deleted files by comparing snapshots
    pub fn detect_changes(&self, current_files: &[(String, i64, String, i64)]) -> Result<ChangeSet> {
        let conn = self.conn.lock().unwrap();

        // Load existing snapshots
        let mut stmt = conn.prepare("SELECT path, mtime, content_hash, size_bytes FROM file_snapshots")?;
        let existing: std::collections::HashMap<String, (i64, String, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .map(|(p, m, h, s)| (p, (m, h, s)))
            .collect();

        let current_map: std::collections::HashMap<String, (i64, String, i64)> = current_files
            .iter()
            .map(|(p, m, h, s)| (p.clone(), (*m, h.clone(), *s)))
            .collect();

        let mut new_files = Vec::new();
        let mut changed_files = Vec::new();
        let mut deleted_files = Vec::new();

        // Find new and changed
        for (path, (mtime, hash, size)) in &current_map {
            match existing.get(path) {
                None => {
                    // New file
                    new_files.push((path.clone(), *mtime, hash.clone(), *size));
                }
                Some((old_mtime, old_hash, _old_size)) => {
                    if old_mtime != mtime || old_hash != hash {
                        // Changed file
                        changed_files.push((path.clone(), *mtime, hash.clone(), *size));
                    }
                }
            }
        }

        // Find deleted
        for path in existing.keys() {
            if !current_map.contains_key(path) {
                deleted_files.push(path.clone());
            }
        }

        Ok(ChangeSet {
            new_files,
            changed_files,
            deleted_files,
            total_existing: existing.len(),
            total_current: current_map.len(),
        })
    }

    /// Invalidate cache by deleting the database — forces full rebuild
    pub fn invalidate(&self) -> Result<()> {
        let _ = std::fs::remove_file(&self.db_path);
        Ok(())
    }
}

#[allow(dead_code)]
pub fn symbol_kind_from_str(s: &str) -> super::SymbolKind {
    match s {
        "function" => super::SymbolKind::Function,
        "class" => super::SymbolKind::Class,
        "struct" => super::SymbolKind::Struct,
        "trait" => super::SymbolKind::Trait,
        "enum" => super::SymbolKind::Enum,
        "interface" => super::SymbolKind::Interface,
        "method" => super::SymbolKind::Method,
        "variable" => super::SymbolKind::Variable,
        "import" => super::SymbolKind::Import,
        "macro" => super::SymbolKind::Macro,
        _ => super::SymbolKind::Other(s.to_string()),
    }
}

/// Result of change detection
#[derive(Debug, Clone)]
pub struct ChangeSet {
    /// New files not in previous snapshot
    pub new_files: Vec<(String, i64, String, i64)>,
    /// Files whose mtime or hash changed
    pub changed_files: Vec<(String, i64, String, i64)>,
    /// Files that existed in snapshot but are gone now
    pub deleted_files: Vec<String>,
    /// Total files in previous snapshot
    pub total_existing: usize,
    /// Total files in current scan
    pub total_current: usize,
}

impl ChangeSet {
    /// Total number of files that need re-indexing
    pub fn total_affected(&self) -> usize {
        self.new_files.len() + self.changed_files.len() + self.deleted_files.len()
    }

    /// Whether incremental update is worthwhile (>0 changes but <50% of files)
    pub fn should_incremental(&self) -> bool {
        let affected = self.total_affected();
        affected > 0 && (self.total_current == 0 || affected < self.total_current / 2)
    }
}
