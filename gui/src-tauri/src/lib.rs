use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

// ── Types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: i64,
    pub updated_at: i64,
    pub mode: String,
    pub model: String,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub system_prompt: String,
    pub max_tokens: u32,
    pub hotkey: String,
    pub ollama_url: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            model: "deepseek-v4-flash".into(),
            api_key: String::new(),
            base_url: "http://127.0.0.1:11434/v1".into(),
            system_prompt: "You are a helpful AI assistant. Be concise and accurate.".into(),
            max_tokens: 4096,
            hotkey: "CmdOrCtrl+Shift+H".into(),
            ollama_url: "http://127.0.0.1:11434".into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub children: Vec<FileTreeNode>,
}

#[derive(Debug, Deserialize)]
pub struct ApplyRequest {
    pub file_path: String,
    pub content: String,
}

// ── App State ───────────────────────────────────────────────────

struct AppState {
    sessions: Mutex<HashMap<String, ChatSession>>,
    current_session_id: Mutex<Option<String>>,
    settings: Mutex<AppSettings>,
    data_dir: Mutex<PathBuf>,
    response_cache: Mutex<HashMap<String, String>>,
}

// ── Request/Response ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub session_id: Option<String>,
    pub prompt: String,
    pub files: Option<Vec<String>>,
    pub mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub message_count: usize,
    pub created_at: i64,
    pub updated_at: i64,
    pub mode: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct ExportRequest {
    pub session_id: String,
    pub format: String,
}

#[derive(Debug, Serialize)]
pub struct ExportResponse {
    pub content: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub ts: i64,
    pub category: String,
    pub confidence: f64,
    pub hit_count: u32,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_used_at: i64,
    pub expires_at: Option<i64>,
}
#[derive(Debug, Deserialize)]
pub struct MemoryExtractRequest {
    pub session_id: String,
    pub text: String,           // conversation text to analyze
}

#[derive(Debug, Deserialize)]
pub struct SettingsUpdate {
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub system_prompt: Option<String>,
    pub max_tokens: Option<u32>,
    pub hotkey: Option<String>,
    pub ollama_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamChunk {
    pub session_id: String,
    pub message_id: String,
    pub content: String,
    pub done: bool,
    pub error: Option<String>,
}

// ── Helpers ─────────────────────────────────────────────────────

fn data_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("hyperagent-gui");
    // On Linux, use ~/.hyperagent-gui as fallback if XDG_DATA_HOME is default
    #[cfg(target_os = "linux")]
    {
        if dir.to_string_lossy().contains(".local/share") {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            let alt = home.join(".hyperagent-gui");
            if alt.exists() {
                dir = alt;
            }
        }
    }
    dir
}

fn find_hyperagent_binary() -> PathBuf {
    // 1. Check env var override
    if let Ok(path) = std::env::var("HYPERAGENT_BINARY") {
        let p = PathBuf::from(&path);
        if p.exists() { return p; }
    }

    // 2. Common install locations per platform
    let candidates = if cfg!(target_os = "windows") {
        vec![
            dirs::home_dir().unwrap_or_default().join(".hyper\\bin\\hyper.exe"),
            dirs::home_dir().unwrap_or_default().join(".hyper\\bin\\hyperagent.exe"),
            PathBuf::from("hyper.exe"),
            PathBuf::from("hyperagent.exe"),
        ]
    } else {
        vec![
            dirs::home_dir().unwrap_or_default().join(".local/bin/hyper"),
            dirs::home_dir().unwrap_or_default().join(".local/bin/hyperagent"),
            dirs::home_dir().unwrap_or_default().join(".hyper/bin/hyper"),
            PathBuf::from("/usr/local/bin/hyper"),
            PathBuf::from("/usr/bin/hyper"),
            // Development fallback (relative to Cargo project root)
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/hyperagent"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/release/hyperagent"),
        ]
    };

    for c in &candidates {
        if c.exists() { return c.clone(); }
    }

    // 3. Try PATH lookup
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let exe = if cfg!(target_os = "windows") {
                dir.join("hyper.exe")
            } else {
                dir.join("hyper")
            };
            if exe.exists() { return exe; }
        }
    }

    // 4. Final fallback — just use the bare name
    PathBuf::from(if cfg!(target_os = "windows") { "hyper.exe" } else { "hyper" })
}

fn ensure_data_dir(dir: &PathBuf) {
    fs::create_dir_all(dir.join("sessions")).ok();
    fs::create_dir_all(dir.join("exports")).ok();
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ── Memory helpers ────────────────────────────────────────────────

fn load_memories() -> Vec<MemoryEntry> {
    let dir = data_dir();
    let path = dir.join("memories.json");
    if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    } else {
        vec![]
    }
}

