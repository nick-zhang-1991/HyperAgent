//! MCP Server — expose HyperAgent as an MCP tool server
//!
//! Allows other MCP clients (Claude Code, Cline, Codex CLI, etc.)
//! to call HyperAgent's code understanding capabilities as tools.
//!
//! # Protocol
//! JSON-RPC 2.0 over stdio (standard MCP transport).
//!
//! # Usage
//! ```bash
//! hyper mcp-server
//! ```
//!
//! Other agents can then add HyperAgent as an MCP server:
//! ```json
//! {
//!   "mcpServers": {
//!     "hyperagent": {
//!       "command": "hyper",
//!       "args": ["mcp-server"]
//!     }
//!   }
//! }
//! ```

use anyhow::Result;
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Optional memory manager shared across MCP server lifetime
type SharedMemory = Option<Arc<Mutex<crate::memory::MemoryManager>>>;

/// Run the MCP server over stdio
pub async fn run_mcp_server(project_root: &Path) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut line_buf = String::new();

    // Initialize memory manager if available
    let mem: SharedMemory = {
        let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let mem_path = home.join(".hyper").join("memory.db");
        if mem_path.exists() {
            if let Ok(store) = crate::memory::SqliteMemoryStore::new(&mem_path) {
                let mgr = crate::memory::MemoryManager::new(Box::new(store), "mcp-server");
                eprintln!("   🧠 Memory loaded: {} entries", mgr.store().count().unwrap_or(0));
                Some(Arc::new(Mutex::new(mgr)))
            } else {
                eprintln!("   ⚠️  Memory database found but couldn't open");
                None
            }
        } else {
            eprintln!("   📝 No memory database yet (create one via hyper dashboard)");
            None
        }
    };

    // Send server info on startup (can be read by client)
    eprintln!("   🔌 HyperAgent MCP Server started");
    eprintln!("   📁 Root: {}", project_root.display());
    eprintln!("   📋 Tools: search_code, explain_code, code_stats, list_symbols, memory_remember, memory_recall");

    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf)?;
        if bytes_read == 0 {
            break; // EOF
        }

        let line = line_buf.trim();
        if line.is_empty() {
            continue;
        }

        // Parse JSON-RPC request
        let request: Value = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let error_resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32700, "message": format!("Parse error: {e}") },
                    "id": null
                });
                let mut out = stdout.lock();
                writeln!(out, "{}", serde_json::to_string(&error_resp)?)?;
                out.flush()?;
                continue;
            }
        };

        let method = request["method"].as_str().unwrap_or("");
        let id = &request["id"];
        let params = request.get("params").unwrap_or(&serde_json::Value::Null);

        let response = match method {
            "initialize" => handle_initialize(id, params),
            "tools/list" => handle_tools_list(id),
            "tools/call" => handle_tools_call(id, params, project_root, &mem).await,
            "notifications/initialized" => {
                // No response needed for notifications
                continue;
            }
            _ => {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32601, "message": format!("Method not found: {method}") },
                    "id": id
                })
            }
        };

        let mut out = stdout.lock();
        writeln!(out, "{}", serde_json::to_string(&response)?)?;
        out.flush()?;
    }

    eprintln!("   🔌 MCP Server stopped");
    Ok(())
}

fn handle_initialize(id: &Value, _params: &Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "protocolVersion": "2025-03-26",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "hyperagent",
                "version": "0.1.0"
            }
        },
        "id": id
    })
}


