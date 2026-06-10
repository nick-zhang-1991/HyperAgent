//! Runtime plugin system — load scripts from `.hyper/tools/` as callable tools.
//!
//! Scripts can be shell scripts (.sh), Python (.py), or any executable.
//! Each script receives tool arguments as JSON on stdin and returns a result as JSON on stdout.
//!
//! Script naming convention:
//!   `.hyper/tools/<tool_name>.sh`   — Shell script tool
//!   `.hyper/tools/<tool_name>.py`   — Python script tool
//!
//! Script interface:
//!   - Receives: JSON object on stdin (tool arguments)
//!   - Returns: JSON on stdout (tool result)
//!   - Exit code 0 = success, non-zero = error
//!
//! Tool description is read from the file header:
//!   # Description: <short description of the tool>
//!
//! Example `.hyper/tools/deploy.py`:
//!   ```python
//!   # Description: Deploy the application to a target environment
//!   # Parameter: env (string, required) - Target environment (staging/production)
//!   # Parameter: version (string, optional) - Version to deploy
//!   import json, sys
//!   args = json.load(sys.stdin)
//!   print(json.dumps({"status": "ok", "output": f"Deployed {args.get('version', 'latest')} to {args['env']}"}))
//!   ```

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// A plugin tool loaded from a script file
#[derive(Debug, Clone)]
pub struct PluginTool {
    pub name: String,
    pub description: String,
    pub script_path: PathBuf,
    pub parameters: Value, // JSON Schema object
}

/// Plugin manager — loads and manages script-based tools
pub struct PluginManager {
    tools_dir: PathBuf,
    tools: Vec<PluginTool>,
}

impl PluginManager {
    /// Create a new plugin manager for the given project root.
    /// Scans `.hyper/tools/` for available plugins.
    pub fn new(project_root: &Path) -> Self {
        let tools_dir = project_root.join(".hyper").join("tools");
        let tools = if tools_dir.exists() {
            Self::scan_directory(&tools_dir)
        } else {
            Vec::new()
        };
        Self { tools_dir, tools }
    }