fn format_memories_for_prompt(memories: &[MemoryEntry]) -> String {
    memories.iter()
        .filter(|m| m.value.len() < 500)
        .map(|m| format!("- {}: {}", m.key, m.value))
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_facts_from_conversation(_prompt: &str, _response: &str) -> Vec<MemoryEntry> {
    vec![] // Stub — will be implemented with LLM extraction
}

fn merge_memories(existing: &mut Vec<MemoryEntry>, new: &[MemoryEntry]) {
    for entry in new {
        if let Some(e) = existing.iter_mut().find(|e| e.key == entry.key) {
            e.value = entry.value.clone();
            e.updated_at = now_ms();
            e.hit_count += 1;
        } else {
            existing.push(entry.clone());
        }
    }
}

fn save_memories(memories: &[MemoryEntry]) {
    let dir = data_dir();
    ensure_data_dir(&dir);
    let path = dir.join("memories.json");
    if let Ok(json) = serde_json::to_string_pretty(memories) {
        let _ = std::fs::write(&path, json);
    }
}

// ── Chat (non-streaming) ────────────────────────────────────────

fn wait_with_timeout(mut child: Child, timeout: Duration) -> Result<Output, String> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child.stdout.take().map(|mut out| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut out, &mut buf).unwrap_or(0);
                    buf
                }).unwrap_or_default();
                let stderr = child.stderr.take().map(|mut out| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut out, &mut buf).unwrap_or(0);
                    buf
                }).unwrap_or_default();
                return Ok(Output { status, stdout, stderr });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err("CLI timed out after 60s".to_string());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("CLI wait error: {}", e)),
        }
    }
}

