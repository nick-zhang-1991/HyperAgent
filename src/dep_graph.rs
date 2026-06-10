//! Dependency graph analysis — scan Cargo.toml/package.json for dependency trees

use anyhow::Result;
use std::path::Path;

/// A single dependency entry
#[derive(Debug, Clone)]
pub struct DepEntry {
    pub name: String,
    pub version: String,
    pub kind: DepKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DepKind {
    Production,
    Dev,
    Build,
}

/// Dependency analysis results
#[derive(Debug, Default)]
pub struct DepGraph {
    pub deps: Vec<DepEntry>,
    pub outdated: Vec<String>,
    pub total_count: usize,
}

/// Analyze dependencies in a project
pub fn analyze(root: &Path) -> Result<DepGraph> {
    let mut graph = DepGraph::default();

    // Cargo.toml analysis
    let cargo_path = root.join("Cargo.toml");
    if cargo_path.exists() {
        analyze_cargo(&cargo_path, &mut graph)?;
    }

    // package.json analysis
    let pkg_path = root.join("package.json");
    if pkg_path.exists() {
        analyze_package_json(&pkg_path, &mut graph)?;
    }

    graph.total_count = graph.deps.len();
    Ok(graph)
}

fn analyze_cargo(path: &Path, graph: &mut DepGraph) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let mut in_deps = false;
    let mut in_dev = false;
    let mut in_build = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("[dependencies]") {
            in_deps = true; in_dev = false; in_build = false;
            continue;
        }
        if trimmed.starts_with("[dev-dependencies]") {
            in_deps = false; in_dev = true; in_build = false;
            continue;
        }
        if trimmed.starts_with("[build-dependencies]") {
            in_deps = false; in_dev = false; in_build = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deps = false; in_dev = false; in_build = false;
            continue;
        }

        if (in_deps || in_dev || in_build) && trimmed.contains('=') && !trimmed.starts_with('#') {
            if let Some((name, version)) = trimmed.split_once('=') {
                let name = name.trim().to_string();
                let version = version.trim().trim_matches('"').trim_matches('\'').to_string();
                if !name.is_empty() && !version.is_empty() && !version.starts_with('{') && !version.starts_with('\"') {
                    let kind = if in_dev { DepKind::Dev }
                        else if in_build { DepKind::Build }
                        else { DepKind::Production };
                    graph.deps.push(DepEntry { name, version, kind });
                }
            }
        }
    }

    Ok(())
}

fn analyze_package_json(path: &Path, graph: &mut DepGraph) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;

    if let Some(deps) = parsed["dependencies"].as_object() {
        for (name, version) in deps {
            graph.deps.push(DepEntry {
                name: name.clone(),
                version: version.as_str().unwrap_or("unknown").to_string(),
                kind: DepKind::Production,
            });
        }
    }

    if let Some(deps) = parsed["devDependencies"].as_object() {
        for (name, version) in deps {
            graph.deps.push(DepEntry {
                name: name.clone(),
                version: version.as_str().unwrap_or("unknown").to_string(),
                kind: DepKind::Dev,
            });
        }
    }

    Ok(())
}