    /// Scan the tools directory for executable scripts
    fn scan_directory(dir: &Path) -> Vec<PluginTool> {
        let mut tools = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return tools,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Only accept certain extensions
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !["sh", "py", "js", "ts", "rb", "pl", "php"].contains(&ext) {
                continue;
            }

            let name = path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            // Check if file is executable (Unix) or always allow .py/.sh
            #[cfg(unix)]
            let is_exec = std::fs::metadata(&path).map(|m| {
                std::os::unix::fs::MetadataExt::mode(&m) & 0o111 != 0
            }).unwrap_or(false);
            #[cfg(not(unix))]
            let is_exec = true; // Windows allows any script

            if !is_exec && ext == "sh" {
                // Shell scripts must be executable
                continue;
            }

            // Read header for description and parameter definitions
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let (description, parameters) = Self::parse_header(&content);

            let tool = PluginTool {
                name,
                description: description.unwrap_or_else(|| format!("Custom tool ({})", path.display())),
                script_path: path,
                parameters: parameters.unwrap_or_else(|| serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                })),
            };
            tools.push(tool);
        }

        tools
    }

    /// Parse script header for tool metadata.
    /// Looks for lines like:
    ///   # Description: <text>
    ///   # Parameter: <name> (<type>, <required|optional>) - <description>
    fn parse_header(content: &str) -> (Option<String>, Option<Value>) {
        let mut description = None;
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if let Some(desc) = line.strip_prefix("# Description:") {
                description = Some(desc.trim().to_string());
            } else if let Some(param) = line.strip_prefix("# Parameter:") {
                let param = param.trim();
                // Format: name (type, required|optional) - description
                if let Some((name_part, rest)) = param.split_once('(') {
                    let name = name_part.trim();
                    if let Some((type_req, _desc)) = rest.split_once(')') {
                        let parts: Vec<&str> = type_req.split(',').collect();
                        let param_type = parts.first().map(|s| s.trim()).unwrap_or("string");
                        let is_required = parts.get(1).map(|s| s.trim() == "required").unwrap_or(false);

                        let js_type = match param_type {
                            "number" | "integer" => "number",
                            "boolean" => "boolean",
                            "array" => "array",
                            "object" => "object",
                            _ => "string",
                        };

                        properties.insert(name.to_string(), serde_json::json!({
                            "type": js_type,
                            "description": _desc.trim_start_matches('-').trim()
                        }));

                        if is_required {
                            required.push(name.to_string());
                        }
                    }
                }
            }
        }

        let params = if properties.is_empty() {
            None
        } else {
            Some(serde_json::json!({
                "type": "object",
                "properties": properties,
                "required": required
            }))
        };

        (description, params)
    }

    /// Get all loaded plugin tools as ToolDefinitions
    pub fn to_tool_definitions(&self) -> Vec<crate::llm::provider::ToolDefinition> {
        self.tools.iter().map(|tool| {
            crate::llm::provider::ToolDefinition {
                tool_type: "function".into(),
                function: crate::llm::provider::ToolFunction {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.parameters.clone(),
                },
            }
        }).collect()
    }

    /// Execute a plugin tool by name
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value> {
        let tool = self.tools.iter()
            .find(|t| t.name == name)
            .ok_or_else(|| anyhow::anyhow!("Plugin tool '{name}' not found"))?;

        // Pass args as JSON on stdin, capture JSON from stdout
        let input = serde_json::to_string(&args)?;

        let mut output = tokio::process::Command::new(&tool.script_path)
            .arg("--stdin") // Pass --stdin flag so scripts know to read from stdin
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn plugin script")?;

        // Write args to stdin and close
        if let Some(ref mut stdin) = output.stdin {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(input.as_bytes()).await?;
            stdin.shutdown().await?;
        }

        let result = output.wait_with_output().await?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            bail!("Plugin '{}' failed: {}", name, stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&result.stdout);
        let value: Value = serde_json::from_str(stdout.trim())
            .context("Plugin must return valid JSON on stdout")?;

        Ok(value)
    }

    /// Get the number of loaded plugins
    pub fn count(&self) -> usize {
        self.tools.len()
    }

    /// Hot-reload: re-scan the tools directory
    pub fn reload(&mut self) {
        self.tools = Self::scan_directory(&self.tools_dir);
    }

    /// List loaded plugin names
    pub fn list_tools(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(suffix: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("hyperagent_plugin_{}_{}", std::process::id(), suffix));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // ── PluginTool ──────────────────────────────────────────

    #[test]
    fn test_plugin_tool_construction() {
        let tool = PluginTool {
            name: "deploy".into(),
            description: "Deploy the app".into(),
            script_path: std::path::PathBuf::from("/tmp/deploy.sh"),
            parameters: serde_json::json!({"type":"object"}),
        };
        assert_eq!(tool.name, "deploy");
        assert_eq!(tool.description, "Deploy the app");
        assert_eq!(tool.script_path.to_str(), Some("/tmp/deploy.sh"));
    }

    #[test]
    fn test_plugin_tool_clone() {
        let tool = PluginTool {
            name: "x".into(),
            description: "y".into(),
            script_path: std::path::PathBuf::from("/a"),
            parameters: serde_json::json!({}),
        };
        let cloned = tool.clone();
        assert_eq!(cloned.name, tool.name);
        assert_eq!(cloned.script_path, tool.script_path);
    }

    // ── PluginManager basic ─────────────────────────────────

    #[test]
    fn test_plugin_manager_new_no_dir() {
        let project = temp_project("no_dir");
        let pm = PluginManager::new(&project);
        assert_eq!(pm.count(), 0);
        assert!(pm.list_tools().is_empty());
    }

    #[test]
    fn test_plugin_manager_new_empty_dir() {
        let project = temp_project("empty");
        std::fs::create_dir_all(project.join(".hyper/tools")).unwrap();
        let pm = PluginManager::new(&project);
        assert_eq!(pm.count(), 0);
    }

    #[test]
    fn test_plugin_manager_scan_finds_scripts() {
        let project = temp_project("scan");
        let tools_dir = project.join(".hyper/tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        // Create a Python script with metadata
        std::fs::write(
            tools_dir.join("hello.py"),
            "# Description: Says hello
# Parameter: name (string, required) - Person to greet
print('hello')
",
        ).unwrap();
        let pm = PluginManager::new(&project);
        assert_eq!(pm.count(), 1);
        let tools = pm.list_tools();
        assert!(tools.contains(&"hello".to_string()));
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn test_plugin_manager_ignores_non_script_files() {
        let project = temp_project("ignore");
        let tools_dir = project.join(".hyper/tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        std::fs::write(tools_dir.join("README.md"), "# Notes").unwrap();
        std::fs::write(tools_dir.join("data.json"), "{}").unwrap();
        let pm = PluginManager::new(&project);
        assert_eq!(pm.count(), 0);
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn test_plugin_manager_supports_multiple_extensions() {
        let project = temp_project("multi_ext");
        let tools_dir = project.join(".hyper/tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        for ext in &["py", "sh", "js", "ts", "rb", "pl", "php"] {
            std::fs::write(tools_dir.join(format!("tool_{}.{}", "alpha", ext)), "#!/bin/sh
").unwrap();
        }
        // Make shell scripts executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for ext in &["sh"] {
                let p = tools_dir.join(format!("tool_alpha.{}", ext));
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let pm = PluginManager::new(&project);
        // Note: Python/JS/TS/RB/PL/PHP don't need exec bit per code, only .sh does
        assert!(pm.count() >= 1);
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn test_plugin_manager_non_executable_sh_skipped() {
        let project = temp_project("non_exec_sh");
        let tools_dir = project.join(".hyper/tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        let sh_path = tools_dir.join("notexec.sh");
        std::fs::write(&sh_path, "#!/bin/sh
echo hi
").unwrap();
        // On Unix, the file is not executable by default
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&sh_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let pm = PluginManager::new(&project);
        #[cfg(unix)]
        assert_eq!(pm.count(), 0, "non-executable .sh should be skipped on unix");
        let _ = std::fs::remove_dir_all(&project);
    }

    #[test]
    fn test_plugin_manager_executable_sh_included() {
        let project = temp_project("exec_sh");
        let tools_dir = project.join(".hyper/tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        let sh_path = tools_dir.join("exec.sh");
        std::fs::write(&sh_path, "#!/bin/sh
echo hi
").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&sh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let pm = PluginManager::new(&project);
        #[cfg(unix)]
        assert!(pm.count() >= 1, "executable .sh should be included on unix");
        let _ = std::fs::remove_dir_all(&project);
    }

    // ── parse_header ────────────────────────────────────────

    #[test]
    fn test_parse_header_description_only() {
        let content = "# Description: A simple tool
import json
";
        let (desc, params) = PluginManager::parse_header(&content);
        assert_eq!(desc, Some("A simple tool".to_string()));
        assert!(params.is_none());
    }

    #[test]
    fn test_parse_header_with_required_param() {
        let content = r#"# Description: Tool with param
# Parameter: env (string, required) - Target environment
print("hi")
"#;
        let (desc, params) = PluginManager::parse_header(&content);
        assert_eq!(desc.as_deref(), Some("Tool with param"));
        let p = params.expect("should have params");
        let props = &p["properties"];
        assert!(props["env"]["type"] == "string");
        let required = p["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "env"));
    }

    #[test]
    fn test_parse_header_with_optional_param() {
        let content = "# Parameter: version (string, optional) - Version
";
        let (desc, params) = PluginManager::parse_header(&content);
        assert!(desc.is_none());
        let p = params.expect("should have params");
        let required = p["required"].as_array().unwrap();
        assert!(!required.iter().any(|v| v == "version"));
    }

    #[test]
    fn test_parse_header_param_types() {
        let types = vec![
            ("number", "number"),
            ("integer", "number"),
            ("boolean", "boolean"),
            ("array", "array"),
            ("object", "object"),
            ("string", "string"),
            ("unknown_type", "string"), // falls back to string
        ];
        for (input_type, expected_json_type) in types {
            let content = format!("# Parameter: p ({}, required) - test
", input_type);
            let (_, params) = PluginManager::parse_header(&content);
            let p = params.expect("should have params");
            assert_eq!(p["properties"]["p"]["type"], expected_json_type,
                "type {} should map to {}", input_type, expected_json_type);
        }
    }

    #[test]
    fn test_parse_header_empty_content() {
        let (desc, params) = PluginManager::parse_header("");
        assert!(desc.is_none());
        assert!(params.is_none());
    }

    #[test]
    fn test_parse_header_no_metadata() {
        let content = "#!/bin/sh
echo hi
";
        let (desc, params) = PluginManager::parse_header(&content);
        assert!(desc.is_none());
        assert!(params.is_none());
    }

    #[test]
    fn test_parse_header_multiple_params() {
        let content = r#"# Description: Multi-param
# Parameter: a (string, required) - First
# Parameter: b (integer, optional) - Second
# Parameter: c (boolean, required) - Third
"#;
        let (_, params) = PluginManager::parse_header(&content);
        let p = params.expect("params");
        let props = &p["properties"];
        assert!(props.get("a").is_some());
        assert!(props.get("b").is_some());
        assert!(props.get("c").is_some());
        let required = p["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
    }

    #[test]
    fn test_parse_header_malformed_param_ignored() {
        // Missing closing paren
        let content = "# Parameter: bad (string, required - no close
";
        let (_, params) = PluginManager::parse_header(&content);
        assert!(params.is_none(), "malformed param should be ignored");
    }

    // ── count, list_tools, reload ───────────────────────────

    #[test]
    fn test_count_empty() {
        let project = temp_project("count_empty");
        let pm = PluginManager::new(&project);
        assert_eq!(pm.count(), 0);
    }

    #[test]
    fn test_list_tools_returns_names() {
        let project = temp_project("list");
        let tools_dir = project.join(".hyper/tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        std::fs::write(tools_dir.join("tool1.py"), "# Description: First
").unwrap();
        std::fs::write(tools_dir.join("tool2.py"), "# Description: Second
").unwrap();
        let pm = PluginManager::new(&project);
        let tools = pm.list_tools();
        assert_eq!(tools.len(), 2);
        assert!(tools.contains(&"tool1".to_string()));
        assert!(tools.contains(&"tool2".to_string()));
    }

    #[test]
    fn test_reload_picks_up_new_tools() {
        let project = temp_project("reload");
        let tools_dir = project.join(".hyper/tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        let mut pm = PluginManager::new(&project);
        assert_eq!(pm.count(), 0);
        // Add a new tool after init
        std::fs::write(tools_dir.join("added.py"), "# Description: Added
").unwrap();
        assert_eq!(pm.count(), 0, "before reload");
        pm.reload();
        assert_eq!(pm.count(), 1);
    }

    #[test]
    fn test_reload_removes_deleted_tools() {
        let project = temp_project("reload_del");
        let tools_dir = project.join(".hyper/tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        std::fs::write(tools_dir.join("temp.py"), "# Description: Temp
").unwrap();
        let mut pm = PluginManager::new(&project);
        assert_eq!(pm.count(), 1);
        std::fs::remove_file(tools_dir.join("temp.py")).unwrap();
        pm.reload();
        assert_eq!(pm.count(), 0);
    }

    // ── to_tool_definitions ─────────────────────────────────

    #[test]
    fn test_to_tool_definitions_empty() {
        let project = temp_project("tdef_empty");
        let pm = PluginManager::new(&project);
        let defs = pm.to_tool_definitions();
        assert!(defs.is_empty());
    }

    #[test]
    fn test_to_tool_definitions_includes_tools() {
        let project = temp_project("tdef");
        let tools_dir = project.join(".hyper/tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        std::fs::write(
            tools_dir.join("greet.py"),
            "# Description: Greet user
# Parameter: name (string, required) - Name
",
        ).unwrap();
        let pm = PluginManager::new(&project);
        let defs = pm.to_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].function.name, "greet");
        assert!(defs[0].function.description.contains("Greet"));
        assert_eq!(defs[0].tool_type, "function");
    }

    // ── call_tool errors ────────────────────────────────────

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let project = temp_project("call_nf");
        let pm = PluginManager::new(&project);
        let result = pm.call_tool("nonexistent", serde_json::json!({})).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("nonexistent"));
        assert!(msg.contains("not found"));
    }
}