#[tauri::command]
fn chat(state: State<AppState>, req: ChatRequest) -> Result<serde_json::Value, String> {
    let now = now_ms();
    let mode = req.mode.unwrap_or_else(|| "ask".to_string());
    let session_id = req.session_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());

    let mut sessions = state.sessions.lock();
    let settings = state.settings.lock();
    let sp = settings.system_prompt.clone();
    drop(settings);

    let session = sessions.entry(session_id.clone()).or_insert_with(|| ChatSession {
        id: session_id.clone(),
        title: req.prompt.chars().take(50).collect(),
        messages: vec![],
        created_at: now,
        updated_at: now,
        mode: mode.clone(),
        model: String::new(),
        system_prompt: sp.clone(),
    });

    let mut file_context = String::new();
    if let Some(ref files) = req.files {
        for f in files {
            if let Ok(content) = fs::read_to_string(f) {
                let t = if content.len() > 50000 { &content[..50000] } else { &content };
                file_context.push_str(&format!("\n\n--- File: {} ---\n{}", f, t));
            }
        }
    }

    let full_prompt = if file_context.is_empty() {
        req.prompt.clone()
    } else {
        format!("{}\n{}", req.prompt, file_context)
    };

    // Check cache
    let cache_key = format!("{}{}", mode, full_prompt);
    {
        let cache = state.response_cache.lock();
        if let Some(cached) = cache.get(&cache_key) {
            let response_text = cached.clone();
            session.messages.push(ChatMessage {
                id: Uuid::new_v4().to_string(),
                role: "assistant".into(),
                content: response_text.clone(),
                timestamp: now,
                files: vec![],
            });
            session.updated_at = now;
            *state.current_session_id.lock() = Some(session_id.clone());
            return Ok(serde_json::json!({
                "output": response_text, "session_id": session_id,
                "success": true, "error": null, "cached": true
            }));
        }
    }

    // Add user message
    session.messages.push(ChatMessage {
        id: Uuid::new_v4().to_string(),
        role: "user".into(),
        content: full_prompt.clone(),
        timestamp: now,
        files: req.files.clone().unwrap_or_default(),
    });

    // ── Memory injection: add relevant memories to prompt ──
    let memories = load_memories();
    let memory_context = format_memories_for_prompt(&memories);
    let augmented_prompt = if memory_context.is_empty() {
        full_prompt.clone()
    } else {
        format!("[MEMORY CONTEXT - facts about user and environment]\n{}\n\n[USER QUERY]\n{}", memory_context, full_prompt)
    };

    // Run hyperagent with augmented prompt
    let cli_path = find_hyperagent_binary();
    let work_dir = std::env::var("HYPERAGENT_WORKDIR").unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
            .to_string_lossy()
            .to_string()
    });
    let mut cmd = Command::new(&cli_path);
    cmd.arg("run").arg("-d").arg(&work_dir)
        .arg("--json").arg("--yes").arg(&augmented_prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Timeout after 60s
    let child = cmd.spawn().map_err(|e| format!("Failed to start CLI: {}", e))?;
    let output = wait_with_timeout(child, std::time::Duration::from_secs(60))
        .map_err(|e| format!("CLI timeout or error: {}", e))?;
    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    
    // Clean response aggressively
    let mut response_text = raw
        .lines()
        .filter(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('{') || t.starts_with('[') || t.starts_with("Cost:") 
            || t.starts_with("⚙️") || t.starts_with("🔄") || t.starts_with("🚀")
            || t.starts_with("---") || t.starts_with("🔍") || t.starts_with("📋") || t.starts_with("🤔")
            || t.starts_with("##") || t.starts_with("cargo") || t.starts_with("# ") || t.starts_with("- `")
            || t.starts_with("Found ") { return false }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    
    // Extract just the 📝 answer line if present: "📝 (round ...) answer text"
    if let Some(idx) = response_text.rfind("📝") {
        let line = &response_text[idx..];
        if let Some(paren) = line.find(')') {
            response_text = line[paren+1..].trim().to_string();
        }
    }
    
    let response_text = if response_text.is_empty() {
        let err = String::from_utf8_lossy(&output.stderr);
        if !err.trim().is_empty() { err.trim().to_string() } else { "No response".into() }
    } else { response_text };

    // ── Auto-extract memories from conversation ──
    {
        let facts = extract_facts_from_conversation(&full_prompt, &response_text);
        if !facts.is_empty() {
            let mut existing = load_memories();
            merge_memories(&mut existing, &facts);
            save_memories(&existing);
        }
    }

    // Cache response
    {
        let mut cache = state.response_cache.lock();
        cache.insert(cache_key, response_text.clone());
        if cache.len() > 100 { cache.clear(); } // simple eviction
    }

    if session.messages.len() == 1 {
        session.title = req.prompt.chars().take(80).collect();
    }
    session.updated_at = now;

    session.messages.push(ChatMessage {
        id: Uuid::new_v4().to_string(),
        role: "assistant".into(),
        content: response_text.clone(),
        timestamp: now,
        files: vec![],
    });

    let dir = state.data_dir.lock();
    ensure_data_dir(&dir);
    if let Ok(json) = serde_json::to_string_pretty(&*session) {
        fs::write(dir.join("sessions").join(format!("{}.json", session_id)), json).ok();
    }
    drop(dir);
    *state.current_session_id.lock() = Some(session_id.clone());
    drop(sessions);

    Ok(serde_json::json!({
        "output": response_text, "session_id": session_id,
        "success": true, "error": null, "cached": false
    }))
}

// ── Streaming Chat ──────────────────────────────────────────────

#[tauri::command]
async fn chat_stream(app: AppHandle, req: ChatRequest) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    let session_id = req.session_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
    let message_id = Uuid::new_v4().to_string();
    let now = now_ms();

    // Create session
    {
        let settings = state.settings.lock();
        let sp = settings.system_prompt.clone();
        drop(settings);
        state.sessions.lock().entry(session_id.clone()).or_insert_with(|| ChatSession {
            id: session_id.clone(),
            title: req.prompt.chars().take(50).collect(),
            messages: vec![],
            created_at: now, updated_at: now,
            mode: req.mode.clone().unwrap_or_default(),
            model: String::new(), system_prompt: sp,
        });
    }

    // Build full prompt
    let mut file_context = String::new();
    if let Some(ref files) = req.files {
        for f in files {
            if let Ok(content) = fs::read_to_string(f) {
                let t = if content.len() > 50000 { &content[..50000] } else { &content };
                file_context.push_str(&format!("\n\n--- File: {} ---\n{}", f, t));
            }
        }
    }
    let full_prompt = if file_context.is_empty() {
        req.prompt.clone()
    } else {
        format!("{}\n{}", req.prompt, file_context)
    };

    // Add user message
    {
        let mut sessions = state.sessions.lock();
        if let Some(session) = sessions.get_mut(&session_id) {
            session.messages.push(ChatMessage {
                id: Uuid::new_v4().to_string(), role: "user".into(),
                content: full_prompt.clone(), timestamp: now,
                files: req.files.clone().unwrap_or_default(),
            });
        }
    }

    // Spawn streaming process
    let sid = session_id.clone();
    let mid = message_id.clone();
    let app_handle = app.clone();
    let data_dir = state.data_dir.lock().clone();

    tauri::async_runtime::spawn(async move {
        let cli_path = find_hyperagent_binary();
        let work_dir = std::env::var("HYPERAGENT_WORKDIR").unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_else(|_| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
                .to_string_lossy()
                .to_string()
        });
        let mut cmd = Command::new(&cli_path);
        cmd.arg("run").arg("-d").arg(&work_dir)
            .arg("-y").arg(&full_prompt)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = app_handle.emit("chat-stream", StreamChunk {
                    session_id: sid.clone(), message_id: mid.clone(),
                    content: String::new(), done: true,
                    error: Some(e.to_string()),
                });
                return;
            }
        };

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        let mut full_response = String::new();

        for line in reader.lines() {
            if let Ok(l) = line {
                full_response.push_str(&l);
                full_response.push('\n');
                let _ = app_handle.emit("chat-stream", StreamChunk {
                    session_id: sid.clone(), message_id: mid.clone(),
                    content: l.clone(), done: false, error: None,
                });
            }
        }

        let _ = child.wait();

        // Save to session
        let sessions_dir = data_dir.join("sessions");
        if let Ok(json) = fs::read_to_string(sessions_dir.join(format!("{}.json", sid))) {
            if let Ok(mut session) = serde_json::from_str::<ChatSession>(&json) {
                session.messages.push(ChatMessage {
                    id: mid.clone(), role: "assistant".into(),
                    content: full_response.clone(), timestamp: now_ms(), files: vec![],
                });
                session.updated_at = now_ms();
                let _ = fs::write(sessions_dir.join(format!("{}.json", sid)),
                    serde_json::to_string_pretty(&session).unwrap_or_default());
            }
        }

        let _ = app_handle.emit("chat-stream", StreamChunk {
            session_id: sid, message_id: mid,
            content: String::new(), done: true, error: None,
        });
    });

    Ok(serde_json::json!({
        "session_id": session_id, "message_id": message_id, "streaming": true
    }))
}

