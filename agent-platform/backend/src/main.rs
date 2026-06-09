//! Agent Orchestration Platform — Backend
//!
//! API:
//!   POST   /api/orgs                    — Create organization
//!   GET    /api/orgs                    — List organizations
//!   POST   /api/orgs/:id/agents         — Create agent with role
//!   GET    /api/orgs/:id/agents         — List agents
//!   POST   /api/orgs/:id/agents/:aid/tasks  — Assign task to agent
//!   GET    /api/orgs/:id/agents/:aid/tasks  — List agent tasks
//!   GET    /api/ws                      — WebSocket real-time status

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

// ── Models ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub org_id: String,
    pub name: String,
    pub role: String,          // "Code Reviewer", "DevOps Engineer", "Security Auditor"...
    pub description: String,   // Natural language role description
    pub status: AgentStatus,
    pub current_task: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Idle,
    Working,
    Completed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub org_id: String,
    pub agent_id: String,
    pub description: String,   // Natural language task description
    pub status: TaskStatus,
    pub result: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

// ── State ──

pub struct AppState {
    orgs: Mutex<HashMap<String, Organization>>,
    agents: Mutex<HashMap<String, Vec<Agent>>>,
    tasks: Mutex<HashMap<String, Vec<Task>>>,
    tx: broadcast::Sender<String>,
}

// ── Handlers ──

async fn create_org(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateOrgRequest>,
) -> Json<Organization> {
    let org = Organization {
        id: Uuid::new_v4().to_string(),
        name: body.name,
        description: body.description,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    state.orgs.lock().await.insert(org.id.clone(), org.clone());
    let _ = state.tx.send(format!("event: org_created, id: {}", org.id));
    Json(org)
}

async fn list_orgs(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<Organization>> {
    let orgs = state.orgs.lock().await;
    Json(orgs.values().cloned().collect())
}

async fn create_agent(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<String>,
    Json(body): Json<CreateAgentRequest>,
) -> Result<Json<Agent>, StatusCode> {
    if !state.orgs.lock().await.contains_key(&org_id) {
        return Err(StatusCode::NOT_FOUND);
    }

    let agent = Agent {
        id: Uuid::new_v4().to_string(),
        org_id: org_id.clone(),
        name: body.name,
        role: body.role,
        description: body.description,
        status: AgentStatus::Idle,
        current_task: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    state.agents.lock().await
        .entry(org_id.clone())
        .or_default()
        .push(agent.clone());

    let _ = state.tx.send(format!("event: agent_created, org: {}, agent: {}", org_id, agent.id));
    Ok(Json(agent))
}

async fn list_agents(
    State(state): State<Arc<AppState>>,
    Path(org_id): Path<String>,
) -> Result<Json<Vec<Agent>>, StatusCode> {
    let agents = state.agents.lock().await;
    Ok(Json(agents.get(&org_id).cloned().unwrap_or_default()))
}

async fn create_task(
    State(state): State<Arc<AppState>>,
    Path((org_id, agent_id)): Path<(String, String)>,
    Json(body): Json<CreateTaskRequest>,
) -> Result<Json<Task>, StatusCode> {
    let task = Task {
        id: Uuid::new_v4().to_string(),
        org_id: org_id.clone(),
        agent_id: agent_id.clone(),
        description: body.description,
        status: TaskStatus::Pending,
        result: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        completed_at: None,
    };

    state.tasks.lock().await
        .entry(org_id.clone())
        .or_default()
        .push(task.clone());

    // Update agent status
    if let Some(agents) = state.agents.lock().await.get_mut(&org_id) {
        if let Some(agent) = agents.iter_mut().find(|a| a.id == agent_id) {
            agent.status = AgentStatus::Working;
            agent.current_task = Some(task.id.clone());
        }
    }

    let _ = state.tx.send(format!("event: task_created, task: {}", task.id));

    // Simulate task execution (in real version, this calls hyperagent CLI)
    let state_clone = state.clone();
    let task_clone = task.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        execute_task(&state_clone, &task_clone).await;
    });

    Ok(Json(task))
}

async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Path((org_id, _agent_id)): Path<(String, String)>,
) -> Result<Json<Vec<Task>>, StatusCode> {
    let tasks = state.tasks.lock().await;
    Ok(Json(tasks.get(&org_id).cloned().unwrap_or_default()))
}

async fn execute_task(state: &Arc<AppState>, task: &Task) {
    // Simulate task execution. In production, call:
    // Command::new("hyperagent").arg("run").arg(&task.description).output()
    
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    if let Some(tasks) = state.tasks.lock().await.get_mut(&task.org_id) {
        if let Some(t) = tasks.iter_mut().find(|t| t.id == task.id) {
            t.status = TaskStatus::Completed;
            t.result = Some(format!("Task '{}' completed successfully.", task.description));
            t.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }
    }

    if let Some(agents) = state.agents.lock().await.get_mut(&task.org_id) {
        if let Some(agent) = agents.iter_mut().find(|a| a.id == task.agent_id) {
            agent.status = AgentStatus::Idle;
            agent.current_task = None;
        }
    }

    let _ = state.tx.send(format!("event: task_completed, task: {}", task.id));
}

// ── WebSocket for real-time updates ──

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(_)) = receiver.next().await {}
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

// ── Request types ──

#[derive(Deserialize)]
struct CreateOrgRequest { name: String, description: String }

#[derive(Deserialize)]
struct CreateAgentRequest { name: String, role: String, description: String }

#[derive(Deserialize)]
struct CreateTaskRequest { description: String }

// ── Server ──

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let (tx, _) = broadcast::channel(100);
    let state = Arc::new(AppState {
        orgs: Mutex::new(HashMap::new()),
        agents: Mutex::new(HashMap::new()),
        tasks: Mutex::new(HashMap::new()),
        tx,
    });

    let app = Router::new()
        .route("/api/orgs", post(create_org).get(list_orgs))
        .route("/api/orgs/{id}/agents", post(create_agent).get(list_agents))
        .route("/api/orgs/{id}/agents/{aid}/tasks", post(create_task).get(list_tasks))
        .route("/api/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("🚀 Agent Platform API starting on http://127.0.0.1:4000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:4000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
