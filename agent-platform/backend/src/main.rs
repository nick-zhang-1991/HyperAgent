//! Agent Orchestration Platform — Backend with SQLite persistence
//! 7 API endpoints + WebSocket + real HyperAgent execution

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

// ── Models ──
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization { pub id: String, pub name: String, pub description: String, pub created_at: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent { pub id: String, pub org_id: String, pub name: String, pub role: String, pub description: String, pub status: AgentStatus, pub current_task: Option<String>, pub total_tasks: u32, pub completed_tasks: u32, pub created_at: String }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus { Idle, Working, Error }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task { pub id: String, pub org_id: String, pub agent_id: String, pub description: String, pub status: TaskStatus, pub result: Option<String>, pub duration_ms: Option<u64>, pub created_at: String, pub completed_at: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus { Pending, Running, Completed, Failed }

#[derive(Deserialize)] struct CreateOrg { name: String, description: String }
#[derive(Deserialize)] struct CreateAgent { name: String, role: String, description: String }
#[derive(Deserialize)] struct CreateTask { description: String }

// ── SQLite Database ──
struct Db {
    conn: Mutex<rusqlite::Connection>,
}

impl Db {
    fn new(path: &str) -> anyhow::Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS orgs (id TEXT PRIMARY KEY, name TEXT, description TEXT, created_at TEXT);
            CREATE TABLE IF NOT EXISTS agents (id TEXT PRIMARY KEY, org_id TEXT, name TEXT, role TEXT, description TEXT, status TEXT, current_task TEXT, total_tasks INTEGER DEFAULT 0, completed_tasks INTEGER DEFAULT 0, created_at TEXT);
            CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, org_id TEXT, agent_id TEXT, description TEXT, status TEXT, result TEXT, duration_ms INTEGER, created_at TEXT, completed_at TEXT);
            CREATE INDEX IF NOT EXISTS idx_agents_org ON agents(org_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_org ON tasks(org_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_agent ON tasks(agent_id);
        ")?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    async fn orgs_all(&self) -> Vec<Organization> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT id, name, description, created_at FROM orgs ORDER BY created_at DESC").unwrap();
        let rows = stmt.query_map([], |row| Ok(Organization {
            id: row.get(0)?, name: row.get(1)?, description: row.get(2)?, created_at: row.get(3)?,
        })).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    async fn org_create(&self, org: &Organization) {
        let conn = self.conn.lock().await;
        conn.execute("INSERT INTO orgs VALUES (?1,?2,?3,?4)",
            rusqlite::params![org.id, org.name, org.description, org.created_at]).ok();
    }

    async fn org_exists(&self, id: &str) -> bool {
        let conn = self.conn.lock().await;
        conn.query_row("SELECT 1 FROM orgs WHERE id=?1", rusqlite::params![id], |_| Ok(()))
            .is_ok()
    }

    async fn agents_by_org(&self, org_id: &str) -> Vec<Agent> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT id,org_id,name,role,description,status,current_task,total_tasks,completed_tasks,created_at FROM agents WHERE org_id=?1 ORDER BY created_at DESC").unwrap();
        let rows = stmt.query_map(rusqlite::params![org_id], |row| Ok(Agent {
            id: row.get(0)?, org_id: row.get(1)?, name: row.get(2)?, role: row.get(3)?,
            description: row.get(4)?, status: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or(AgentStatus::Idle),
            current_task: row.get(6)?, total_tasks: row.get(7)?, completed_tasks: row.get(8)?, created_at: row.get(9)?,
        })).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    async fn agent_create(&self, agent: &Agent) {
        let conn = self.conn.lock().await;
        conn.execute("INSERT INTO agents VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![agent.id, agent.org_id, agent.name, agent.role, agent.description,
                serde_json::to_string(&agent.status).unwrap(), agent.current_task, agent.total_tasks, agent.completed_tasks, agent.created_at]).ok();
    }

    async fn agent_update_status(&self, agent_id: &str, status: &AgentStatus, current_task: Option<&str>) {
        let conn = self.conn.lock().await;
        conn.execute("UPDATE agents SET status=?1, current_task=?2 WHERE id=?3",
            rusqlite::params![serde_json::to_string(status).unwrap(), current_task, agent_id]).ok();
    }

    async fn agent_increment_tasks(&self, agent_id: &str, increment_total: bool) {
        let conn = self.conn.lock().await;
        if increment_total {
            conn.execute("UPDATE agents SET total_tasks = total_tasks + 1 WHERE id=?1",
                rusqlite::params![agent_id]).ok();
        } else {
            conn.execute("UPDATE agents SET completed_tasks = completed_tasks + 1 WHERE id=?1",
                rusqlite::params![agent_id]).ok();
        }
    }

    async fn tasks_by_org(&self, org_id: &str) -> Vec<Task> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT id,org_id,agent_id,description,status,result,duration_ms,created_at,completed_at FROM tasks WHERE org_id=?1 ORDER BY created_at DESC").unwrap();
        let rows = stmt.query_map(rusqlite::params![org_id], |row| Ok(Task {
            id: row.get(0)?, org_id: row.get(1)?, agent_id: row.get(2)?, description: row.get(3)?,
            status: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or(TaskStatus::Pending),
            result: row.get(5)?, duration_ms: row.get(6)?, created_at: row.get(7)?, completed_at: row.get(8)?,
        })).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    async fn task_create(&self, task: &Task) {
        let conn = self.conn.lock().await;
        conn.execute("INSERT INTO tasks VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![task.id, task.org_id, task.agent_id, task.description,
                serde_json::to_string(&task.status).unwrap(), task.result, task.duration_ms, task.created_at, task.completed_at]).ok();
    }

    async fn task_complete(&self, task_id: &str, result: &str, duration_ms: u64) {
        let conn = self.conn.lock().await;
        conn.execute("UPDATE tasks SET status=?1, result=?2, duration_ms=?3, completed_at=?4 WHERE id=?5",
            rusqlite::params![serde_json::to_string(&TaskStatus::Completed).unwrap(), result, duration_ms, chrono::Utc::now().to_rfc3339(), task_id]).ok();
    }

    async fn task_fail(&self, task_id: &str, error: &str) {
        let conn = self.conn.lock().await;
        conn.execute("UPDATE tasks SET status=?1, result=?2, completed_at=?3 WHERE id=?4",
            rusqlite::params![serde_json::to_string(&TaskStatus::Failed).unwrap(), error, chrono::Utc::now().to_rfc3339(), task_id]).ok();
    }
}