// ── Session management ──────────────────────────────────────────

#[tauri::command]
fn create_session(state: State<AppState>) -> Result<SessionInfo, String> {
    let id = Uuid::new_v4().to_string();
    let now = now_ms();
    let settings = state.settings.lock();
    let session = ChatSession {
        id: id.clone(), title: "New Conversation".into(), messages: vec![],
        created_at: now, updated_at: now, mode: "ask".into(),
        model: settings.model.clone(), system_prompt: settings.system_prompt.clone(),
    };
    drop(settings);
    state.sessions.lock().insert(id.clone(), session.clone());
    *state.current_session_id.lock() = Some(id.clone());
    let dir = state.data_dir.lock();
    ensure_data_dir(&dir);
    if let Ok(json) = serde_json::to_string_pretty(&session) {
        fs::write(dir.join("sessions").join(format!("{}.json", id)), json).ok();
    }
    Ok(SessionInfo { id, title: session.title, message_count: 0, created_at: session.created_at, updated_at: session.updated_at, mode: session.mode, model: session.model })
}

#[tauri::command]
fn list_sessions(state: State<AppState>) -> Result<Vec<SessionInfo>, String> {
    let sessions = state.sessions.lock();
    let mut list: Vec<SessionInfo> = sessions.values().map(|s| SessionInfo {
        id: s.id.clone(), title: s.title.clone(), message_count: s.messages.len(),
        created_at: s.created_at, updated_at: s.updated_at,
        mode: s.mode.clone(), model: s.model.clone(),
    }).collect();
    list.sort_by_key(|s| -s.updated_at);
    Ok(list)
}

#[tauri::command]
fn switch_session(state: State<AppState>, session_id: String) -> Result<SessionInfo, String> {
    let sessions = state.sessions.lock();
    let s = sessions.get(&session_id).ok_or("Not found")?;
    let info = SessionInfo { id: s.id.clone(), title: s.title.clone(), message_count: s.messages.len(), created_at: s.created_at, updated_at: s.updated_at, mode: s.mode.clone(), model: s.model.clone() };
    drop(sessions);
    *state.current_session_id.lock() = Some(session_id);
    Ok(info)
}

#[tauri::command]
fn delete_session(state: State<AppState>, session_id: String) -> Result<(), String> {
    state.sessions.lock().remove(&session_id);
    if state.current_session_id.lock().as_ref() == Some(&session_id) {
        *state.current_session_id.lock() = state.sessions.lock().values().next().map(|s| s.id.clone());
    }
    let dir = state.data_dir.lock();
    fs::remove_file(dir.join("sessions").join(format!("{}.json", session_id))).ok();
    Ok(())
}

