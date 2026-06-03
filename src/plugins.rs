//! Plugin System — discover and load plugins from .hyper/plugins/
//!
//! Two plugin types:
//! - **Binary**: A standalone executable launched as an MCP server subprocess
//! - **Script**: A shell/Python script that receives JSON on stdin, returns JSON on stdout
//!
//! Plugin manifest: plugin.toml in the plugin directory.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Plugin type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    /// Standalone binary launched as MCP server subprocess
    Binary,
    /// Script run via shell (bash/python/node)
    Script,
}

/// Plugin manifest (plugin.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub plugin_type: PluginType,
    pub command: String,          // e.g., "python3 myscript.py" or "./my-agent-server"
    pub args: Vec<String>,        // Extra arguments
    pub tools: Vec<PluginTool>,   // Tools this plugin provides
    pub author: Option<String>,
    pub homepage: Option<String>,
}

/// A tool provided by a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A discovered plugin
#[derive(Debug, Clone)]
pub struct Plugin {
    pub dir: PathBuf,
    pub manifest: PluginManifest,
}

/// Plugin registry — manages discovered plugins
#[derive(Debug)]
pub struct PluginRegistry {
    plugins: Vec<Plugin>,
    plugin_dir: PathBuf,
}

impl PluginRegistry {
    /// Create a new registry and scan for plugins
    pub fn new(project_root: &Path) -> Self {
        let plugin_dir = project_root.join(".hyper").join("plugins");
        let plugins = Self::discover(&plugin_dir);
        Self { plugins, plugin_dir }
    }

    /// Scan the plugin directory for manifests
    fn discover(plugin_dir: &Path) -> Vec<Plugin> {
        let mut plugins = Vec::new();

        if !plugin_dir.exists() {
            return plugins;
        }

        let entries = match std::fs::read_dir(plugin_dir) {
            Ok(e) => e,
            Err(_) => return plugins,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("plugin.toml");
            if !manifest_path.exists() {
                continue;
            }

            let content = match std::fs::read_to_string(&manifest_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let manifest: PluginManifest = match toml::from_str(&content) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("   ⚠️  Plugin '{}' parse error: {e}", path.display());
                    continue;
                }
            };

            plugins.push(Plugin {
                dir: path,
                manifest,
            });
        }

        plugins
    }

    /// Get all discovered plugins
    pub fn plugins(&self) -> &[Plugin] {
        &self.plugins
    }

    /// Get a plugin by name
    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.manifest.name == name)
    }

    /// Get plugin count
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Check if any plugins are loaded
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Convert plugins to JSON tool definitions (for MCP integration)
    pub fn to_tool_definitions(&self) -> Vec<serde_json::Value> {
        let mut tools = Vec::new();

        for plugin in &self.plugins {
            for tool in &plugin.manifest.tools {
                tools.push(serde_json::json!({
                    "name": format!("plugin_{}", tool.name),
                    "description": tool.description,
                    "inputSchema": tool.input_schema,
                }));
            }
        }

        tools
    }

    /// Render plugin list for display
    pub fn render(&self) -> String {
        if self.plugins.is_empty() {
            return "   No plugins installed.".to_string();
        }

        let mut output = String::from("   📦 Installed Plugins\n");
        for plugin in &self.plugins {
            let m = &plugin.manifest;
            let type_str = match m.plugin_type {
                PluginType::Binary => "🔧 binary",
                PluginType::Script => "📜 script",
            };
            output.push_str(&format!(
                "   • {} v{} — {} ({})\n",
                m.name, m.version, m.description, type_str
            ));
            if !m.tools.is_empty() {
                for tool in &m.tools {
                    output.push_str(&format!("     ├─ plugin_{}\n", tool.name));
                }
            }
        }
        output
    }
}

/// Create a sample plugin for reference
pub fn create_sample_plugin(project_root: &Path) -> anyhow::Result<()> {
    let plugin_dir = project_root.join(".hyper").join("plugins").join("example");
    std::fs::create_dir_all(&plugin_dir)?;

    let manifest = PluginManifest {
        name: "example".into(),
        version: "0.1.0".into(),
        description: "Example HyperAgent plugin".into(),
        plugin_type: PluginType::Script,
        command: "python3".into(),
        args: vec!["plugin.py".into()],
        tools: vec![
            PluginTool {
                name: "hello".into(),
                description: "Say hello to someone".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Name to greet"
                        }
                    },
                    "required": ["name"]
                }),
            },
        ],
        author: Some("HyperAgent".into()),
        homepage: Some("https://github.com/nick-zhang-1991/HyperAgent".into()),
    };

    let toml_str = toml::to_string_pretty(&manifest)?;
    std::fs::write(plugin_dir.join("plugin.toml"), toml_str)?;

    // Create a sample plugin script
    let script = r#"#!/usr/bin/env python3
import json, sys

def handle_tool(name, args):
    if name == "plugin_hello":
        return {"result": f"Hello, {args.get('name', 'world')}!"}
    return {"error": f"Unknown tool: {name}"}

for line in sys.stdin:
    try:
        req = json.loads(line)
        result = handle_tool(req.get("method", ""), req.get("params", {}))
        print(json.dumps({"jsonrpc": "2.0", "id": req.get("id"), "result": result}))
    except Exception as e:
        print(json.dumps({"jsonrpc": "2.0", "error": {"message": str(e)}}))
"#;
    std::fs::write(plugin_dir.join("plugin.py"), script)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_empty_registry() {
        let dir = std::env::temp_dir().join("hyper-plugin-test-empty");
        let _ = std::fs::remove_dir_all(&dir);
        let registry = PluginRegistry::new(&dir);
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_discover_plugin() {
        let dir = std::env::temp_dir().join("hyper-plugin-test-discover");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".hyper").join("plugins").join("test-plugin")).unwrap();

        let manifest = r#"
name = "test-plugin"
version = "0.1.0"
description = "A test plugin"
plugin_type = "script"
command = "python3"
args = ["plugin.py"]

[[tools]]
name = "greet"
description = "Greet someone"
input_schema = { type = "object", properties = { name = { type = "string" } }, required = ["name"] }
"#;
        std::fs::write(
            dir.join(".hyper").join("plugins").join("test-plugin").join("plugin.toml"),
            manifest,
        ).unwrap();

        let registry = PluginRegistry::new(&dir);
        assert_eq!(registry.len(), 1);

        let plugin = registry.get("test-plugin").unwrap();
        assert_eq!(plugin.manifest.name, "test-plugin");
        assert_eq!(plugin.manifest.version, "0.1.0");
        assert_eq!(plugin.manifest.tools.len(), 1);
        assert_eq!(plugin.manifest.tools[0].name, "greet");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_render_empty() {
        let dir = std::env::temp_dir().join("hyper-plugin-test-render");
        let _ = std::fs::remove_dir_all(&dir);
        let registry = PluginRegistry::new(&dir);
        let output = registry.render();
        assert!(output.contains("No plugins installed"));
    }
}
