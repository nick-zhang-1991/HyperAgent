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
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

use crate::{i18n, llm::Message};

/// Shared application state
struct AppState {
    configs: Vec<crate::llm::pool::ProviderConfig>,
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
        config.model.as_deref().unwrap_or("gpt-4o"),
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

pub async fn start_server(port: u16, host: &str) -> anyhow::Result<()> {
    let config = crate::config::Config::load()
        .map_err(|e| anyhow::anyhow!("Failed to load config: {e}"))?;

    let state = Arc::new(AppState {
        configs: config.llm.providers,
        sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
    });

    let app = Router::new()
        .route("/api/chat", post(chat_handler))
        .route("/api/health", get(health_handler))
        .route("/api/sessions", get(sessions_handler))
        .route("/api/share/{token}", get(share_handler))
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