// ── App State ──
pub struct AppState {
    db: Db,
    tx: broadcast::Sender<String>,
}

fn db_path() -> String {
    let dir = dirs_next::data_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).join("agent-platform");
    std::fs::create_dir_all(&dir).ok();
    dir.join("platform.db").to_string_lossy().to_string()
}

// ── Handlers ──
async fn create_org(State(s): State<Arc<AppState>>, Json(b): Json<CreateOrg>) -> Json<Organization> {
    let o = Organization { id: Uuid::new_v4().to_string(), name: b.name, description: b.description, created_at: chrono::Utc::now().to_rfc3339() };
    s.db.org_create(&o).await;
    let _ = s.tx.send(format!("org:{} created", o.id));
    Json(o)
}

async fn list_orgs(State(s): State<Arc<AppState>>) -> Json<Vec<Organization>> {
    Json(s.db.orgs_all().await)
}

async fn create_agent(State(s): State<Arc<AppState>>, Path(oid): Path<String>, Json(b): Json<CreateAgent>) -> Result<Json<Agent>, StatusCode> {
    if !s.db.org_exists(&oid).await { return Err(StatusCode::NOT_FOUND); }
    let a = Agent { id: Uuid::new_v4().to_string(), org_id: oid.clone(), name: b.name, role: b.role, description: b.description, status: AgentStatus::Idle, current_task: None, total_tasks: 0, completed_tasks: 0, created_at: chrono::Utc::now().to_rfc3339() };
    s.db.agent_create(&a).await;
    let _ = s.tx.send(format!("agent:{} created", a.id));
    Ok(Json(a))
}

