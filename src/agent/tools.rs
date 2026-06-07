//! Built-in tool definitions for HyperAgent.
//!
//! This module contains all built-in tool definitions that the agent can use.
//! Tool execution handlers remain in the orchestrator since they need access
//! to agent state (filesystem, provider, memory, etc.).

use crate::llm::provider::{ToolDefinition, ToolFunction};

/// Built-in tool definitions, filtered by mode.
///
/// - `ask` mode: only read/search tools (no execution)
/// - `general`/`task`/`code` mode: all tools
pub fn builtin_tool_definitions(mode: &str, with_memory: bool) -> Vec<ToolDefinition> {
    let mut tools = vec![
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "web_search".into(),
                description: "Search the web for current information. Use for research, news, documentation, and fact-checking.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "read_file".into(),
                description: "Read a file from the project directory. Use to examine code, configs, or documentation.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path from project root (e.g. 'src/main.rs')"
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "run_bash".into(),
                description: "Execute a bash command in the project directory. Use for compilation, testing, file operations, or exploring the filesystem.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The bash command to execute"
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "knowledge_search".into(),
                description: "Search the project's local knowledge base for relevant documentation. Use to find information about the codebase without reading full files.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "memory_search".into(),
                description: "Search persistent memory for past decisions, preferences, code patterns, or project facts. Use to recall context from earlier sessions or tasks.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query describing what to recall"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default: 5)",
                            "default": 5
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "memory_add".into(),
                description: "Add a fact or observation to persistent memory. The agent will remember it across sessions. Use to save user preferences, project conventions, important decisions, and patterns discovered during work.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "The fact or observation to remember"
                        },
                        "memory_type": {
                            "type": "string",
                            "description": "Type: 'user_preference' (user likes/dislikes/habits), 'codebase_fact' (code architecture/patterns), 'decision' (design decisions made), 'bug_fix' (bug and how it was fixed), 'learned' (general knowledge), 'ephemeral' (temporary note)",
                            "enum": ["user_preference", "codebase_fact", "decision", "skill", "personal_context", "ephemeral"]
                        }
                    },
                    "required": ["content"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "python_repl".into(),
                description: "Execute Python code in a persistent REPL sandbox. State (variables, imports, functions) persists across calls. Supports data analysis, visualization, scripting, and computations. Prefer this over run_bash for Python work.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "Python code to execute. Variables persist between calls."
                        }
                    },
                    "required": ["code"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "read_document".into(),
                description: "Read and parse a document file (PDF, Word .docx, Excel .xlsx/.xls, or plain text). Uses system tools (pdftotext) or Python libraries to extract text content. Handles tables, formatting, and multi-page documents.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the document file (relative to project root or absolute)"
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "browser".into(),
                description: "Control a headless Chrome browser. Commands: open <url> (navigate & get page text), screenshot (capture visual), source (get full HTML), eval <js> (run JavaScript), close (kill browser). State persists across calls within the same session.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The browser command: 'open' (navigate to URL and get text), 'screenshot' (capture screenshot), 'source' (get HTML source), 'eval' (run JavaScript expression), 'close' (kill browser process)"
                        },
                        "url": {
                            "type": "string",
                            "description": "URL to navigate to (required for 'open' command)"
                        },
                        "js": {
                            "type": "string",
                            "description": "JavaScript expression to evaluate (required for 'eval' command)"
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "platform_setup".into(),
                description: "Check available tools (Python, Chrome, pdftotext) and get install instructions for the current operating system (macOS/Linux/Windows). Use this when a tool is missing or to verify the environment.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "description": "'check' to detect available tools, 'guide' to show install instructions, 'install_python_pkg' to pip install a specific package (e.g. 'pandas','openpyxl','python-docx','pymupdf','websocket-client')"
                        },
                        "package": {
                            "type": "string",
                            "description": "Python package name to install (required when action='install_python_pkg')"
                        }
                    },
                    "required": ["action"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "analyze_image".into(),
                description: "Analyze an image file using vision AI. Supports PNG, JPEG, GIF, WebP. Describe what you see, read text in images, identify objects, analyze screenshots, or extract visual information. Path can be absolute or relative to project root.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the image file (absolute or relative to project root)"
                        },
                        "prompt": {
                            "type": "string",
                            "description": "Optional specific question about the image (default: 'Describe this image in detail')"
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
    ];

    if with_memory {
        tools.push(ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "memory_remove".into(),
                description: "Remove a memory entry by its ID. Use to delete outdated or incorrect memories.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "The memory ID to remove"
                        }
                    },
                    "required": ["id"]
                }),
            },
        });
    }

    // In ask mode, only expose read/search tools
    if mode == "ask" {
        let safe_tools: std::collections::HashSet<&str> = [
            "web_search", "read_file", "knowledge_search",
            "memory_search", "read_document", "browser",
            "analyze_image", "platform_setup",
        ].into();
        tools.retain(|t| safe_tools.contains(t.function.name.as_str()));
    }

    tools
}
