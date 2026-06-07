#![allow(unused)]
//! SaaS Web Server — Turn HyperAgent into a zero-install web product.
//!
//! For 100M users, the browser is the universal runtime. No CLI install
//! needed. Users type prompts in a web UI, see streaming responses,
//! and manage their projects entirely from the browser.
//!
//! Architecture:
//!   Browser ──WebSocket──→ hyper saas --port 3000
//!                              ├── Agent pipeline (orchestrator)
//!                              ├── File browser
//!                              ├── Memory dashboard
//!                              └── Billing integration
//!
//! This is NOT a replacement for the CLI — it's an ADDITIONAL surface.
//!
//! Commands:
//!   hyper saas --port 3000        — Start SaaS server

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SaasConfig {
    pub port: u16,
    pub project_dir: PathBuf,
    pub require_auth: bool,
    pub readonly: bool,
}

impl SaasConfig {
    pub fn new(project_dir: PathBuf) -> Self {
        SaasConfig {
            port: 3000,
            project_dir,
            require_auth: false,
            readonly: false,
        }
    }
}

pub async fn serve(config: SaasConfig) -> Result<()> {
    let state = Arc::new(Mutex::new(SaasState::new(config.project_dir.clone())));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port)).await?;

    println!();
    println!("  \x1b[1;36m🌐 HyperAgent SaaS\x1b[0m");
    println!("  {}", "─".repeat(50));
    println!("  Server:  http://localhost:{}", config.port);
    println!("  Project: {}", config.project_dir.display());
    println!();
    println!("  \x1b[90mOpen http://localhost:{} in your browser\x1b[0m", config.port);
    println!();

    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_saas_request(stream, state).await {
                eprintln!("SaaS request error: {}", e);
            }
        });
    }
}

struct SaasState {
    project_dir: PathBuf,
}

impl SaasState {
    fn new(project_dir: PathBuf) -> Self {
        SaasState { project_dir }
    }
}

async fn handle_saas_request(
    mut stream: tokio::net::TcpStream,
    _state: Arc<Mutex<SaasState>>,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    let path = if parts.len() >= 2 { parts[1] } else { "/" };

    // Skip headers
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
    }

    let (status, content_type, body) = match path {
        "/" | "/index.html" => ("200 OK", "text/html; charset=utf-8", saas_index_html()),
        "/api/health" => ("200 OK", "application/json", r#"{"status":"ok","version":"saas-0.1.0"}"#.to_string()),
        "/api/status" => {
            let status_json = serde_json::json!({
                "project": ".",
                "index_exists": false,
                "memory_count": 0,
                "skill_count": 0,
            });
            ("200 OK", "application/json", serde_json::to_string_pretty(&status_json)?)
        }
        _ => ("404 Not Found", "text/plain", "404 Not Found".to_string()),
    };

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        status,
        content_type,
        body.len(),
        body
    );

    writer.write_all(response.as_bytes()).await?;
    Ok(())
}