#[tauri::command]
fn get_messages(state: State<AppState>, session_id: String) -> Result<Vec<ChatMessage>, String> {
    state.sessions.lock().get(&session_id).map(|s| s.messages.clone()).ok_or("Not found".into())
}

#[tauri::command]
fn get_current_session(state: State<AppState>) -> Result<Option<SessionInfo>, String> {
    let cid = state.current_session_id.lock().clone();
    match cid {
        Some(id) => {
            let sessions = state.sessions.lock();
            sessions.get(&id).map(|s| SessionInfo {
                id: s.id.clone(), title: s.title.clone(), message_count: s.messages.len(),
                created_at: s.created_at, updated_at: s.updated_at,
                mode: s.mode.clone(), model: s.model.clone(),
            }).ok_or("Not found".into()).map(Some)
        }
        None => Ok(None),
    }
}

// ── Export / Share ──────────────────────────────────────────────

#[tauri::command]
fn export_conversation(state: State<AppState>, req: ExportRequest) -> Result<ExportResponse, String> {
    let sessions = state.sessions.lock();
    let s = sessions.get(&req.session_id).ok_or("Not found")?;
    let content = match req.format.as_str() {
        "markdown" => {
            let mut md = format!("# {}\n\n*Mode: {}*\n\n---\n\n", s.title, s.mode);
            for msg in &s.messages {
                md.push_str(&format!("### {} \n\n{}\n\n---\n\n",
                    if msg.role == "user" { "🧑 You" } else { "🤖 Assistant" }, msg.content));
            }
            md
        }
        _ => serde_json::to_string_pretty(s).map_err(|e| e.to_string())?,
    };
    let fname = format!("{}_{}.{}", s.title.replace(['/','\\',':','?'], "_"), &s.id[..8],
        if req.format == "markdown" { "md" } else { "json" });
    let dir = state.data_dir.lock();
    let fp = dir.join("exports").join(&fname);
    fs::write(&fp, &content).map_err(|e| e.to_string())?;
    Ok(ExportResponse { content, file_path: fp.to_string_lossy().to_string() })
}

#[tauri::command]
fn share_conversation(state: State<AppState>, session_id: String) -> Result<String, String> {
    let sessions = state.sessions.lock();
    let s = sessions.get(&session_id).ok_or("Not found")?;
    let json = serde_json::to_string(s).map_err(|e| e.to_string())?;
    use base64::Engine;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.as_bytes()))
}

// ── File tree ───────────────────────────────────────────────────

#[tauri::command]
fn list_directory(path: String) -> Result<Vec<FileTreeNode>, String> {
    fn walk(dir: &PathBuf, depth: u32) -> Result<Vec<FileTreeNode>, String> {
        if depth > 4 { return Ok(vec![]); }
        let mut children = vec![];
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                if name.starts_with('.') || name == "target" || name == "node_modules" { continue; }
                let is_dir = path.is_dir();
                let size = if is_dir { 0 } else { entry.metadata().map(|m| m.len()).unwrap_or(0) };
                let grandchildren = if is_dir { walk(&path, depth + 1).unwrap_or_default() } else { vec![] };
                children.push(FileTreeNode { name: name.clone(), path: path.to_string_lossy().to_string(), is_dir, size, children: grandchildren });
            }
        }
        children.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(children)
    }
    walk(&PathBuf::from(&path), 0)
}

// ── Apply changes to files ──────────────────────────────────────

#[tauri::command]
fn apply_changes(_state: State<AppState>, req: ApplyRequest) -> Result<String, String> {
    let path = PathBuf::from(&req.file_path);
    // Backup
    let backup = format!("{}.bak", req.file_path);
    if path.exists() {
        fs::copy(&path, &backup).map_err(|e| e.to_string())?;
    }
    fs::write(&path, &req.content).map_err(|e| e.to_string())?;
    Ok(format!("Applied to {}. Backup at {}", req.file_path, backup))
}

#[tauri::command]
fn get_file_diff(path: String, new_content: String) -> Result<String, String> {
    let old = if PathBuf::from(&path).exists() {
        fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };
    // Simple diff: show added/removed lines
    let mut diff = String::new();
    diff.push_str(&format!("--- a/{}\n+++ b/{}\n@@ ... @@\n", path, path));
    for (i, line) in new_content.lines().enumerate() {
        if i < old.lines().count() {
            let old_line = old.lines().nth(i).unwrap_or("");
            if line != old_line {
                diff.push_str(&format!("-{}\n+{}\n", old_line, line));
            } else {
                diff.push_str(&format!(" {}\n", line));
            }
        } else {
            diff.push_str(&format!("+{}\n", line));
        }
    }
    Ok(diff)
}

