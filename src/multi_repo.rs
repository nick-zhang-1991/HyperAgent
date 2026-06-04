//! Multi-Repository Agent — operate across multiple git repos
//!
//! Indexes multiple repositories and supports cross-repo search,
//! parallel operations, and aggregated context.
//!
//! Usage:
//!   hyper repo add ./backend ./frontend    # Register repos
//!   hyper repo list                         # List repos
//!   hyper repo search "api endpoint"        # Cross-repo search

use crate::index::HyperIndex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A registered repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub name: String,
    pub path: PathBuf,
    pub added_at: String,
    pub last_indexed: Option<String>,
}

/// Multi-repo configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiRepoConfig {
    pub repos: Vec<RepoEntry>,
}

/// Multi-repo manager
pub struct MultiRepoManager {
    config_path: PathBuf,
    config: MultiRepoConfig,
}

impl MultiRepoManager {
    /// Create a new manager for the given project
    pub fn new(project_root: &Path) -> Self {
        let config_path = project_root.join(".hyper").join("repos.toml");
        let config = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|c| toml::from_str(&c).ok())
                .unwrap_or(MultiRepoConfig { repos: vec![] })
        } else {
            MultiRepoConfig { repos: vec![] }
        };
        Self { config_path, config }
    }

    /// Save config
    fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(&self.config)?;
        std::fs::write(&self.config_path, content)?;
        Ok(())
    }

    /// Add a repository
    pub fn add(&mut self, path: &Path) -> anyhow::Result<String> {
        let canonical = path.canonicalize()?;
        let name = canonical
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        if self.config.repos.iter().any(|r| r.path == canonical) {
            anyhow::bail!("Repository '{}' already registered", canonical.display());
        }

        // Check it's a valid git repo or directory
        if !canonical.exists() {
            anyhow::bail!("Path does not exist: {}", canonical.display());
        }

        // Try to index it
        let index_path = canonical.join(".hyper").join("index");
        std::fs::create_dir_all(canonical.join(".hyper"))?;

        self.config.repos.push(RepoEntry {
            name,
            path: canonical.clone(),
            added_at: chrono::Utc::now().to_rfc3339(),
            last_indexed: None,
        });

        self.save()?;
        Ok(format!("   ✅ Added repository: {}", canonical.display()))
    }

    /// Remove a repository
    pub fn remove(&mut self, name_or_path: &str) -> anyhow::Result<String> {
        let pos = self.config.repos.iter().position(|r| {
            r.name == name_or_path
                || r.path.to_string_lossy().as_ref() == name_or_path
        });
        match pos {
            Some(i) => {
                let removed = self.config.repos.remove(i);
                self.save()?;
                Ok(format!("   ✅ Removed: {} ({})", removed.name, removed.path.display()))
            }
            None => anyhow::bail!("Repository '{}' not found. Use /repo list to see registered repos.", name_or_path),
        }
    }

    /// List registered repositories
    pub fn list(&self) -> String {
        if self.config.repos.is_empty() {
            return "   No repositories registered. Use `/repo add <path>` to add one.".to_string();
        }

        let mut output = String::from("   📚 Registered Repositories\n");
        for (i, repo) in self.config.repos.iter().enumerate() {
            let indexed = repo.last_indexed.as_deref().unwrap_or("never");
            output.push_str(&format!(
                "   {}. {} — {} (indexed: {indexed})\n",
                i + 1,
                repo.name,
                repo.path.display(),
            ));
        }
        output
    }

    /// Search across all registered repositories
    pub fn search(&self, query: &str) -> String {
        if self.config.repos.is_empty() {
            return "   No repositories registered.".to_string();
        }

        let mut output = String::new();
        for repo in &self.config.repos {
            output.push_str(&format!("   🔍 {} ({}):\n", repo.name, repo.path.display()));

            if !repo.path.join(".hyper").join("index").exists() {
                output.push_str("      ⚠️  Not indexed. Run `hyper init` in this repo first.\n");
                continue;
            }

            match HyperIndex::new_or_load(&repo.path) {
                Ok(mut idx) => {
                    if !idx.has_cache() {
                        if let Err(e) = idx.build() {
                            output.push_str(&format!("      ⚠️  Index error: {e}\n"));
                            continue;
                        }
                    }
                    let files = idx.get_relevant_files(query, 5, 2000);
                    if files.is_empty() {
                        output.push_str("      No matches found.\n");
                    } else {
                        for f in &files {
                            output.push_str(&format!(
                                "      📄 {} (score: {:.2})\n",
                                f.path.display(),
                                f.score
                            ));
                        }
                    }
                }
                Err(e) => {
                    output.push_str(&format!("      ⚠️  Index load error: {e}\n"));
                }
            }
        }
        output
    }

    /// Get the list of repo paths
    pub fn repo_paths(&self) -> Vec<PathBuf> {
        self.config.repos.iter().map(|r| r.path.clone()).collect()
    }

    /// Check if any repos are registered
    pub fn is_empty(&self) -> bool {
        self.config.repos.is_empty()
    }

    /// Get count of registered repos
    pub fn len(&self) -> usize {
        self.config.repos.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        let dir = std::env::temp_dir().join("hyper-multirepo-test-empty");
        let _ = std::fs::create_dir_all(&dir);
        let mgr = MultiRepoManager::new(&dir);
        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);
        let list = mgr.list();
        assert!(list.contains("No repositories"));
    }

    #[test]
    fn test_add_and_list() {
        let dir = std::env::temp_dir().join("hyper-multirepo-test-add");
        let _ = std::fs::create_dir_all(&dir);
        let mut mgr = MultiRepoManager::new(&dir);

        // Create a test directory to add as a "repo"
        let test_repo = dir.join("test-repo");
        std::fs::create_dir_all(&test_repo).unwrap();

        mgr.add(&test_repo).unwrap();
        assert_eq!(mgr.len(), 1);

        let list = mgr.list();
        assert!(list.contains("test-repo"));

        // Duplicate add should fail
        assert!(mgr.add(&test_repo).is_err());

        mgr.remove("test-repo").unwrap();
        assert!(mgr.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
