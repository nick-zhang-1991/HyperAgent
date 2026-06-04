//! Memory & Skills Web Dashboard — lightweight async HTTP server
//!
//! Serves a REST API + embedded HTML/JS frontend for browsing,
//! editing, and managing memories and skills.
//!
//! Uses tokio::net::TcpListener (no extra dependency — same pattern as web_ui.rs).
//!
//! # Usage
//! ```bash
//! hyper dashboard --port 8080
//! ```

use crate::memory::{MemoryManager, MemoryType};
use crate::skills::SkillsRegistry;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// Dashboard state shared between HTTP handler and data sources
pub struct DashboardState {
    pub mem_manager: Option<MemoryManager>,
    pub skills_dir: Option<std::path::PathBuf>,
    pub config_path: Option<std::path::PathBuf>,
}

/// Start the memory & skills web dashboard
pub async fn serve(port: u16, state: Arc<Mutex<DashboardState>>) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr).await?;

    println!("   🖥️  HyperAgent Dashboard started");
    println!("   🌐 Open: http://{addr}");
    println!("   📊 Memory & Skills Management");
    println!("   Press Ctrl+C to stop\n");

    loop {
        let (mut stream, peer) = listener.accept().await?;
        let state = state.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 8192];
            let n = match stream.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let response = handle_request(&request, &state).await;
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.flush().await;
        });
    }
}

async fn handle_request(request: &str, state: &Arc<Mutex<DashboardState>>) -> String {
    let first_line = request.lines().next().unwrap_or("GET / HTTP/1.1");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return http_response(400, "Bad Request", "text/plain");
    }

    let method = parts[0];
    let path = parts[1];

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => http_response(200, HTML, "text/html; charset=utf-8"),
        ("GET", "/api/memories") => handle_list_memories(state).await,
        ("GET", "/api/skills") => handle_list_skills(state).await,
        ("DELETE", path) if path.starts_with("/api/memories/") => handle_delete_memory(state, path).await,
        ("DELETE", path) if path.starts_with("/api/skills/") => handle_delete_skill(state, path).await,
        ("GET", "/api/stats") => handle_stats(state).await,
        ("GET", "/api/config") => handle_get_config(state).await,
        ("POST", "/api/config") => handle_set_config(state, &request).await,
        _ => http_response(404, "{\"error\": \"Not found\"}", "application/json"),
    }
}

async fn handle_list_memories(state: &Arc<Mutex<DashboardState>>) -> String {
    let guard = state.lock().await;
    match &guard.mem_manager {
        Some(mgr) => {
            let count = mgr.store().count().unwrap_or(0);
            // Use query to get recent memories
            let q = crate::memory::MemoryQuery {
                text: String::new(),
                limit: 100,
                memory_type: None,
                entity: None,
                max_age: None,
            };
            let memories = mgr.store().query(&q).unwrap_or_default();
            let json: Vec<serde_json::Value> = memories.iter().map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "content": m.content.chars().take(200).collect::<String>(),
                    "type": format!("{:?}", m.memory_type),
                    "importance": m.importance,
                    "created": m.created_at.to_rfc3339(),
                    "accessed": m.last_accessed.to_rfc3339(),
                })
            }).collect();
            http_json(&serde_json::json!({"memories": json, "total": count}))
        }
        None => http_json(&serde_json::json!({"memories": [], "total": 0, "note": "No memory manager available"})),
    }
}

#[allow(dead_code)]
async fn handle_get_memory(state: &Arc<Mutex<DashboardState>>, path: &str) -> String {
    let _id = path.trim_start_matches("/api/memories/");
    let guard = state.lock().await;
    match &guard.mem_manager {
        Some(_mgr) => {
            http_json(&serde_json::json!({"error": "get_by_id not available from web UI"}))
        }
        None => http_json(&serde_json::json!({"error": "No memory manager"})),
    }
}

#[allow(dead_code)]
async fn handle_get_skill(state: &Arc<Mutex<DashboardState>>, path: &str) -> String {
    let _name = path.trim_start_matches("/api/skills/");
    let _guard = state.lock().await;
    http_json(&serde_json::json!({"error": "get_by_id not available from web UI"}))
}

