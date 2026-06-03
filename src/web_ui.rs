//! Kanban Web UI — lightweight async HTTP server
//!
//! Serves a REST API + embedded HTML/JS frontend.
//! Uses tokio::net::TcpListener (no extra dependency).
//!
//! # Usage
//! ```bash
//! hyper kanban web --port 8080
//! ```

use crate::kanban::{KanbanBoard, Priority};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// Start the kanban web server
pub async fn serve(port: u16, project_root: &PathBuf) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr).await?;
    let board = Arc::new(Mutex::new(KanbanBoard::new(project_root, 3)));

    println!("   🖥️  Kanban Web UI started");
    println!("   🌐 Open: http://{addr}");
    println!("   📁 Project: {}", project_root.display());
    println!("   Press Ctrl+C to stop\n");

    // Try to open browser
    let addr_clone = addr.clone();
    tokio::spawn(async move {
        // Small delay to ensure server is ready
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open")
                .arg(&format!("http://{addr_clone}"))
                .spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(&format!("http://{addr_clone}"))
                .spawn();
        }
    });

    loop {
        let (mut stream, peer) = listener.accept().await?;
        let board = board.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(&mut stream, board, peer.to_string()).await {
                eprintln!("   ⚠️  {e}");
            }
        });
    }
}

async fn handle_connection(
    stream: &mut tokio::net::TcpStream,
    board: Arc<Mutex<KanbanBoard>>,
    _peer: String,
) -> Result<()> {
    let (reader, mut writer) = stream.split();
    let mut buf_reader = BufReader::new(reader);
    let mut request_line = String::new();
    buf_reader.read_line(&mut request_line).await?;

    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0];
    let path = parts[1];

    // Read headers to find Content-Length
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        buf_reader.read_line(&mut header).await?;
        if header.trim().is_empty() {
            break;
        }
        if header.to_lowercase().starts_with("content-length:") {
            if let Some(val) = header.split(':').nth(1) {
                content_length = val.trim().parse().unwrap_or(0);
            }
        }
    }

    // Read body if present
    let mut body = Vec::new();
    if content_length > 0 {
        body.resize(content_length, 0);
        buf_reader.read_exact(&mut body).await?;
    }
    let body_str = String::from_utf8_lossy(&body);

    // Route
    let (status_line, content_type, response_body) = match (method, path) {
        ("GET", "/" | "/index.html") => html_response().await,
        ("GET", "/api/cards") => api_list_cards(&board).await,
        ("POST", "/api/cards") => api_add_card(&board, &body_str).await,
        ("GET", "/api/cards/stream") => api_cards_stream(&board).await,
        ("POST", "/api/cards/move") => api_move_card(&board, &body_str).await,
        ("DELETE", path) if path.starts_with("/api/cards/") => {
            let id = path.trim_start_matches("/api/cards/");
            api_delete_card(&board, id).await
        }
        ("GET", path) if path.starts_with("/api/cards/") => {
            let id = path.trim_start_matches("/api/cards/");
            api_get_card(&board, id).await
        }
        _ => (
            "HTTP/1.1 404 NOT FOUND".to_string(),
            "text/plain".to_string(),
            "Not Found".to_string(),
        ),
    };

    let response = format!(
        "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{response_body}",
        response_body.len()
    );
    writer.write_all(response.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

async fn html_response() -> (String, String, String) {
    let html = include_str!("../web/kanban.html");
    ("HTTP/1.1 200 OK".to_string(), "text/html; charset=utf-8".to_string(), html.to_string())
}

async fn api_list_cards(board: &Arc<Mutex<KanbanBoard>>) -> (String, String, String) {
    let b = board.lock().await;
    let cards = b.list_cards().await;
    let json = serde_json::to_string_pretty(&cards).unwrap_or_else(|_| "[]".to_string());
    (
        "HTTP/1.1 200 OK".to_string(),
        "application/json".to_string(),
        json,
    )
}

async fn api_get_card(board: &Arc<Mutex<KanbanBoard>>, id: &str) -> (String, String, String) {
    let b = board.lock().await;
    match b.get_card(id).await {
        Some(card) => {
            let json = serde_json::to_string_pretty(&card).unwrap_or_else(|_| "{}".to_string());
            ("HTTP/1.1 200 OK".to_string(), "application/json".to_string(), json)
        }
        None => (
            "HTTP/1.1 404 NOT FOUND".to_string(),
            "application/json".to_string(),
            format!(r#"{{"error":"Card '{id}' not found"}}"#),
        ),
    }
}

async fn api_add_card(
    board: &Arc<Mutex<KanbanBoard>>,
    body: &str,
) -> (String, String, String) {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return (
                "HTTP/1.1 400 BAD REQUEST".to_string(),
                "application/json".to_string(),
                format!(r#"{{"error":"Invalid JSON: {e}"}}"#),
            );
        }
    };

    let title = parsed["title"].as_str().unwrap_or("Untitled");
    let description = parsed["description"].as_str().unwrap_or("");
    let priority_str = parsed["priority"].as_str().unwrap_or("medium");
    let mode = parsed["mode"].as_str().unwrap_or("code");
    let deps: Vec<String> = parsed["dependencies"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let tags: Vec<String> = parsed["tags"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let priority = match priority_str {
        "critical" => Priority::Critical,
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };

    let b = board.lock().await;
    let card_id = b.add_card(title, description, priority, mode, deps, tags).await;

    match b.get_card(&card_id).await {
        Some(card) => {
            let json = serde_json::to_string_pretty(&card).unwrap_or_else(|_| "{}".to_string());
            (
                "HTTP/1.1 201 CREATED".to_string(),
                "application/json".to_string(),
                json,
            )
        }
        None => (
            "HTTP/1.1 500 ERROR".to_string(),
            "application/json".to_string(),
            r#"{"error":"Card created but not found"}"#.to_string(),
        ),
    }
}

async fn api_move_card(
    board: &Arc<Mutex<KanbanBoard>>,
    body: &str,
) -> (String, String, String) {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return (
                "HTTP/1.1 400 BAD REQUEST".to_string(),
                "application/json".to_string(),
                format!(r#"{{"error":"Invalid JSON: {e}"}}"#),
            );
        }
    };

    let id = parsed["id"].as_str().unwrap_or("");
    let action = parsed["action"].as_str().unwrap_or("");

    if id.is_empty() || action.is_empty() {
        return (
            "HTTP/1.1 400 BAD REQUEST".to_string(),
            "application/json".to_string(),
            r#"{"error":"'id' and 'action' required"}"#.to_string(),
        );
    }

    let b = board.lock().await;
    let result = match action {
        "block" => b.block_card(id).await,
        "fail" => b.fail_card(id, "Manually failed via web UI").await,
        "start" => {
            match b.get_card(id).await {
                Some(card) => {
                    let agent_id = format!("web-agent-{}", &card.id[..8]);
                    let worktree = std::path::PathBuf::from(".hyper/worktrees").join(id);
                    b.start_card(id, &agent_id, worktree).await
                }
                None => {
                    return (
                        "HTTP/1.1 404 NOT FOUND".to_string(),
                        "application/json".to_string(),
                        format!(r#"{{"error":"Card '{id}' not found"}}"#),
                    );
                }
            }
        }
        "complete" => {
            let result = crate::kanban::CardResult {
                summary: "Completed via web UI".to_string(),
                files_changed: vec![],
                tokens_used: 0,
                exit_code: 0,
            };
            b.complete_card(id, result).await
        }
        _ => {
            return (
                "HTTP/1.1 400 BAD REQUEST".to_string(),
                "application/json".to_string(),
                format!(r#"{{"error":"Unknown action: {action}"}}"#),
            );
        }
    };

    match result {
        Ok(()) => (
            "HTTP/1.1 200 OK".to_string(),
            "application/json".to_string(),
            r#"{"status":"ok"}"#.to_string(),
        ),
        Err(e) => (
            "HTTP/1.1 400 BAD REQUEST".to_string(),
            "application/json".to_string(),
            format!(r#"{{"error":"{e}"}}"#),
        ),
    }
}

async fn api_delete_card(
    board: &Arc<Mutex<KanbanBoard>>,
    id: &str,
) -> (String, String, String) {
    let b = board.lock().await;
    let mut cards = b.list_cards().await;
    cards.retain(|c| c.id != id);
    // We can't remove from KanbanBoard directly, so mark as failed
    match b.fail_card(id, "Deleted via web UI").await {
        Ok(()) => (
            "HTTP/1.1 200 OK".to_string(),
            "application/json".to_string(),
            r#"{"status":"deleted"}"#.to_string(),
        ),
        Err(e) => (
            "HTTP/1.1 404 NOT FOUND".to_string(),
            "application/json".to_string(),
            format!(r#"{{"error":"{e}"}}"#),
        ),
    }
}

async fn api_cards_stream(board: &Arc<Mutex<KanbanBoard>>) -> (String, String, String) {
    // Same as list but for SSE-like polling
    api_list_cards(board).await
}