// ── Screenshot ──────────────────────────────────────────────────

#[tauri::command]
fn capture_screenshot() -> Result<String, String> {
    let path = data_dir().join("screenshots");
    fs::create_dir_all(&path).ok();
    let filename = format!("screenshot_{}.png", now_ms());
    let full_path = path.join(&filename);

    #[cfg(target_os = "macos")]
    {
        Command::new("screencapture")
            .args(&["-x", full_path.to_str().unwrap()])
            .output()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        // Use PowerShell for Windows
        let ps_script = format!(
            r#"Add-Type -AssemblyName System.Windows.Forms; $img = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $bmp = New-Object System.Drawing.Bitmap($img.Width, $img.Height); $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen(0,0,0,0,$img.Size); $bmp.Save('{}')"#,
            full_path.to_str().unwrap()
        );
        Command::new("powershell")
            .args(&["-Command", &ps_script])
            .output()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        // Try import (ImageMagick) first, fallback to scrot, then to xdg-desktop-portal
        let import_result = Command::new("import")
            .args(&["-window", "root", full_path.to_str().unwrap()])
            .output();
        if import_result.is_err() || !import_result.unwrap().status.success() {
            let scrot_result = Command::new("scrot")
                .args(&["-z", full_path.to_str().unwrap()])
                .output()
                .map_err(|e| e.to_string())?;
            if !scrot_result.status.success() {
                // Try GNOME's dbus screenshot as last resort
                let _ = Command::new("dbus-send")
                    .args(&["--print-reply", "--dest=org.gnome.Shell.Screenshot",
                            "/org/gnome/Shell/Screenshot",
                            "org.gnome.Shell.Screenshot.Screenshot",
                            format!("boolean:false"),
                            format!("boolean:false"),
                            format!("string:{}", full_path.to_str().unwrap())])
                    .output();
            }
        }
    }

    // Read and base64 encode
    let bytes = fs::read(&full_path).map_err(|e| e.to_string())?;
    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

// ── Ollama integration ──────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    pub size: u64,
}

#[tauri::command]
fn list_ollama_models(app: AppHandle) -> Result<Vec<OllamaModel>, String> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock();
    let url = format!("{}/api/tags", settings.ollama_url);
    drop(settings);

    let client = reqwest::blocking::Client::new();
    let resp = client.get(&url).timeout(std::time::Duration::from_secs(5)).send().map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;

    let models: Vec<OllamaModel> = json["models"].as_array().unwrap_or(&vec![]).iter().map(|m| OllamaModel {
        name: m["name"].as_str().unwrap_or("unknown").to_string(),
        size: m["size"].as_u64().unwrap_or(0),
    }).collect();
    Ok(models)
}

// ── Settings ────────────────────────────────────────────────────

#[tauri::command]
fn get_settings(state: State<AppState>) -> Result<AppSettings, String> {
    Ok(state.settings.lock().clone())
}

#[tauri::command]
fn update_settings(state: State<AppState>, update: SettingsUpdate) -> Result<AppSettings, String> {
    let mut s = state.settings.lock();
    if let Some(v) = update.model { s.model = v; }
    if let Some(v) = update.api_key { s.api_key = v; }
    if let Some(v) = update.base_url { s.base_url = v; }
    if let Some(v) = update.system_prompt { s.system_prompt = v; }
    if let Some(v) = update.max_tokens { s.max_tokens = v; }
    if let Some(v) = update.hotkey { s.hotkey = v; }
    if let Some(v) = update.ollama_url { s.ollama_url = v; }
    let dir = state.data_dir.lock();
    ensure_data_dir(&dir);
    if let Ok(json) = serde_json::to_string_pretty(&*s) {
        fs::write(dir.join("settings.json"), json).ok();
    }
    Ok(s.clone())
}

#[tauri::command]
fn clear_cache(state: State<AppState>) -> Result<(), String> {
    state.response_cache.lock().clear();
    Ok(())
}

// ── Load saved data ─────────────────────────────────────────────

fn load_sessions(dir: &PathBuf) -> HashMap<String, ChatSession> {
    let mut map = HashMap::new();
    if let Ok(entries) = fs::read_dir(dir.join("sessions")) {
        for entry in entries.flatten() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let Ok(s) = serde_json::from_str::<ChatSession>(&content) {
                    map.insert(s.id.clone(), s);
                }
            }
        }
    }
    map
}