async fn handle_delete_memory(state: &Arc<Mutex<DashboardState>>, path: &str) -> String {
    let id = path.trim_start_matches("/api/memories/");
    let guard = state.lock().await;
    match &guard.mem_manager {
        Some(mgr) => {
            match mgr.store().delete(id) {
                Ok(_) => http_json(&serde_json::json!({"status": "deleted", "id": id})),
                Err(e) => http_json(&serde_json::json!({"error": format!("{e}")})),
            }
        }
        None => http_json(&serde_json::json!({"error": "No memory manager"})),
    }
}

async fn handle_list_skills(state: &Arc<Mutex<DashboardState>>) -> String {
    let guard = state.lock().await;
    match &guard.skills_dir {
        Some(dir) => {
            let registry = SkillsRegistry::new(dir);
            if registry.is_empty() {
                return http_json(&serde_json::json!({"skills": [], "total": 0}));
            }
            let skills: Vec<serde_json::Value> = registry.list().iter().map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "description": s.description,
                    "category": s.category,
                    "tags": s.tags,
                    "path": s.path.to_string_lossy(),
                })
            }).collect();
            http_json(&serde_json::json!({"skills": skills, "total": skills.len()}))
        }
        None => http_json(&serde_json::json!({"skills": [], "total": 0, "note": "No skills directory"})),
    }
}

async fn handle_delete_skill(state: &Arc<Mutex<DashboardState>>, path: &str) -> String {
    let name = path.trim_start_matches("/api/skills/");
    let guard = state.lock().await;
    match &guard.skills_dir {
        Some(dir) => {
            let registry = SkillsRegistry::new(dir);
            match registry.delete(name) {
                Ok(_) => http_json(&serde_json::json!({"status": "deleted", "name": name})),
                Err(e) => http_json(&serde_json::json!({"error": format!("{e}")})),
            }
        }
        None => http_json(&serde_json::json!({"error": "No skills directory"})),
    }
}