fn tool_definitions() -> Vec<Value> {
    vec![
        serde_json::json!({
            "name": "hyperagent_search_code",
            "description": "Search the codebase for symbols (functions, classes, structs, imports). Returns matching file paths and line numbers.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The symbol name or pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Project root directory (default: current dir)"
                    }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "hyperagent_explain_code",
            "description": "Explain a code file or function in detail. Uses the configured LLM to analyze and explain the code's purpose, architecture, and potential issues.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "File path or function name to explain"
                    },
                    "path": {
                        "type": "string",
                        "description": "Project root directory (default: current dir)"
                    }
                },
                "required": ["target"]
            }
        }),
        serde_json::json!({
            "name": "hyperagent_code_stats",
            "description": "Show codebase statistics: file count, line count, language breakdown, symbol counts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Project root directory (default: current dir)"
                    }
                }
            }
        }),
        serde_json::json!({
            "name": "hyperagent_list_symbols",
            "description": "List all symbols (functions, classes, structs) in a file or directory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "File or directory path to analyze"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Filter by symbol kind: function, struct, class, trait, enum, all (default: all)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Project root directory (default: current dir)"
                    }
                },
                "required": ["target"]
            }
        }),
        serde_json::json!({
            "name": "hyperagent_memory_remember",
            "description": "Store a memory: save a fact, preference, bug fix, or decision for future recall. Memories persist across sessions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The memory content to store (fact, preference, decision, etc.)"
                    },
                    "memory_type": {
                        "type": "string",
                        "description": "Type: user_preference, codebase_fact, action_outcome, decision, bug_fix, learned, ephemeral",
                        "enum": ["user_preference", "codebase_fact", "action_outcome", "decision", "bug_fix", "learned", "ephemeral"]
                    }
                },
                "required": ["content"]
            }
        }),
        serde_json::json!({
            "name": "hyperagent_memory_recall",
            "description": "Recall relevant memories based on a query. Returns matches ranked by relevance, including graph-traversed related items.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query to find relevant memories"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max memories to return (default: 10)"
                    }
                },
                "required": ["query"]
            }
        }),
    ]
}

fn handle_tools_list(id: &Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "tools": tool_definitions()
        },
        "id": id
    })
}

async fn handle_tools_call(id: &Value, params: &Value, project_root: &Path, mem: &SharedMemory) -> Value {
    let tool_name = params["name"].as_str().unwrap_or("");
    let args = params.get("arguments").unwrap_or(&serde_json::Value::Null);
    let root = args.get("path").and_then(|p| p.as_str())
        .map(Path::new)
        .unwrap_or(project_root);

    let result = match tool_name {
        "hyperagent_memory_remember" => {
            handle_memory_remember(id, args, mem).await
        }
        "hyperagent_memory_recall" => {
            handle_memory_recall(id, args, mem).await
        }
        "hyperagent_search_code" => {
            let query = args["query"].as_str().unwrap_or("");
            if query.is_empty() {
                error_json(id, "Missing required parameter: query")
            } else {
                match search_code(query, root) {
                    Ok(content) => success_json(id, &content),
                    Err(e) => error_json(id, &e.to_string()),
                }
            }
        }
        "hyperagent_explain_code" => {
            let target = args["target"].as_str().unwrap_or("");
            if target.is_empty() {
                error_json(id, "Missing required parameter: target")
            } else {
                match explain_code(target, root).await {
                    Ok(content) => success_json(id, &content),
                    Err(e) => error_json(id, &e.to_string()),
                }
            }
        }
        "hyperagent_code_stats" => {
            match code_stats(root) {
                Ok(content) => success_json(id, &content),
                Err(e) => error_json(id, &e.to_string()),
            }
        }
        "hyperagent_list_symbols" => {
            let target = args["target"].as_str().unwrap_or("");
            let kind = args["kind"].as_str().unwrap_or("all");
            if target.is_empty() {
                error_json(id, "Missing required parameter: target")
            } else {
                match list_symbols(target, kind, root) {
                    Ok(content) => success_json(id, &content),
                    Err(e) => error_json(id, &e.to_string()),
                }
            }
        }
        _ => error_json(id, &format!("Unknown tool: {tool_name}")),
    };

    result
}

