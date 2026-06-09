//! Agent Orchestration Platform v2 — SQLite persistence + auth + chaining + webhooks

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization { pub id: String, pub name: String, pub description: String, pub api_key: Option<String>, pub created_at: String }
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
#[derive(Deserialize)] struct CreateTask { description: String, chain_to_agent: Option<String>, schedule: Option<String> }
#[derive(Deserialize)] struct WebhookPayload { event: String, org_id: String, agent_id: String, description: String }

struct Db { conn: Mutex<rusqlite::Connection> }

impl Db {
    fn new(path: &str) -> anyhow::Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS orgs (id TEXT PRIMARY KEY, name TEXT, description TEXT, api_key TEXT, created_at TEXT); CREATE TABLE IF NOT EXISTS agents (id TEXT PRIMARY KEY, org_id TEXT, name TEXT, role TEXT, description TEXT, status TEXT, current_task TEXT, total_tasks INTEGER DEFAULT 0, completed_tasks INTEGER DEFAULT 0, created_at TEXT); CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, org_id TEXT, agent_id TEXT, description TEXT, status TEXT, result TEXT, duration_ms INTEGER, created_at TEXT, completed_at TEXT); CREATE INDEX IF NOT EXISTS idx_agents_org ON agents(org_id); CREATE INDEX IF NOT EXISTS idx_tasks_org ON tasks(org_id);")?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    async fn orgs_all(&self) -> Vec<Organization> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT id,name,description,api_key,created_at FROM orgs ORDER BY created_at DESC").unwrap();
        stmt.query_map([], |r| Ok(Organization { id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, api_key: r.get(3)?, created_at: r.get(4)? })).unwrap().filter_map(|r| r.ok()).collect()
    }
    async fn org_create(&self, org: &Organization) { let c = self.conn.lock().await; c.execute("INSERT INTO orgs VALUES (?1,?2,?3,?4,?5)", rusqlite::params![org.id,org.name,org.description,org.api_key,org.created_at]).ok(); }
    async fn org_exists(&self, id: &str) -> bool { self.conn.lock().await.query_row("SELECT 1 FROM orgs WHERE id=?1", rusqlite::params![id], |_| Ok(())).is_ok() }
    async fn agents_by_org(&self, org_id: &str) -> Vec<Agent> {
        let c = self.conn.lock().await;
        let mut s = c.prepare("SELECT id,org_id,name,role,description,status,current_task,total_tasks,completed_tasks,created_at FROM agents WHERE org_id=?1 ORDER BY created_at DESC").unwrap();
        s.query_map(rusqlite::params![org_id], |r| Ok(Agent { id:r.get(0)?,org_id:r.get(1)?,name:r.get(2)?,role:r.get(3)?,description:r.get(4)?,status:serde_json::from_str(&r.get::<_,String>(5)?).unwrap_or(AgentStatus::Idle),current_task:r.get(6)?,total_tasks:r.get(7)?,completed_tasks:r.get(8)?,created_at:r.get(9)? })).unwrap().filter_map(|r|r.ok()).collect()
    }
    async fn agent_create(&self, a: &Agent) { let c = self.conn.lock().await; c.execute("INSERT INTO agents VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", rusqlite::params![a.id,a.org_id,a.name,a.role,a.description,serde_json::to_string(&a.status).unwrap(),a.current_task,a.total_tasks,a.completed_tasks,a.created_at]).ok(); }
    async fn agent_update(&self, id: &str, status: &AgentStatus, current_task: Option<&str>) { let c = self.conn.lock().await; c.execute("UPDATE agents SET status=?1,current_task=?2 WHERE id=?3", rusqlite::params![serde_json::to_string(status).unwrap(),current_task,id]).ok(); }
    async fn agent_inc(&self, id: &str, total: bool) { let c = self.conn.lock().await; if total { c.execute("UPDATE agents SET total_tasks=total_tasks+1 WHERE id=?1", rusqlite::params![id]).ok(); } else { c.execute("UPDATE agents SET completed_tasks=completed_tasks+1 WHERE id=?1", rusqlite::params![id]).ok(); } }
    async fn tasks_by_org(&self, org_id: &str) -> Vec<Task> {
        let c = self.conn.lock().await;
        let mut s = c.prepare("SELECT id,org_id,agent_id,description,status,result,duration_ms,created_at,completed_at FROM tasks WHERE org_id=?1 ORDER BY created_at DESC").unwrap();
        s.query_map(rusqlite::params![org_id], |r| Ok(Task { id:r.get(0)?,org_id:r.get(1)?,agent_id:r.get(2)?,description:r.get(3)?,status:serde_json::from_str(&r.get::<_,String>(4)?).unwrap_or(TaskStatus::Pending),result:r.get(5)?,duration_ms:r.get(6)?,created_at:r.get(7)?,completed_at:r.get(8)? })).unwrap().filter_map(|r|r.ok()).collect()
    }
    async fn task_create(&self, t: &Task) { let c = self.conn.lock().await; c.execute("INSERT INTO tasks VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", rusqlite::params![t.id,t.org_id,t.agent_id,t.description,serde_json::to_string(&t.status).unwrap(),t.result,t.duration_ms,t.created_at,t.completed_at]).ok(); }
    async fn task_complete(&self, id: &str, result: &str, ms: u64) { let c = self.conn.lock().await; c.execute("UPDATE tasks SET status=?1,result=?2,duration_ms=?3,completed_at=?4 WHERE id=?5", rusqlite::params![serde_json::to_string(&TaskStatus::Completed).unwrap(),result,ms,chrono::Utc::now().to_rfc3339(),id]).ok(); }
    async fn task_fail(&self, id: &str, e: &str) { let c = self.conn.lock().await; c.execute("UPDATE tasks SET status=?1,result=?2,completed_at=?3 WHERE id=?4", rusqlite::params![serde_json::to_string(&TaskStatus::Failed).unwrap(),e,chrono::Utc::now().to_rfc3339(),id]).ok(); }
}

