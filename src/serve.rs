//! HTTP server for HyperAgent Web UI with SSE streaming.
//!
//! ```
//! hyper serve --port 3000
//! ```

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

use crate::{i18n, llm::Message};

/// Shared application state
struct AppState {
    configs: Vec<crate::router::ProviderConfig>,
    sessions: Arc<Mutex<std::collections::HashMap<String, Vec<Message>>>>,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    session_id: Option<String>,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
    session_id: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

const WEB_SYSTEM_PROMPT: &str = r#"You are HyperAgent, a coding assistant. You help users with:
- Code generation, review, and explanation
- Project analysis and architecture advice
- Debugging and troubleshooting
- General programming questions

Keep responses concise and actionable. When showing code, format it properly.
"#;

/// POST /api/chat — SSE streaming response
async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> axum::response::Response {
    let session_id = req.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let messages = {
        let sessions = state.sessions.lock().await;
        sessions.get(&session_id).cloned().unwrap_or_default()
    };

    let mut req_messages = messages.clone();
    if req_messages.is_empty() {
        req_messages.push(Message::text("system", WEB_SYSTEM_PROMPT));
    }
    req_messages.push(Message::text("user", &req.message));

    // Take first available config
    let config = match state.configs.first() {
        Some(c) => c.clone(),
        None => {
            return Json(ErrorResponse {
                error: "No LLM provider configured".into(),
            }).into_response();
        }
    };

    // Create provider for this request
    let provider = match crate::llm::LlmProvider::new(
        config.default_model.as_str(),
        &config.base_url,
        &config.api_key,
    ) {
        Ok(p) => p,
        Err(e) => {
            return Json(ErrorResponse {
                error: format!("Provider init failed: {e}"),
            }).into_response();
        }
    };

    // Try streaming
    match provider.chat_stream(req_messages.clone()).await {
        Ok(streaming) => {
            let sessions = state.sessions.clone();
            let sid = session_id.clone();
            use futures::StreamExt;

            let body = axum::body::Body::from_stream(async_stream::stream! {
                let mut full = String::new();
                // Pin the stream so we can poll it
                tokio::pin!(streaming);
                while let Some(chunk) = streaming.next().await {
                    // chunk is String (StreamingResponse::Item = String)
                    full.push_str(&chunk);
                    yield Ok::<_, std::convert::Infallible>(
                        axum::body::Bytes::from(format!("data: {}\n\n", chunk))
                    );
                }
                // Save to session
                let mut sessions = sessions.lock().await;
                let mut updated = req_messages.clone();
                updated.push(Message::text("assistant", &full));
                // Trim large history
                let mut to_store = vec![updated[0].clone()]; // system prompt
                let keep = if updated.len() > 42 { updated.split_off(updated.len() - 40) } else { updated[1..].to_vec() };
                to_store.extend(keep);
                sessions.insert(sid, to_store);
                yield Ok(axum::body::Bytes::from("data: [DONE]\n\n"));
            });

            axum::response::Response::builder()
                .header("Content-Type", "text/event-stream")
                .header("Cache-Control", "no-cache")
                .header("Connection", "keep-alive")
                .body(body)
                .unwrap()
        }
        Err(e) => {
            // Fallback to non-streaming
            match provider.chat(req_messages.clone()).await {
                Ok(response) => {
                    let mut sessions = state.sessions.lock().await;
                    let mut updated = req_messages;
                    updated.push(Message::text("assistant", &response));
                    sessions.insert(session_id.clone(), updated);

                    Json(ChatResponse { response, session_id }).into_response()
                }
                Err(e2) => Json(ErrorResponse {
                    error: format!("LLM error: {e2} (stream: {e})"),
                }).into_response(),
            }
        }
    }
}

/// GET /api/health
async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "name": "HyperAgent"
    }))
}

/// GET /api/sessions
async fn sessions_handler(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let sessions = state.sessions.lock().await;
    let count = sessions.len();
    let ids: Vec<&String> = sessions.keys().take(20).collect();
    Json(serde_json::json!({
        "count": count,
        "sessions": ids,
    }))
}