/// Handle memory_remember MCP tool
async fn handle_memory_remember(id: &Value, args: &Value, mem: &SharedMemory) -> Value {
    let mgr = match mem {
        Some(m) => m.lock().await,
        None => return error_json(id, "Memory system not initialized. Run `hyper dashboard` first to create a memory database."),
    };
    let content = args["content"].as_str().unwrap_or("");
    if content.is_empty() {
        return error_json(id, "Missing required parameter: content");
    }
    let mem_type = args["memory_type"].as_str().unwrap_or("learned");
    let memory_type = match mem_type {
        "user_preference" => crate::memory::MemoryType::UserPreference,
        "codebase_fact" => crate::memory::MemoryType::CodebaseFact,
        "action_outcome" => crate::memory::MemoryType::ActionOutcome,
        "decision" => crate::memory::MemoryType::Decision,
        "bug_fix" => crate::memory::MemoryType::BugFix,
        "ephemeral" => crate::memory::MemoryType::Ephemeral,
        _ => crate::memory::MemoryType::Learned,
    };
    match mgr.remember(content, memory_type) {
        Ok(id_str) => success_json(id, &format!("Memory stored with ID: {id_str}")),
        Err(e) => error_json(id, &format!("Failed to store memory: {e}")),
    }
}

/// Handle memory_recall MCP tool
async fn handle_memory_recall(id: &Value, args: &Value, mem: &SharedMemory) -> Value {
    let mgr = match mem {
        Some(m) => m.lock().await,
        None => return error_json(id, "Memory system not initialized. Run `hyper dashboard` first to create a memory database."),
    };
    let query = args["query"].as_str().unwrap_or("");
    if query.is_empty() {
        return error_json(id, "Missing required parameter: query");
    }
    let limit = args["limit"].as_u64().unwrap_or(10) as usize;
    match mgr.recall(query, limit) {
        Ok(memories) => {
            let results: Vec<serde_json::Value> = memories.iter().map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "content": m.content,
                    "type": m.memory_type.to_string(),
                    "importance": m.importance,
                    "entities": m.entities,
                    "created": m.created_at.to_rfc3339(),
                })
            }).collect();
            let json = serde_json::json!({
                "count": results.len(),
                "results": results
            });
            success_json(id, &serde_json::to_string_pretty(&json).unwrap_or_default())
        }
        Err(e) => error_json(id, &format!("Failed to recall memories: {e}")),
    }
}

fn success_json(id: &Value, content: &str) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "content": [
                {
                    "type": "text",
                    "text": content
                }
            ]
        },
        "id": id
    })
}

fn error_json(id: &Value, message: &str) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "error": { "code": -32000, "message": message },
        "id": id
    })
}

fn search_code(query: &str, root: &Path) -> Result<String> {
    use crate::refactor::find_references;
    let excludes = ["target", ".git", "node_modules", ".hyper"];
    let refs = find_references(root, query, &excludes)?;

    if refs.is_empty() {
        return Ok(format!("No results found for '{query}' in {}", root.display()));
    }

    let mut output = String::new();
    let total: usize = refs.iter().map(|(_, l)| l.len()).sum();
    output.push_str(&format!("Found {total} references for '{query}' in {} files:\n\n", refs.len()));

    for (path, lines) in &refs {
        let relative = path.strip_prefix(root).unwrap_or(path);
        output.push_str(&format!("📄 {}\n", relative.display()));
        let content = std::fs::read_to_string(path).unwrap_or_default();
        for line_num in lines.iter().take(10) {
            if let Some(line) = content.lines().nth(line_num - 1) {
                output.push_str(&format!("  {}: {}\n", line_num, line.trim()));
            }
        }
        if lines.len() > 10 {
            output.push_str(&format!("  ... and {} more matches\n", lines.len() - 10));
        }
        output.push('\n');
    }

    Ok(output)
}

async fn explain_code(target: &str, root: &Path) -> Result<String> {
    let target_path = root.join(target);
    let content = if target_path.exists() && target_path.is_file() {
        std::fs::read_to_string(&target_path)?
    } else if root.join(format!("{target}.rs")).exists() {
        std::fs::read_to_string(root.join(format!("{target}.rs")))?
    } else if root.join(format!("{target}.py")).exists() {
        std::fs::read_to_string(root.join(format!("{target}.py")))?
    } else {
        // Try to find the file via search
        let excludes = ["target", ".git", "node_modules"];
        let refs = crate::refactor::find_references(root, target, &excludes)?;
        if refs.is_empty() {
            anyhow::bail!("No file or symbol '{target}' found in project");
        }
        let first_file = &refs[0].0;
        let mut text = format!("File: {}\n\n", first_file.strip_prefix(root).unwrap_or(first_file).display());
        text.push_str(&std::fs::read_to_string(first_file)?);
        return Ok(text);
    };

    // Truncate to reasonable size
    if content.len() > 100_000 {
        Ok(format!("{}(file truncated: {} chars)", &content[..100_000], content.len()))
    } else {
        Ok(format!("```\n{}\n```", content))
    }
}

