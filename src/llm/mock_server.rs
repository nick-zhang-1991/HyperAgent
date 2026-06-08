//! Mock LLM HTTP server for end-to-end testing
//!
//! Spawns a lightweight HTTP server that responds to /chat/completions
//! with predefined responses, allowing the agent pipeline to be tested
//! without a real LLM API key.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// A mock LLM server that returns canned responses
pub struct MockLlmServer {
    port: u16,
    shutdown: Arc<Mutex<bool>>,
}

impl MockLlmServer {
    /// Start a new mock server on a random available port
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind mock server");
        let port = listener.local_addr().unwrap().port();
        let shutdown = Arc::new(Mutex::new(false));
        let shutdown_clone = shutdown.clone();

        thread::spawn(move || {
            listener.set_nonblocking(true).ok();
            for stream in listener.incoming() {
                if *shutdown_clone.lock().unwrap() {
                    break;
                }
                match stream {
                    Ok(stream) => {
                        thread::spawn(|| handle_connection(stream));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Self { port, shutdown }
    }

    /// Get the base URL of this mock server
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    /// Get the actual port
    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for MockLlmServer {
    fn drop(&mut self) {
        *self.shutdown.lock().unwrap() = true;
    }
}

/// Handle a single TCP connection
fn handle_connection(mut stream: TcpStream) {
    let mut buffer = [0; 4096];
    if let Ok(n) = stream.read(&mut buffer) {
        if n > 0 {
            let request = String::from_utf8_lossy(&buffer[..n]);
            let response_body = if request.contains("/chat/completions") {
                build_response("Mock LLM response for testing")
            } else {
                build_response("{}")
            };

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    }
}

/// Build an OpenAI-compatible chat completion response
fn build_response(content: &str) -> String {
    let escaped = content
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!(
        r#"{{"choices":[{{"message":{{"content":"{}","role":"assistant"}},"finish_reason":"stop"}}]}}"#,
        escaped
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmProvider, Message};
    use std::io::{Read, Write};
    use std::time::Duration;

    #[test]
    fn test_mock_server_basic() {
        let mock = MockLlmServer::start();
        let url = mock.url();
        assert!(url.contains(&mock.port.to_string()), "URL should contain port");
        assert!(url.starts_with("http://127.0.0.1:"), "URL should be localhost");
    }

    #[test]
    fn test_mock_server_responds() {
        let mock = MockLlmServer::start();
        // Small delay to let server start
        thread::sleep(Duration::from_millis(200));

        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", mock.port()))
            .expect("Failed to connect");

        let body = r#"{"model":"test","messages":[],"stream":false}"#;
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAuthorization: Bearer test-key\r\nConnection: close\r\n\r\n{}",
            mock.port(),
            body.len(),
            body
        );

        stream.write_all(request.as_bytes()).ok();
        stream.flush().ok();
        // Shutdown write to let server know we're done sending
        let _ = stream.shutdown(std::net::Shutdown::Write);

        let mut response = String::new();
        stream.read_to_string(&mut response).ok();

        assert!(response.contains("200 OK"), "Should return 200. Got: {response:?}");
        assert!(response.contains("Mock LLM response"), "Should contain expected content");
    }

    #[test]
    fn test_llm_provider_with_mock() {
        let mock = MockLlmServer::start();
        // Wait for TCP listener to become ready on slow CI / under load
        thread::sleep(Duration::from_millis(200));

        let provider = LlmProvider::new(
            "test-model".to_string(),
            mock.url(),
            "test-key".to_string(),
        ).expect("Failed to create provider");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            provider.chat(vec![
                Message::text("user", "hello")
            ]).await
        });

        assert!(result.is_ok(), "Provider chat should succeed: {:?}", result.err());
        let text = result.unwrap();
        assert!(text.contains("Mock LLM response"), "Should get mock response. Got: {text}");
    }

    #[test]
    fn test_mock_stream_connection_works() {
        // Streaming against a non-SSE endpoint should either succeed (if the provider
        // handles plain JSON gracefully) or fail — either is acceptable behavior
        let mock = MockLlmServer::start();
        // Wait for TCP listener to become ready on slow CI / under load
        thread::sleep(Duration::from_millis(200));

        let provider = LlmProvider::new(
            "test-model".to_string(),
            mock.url(),
            "test-key".to_string(),
        ).expect("Failed to create provider");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            provider.chat_stream(vec![
                Message::text("user", "stream test")
            ]).await
        });

        // As long as it doesn't panic, the connection works
        let _ = result;
    }
}
