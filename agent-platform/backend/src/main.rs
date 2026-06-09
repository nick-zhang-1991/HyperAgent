//! Agent Orchestration Platform — Backend
//! 7 API endpoints + WebSocket real-time events

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization { pub id: String, pub name: String, pub description: String, pub created_at: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent { pub id: String, pub org_id: String, pub name: String, pub role: String, pub description: String, pub status: AgentStatus, pub current_task: Option<String>, pub created_at: String }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus { Idle, Working, Completed, Error }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task { pub id: String, pub org_id: String, pub agent_id: String, pub description: String, pub status: TaskStatus, pub result: Option<String>, pub created_at: String, pub completed_at: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus { Pending, Running, Completed, Failed }

pub struct AppState { orgs: Mutex<HashMap<String, Organization>>, agents: Mutex<HashMap<String, Vec<Agent>>>, tasks: Mutex<HashMap<String, Vec<Task>>>, tx: broadcast::Sender<String> }

#[derive(Deserialize)] struct CreateOrg { name: String, description: String }
#[derive(Deserialize)] struct CreateAgent { name: String, role: String, description: String }
#[derive(Deserialize)] struct CreateTask { description: String }

async fn create_org(State(s): State<Arc<AppState>>, Json(b): Json<CreateOrg>) -> Json<Organization> {
    let o = Organization { id: Uuid::new_v4().to_string(), name: b.name, description: b.description, created_at: chrono::Utc::now().to_rfc3339() };
    s.orgs.lock().await.insert(o.id.clone(), o.clone());
    let _ = s.tx.send(format!("org:{} created", o.id));
    Json(o)
}

async fn list_orgs(State(s): State<Arc<AppState>>) -> Json<Vec<Organization>> {
    Json(s.orgs.lock().await.values().cloned().collect())
}

async fn create_agent(State(s): State<Arc<AppState>>, Path(oid): Path<String>, Json(b): Json<CreateAgent>) -> Result<Json<Agent>, StatusCode> {
    if !s.orgs.lock().await.contains_key(&oid) { return Err(StatusCode::NOT_FOUND); }
    let a = Agent { id: Uuid::new_v4().to_string(), org_id: oid.clone(), name: b.name, role: b.role, description: b.description, status: AgentStatus::Idle, current_task: None, created_at: chrono::Utc::now().to_rfc3339() };
    s.agents.lock().await.entry(oid.clone()).or_default().push(a.clone());
    let _ = s.tx.send(format!("agent:{} created in org:{}", a.id, oid));
    Ok(Json(a))
}

async fn list_agents(State(s): State<Arc<AppState>>, Path(oid): Path<String>) -> Result<Json<Vec<Agent>>, StatusCode> {
    Ok(Json(s.agents.lock().await.get(&oid).cloned().unwrap_or_default()))
}

async fn create_task(State(s): State<Arc<AppState>>, Path((oid, aid)): Path<(String, String)>, Json(b): Json<CreateTask>) -> Result<Json<Task>, StatusCode> {
    let t = Task { id: Uuid::new_v4().to_string(), org_id: oid.clone(), agent_id: aid.clone(), description: b.description, status: TaskStatus::Pending, result: None, created_at: chrono::Utc::now().to_rfc3339(), completed_at: None };
    s.tasks.lock().await.entry(oid.clone()).or_default().push(t.clone());
    if let Some(agents) = s.agents.lock().await.get_mut(&oid) {
        if let Some(a) = agents.iter_mut().find(|a| a.id == aid) { a.status = AgentStatus::Working; a.current_task = Some(t.id.clone()); }
    }
    let _ = s.tx.send(format!("task:{} created for agent:{}", t.id, aid));
    let sc = s.clone(); let tc = t.clone();
    tokio::spawn(async move { tokio::time::sleep(std::time::Duration::from_secs(2)).await; execute(&sc, &tc).await; });
    Ok(Json(t))
}

async fn list_tasks(State(s): State<Arc<AppState>>, Path((oid, _aid)): Path<(String, String)>) -> Result<Json<Vec<Task>>, StatusCode> {
    Ok(Json(s.tasks.lock().await.get(&oid).cloned().unwrap_or_default()))
}

async fn execute(s: &Arc<AppState>, t: &Task) {
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    if let Some(tasks) = s.tasks.lock().await.get_mut(&t.org_id) {
        if let Some(task) = tasks.iter_mut().find(|x| x.id == t.id) {
            task.status = TaskStatus::Completed;
            task.result = Some(format!("Task '{}' completed successfully.", t.description));
            task.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }
    }
    if let Some(agents) = s.agents.lock().await.get_mut(&t.org_id) {
        if let Some(a) = agents.iter_mut().find(|x| x.id == t.agent_id) { a.status = AgentStatus::Idle; a.current_task = None; }
    }
    let _ = s.tx.send(format!("task:{} completed", t.id));
}

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let (tx, _) = broadcast::channel(100);
    let state = Arc::new(AppState { orgs: Mutex::new(HashMap::new()), agents: Mutex::new(HashMap::new()), tasks: Mutex::new(HashMap::new()), tx });
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