fn load_settings(dir: &PathBuf) -> AppSettings {
    let path = dir.join("settings.json");
    fs::read_to_string(&path).ok().and_then(|c| serde_json::from_str(&c).ok()).unwrap_or_default()
}

// ── Memory System ────────────────────────────────────────────────

#[tauri::command]
fn get_memories(state: State<AppState>) -> Result<Vec<MemoryEntry>, String> {
    let dir = state.data_dir.lock();
    let path = dir.join("memories.json");
    if path.exists() {
        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
fn set_memory(state: State<AppState>, key: String, value: String) -> Result<(), String> {
    let dir = state.data_dir.lock();
    let path = dir.join("memories.json");
    let mut memories: Vec<MemoryEntry> = if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        vec![]
    };
    
    if let Some(entry) = memories.iter_mut().find(|m| m.key == key) {
        entry.value = value;
        entry.updated_at = now_ms();
    } else {
        memories.push(MemoryEntry { key, value, category: "user_pref".into(), confidence: 1.0, hit_count: 1, ts: now_ms(), created_at: now_ms(), updated_at: now_ms(), last_used_at: now_ms(), expires_at: None });
    }
    
    fs::write(&path, serde_json::to_string_pretty(&memories).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_memory(state: State<AppState>, key: String) -> Result<(), String> {
    let dir = state.data_dir.lock();
    let path = dir.join("memories.json");
    let mut memories: Vec<MemoryEntry> = if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        return Ok(());
    };
    memories.retain(|m| m.key != key);
    fs::write(&path, serde_json::to_string_pretty(&memories).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

// ── Context Window Estimate ─────────────────────────────────────

#[tauri::command]
fn estimate_context(state: State<AppState>, session_id: String) -> Result<serde_json::Value, String> {
    let sessions = state.sessions.lock();
    let s = sessions.get(&session_id).ok_or("Not found")?;
    let mut total_chars = 0usize;
    for msg in &s.messages {
        total_chars += msg.content.len();
    }
    let estimated_tokens = total_chars / 4;
    let max_tokens = state.settings.lock().max_tokens as usize;
    Ok(serde_json::json!({
        "used_tokens": estimated_tokens,
        "max_tokens": max_tokens,
        "percent": (estimated_tokens as f64 / max_tokens as f64 * 100.0).min(100.0)
    }))
}

// ── Themes ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub colors: serde_json::Value,
}

#[tauri::command]
fn get_themes() -> Result<Vec<ThemeInfo>, String> {
    Ok(vec![
        ThemeInfo { id: "dark".into(), name: "Dark".into(), colors: serde_json::json!({"bg":"240 10% 4%","fg":"0 0% 98%","primary":"217 91% 60%","accent":"240 4% 16%"}) },
        ThemeInfo { id: "light".into(), name: "Light".into(), colors: serde_json::json!({"bg":"0 0% 100%","fg":"240 10% 4%","primary":"240 6% 10%","accent":"240 5% 96%"}) },
        ThemeInfo { id: "midnight".into(), name: "Midnight Blue".into(), colors: serde_json::json!({"bg":"222 47% 11%","fg":"210 40% 98%","primary":"217 91% 60%","accent":"217 33% 17%"}) },
        ThemeInfo { id: "forest".into(), name: "Forest".into(), colors: serde_json::json!({"bg":"150 30% 8%","fg":"150 10% 95%","primary":"142 71% 45%","accent":"150 20% 15%"}) },
        ThemeInfo { id: "sunset".into(), name: "Sunset".into(), colors: serde_json::json!({"bg":"25 50% 8%","fg":"30 20% 95%","primary":"25 95% 53%","accent":"25 30% 15%"}) },
        ThemeInfo { id: "monochrome".into(), name: "Monochrome".into(), colors: serde_json::json!({"bg":"0 0% 5%","fg":"0 0% 95%","primary":"0 0% 60%","accent":"0 0% 15%"}) },
    ])
}

// ── Update check ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub download_url: Option<String>,
}

#[tauri::command]
fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    Ok(UpdateInfo {
        current_version: current.clone(),
        latest_version: current,
        update_available: false,
        download_url: None,
    })
}

// ── Image Generation ────────────────────────────────────────────
#[derive(Debug, Deserialize)]
pub struct ImageGenRequest {
    pub prompt: String,
    pub size: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImageGenResponse {
    pub url: Option<String>,
    pub b64: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
fn generate_image(app: AppHandle, req: ImageGenRequest) -> Result<ImageGenResponse, String> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock();
    let api_key = settings.api_key.clone();
    let size = req.size.clone().unwrap_or_else(|| "1024x1024".into());

    if api_key.is_empty() {
        return Ok(ImageGenResponse { url: None, b64: None, error: Some("API key not configured".into()) });
    }

    // Try DALL-E compatible API
    let client = reqwest::blocking::Client::new();
    let body = serde_json::json!({
        "prompt": req.prompt,
        "n": 1,
        "size": size,
        "model": req.model.unwrap_or_else(|| "dall-e-3".into()),
    });

    match client
        .post("https://api.openai.com/v1/images/generations")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
    {
        Ok(resp) => {
            let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
            let url = json["data"][0]["url"].as_str().map(|s| s.to_string());
            Ok(ImageGenResponse { url, b64: None, error: None })
        }
        Err(e) => Ok(ImageGenResponse { url: None, b64: None, error: Some(e.to_string()) }),
    }
}

// ── Terminal Execution ──────────────────────────────────────────
#[derive(Debug, Deserialize)]
pub struct TerminalRequest {
    pub command: String,
    pub workdir: Option<String>,
}

#[tauri::command]
fn run_terminal(req: TerminalRequest) -> Result<String, String> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", &req.command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", &req.command]);
        c
    };

    if let Some(dir) = &req.workdir {
        cmd.current_dir(dir);
    }

    let output = cmd.output().map_err(|e| format!("Failed to execute: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Ok(format!("Exit {}:\n{}", output.status.code().unwrap_or(-1), if stderr.is_empty() { stdout } else { stderr }))
    }
}