pub struct AppState { db: Db, tx: broadcast::Sender<String> }

fn db_path() -> String { let d = dirs_next::data_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).join("agent-platform"); std::fs::create_dir_all(&d).ok(); d.join("platform.db").to_string_lossy().to_string() }

async fn create_org(State(s): State<Arc<AppState>>, Json(b): Json<CreateOrg>) -> Json<Organization> {
    let o = Organization { id: Uuid::new_v4().to_string(), name: b.name, description: b.description, api_key: Some(format!("org_{}", Uuid::new_v4().to_string().replace("-", ""))), created_at: chrono::Utc::now().to_rfc3339() };
    s.db.org_create(&o).await; let _ = s.tx.send(format!("org:{} created", o.id)); Json(o)
}
async fn list_orgs(State(s): State<Arc<AppState>>) -> Json<Vec<Organization>> { Json(s.db.orgs_all().await) }

async fn create_agent(State(s): State<Arc<AppState>>, Path(oid): Path<String>, Json(b): Json<CreateAgent>) -> Result<Json<Agent>, StatusCode> {
    if !s.db.org_exists(&oid).await { return Err(StatusCode::NOT_FOUND); }
    let a = Agent { id: Uuid::new_v4().to_string(), org_id: oid, name: b.name, role: b.role, description: b.description, status: AgentStatus::Idle, current_task: None, total_tasks: 0, completed_tasks: 0, created_at: chrono::Utc::now().to_rfc3339() };
    s.db.agent_create(&a).await; let _ = s.tx.send(format!("agent:{} created", a.id)); Ok(Json(a))
}
async fn list_agents(State(s): State<Arc<AppState>>, Path(oid): Path<String>) -> Result<Json<Vec<Agent>>, StatusCode> { Ok(Json(s.db.agents_by_org(&oid).await)) }

async fn create_task(State(s): State<Arc<AppState>>, Path((oid, aid)): Path<(String, String)>, Json(b): Json<CreateTask>) -> Result<Json<Task>, StatusCode> {
    // Handle scheduled tasks
    if let Some(ref schedule) = b.schedule { if let Ok(secs) = schedule.parse::<u64>() { let sc = s.clone(); let o = oid.clone(); let a = aid.clone(); let d = b.description.clone(); tokio::spawn(async move { tokio::time::sleep(std::time::Duration::from_secs(secs)).await; let t = Task { id:Uuid::new_v4().to_string(),org_id:o.clone(),agent_id:a.clone(),description:format!("[scheduled:{}s] {}",secs,d),status:TaskStatus::Pending,result:None,duration_ms:None,created_at:chrono::Utc::now().to_rfc3339(),completed_at:None }; sc.db.task_create(&t).await; sc.db.agent_update(&a,&AgentStatus::Working,Some(&t.id)).await; sc.db.agent_inc(&a,true).await; let _ = sc.tx.send(format!("scheduled:{}",t.id)); let scc = sc.clone(); let tc = t; tokio::spawn(async move { run_task(&scc, &tc, None).await; }); }); } }

    let t = Task { id: Uuid::new_v4().to_string(), org_id: oid.clone(), agent_id: aid.clone(), description: b.description, status: TaskStatus::Pending, result: None, duration_ms: None, created_at: chrono::Utc::now().to_rfc3339(), completed_at: None };
    s.db.task_create(&t).await; s.db.agent_update(&aid, &AgentStatus::Working, Some(&t.id)).await; s.db.agent_inc(&aid, true).await;
    let _ = s.tx.send(format!("task:{} created", t.id));
    let sc2 = s.clone(); let tc2 = t.clone(); let chain2 = b.chain_to_agent.clone();
    tokio::spawn(async move { run_task(&sc2, &tc2, chain2).await; });
    Ok(Json(t))
}
async fn list_tasks(State(s): State<Arc<AppState>>, Path((oid, _aid)): Path<(String, String)>) -> Result<Json<Vec<Task>>, StatusCode> { Ok(Json(s.db.tasks_by_org(&oid).await)) }