async fn handle_stats(state: &Arc<Mutex<DashboardState>>) -> String {
    let guard = state.lock().await;
    let mem_count = match &guard.mem_manager {
        Some(mgr) => mgr.store().count().unwrap_or(0),
        None => 0,
    };
    let skill_count = match &guard.skills_dir {
        Some(dir) => SkillsRegistry::new(dir).len(),
        None => 0,
    };
    http_json(&serde_json::json!({
        "memories": mem_count,
        "skills": skill_count,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Read config file and return as JSON
async fn handle_get_config(state: &Arc<Mutex<DashboardState>>) -> String {
    let guard = state.lock().await;
    match &guard.config_path {
        Some(path) if path.exists() => {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    // Parse TOML, convert to JSON
                    match content.parse::<toml::Value>() {
                        Ok(toml_val) => {
                            let json_val = toml_to_json(&toml_val);
                            http_json(&json_val)
                        }
                        Err(e) => http_response(500, &format!("{{\"error\": \"Parse error: {e}\"}}"), "application/json"),
                    }
                }
                Err(e) => http_response(500, &format!("{{\"error\": \"Read error: {e}\"}}"), "application/json"),
            }
        }
        _ => http_json(&serde_json::json!({"config_path": "not found"})),
    }
}

/// Write config file from POST body
async fn handle_set_config(state: &Arc<Mutex<DashboardState>>, request: &str) -> String {
    let guard = state.lock().await;
    let config_path = match &guard.config_path {
        Some(p) => p.clone(),
        None => return http_response(400, "{\"error\": \"No config path configured\"}", "application/json"),
    };

    // Extract JSON body from POST request
    let body = match extract_post_body(request) {
        Some(b) => b,
        None => return http_response(400, "{\"error\": \"No body found\"}", "application/json"),
    };

    // Parse as JSON
    let json_val: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => return http_response(400, &format!("{{\"error\": \"Invalid JSON: {e}\"}}"), "application/json"),
    };

    // Read existing config if it exists
    let mut existing = if config_path.exists() {
        match std::fs::read_to_string(&config_path) {
            Ok(c) => c.parse::<toml::Value>().unwrap_or(toml::Value::Table(toml::value::Table::new())),
            Err(_) => toml::Value::Table(toml::value::Table::new()),
        }
    } else {
        toml::Value::Table(toml::value::Table::new())
    };

    // Merge JSON into existing TOML (flat merge for simplicity)
    if let (toml::Value::Table(ref mut table), serde_json::Value::Object(map)) = (&mut existing, &json_val) {
        for (key, value) in map {
            let toml_value = json_to_toml_value(value);
            table.insert(key.clone(), toml_value);
        }
    }

    // Serialize back to TOML
    let toml_str = toml::to_string_pretty(&existing).unwrap_or_default();
    match std::fs::write(&config_path, &toml_str) {
        Ok(_) => {
            let msg = serde_json::json!({"status": "ok", "path": config_path.to_string_lossy()});
            http_json(&msg)
        }
        Err(e) => http_response(500, &format!("{{\"error\": \"Write failed: {e}\"}}"), "application/json"),
    }
}

/// Simple POST body extraction from raw HTTP request
fn extract_post_body(req: &str) -> Option<String> {
    // Find the double CRLF separating headers from body
    if let Some(pos) = req.find("\r\n\r\n") {
        let body = &req[pos + 4..];
        let trimmed = body.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    } else if let Some(pos) = req.find("\n\n") {
        let body = &req[pos + 2..];
        let trimmed = body.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    } else {
        None
    }
}

/// Convert toml::Value to serde_json::Value
fn toml_to_json(v: &toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(toml_to_json).collect()),
        toml::Value::Table(tbl) => {
            let mut map = serde_json::Map::new();
            for (k, v) in tbl {
                map.insert(k.clone(), toml_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}

/// Convert serde_json::Value to toml::Value
fn json_to_toml_value(v: &serde_json::Value) -> toml::Value {
    match v {
        serde_json::Value::String(s) => toml::Value::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { toml::Value::Integer(i) }
            else if let Some(f) = n.as_f64() { toml::Value::Float(f) }
            else { toml::Value::String(n.to_string()) }
        }
        serde_json::Value::Bool(b) => toml::Value::Boolean(*b),
        serde_json::Value::Array(arr) => toml::Value::Array(arr.iter().map(json_to_toml_value).collect()),
        serde_json::Value::Object(map) => {
            let mut tbl = toml::value::Table::new();
            for (k, v) in map {
                tbl.insert(k.clone(), json_to_toml_value(v));
            }
            toml::Value::Table(tbl)
        }
        serde_json::Value::Null => toml::Value::String("null".to_string()),
    }
}

fn http_response(status: u16, body: &str, content_type: &str) -> String {
    let status_text = match status {
        200 => "OK",
        404 => "Not Found",
        400 => "Bad Request",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{body}",
        body.len()
    )
}

fn http_json(value: &serde_json::Value) -> String {
    let body = serde_json::to_string_pretty(value).unwrap_or_default();
    http_response(200, &body, "application/json")
}

const HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>HyperAgent Dashboard</title>
<style>
  :root { --bg: #1a1a2e; --card: #16213e; --accent: #0f3460; --text: #e0e0e0; --green: #00b894; --red: #d63031; --blue: #0984e3; }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: var(--bg); color: var(--text); padding: 20px; }
  h1 { font-size: 1.5rem; margin-bottom: 20px; display: flex; align-items: center; gap: 10px; }
  h1 small { font-size: 0.8rem; opacity: 0.6; }
  .tabs { display: flex; gap: 4px; margin-bottom: 20px; }
  .tab { padding: 8px 20px; background: var(--card); border: none; color: var(--text); cursor: pointer; border-radius: 6px 6px 0 0; font-size: 0.9rem; }
  .tab.active { background: var(--accent); font-weight: bold; }
  .tab:hover { background: #1a3a6e; }
  .panel { display: none; }
  .panel.active { display: block; }
  .stats { display: flex; gap: 20px; margin-bottom: 20px; }
  .stat-card { background: var(--card); padding: 15px 25px; border-radius: 8px; text-align: center; }
  .stat-card .num { font-size: 2rem; font-weight: bold; color: var(--green); }
  .stat-card .label { font-size: 0.8rem; opacity: 0.7; margin-top: 4px; }
  .item { background: var(--card); padding: 12px 16px; margin-bottom: 8px; border-radius: 6px; cursor: pointer; transition: all 0.1s; }
  .item:hover { background: var(--accent); }
  .item .title { font-weight: bold; margin-bottom: 4px; }
  .item .meta { font-size: 0.8rem; opacity: 0.6; display: flex; gap: 10px; flex-wrap: wrap; }
  .item .preview { font-size: 0.85rem; margin-top: 4px; opacity: 0.8; max-height: 3em; overflow: hidden; }
  .tag { display: inline-block; background: var(--accent); padding: 2px 8px; border-radius: 10px; font-size: 0.75rem; }
  .detail-overlay { display: none; position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.7); z-index: 100; align-items: center; justify-content: center; }
  .detail-overlay.show { display: flex; }
  .detail-card { background: var(--card); padding: 25px; border-radius: 12px; max-width: 700px; width: 90%; max-height: 80vh; overflow-y: auto; }
  .detail-card h2 { margin-bottom: 10px; }
  .detail-card .body { white-space: pre-wrap; font-family: monospace; font-size: 0.85rem; background: rgba(0,0,0,0.3); padding: 12px; border-radius: 6px; margin-top: 10px; max-height: 50vh; overflow-y: auto; }
  .close-btn { float: right; background: none; border: none; color: var(--text); font-size: 1.5rem; cursor: pointer; }
  .del-btn { background: var(--red); color: white; border: none; padding: 6px 14px; border-radius: 4px; cursor: pointer; margin-top: 10px; }
  .del-btn:hover { opacity: 0.8; }
  .type-badge { display: inline-block; padding: 2px 8px; border-radius: 10px; font-size: 0.75rem; }
  .loading { text-align: center; padding: 40px; opacity: 0.5; }
  .empty { text-align: center; padding: 40px; opacity: 0.5; }
  .search-box { width: 100%; padding: 10px; background: var(--card); border: 1px solid var(--accent); color: var(--text); border-radius: 6px; margin-bottom: 15px; font-size: 0.9rem; }
  @media (max-width: 600px) { .stats { flex-direction: column; } }
</style>
</head>
<body>
<h1>⚡ HyperAgent Dashboard <small id="version"></small></h1>

<div class="stats" id="stats">
  <div class="stat-card"><div class="num" id="mem-count">-</div><div class="label">Memories</div></div>
  <div class="stat-card"><div class="num" id="skill-count">-</div><div class="label">Skills</div></div>
</div>

<div class="tabs">
  <button class="tab active" onclick="switchTab('memories')">🧠 Memories</button>
  <button class="tab" onclick="switchTab('skills')">📚 Skills</button>
</div>

<div id="panel-memories" class="panel active">
  <input type="text" class="search-box" id="mem-search" placeholder="Search memories..." oninput="filterMemories()">
  <div id="mem-list"></div>
</div>

<div id="panel-skills" class="panel">
  <input type="text" class="search-box" id="skill-search" placeholder="Search skills..." oninput="filterSkills()">
  <div id="skill-list"></div>
</div>

<div class="detail-overlay" id="detail-overlay" onclick="closeDetail(event)">
  <div class="detail-card" id="detail-card">
    <button class="close-btn" onclick="closeDetail()">&times;</button>
    <h2 id="detail-title"></h2>
    <div class="meta" id="detail-meta"></div>
    <div class="body" id="detail-body"></div>
    <button class="del-btn" id="detail-delete" onclick="deleteItem()">🗑️ Delete</button>
  </div>
</div>

<script>
let memories = [], skills = [], detailType = '';

async function fetchJSON(url) {
  try {
    const r = await fetch(url);
    return await r.json();
  } catch(e) { return null; }
}

async function loadStats() {
  const data = await fetchJSON('/api/stats');
  if (!data) return;
  document.getElementById('mem-count').textContent = data.memories;
  document.getElementById('skill-count').textContent = data.skills;
  document.getElementById('version').textContent = 'v' + data.version;
}

async function loadMemories() {
  const data = await fetchJSON('/api/memories');
  memories = data?.memories || [];
  renderMemories();
}

async function loadSkills() {
  const data = await fetchJSON('/api/skills');
  skills = data?.skills || [];
  renderSkills();
}

function renderMemories(filter) {
  const list = document.getElementById('mem-list');
  const items = filter ? memories.filter(m => m.content.toLowerCase().includes(filter)) : memories;
  if (items.length === 0) { list.innerHTML = '<div class="empty">No memories found</div>'; return; }
  list.innerHTML = items.map(m => `
    <div class="item" onclick="showMemory('${m.id}')">
      <div class="title">${escapeHtml(m.content.slice(0, 80))}${m.content.length > 80 ? '...' : ''}</div>
      <div class="meta">
        <span class="type-badge">${m.type}</span>
        <span>⭐ ${m.importance}</span>
        <span>${new Date(m.created).toLocaleDateString()}</span>
      </div>
    </div>
  `).join('');
}

function renderSkills(filter) {
  const list = document.getElementById('skill-list');
  const items = filter ? skills.filter(s => s.name.toLowerCase().includes(filter) || s.description.toLowerCase().includes(filter)) : skills;
  if (items.length === 0) { list.innerHTML = '<div class="empty">No skills found</div>'; return; }
  list.innerHTML = items.map(s => `
    <div class="item" onclick="showSkill('${s.name}')">
      <div class="title">${s.name}</div>
      <div class="preview">${escapeHtml(s.description)}</div>
      <div class="meta">
        <span class="tag">${s.category || 'uncategorized'}</span>
        ${(s.tags || []).map(t => `<span class="tag">${t}</span>`).join('')}
      </div>
    </div>
  `).join('');
}

function filterMemories() {
  const q = document.getElementById('mem-search').value.toLowerCase();
  renderMemories(q);
}

function filterSkills() {
  const q = document.getElementById('skill-search').value.toLowerCase();
  renderSkills(q);
}

function switchTab(name) {
  document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
  document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
  document.getElementById('panel-' + name).classList.add('active');
  document.querySelector(`.tab[onclick*="'${name}'"]`).classList.add('active');
}

async function showMemory(id) {
  const data = await fetchJSON('/api/memories/' + id);
  if (!data || data.error) return;
  detailType = 'memory';
  document.getElementById('detail-title').textContent = '🧠 Memory';
  document.getElementById('detail-meta').innerHTML = `
    <span class="type-badge">${data.type}</span> | ⭐ ${data.importance} | Created: ${new Date(data.created).toLocaleString()}
  `;
  document.getElementById('detail-body').textContent = data.content;
  document.getElementById('detail-delete').style.display = 'inline-block';
  document.getElementById('detail-delete').dataset.id = id;
  document.getElementById('detail-overlay').classList.add('show');
}

async function showSkill(name) {
  const data = await fetchJSON('/api/skills/' + name);
  if (!data || data.error) return;
  detailType = 'skill';
  document.getElementById('detail-title').textContent = '📚 ' + data.name;
  document.getElementById('detail-meta').innerHTML = `
    <span class="tag">${data.category || 'uncategorized'}</span>
    ${(data.tags || []).map(t => `<span class="tag">${t}</span>`).join('')}
  `;
  document.getElementById('detail-body').textContent = '---\nname: ' + data.name + '\ndescription: ' + data.description + '\n---\n\n' + data.content;
  document.getElementById('detail-delete').style.display = 'inline-block';
  document.getElementById('detail-delete').dataset.id = name;
  document.getElementById('detail-overlay').classList.add('show');
}

async function deleteItem() {
  const id = document.getElementById('detail-delete').dataset.id;
  if (!id || !confirm('Delete this ' + detailType + '?')) return;
  const endpoint = detailType === 'memory' ? '/api/memories/' : '/api/skills/';
  const r = await fetch(endpoint + id, { method: 'DELETE' });
  const data = await r.json();
  if (data.status === 'deleted') {
    closeDetail();
    loadMemories();
    loadSkills();
    loadStats();
  } else {
    alert('Delete failed: ' + (data.error || 'unknown'));
  }
}

function closeDetail(e) {
  if (e && e.target !== document.getElementById('detail-overlay')) return;
  document.getElementById('detail-overlay').classList.remove('show');
}

function escapeHtml(s) {
  const d = document.createElement('div');
  d.textContent = s;
  return d.innerHTML;
}

loadStats();
loadMemories();
loadSkills();
setInterval(loadStats, 10000);
setInterval(loadMemories, 15000);
setInterval(loadSkills, 15000);
</script>
</body>
</html>"#;
