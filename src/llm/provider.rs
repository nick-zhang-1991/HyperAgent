use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::streaming::StreamingResponse;

/// OpenAI-compatible LLM provider
#[derive(Debug, Clone)]
pub struct LlmProvider {
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    client: Client,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
}

/// OpenAI-compatible tool/function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A tool call returned by the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// Response from an LLM call that may include tool calls
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    pub message: ChatResponseMessage,
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponseMessage {
    pub content: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Adaptive max_tokens based on prompt complexity
fn adaptive_max_tokens(messages: &[Message]) -> u32 {
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    let estimated = (total_chars / 4 * 3 / 10).clamp(1024, 16384);
    estimated as u32
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ChatChunk {
    choices: Vec<Choice>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Choice {
    delta: Delta,
    finish_reason: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
}

#[allow(dead_code)]
/// Retry with exponential backoff for transient LLM API errors
async fn retry_chat<F, Fut>(f: F, max_retries: u32) -> Result<String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let mut attempt = 0u32;
    loop {
        match f().await {
            Ok(r) => return Ok(r),
            Err(e) => {
                let err_str = e.to_string();
                let is_retryable = err_str.contains("5")
                    || err_str.contains("429")
                    || err_str.contains("timeout")
                    || err_str.contains("Connection")
                    || err_str.contains("eof")
                    || err_str.contains("reset");
                if !is_retryable || attempt >= max_retries {
                    return Err(e);
                }
                attempt += 1;
                let delay = Duration::from_millis(500 * 2u64.pow(attempt.saturating_sub(1)));
                eprintln!(
                    "   ⚠️  LLM call failed (attempt {attempt}/{max_retries}), retrying in {}ms: {err_str}",
                    delay.as_millis()
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

impl LlmProvider {
    pub fn from_env_or(
        model: Option<String>,
        base_url: Option<String>,
        api_key: Option<String>,
    ) -> Result<Self> {
        let model = model.unwrap_or_else(|| {
            std::env::var("HYPER_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".to_string())
        });

        let base_url = base_url.unwrap_or_else(|| {
            std::env::var("HYPER_LLM_BASE_URL")
                .or_else(|_| std::env::var("DEEPSEEK_BASE_URL"))
                .unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string())
        });

        let api_key = api_key.unwrap_or_else(|| {
            std::env::var("HYPER_LLM_API_KEY")
                .or_else(|_| std::env::var("DEEPSEEK_API_KEY"))
                .unwrap_or_else(|_| {
                    eprintln!("⚠️  No API key found. Set HYPER_LLM_API_KEY or DEEPSEEK_API_KEY");
                    String::new()
                })
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(8)
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            model,
            base_url,
            api_key,
            client,
        })
    }

    pub fn new(model: impl Into<String>, base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(8)
            .build()?;
        Ok(Self {
            model: model.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            client,
        })
    }

    /// Chat completion with tool-calling support
    /// Returns the response message which may contain tool_calls
    pub async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<ChatResponseMessage> {
        let model = self.model.clone();
        let base_url = self.base_url.trim_end_matches('/').to_string();
        let api_key = self.api_key.clone();
        let client = self.client.clone();
        let max_tokens = adaptive_max_tokens(&messages);

        let body = ChatRequest {
            model,
            messages,
            stream: false,
            temperature: Some(0.1),
            max_tokens: Some(max_tokens),
            tools,
        };

        let req = client
            .post(format!("{}/chat/completions", base_url))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header("x-requires-prompt-cache", "true");

        let resp = req
            .json(&body)
            .send()
            .await
            .context("LLM request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let hint = match status.as_u16() {
                401 => "Check your API key",
                403 => "API key lacks permissions",
                429 => "Rate limited — reduce request frequency",
                500..=599 => "Server error, try again later",
                _ => "Unexpected response",
            };
            anyhow::bail!("LLM API error {status}: {text}\n   Hint: {hint}");
        }

        let data: ChatResponse = resp.json().await?;
        Ok(data.choices.into_iter().next()
            .map(|c| c.message)
            .unwrap_or_else(|| ChatResponseMessage {
                content: None,
                tool_calls: vec![],
            }))
    }

    pub async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        let result = self.chat_with_tools(messages, None).await?;
        Ok(result.content.unwrap_or_default())
    }

    pub async fn chat_stream(
        &self,
        messages: Vec<Message>,
    ) -> Result<StreamingResponse> {
        let body = ChatRequest {
            model: self.model.clone(),
            messages,
            stream: true,
            temperature: Some(0.1),
            max_tokens: Some(16384),
            tools: None,
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url.trim_end_matches('/')))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("LLM streaming request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM API error {status}: {text}");
        }

        Ok(StreamingResponse::new(resp))
    }
}