/// GET /api/share/:token
async fn share_handler(
    axum::extract::Path(token): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let share_store = crate::session::ShareStore::new();
    match share_store.resolve(&token) {
        Some(session_id) => {
            let sessions = crate::session::SessionManager::new()
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                    error: format!("Session error: {e}")
                })))?;
            match sessions.load(&session_id) {
                Ok(session) => {
                    let messages: Vec<serde_json::Value> = session.messages.iter().map(|m| {
                        serde_json::json!({"role": m.role, "content": m.content})
                    }).collect();
                    Ok(Json(serde_json::json!({
                        "session_id": session_id,
                        "summary": session.summary_line(),
                        "messages": messages,
                        "token": token,
                    })))
                }
                Err(e) => Err((StatusCode::NOT_FOUND, Json(ErrorResponse {
                    error: format!("Session not found: {e}")
                }))),
            }
        }
        None => Err((StatusCode::NOT_FOUND, Json(ErrorResponse {
            error: "Invalid or expired token".into()
        }))),
    }
}


/// POST /api/analyze — run deep code analysis
async fn analyze_handler(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let project = state.sessions.lock().await;
    let root = std::env::current_dir().unwrap_or_default();
    match crate::analyze::run(&root, false) {
        Ok(issues) => Json(serde_json::json!({
            "ok": true,
            "total": issues.len(),
            "critical": issues.iter().filter(|i| matches!(i.severity, crate::analyze::Severity::Critical)).count(),
            "issues": issues,
        })),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e.to_string()})),
    }
}

/// POST /api/eval — run self-evaluation benchmark
async fn eval_handler() -> Json<serde_json::Value> {
    let root = std::env::current_dir().unwrap_or_default();
    match crate::eval::run_all(&root, true) {
        Ok(report) => Json(serde_json::json!({
            "ok": true,
            "total": report.total,
            "passed": report.passed,
            "pass_rate": report.pass_rate,
        })),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e.to_string()})),
    }
}

/// GET /api/memory/global — list global memories
async fn global_memory_handler(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let store = crate::memory::SqliteMemoryStore::new(
        &std::path::PathBuf::from(".hyper/memory.db")
    );
    match store {
        Ok(store) => {
            let mgr = crate::memory::MemoryManager::new(Box::new(store), "web");
            match mgr.global_search("*", 20) {
                Ok(items) => Json(serde_json::json!({
                    "ok": true,
                    "count": items.len(),
                    "memories": items.iter().map(|m| serde_json::json!({
                        "content": m.content,
                        "importance": m.importance,
                    })).collect::<Vec<_>>(),
                })),
                Err(e) => Json(serde_json::json!({"ok": false, "error": e.to_string()})),
            }
        }
        Err(e) => Json(serde_json::json!({"ok": false, "error": e.to_string()})),
    }
}

/// POST /api/feedback — record user feedback
#[derive(Deserialize)]
struct FeedbackRequest {
    kind: String, // "good" or "bad"
    reason: String,
}

async fn feedback_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FeedbackRequest>,
) -> Json<serde_json::Value> {
    let store = crate::memory::SqliteMemoryStore::new(
        &std::path::PathBuf::from(".hyper/memory.db")
    );
    match store {
        Ok(store) => {
            let mgr = crate::memory::MemoryManager::new(Box::new(store), "web")
                .with_global_promote(0.5);
            let text = if req.kind == "good" {
                format!("✅ [FEEDBACK] User approved: {}", req.reason)
            } else {
                format!("❌ [CORRECTION] User corrected: {}. DO NOT repeat this.", req.reason)
            };
            match mgr.remember(&text, crate::memory::MemoryType::Correction) {
                Ok(_) => Json(serde_json::json!({"ok": true})),
                Err(e) => Json(serde_json::json!({"ok": false, "error": e.to_string()})),
            }
        }
        Err(e) => Json(serde_json::json!({"ok": false, "error": e.to_string()})),
    }
}