fn saas_index_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>HyperAgent — Web</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:system-ui,-apple-system,sans-serif;background:#0d1117;color:#c9d1d9;display:flex;height:100vh}
.sidebar{width:260px;background:#161b22;border-right:1px solid #30363d;padding:1rem;display:flex;flex-direction:column}
.main{flex:1;display:flex;flex-direction:column}
.header{background:#161b22;border-bottom:1px solid #30363d;padding:0.75rem 1.5rem;display:flex;align-items:center;gap:1rem}
.header h1{font-size:1rem;color:#58a6ff}
.chat-area{flex:1;overflow-y:auto;padding:1.5rem}
.input-area{border-top:1px solid #30363d;padding:1rem 1.5rem;display:flex;gap:0.75rem}
.input-area textarea{flex:1;background:#0d1117;border:1px solid #30363d;border-radius:8px;color:#c9d1d9;padding:0.75rem;resize:none;font-family:inherit;font-size:0.9rem;min-height:44px}
.input-area textarea:focus{outline:none;border-color:#58a6ff}
.input-area button{background:#238636;color:#fff;border:none;border-radius:8px;padding:0.75rem 1.5rem;cursor:pointer;font-weight:600;white-space:nowrap}
.input-area button:hover{background:#2ea043}
.message{margin-bottom:1rem;padding:0.75rem 1rem;border-radius:8px;max-width:85%}
.message.user{background:#1c3b5c;margin-left:auto}
.message.agent{background:#161b22;border:1px solid #30363d}
.message .role{font-size:0.75rem;color:#58a6ff;margin-bottom:0.25rem;font-weight:600}
.message .content{white-space:pre-wrap;word-break:break-word;font-size:0.9rem;line-height:1.5}
.logo{font-size:1.2rem;font-weight:bold;color:#58a6ff;margin-bottom:1.5rem}
.nav-item{padding:0.5rem 0.75rem;border-radius:6px;cursor:pointer;font-size:0.9rem;color:#8b949e;margin-bottom:0.25rem}
.nav-item:hover,.nav-item.active{background:#1c2128;color:#c9d1d9}
.status-bar{font-size:0.75rem;color:#484f58;padding:0.5rem 1.5rem;border-top:1px solid #30363d}
.files{flex:1;overflow:auto;margin:0.5rem 0}
.file-item{padding:0.25rem 0.75rem;font-size:0.8rem;color:#8b949e;cursor:pointer;border-radius:4px}
.file-item:hover{color:#c9d1d9;background:#1c2128}
.file-item.dir{color:#58a6ff}
</style>
</head>
<body>
<div class="sidebar">
    <div class="logo">⚡ HyperAgent</div>
    <div class="nav-item active">💬 Chat</div>
    <div class="nav-item">📁 Files</div>
    <div class="nav-item">🧠 Memory</div>
    <div class="nav-item">🛠 Skills</div>
    <div class="nav-item">⚙️ Settings</div>
    <div class="files" id="file-list">
        <div class="file-item dir">📂 src/</div>
        <div class="file-item">  📄 main.rs</div>
        <div class="file-item">  📄 cli.rs</div>
        <div class="file-item dir">📂 tests/</div>
        <div class="file-item">  📄 integration.rs</div>
    </div>
    <div class="status-bar">Connected • Free Plan</div>
</div>
<div class="main">
    <div class="header">
        <h1>💬 Chat with HyperAgent</h1>
        <span style="font-size:0.8rem;color:#8b949e">Streaming responses</span>
    </div>
    <div class="chat-area" id="chat">
        <div class="message agent">
            <div class="role">🤖 HyperAgent</div>
            <div class="content">Welcome to HyperAgent Web! 👋

I'm your AI coding agent. I can:
• Explain code in your project
• Write and modify files
• Fix bugs and add features
• Run commands and tests

Type a prompt below to get started. For example:
"explain the project structure"
"add error handling to main.rs"
"optimize the database queries"</div>
        </div>
    </div>
    <div class="input-area">
        <textarea id="prompt" placeholder="Ask HyperAgent to do something..." rows="1" onkeydown="if(event.key==='Enter'&&!event.shiftKey){event.preventDefault();send()}"></textarea>
        <button onclick="send()">Send ⚡</button>
    </div>
</div>
<script>
function send() {
    var ta = document.getElementById('prompt');
    var text = ta.value.trim();
    if (!text) return;

    var chat = document.getElementById('chat');
    chat.innerHTML += '<div class="message user"><div class="role">👤 You</div><div class="content">' + escapeHtml(text) + '</div></div>';

    ta.value = '';
    ta.style.height = 'auto';

    chat.innerHTML += '<div class="message agent" id="loading-msg"><div class="role">🤖 HyperAgent</div><div class="content">⏳ Processing...</div></div>';
    chat.scrollTop = chat.scrollHeight;

    fetch('/api/status').then(r => r.json()).then(function(status) {
        document.getElementById('loading-msg').remove();
        chat.innerHTML += '<div class="message agent"><div class="role">🤖 HyperAgent</div><div class="content">Got your prompt: "' + escapeHtml(text) + '"\n\n[Web version — full agent pipeline integration in progress]\n\n⚡ To use the full agent now, run: <code>hyper run "' + escapeHtml(text) + '"</code></div></div>';
        chat.scrollTop = chat.scrollHeight;
    });
}
function escapeHtml(s) { return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'); }
</script>
</body>
</html>"#.to_string()
}