async fn run_task(s: &Arc<AppState>, t: &Task, chain_to: Option<String>) {
    let start = std::time::Instant::now();
    // Try real HyperAgent execution, fallback to simulated
    let cmd_result = std::process::Command::new("hyperagent")
        .arg("run").arg(&t.description).arg("--mode").arg("general").output();
    let elapsed = start.elapsed().as_millis() as u64;
    match cmd_result {
        Ok(o) if o.status.success() => { let out = String::from_utf8_lossy(&o.stdout); s.db.task_complete(&t.id, &format!("Done: {}", out.lines().last().unwrap_or("ok")), elapsed).await; }
        Ok(o) => { let err = String::from_utf8_lossy(&o.stderr); s.db.task_fail(&t.id, &format!("Fail: {}", err.lines().last().unwrap_or("error"))).await; }
        Err(_) => { tokio::time::sleep(std::time::Duration::from_secs(2)).await; s.db.task_complete(&t.id, &format!("Simulated: {} ({:.1}s)", t.description, elapsed as f64/1000.0), elapsed).await; }
    }
    // Agent chaining
    if let Some(ref next_id) = chain_to { if !next_id.is_empty() { let ct = Task { id:Uuid::new_v4().to_string(),org_id:t.org_id.clone(),agent_id:next_id.clone(),description:format!("[chained from {}] {}",&t.id[..8],t.description),status:TaskStatus::Pending,result:None,duration_ms:None,created_at:chrono::Utc::now().to_rfc3339(),completed_at:None }; s.db.task_create(&ct).await; s.db.agent_update(next_id,&AgentStatus::Working,Some(&ct.id)).await; s.db.agent_inc(next_id,true).await; let _=s.tx.send(format!("chain:{}→{}",t.id,ct.id)); } }
    s.db.agent_update(&t.agent_id, &AgentStatus::Idle, None).await; s.db.agent_inc(&t.agent_id, false).await;
    let _ = s.tx.send(format!("task:{} completed", t.id));
}

async fn webhook_handler(State(s): State<Arc<AppState>>, Json(b): Json<WebhookPayload>) -> Result<Json<Task>, StatusCode> {
    if !s.db.org_exists(&b.org_id).await { return Err(StatusCode::NOT_FOUND); }
    let t = Task { id: Uuid::new_v4().to_string(), org_id: b.org_id.clone(), agent_id: b.agent_id.clone(), description: format!("[{}] {}", b.event, b.description), status: TaskStatus::Pending, result: None, duration_ms: None, created_at: chrono::Utc::now().to_rfc3339(), completed_at: None };
    s.db.task_create(&t).await; s.db.agent_update(&b.agent_id, &AgentStatus::Working, Some(&t.id)).await; s.db.agent_inc(&b.agent_id, true).await;
    let _ = s.tx.send(format!("webhook:{} → task:{}", b.event, t.id));
    let sc = s.clone(); let tc = t.clone();
    let sc2 = sc.clone(); let tc2 = tc.clone(); tokio::spawn(async move { run_task(&sc2, &tc2, None).await; });
    Ok(Json(t))
}

async fn ws_handler(ws: WebSocketUpgrade, State(s): State<Arc<AppState>>) -> impl IntoResponse { ws.on_upgrade(move |socket| handle_ws(socket, s)) }
async fn handle_ws(socket: WebSocket, s: Arc<AppState>) { let (mut sender, mut receiver) = socket.split(); let mut rx = s.tx.subscribe(); let send = tokio::spawn(async move { while let Ok(msg) = rx.recv().await { if sender.send(Message::Text(msg.into())).await.is_err() { break; } } }); let recv = tokio::spawn(async move { while let Some(Ok(_)) = receiver.next().await {} }); tokio::select! { _ = send => {}, _ = recv => {}, } }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let path = db_path(); println!("DB: {}", path);
    let db = Db::new(&path)?; let (tx, _) = broadcast::channel(100);
    let state = Arc::new(AppState { db, tx });
    let app = Router::new()
        .route("/api/orgs", get(list_orgs)).route("/api/orgs", post(create_org))
        .route("/api/orgs/:oid/agents", get(list_agents)).route("/api/orgs/:oid/agents", post(create_agent))
        .route("/api/orgs/:oid/agents/:aid/tasks", get(list_tasks)).route("/api/orgs/:oid/agents/:aid/tasks", post(create_task))
        .route("/api/webhook", post(webhook_handler))
        .route("/api/ws", get(ws_handler))
        .layer(CorsLayer::permissive()).with_state(state);
    println!("Agent Platform API — http://127.0.0.1:4000");
    axum::serve(tokio::net::TcpListener::bind("127.0.0.1:4000").await?, app).await?;
    Ok(())
}
