//! HTTP server for HyperAgent Web UI
//!
//! ```
//! hyper serve --port 3000
//! ```

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Shared application state
struct AppState {
    // Will hold agent configuration
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

/// POST /api/chat — send a message to the agent
async fn chat_handler(
    State(_state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session_id = req.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // For now, echo back. Phase 2: pipe through LLM provider.
    let response = format!(
        "HyperAgent Web API received: \"{}\"\n\nThis is a stub. Real implementation will pipe through the LLM provider.",
        req.message
    );

    Ok(Json(ChatResponse {
        response,
        session_id,
    }))
}

/// GET /api/health — health check
async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "name": "HyperAgent"
    }))
}

/// Start the HTTP server
pub async fn start_server(port: u16, host: &str) -> anyhow::Result<()> {
    let state = Arc::new(AppState {});

    let app = Router::new()
        .route("/api/chat", post(chat_handler))
        .route("/api/health", axum::routing::get(health_handler))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    println!("🌐 HyperAgent Web API starting on http://{}", addr);
    println!("   POST /api/chat    — Send message");
    println!("   GET  /api/health  — Health check");
    println!();
    println!("   📖 Web UI: run `npm run dev` in gui/ for development");
    println!("   🖥️  Desktop: run `cd gui && pnpm tauri dev` for Tauri app");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
