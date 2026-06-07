#![allow(unused)]

use futures::StreamExt;
use reqwest::Response;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

/// Streaming response from LLM API
pub struct StreamingResponse {
    rx: mpsc::Receiver<String>,
}

impl StreamingResponse {
    pub fn new(resp: Response) -> Self {
        let (tx, rx) = mpsc::channel::<String>(128);

        tokio::spawn(async move {
            let mut stream = resp.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        // Process SSE lines
                        while let Some(line_end) = buffer.find('\n') {
                            let line = buffer[..line_end].trim().to_string();
                            buffer = buffer[line_end + 1..].to_string();

                            if let Some(data) = line.strip_prefix("data: ") {
                                if data == "[DONE]" {
                                    break;
                                }
                                // Try to parse JSON chunk
                                if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                                    if let Some(content) = chunk["choices"][0]["delta"]["content"].as_str() {
                                        let _ = tx.send(content.to_string()).await;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Stream error: {e}");
                        break;
                    }
                }
            }
        });

        Self { rx }
    }

    #[allow(dead_code)]
    /// Collect all streamed content into a single string
    pub async fn collect_string(self) -> String {
        let mut result = String::new();
        let mut rx = self.rx;
        while let Some(chunk) = rx.recv().await {
            result.push_str(&chunk);
        }
        result
    }

    /// Iterate over chunks as they arrive
    pub fn into_receiver(self) -> mpsc::Receiver<String> {
        self.rx
    }
}

/// A stream wrapper for async iteration
#[allow(dead_code)]
pub struct TextStream {
    rx: mpsc::Receiver<String>,
}

impl TextStream {
    pub fn new(rx: mpsc::Receiver<String>) -> Self {
        Self { rx }
    }
}

impl futures::Stream for TextStream {
    type Item = String;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

impl futures::Stream for StreamingResponse {
    type Item = String;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}
