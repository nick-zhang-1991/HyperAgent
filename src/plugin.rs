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
use std::collections::HashMap;
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