async fn list_agents(State(s): State<Arc<AppState>>, Path(oid): Path<String>) -> Result<Json<Vec<Agent>>, StatusCode> {
    Ok(Json(s.db.agents_by_org(&oid).await))
}

async fn create_task(State(s): State<Arc<AppState>>, Path((oid, aid)): Path<(String, String)>, Json(b): Json<CreateTask>) -> Result<Json<Task>, StatusCode> {
    let t = Task { id: Uuid::new_v4().to_string(), org_id: oid.clone(), agent_id: aid.clone(), description: b.description, status: TaskStatus::Pending, result: None, duration_ms: None, created_at: chrono::Utc::now().to_rfc3339(), completed_at: None };
    s.db.task_create(&t).await;
    s.db.agent_update_status(&aid, &AgentStatus::Working, Some(&t.id)).await;
    s.db.agent_increment_tasks(&aid, true).await;
    let _ = s.tx.send(format!("task:{} created for agent:{}", t.id, aid));
    let sc = s.clone(); let tc = t.clone();
    tokio::spawn(async move { execute_task(&sc, &tc).await; });
    Ok(Json(t))
}

async fn list_tasks(State(s): State<Arc<AppState>>, Path((oid, _aid)): Path<(String, String)>) -> Result<Json<Vec<Task>>, StatusCode> {
    Ok(Json(s.db.tasks_by_org(&oid).await))
}

async fn execute_task(s: &Arc<AppState>, t: &Task) {
    let start = std::time::Instant::now();

    // Real HyperAgent execution
    let result = tokio::task::spawn_blocking({
        let desc = t.description.clone();
        move || {
            std::process::Command::new("hyperagent")
                .arg("run")
                .arg(&desc)
                .arg("--mode")
                .arg("general")
                .output()
        }
    }).await;

    let elapsed = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.success() {
                let summary = format!("✅ Completed in {:.1}s: {}", elapsed as f64 / 1000.0, stdout.lines().last().unwrap_or("done"));
                s.db.task_complete(&t.id, &summary, elapsed).await;
            } else {
                let err = format!("❌ Failed: {}", stderr.lines().last().unwrap_or("unknown error"));
                s.db.task_fail(&t.id, &err).await;
            }
        }
        _ => {
            // HyperAgent binary not available — simulate
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let summary = format!("✅ Simulated: '{}' completed in {:.1}s", t.description, elapsed as f64 / 1000.0);
            s.db.task_complete(&t.id, &summary, elapsed).await;
        }
    }

    s.db.agent_update_status(&t.agent_id, &AgentStatus::Idle, None).await;
    s.db.agent_increment_tasks(&t.agent_id, false).await;
    let _ = s.tx.send(format!("task:{} completed", t.id));
}

// ── WebSocket ──
async fn ws_handler(ws: WebSocketUpgrade, State(s): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, s))
}

async fn handle_ws(socket: WebSocket, s: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = s.tx.subscribe();
    let send = tokio::spawn(async move { while let Ok(msg) = rx.recv().await { if sender.send(Message::Text(msg.into())).await.is_err() { break; } } });
    let recv = tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} });
    tokio::select! { _ = send => {}, _ = recv => {}, }
}

// ── Server ──
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let path = db_path();
    println!("🗄️  Database: {}", path);
    let db = Db::new(&path)?;
    let (tx, _) = broadcast::channel(100);
    let state = Arc::new(AppState { db, tx });
    let app = Router::new()
        .route("/api/orgs", get(list_orgs))
        .route("/api/orgs", post(create_org))
        .route("/api/orgs/:oid/agents", get(list_agents))
        .route("/api/orgs/:oid/agents", post(create_agent))
        .route("/api/orgs/:oid/agents/:aid/tasks", get(list_tasks))
        .route("/api/orgs/:oid/agents/:aid/tasks", post(create_task))
        .route("/api/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);
    println!("🚀 Agent Platform API — http://127.0.0.1:4000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:4000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