// ── Agent Task (multi-step) ─────────────────────────────────────
#[derive(Debug, Deserialize)]
pub struct AgentRequest {
    pub task: String,
    pub session_id: Option<String>,
    pub max_steps: Option<u32>,
}

#[tauri::command]
async fn agent_task(app: AppHandle, req: AgentRequest) -> Result<String, String> {
    let max_steps = req.max_steps.unwrap_or(5);
    let mut results: Vec<String> = vec![];

    for step in 0..max_steps {
        let step_prompt = if step == 0 {
            format!("Task: {}\n\nBreak this down into steps and execute step 1. Be concise.", req.task)
        } else {
            format!("Continue with step {}. Previous results:\n{}\n\nExecute the next step now.", step + 1, results.join("\n---\n"))
        };

        let cli = find_hyperagent_binary();
        let work_dir = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
        let output = Command::new(&cli)
            .args(["run", "-d", &work_dir, "--json", "--yes", &step_prompt])
            .output()
            .map_err(|e| format!("Step {} failed: {}", step + 1, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        results.push(stdout.trim().to_string());

        // Emit progress
        let _ = app.emit("agent-step", serde_json::json!({
            "step": step + 1,
            "total": max_steps,
            "output": results.last().unwrap(),
        }));
    }

    Ok(results.join("\n\n## Step ").trim().to_string())
}

// ── Multi-window ────────────────────────────────────────────────

#[tauri::command]
fn open_new_window(app: AppHandle, label: Option<String>) -> Result<String, String> {
    use tauri::WebviewWindowBuilder;
    let label = label.unwrap_or_else(|| Uuid::new_v4().to_string());
    WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App("index.html".into()))
        .title("HyperAgent Desktop")
        .inner_size(1000.0, 700.0)
        .min_inner_size(600.0, 400.0)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(label)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let dir = data_dir();
    ensure_data_dir(&dir);

    let sessions = load_sessions(&dir);
    let settings = load_settings(&dir);

    let app_state = AppState {
        sessions: Mutex::new(sessions),
        current_session_id: Mutex::new(None),
        settings: Mutex::new(settings),
        data_dir: Mutex::new(dir.clone()),
        response_cache: Mutex::new(HashMap::new()),
    };

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            chat, chat_stream, create_session, list_sessions, switch_session,
            delete_session, get_messages, get_current_session,
            export_conversation, share_conversation,
            list_directory, apply_changes, get_file_diff,
            capture_screenshot, list_ollama_models,
            get_settings, update_settings, clear_cache,
            get_memories, set_memory, delete_memory,
            estimate_context, get_themes, check_update,
            generate_image, run_terminal, agent_task, open_new_window,
        ])
        .setup(|app| {
            // Menu bar tray (cross-platform: macOS menu bar, Linux/Windows system tray)
            {
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
                let _tray = TrayIconBuilder::new()
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("HyperAgent Desktop")
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                            if let Some(window) = tray.app_handle().get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    })
                    .build(app)?;
            }

            if cfg!(debug_assertions) {
                app.handle().plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