/// Display dependency graph
pub fn display_graph(graph: &DepGraph) {
    if graph.deps.is_empty() {
        println!("   No dependencies found.");
        return;
    }

    println!("\n📦 Dependency Graph:");
    println!("{}", "-".repeat(60));
    println!("  Total: {} dependencies", graph.total_count);

    let prod_count = graph.deps.iter().filter(|d| d.kind == DepKind::Production).count();
    let dev_count = graph.deps.iter().filter(|d| d.kind == DepKind::Dev).count();
    println!("  Production: {prod_count} | Dev: {dev_count}");

    println!("\n  Dependencies:");
    for dep in &graph.deps {
        let kind_str = match dep.kind {
            DepKind::Production => "",
            DepKind::Dev => " (dev)",
            DepKind::Build => " (build)",
        };
        println!("    {}@{} {}", dep.name, dep.version, kind_str);
    }

    if !graph.outdated.is_empty() {
        println!("\n  ⚠️  Potentially outdated:");
        for o in &graph.outdated {
            println!("    {o}");
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("hyperagent_depgraph_{}_{}", std::process::id(), suffix));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn test_analyze_cargo_dependencies() {
        let dir = temp_dir("cargo");
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
serde = "1.0"
anyhow = "1.0"

[dev-dependencies]
tempfile = "3.0"
"#,
        ).unwrap();
        let graph = analyze(&dir).unwrap();
        assert_eq!(graph.deps.len(), 3);
        assert_eq!(graph.total_count, 3);

        let prod: Vec<_> = graph.deps.iter().filter(|d| d.kind == DepKind::Production).collect();
        let dev: Vec<_> = graph.deps.iter().filter(|d| d.kind == DepKind::Dev).collect();
        assert_eq!(prod.len(), 2);
        assert_eq!(dev.len(), 1);
        assert_eq!(dev[0].name, "tempfile");
        assert_eq!(dev[0].version, "3.0");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_cargo_build_dependencies() {
        let dir = temp_dir("build");
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "t"

[build-dependencies]
cc = "1.0"

[dependencies]
serde = "1.0"
"#,
        ).unwrap();
        let graph = analyze(&dir).unwrap();
        let build: Vec<_> = graph.deps.iter().filter(|d| d.kind == DepKind::Build).collect();
        assert_eq!(build.len(), 1);
        assert_eq!(build[0].name, "cc");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_cargo_table_inline_edition_ignored() {
        // Inline tables (starting with `{`) should be ignored
        let dir = temp_dir("inline");
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "t"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
"#,
        ).unwrap();
        let graph = analyze(&dir).unwrap();
        // Inline tables should be skipped (start with `{`)
        assert_eq!(graph.deps.len(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_cargo_no_deps_section() {
        let dir = temp_dir("no_deps");
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "t"
version = "0.1.0"
"#,
        ).unwrap();
        let graph = analyze(&dir).unwrap();
        assert!(graph.deps.is_empty());
        assert_eq!(graph.total_count, 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_cargo_comments_ignored() {
        let dir = temp_dir("comments");
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"[dependencies]
# this is a comment with = sign
serde = "1.0"
"#,
        ).unwrap();
        let graph = analyze(&dir).unwrap();
        // Comments should be ignored
        assert_eq!(graph.deps.len(), 1);
        assert_eq!(graph.deps[0].name, "serde");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_package_json() {
        let dir = temp_dir("pkg");
        std::fs::write(
            dir.join("package.json"),
            r#"{
  "name": "test",
  "version": "1.0.0",
  "dependencies": {
    "react": "^18.0.0",
    "lodash": "^4.17.0"
  },
  "devDependencies": {
    "typescript": "^5.0.0"
  }
}"#,
        ).unwrap();
        let graph = analyze(&dir).unwrap();
        assert_eq!(graph.deps.len(), 3);

        let prod: Vec<_> = graph.deps.iter().filter(|d| d.kind == DepKind::Production).collect();
        let dev: Vec<_> = graph.deps.iter().filter(|d| d.kind == DepKind::Dev).collect();
        assert_eq!(prod.len(), 2);
        assert_eq!(dev.len(), 1);
        assert_eq!(dev[0].name, "typescript");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_package_json_non_string_version() {
        let dir = temp_dir("pkg_obj");
        std::fs::write(
            dir.join("package.json"),
            r#"{
  "name": "t",
  "dependencies": {
    "good": "1.0.0",
    "weird": { "version": "2.0.0" }
  }
}"#,
        ).unwrap();
        let graph = analyze(&dir).unwrap();
        // The non-string version should fall back to "unknown"
        assert_eq!(graph.deps.len(), 2);
        let weird = graph.deps.iter().find(|d| d.name == "weird").unwrap();
        assert_eq!(weird.version, "unknown");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_package_json_empty() {
        let dir = temp_dir("pkg_empty");
        std::fs::write(dir.join("package.json"), r#"{"name":"t","version":"1.0"}"#).unwrap();
        let graph = analyze(&dir).unwrap();
        assert!(graph.deps.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_both_cargo_and_package_json() {
        let dir = temp_dir("both");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[dependencies]\nserde = \"1.0\"",
        ).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"dependencies": {"react": "18.0.0"}}"#,
        ).unwrap();
        let graph = analyze(&dir).unwrap();
        assert_eq!(graph.deps.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_analyze_empty_dir() {
        let dir = temp_dir("empty");
        let graph = analyze(&dir).unwrap();
        assert!(graph.deps.is_empty());
        assert_eq!(graph.total_count, 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_dep_kind_equality() {
        assert_eq!(DepKind::Production, DepKind::Production);
        assert_eq!(DepKind::Dev, DepKind::Dev);
        assert_eq!(DepKind::Build, DepKind::Build);
        assert_ne!(DepKind::Production, DepKind::Dev);
    }

    #[test]
    fn test_dep_entry_clone() {
        let e = DepEntry { name: "x".into(), version: "1.0".into(), kind: DepKind::Production };
        let cloned = e.clone();
        assert_eq!(cloned.name, e.name);
        assert_eq!(cloned.version, e.version);
        assert_eq!(cloned.kind, e.kind);
    }

    #[test]
    fn test_dep_graph_default() {
        let g = DepGraph::default();
        assert!(g.deps.is_empty());
        assert!(g.outdated.is_empty());
        assert_eq!(g.total_count, 0);
    }

    #[test]
    fn test_analyze_cargo_with_other_sections() {
        // Test that [features] and other sections don't get treated as deps
        let dir = temp_dir("features");
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "t"

[dependencies]
serde = "1.0"

[features]
default = ["something"]

[[bin]]
name = "t"
"#,
        ).unwrap();
        let graph = analyze(&dir).unwrap();
        // Should only have serde from [dependencies]
        assert_eq!(graph.deps.len(), 1);
        assert_eq!(graph.deps[0].name, "serde");
        std::fs::remove_dir_all(&dir).ok();
    }
}
