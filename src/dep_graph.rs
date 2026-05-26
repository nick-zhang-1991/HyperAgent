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