pub async fn start_server(port: u16, host: &str) -> anyhow::Result<()> {
    let config = crate::config::AppConfig::load();

    let state = Arc::new(AppState {
        configs: config.providers,
        sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
    });

    let app = Router::new()
        .route("/api/chat", post(chat_handler))
        .route("/api/health", get(health_handler))
        .route("/api/sessions", get(sessions_handler))
        .route("/api/share/{token}", get(share_handler))
        .route("/api/analyze", post(analyze_handler))
        .route("/api/eval", post(eval_handler))
        .route("/api/memory/global", get(global_memory_handler))
        .route("/api/feedback", post(feedback_handler))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    println!("🌐 HyperAgent Web API starting on http://{}", addr);
    println!("   {}", i18n::t("serve_chat"));
    println!("   {}", i18n::t("serve_health"));
    println!("   {}", i18n::t("serve_sessions"));
    println!("   {}", i18n::t("serve_share"));
    println!();
    println!("   {}", i18n::t("serve_web_ui"));
    println!("   {}", i18n::t("serve_desktop"));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_health_handler() {
        let resp = health_handler().await;
        let v = resp.0;
        assert_eq!(v["status"], "ok");
        assert_eq!(v["name"], "HyperAgent");
        assert!(v["version"].is_string());
    }

    #[tokio::test]
    async fn test_sessions_handler_empty() {
        let state = Arc::new(AppState {
            configs: vec![],
            sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        });
        let resp = sessions_handler(axum::extract::State(state)).await;
        let v = resp.0;
        assert_eq!(v["count"], 0);
        assert!(v["sessions"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_sessions_handler_with_sessions() {
        let mut map = std::collections::HashMap::new();
        map.insert("s1".to_string(), vec![]);
        map.insert("s2".to_string(), vec![]);
        let state = Arc::new(AppState {
            configs: vec![],
            sessions: Arc::new(Mutex::new(map)),
        });
        let resp = sessions_handler(axum::extract::State(state)).await;
        let v = resp.0;
        assert_eq!(v["count"], 2);
        assert_eq!(v["sessions"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_error_response_construction() {
        let e = ErrorResponse { error: "test error".into() };
        assert_eq!(e.error, "test error");
    }

    #[test]
    fn test_error_response_serialize() {
        let e = ErrorResponse { error: "boom".into() };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"error\":\"boom\""));
    }

    #[test]
    fn test_chat_request_construction() {
        let r = ChatRequest {
            message: "hello".into(),
            session_id: Some("s1".into()),
        };
        assert_eq!(r.message, "hello");
        assert_eq!(r.session_id, Some("s1".into()));
    }

    #[test]
    fn test_chat_request_no_session() {
        let r = ChatRequest {
            message: "hi".into(),
            session_id: None,
        };
        assert!(r.session_id.is_none());
    }

    #[test]
    fn test_chat_request_deserialize() {
        let json = r#"{"message": "test", "session_id": "abc"}"#;
        let r: ChatRequest = serde_json::from_str(json).unwrap();
        assert_eq!(r.message, "test");
        assert_eq!(r.session_id, Some("abc".into()));
    }

    #[test]
    fn test_chat_response_serialize() {
        let r = ChatResponse {
            response: "hello".into(),
            session_id: "s1".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"response\":\"hello\""));
        assert!(json.contains("\"session_id\":\"s1\""));
    }

    #[test]
    fn test_feedback_request_construction() {
        let r = FeedbackRequest {
            kind: "good".into(),
            reason: "Great job".into(),
        };
        assert_eq!(r.kind, "good");
    }

    #[test]
    fn test_feedback_request_deserialize_bad() {
        let json = r#"{"kind": "bad", "reason": "wrong answer"}"#;
        let r: FeedbackRequest = serde_json::from_str(json).unwrap();
        assert_eq!(r.kind, "bad");
        assert_eq!(r.reason, "wrong answer");
    }

    #[test]
    fn test_app_state_construction() {
        let state = AppState {
            configs: vec![],
            sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };
        assert_eq!(state.configs.len(), 0);
    }
}