fn code_stats(root: &Path) -> Result<String> {
    use std::collections::HashMap;

    let mut stats: HashMap<String, (usize, usize)> = HashMap::new(); // (files, lines)

    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(true)
        .build();

    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let ext = entry.path().extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            let lines = content.lines().count();
            let entry = stats.entry(ext).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += lines;
        }
    }

    let mut output = format!("📊 Codebase Statistics for {}\n\n", root.display());
    output.push_str(&format!("{:<10} {:>8} {:>10}\n", "Extension", "Files", "Lines"));
    output.push_str(&format!("{:-<10} {:->8} {:->10}\n", "", "", ""));

    let total_files: usize = stats.values().map(|(f, _)| f).sum();
    let total_lines: usize = stats.values().map(|(_, l)| l).sum();

    let mut sorted: Vec<_> = stats.into_iter().collect();
    sorted.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));

    for (ext, (files, lines)) in sorted {
        output.push_str(&format!("{:<10} {:>8} {:>10}\n", ext, files, lines));
    }
    output.push_str(&format!("{:-<10} {:->8} {:->10}\n", "", "", ""));
    output.push_str(&format!("{:<10} {:>8} {:>10}\n", "TOTAL", total_files, total_lines));

    Ok(output)
}

fn list_symbols(target: &str, kind_filter: &str, root: &Path) -> Result<String> {
    let target_path = root.join(target);
    if !target_path.exists() {
        anyhow::bail!("Path not found: {target}");
    }

    let mut output = String::new();

    if target_path.is_file() {
        // Single file
        let content = std::fs::read_to_string(&target_path)?;
        let symbols = extract_symbols_from_content(&content, kind_filter);
        output.push_str(&format!("Symbols in {}:\n\n", target));
        for s in &symbols {
            output.push_str(&format!("  {s}\n"));
        }
        if symbols.is_empty() {
            output.push_str("  (no symbols found)\n");
        }
    } else {
        // Directory — walk recursively
        let walker = ignore::WalkBuilder::new(&target_path)
            .standard_filters(true)
            .build();

        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let symbols = extract_symbols_from_content(&content, kind_filter);
                if !symbols.is_empty() {
                    let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
                    output.push_str(&format!("📄 {}\n", relative.display()));
                    for s in &symbols {
                        output.push_str(&format!("  {s}\n"));
                    }
                    output.push('\n');
                }
            }
        }
    }

    if output.is_empty() {
        output = format!("No symbols found in {target}");
    }

    Ok(output)
}

fn extract_symbols_from_content(content: &str, _kind_filter: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let patterns = [
        (r"(?:pub\s+)?(?:async\s+)?fn\s+(\w+)", "fn"),
        (r"(?:pub\s+)?struct\s+(\w+)", "struct"),
        (r"(?:pub\s+)?trait\s+(\w+)", "trait"),
        (r"(?:pub\s+)?enum\s+(\w+)", "enum"),
        (r"(?:pub\s+)?type\s+(\w+)", "type"),
        (r"(?:pub\s+)?mod\s+(\w+)", "mod"),
        (r"class\s+(\w+)", "class"),
        (r"(?:export\s+)?interface\s+(\w+)", "interface"),
        (r"def\s+(\w+)\s*\(", "def"),
        (r"func\s+(\w+)\s*\(", "func"),
    ];

    for (pattern, kind) in &patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            for cap in re.captures_iter(content) {
                if let Some(name) = cap.get(1) {
                    symbols.push(format!("{kind}:{}", name.as_str()));
                }
            }
        }
    }

    symbols.sort();
    symbols.dedup();
    symbols
}
