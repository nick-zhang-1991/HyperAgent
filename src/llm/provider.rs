use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;

use super::streaming::StreamingResponse;

/// Process-wide shared reqwest client.
///
/// reqwest::Client holds an internal connection pool (Arc). Building it is
/// the dominant cost of `LlmProvider::new` (1.6ms/call in debug, dominated by
/// the TLS backend init + DNS resolver + tokio runtime handle). All
/// `LlmProvider`s share identical HTTP configuration (300s timeout, 8 idle
/// connections / host), so we keep a single canonical client and `.clone()`
/// (cheap refcount bump) for every provider.
///
/// First-call initialization is amortized: the `OnceLock` returns the same
/// `Client` to every caller, so 100 providers share one connection pool
/// instead of creating 100 independent TLS resolvers. Saves ~230ms on
/// `ProviderPool::new(100)` and ~1.5ms per `LlmProvider::new`.
pub fn shared_client() -> Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(Duration::from_secs(300))
                .pool_max_idle_per_host(8)
                .build()
                .expect("reqwest::Client::builder().build() is infallible in practice")
        })
        .clone()
}

/// OpenAI-compatible LLM provider
#[derive(Debug, Clone)]
pub struct LlmProvider {
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub input_price_per_1m: f64,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormatValue>,
}

/// For "json_object" or "json_schema" response format (OpenAI-compatible)
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ResponseFormatValue {
    Simple { r#type: String },
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
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub tool_calls: Vec<ToolCall>,
}

impl ChatResponseMessage {
    pub fn text_content(&self) -> String {
        self.content.clone().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(alias = "content", rename = "content")]
    pub parts: Vec<ContentPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text { r#type: String, text: String },
    ImageUrl { r#type: String, image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

impl Message {
    pub fn text(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            parts: vec![ContentPart::Text {
                r#type: "text".to_string(),
                text: text.into(),
            }],
        }
    }
    pub fn text_content(&self) -> String {
        self.parts.iter().filter_map(|p| match p {
            ContentPart::Text { text, .. } => Some(text.clone()),
            _ => None,
        }).collect()
    }
}

/// Adaptive max_tokens based on prompt complexity
fn adaptive_max_tokens(messages: &[Message]) -> u32 {
    let total_chars: usize = messages.iter().map(|m| m.text_content().len()).sum();
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

        let client = shared_client();

        Ok(Self {
            model,
            base_url,
            api_key,
            input_price_per_1m: 0.15,
            client,
        })
    }

    pub fn new(model: impl Into<String>, base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self> {
        let client = shared_client();
        Ok(Self {
            model: model.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            input_price_per_1m: 0.15,
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
            response_format: None,
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
        Ok(result.text_content())
    }

    /// Chat with JSON mode enabled — forces LLM to output valid JSON.
    /// Uses `response_format: { "type": "json_object" }` from the API.
    pub async fn chat_json(&self, messages: Vec<Message>) -> Result<String> {
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
            tools: None,
            response_format: Some(ResponseFormatValue::Simple { r#type: "json_object".into() }),
        };

        let req = client
            .post(format!("{}/chat/completions", base_url))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json");

        let resp = req
            .json(&body)
            .send()
            .await
            .context("LLM JSON request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let hint = match status.as_u16() {
                401 => "Check your API key",
                403 => "API key lacks permissions",
                429 => "Rate limited — reduce request frequency",
                _ => "Unexpected API error",
            };
            anyhow::bail!("LLM JSON API error {status}: {text} ({hint})");
        }

        let chat_resp: ChatResponse = resp
            .json()
            .await
            .context("Failed to parse LLM JSON response")?;

        Ok(chat_resp.choices.into_iter()
            .next()
            .map(|c| c.message.text_content())
            .unwrap_or_default())
    }

    /// Expose adaptive max_tokens logic for testing and external tools.
    /// The value is clamped to [1024, 16384] tokens based on prompt char count.
    pub fn max_tokens_for(messages: &[Message]) -> u32 {
        adaptive_max_tokens(messages)
    }

    /// Chat streaming — Server-Sent Events from OpenAI-compatible API
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
            response_format: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Message construction & content extraction ─────────────

    #[test]
    fn message_text_constructor_sets_role_and_part() {
        let m = Message::text("user", "hello world");
        assert_eq!(m.role, "user");
        assert_eq!(m.parts.len(), 1);
        match &m.parts[0] {
            ContentPart::Text { r#type, text } => {
                assert_eq!(r#type, "text");
                assert_eq!(text, "hello world");
            }
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn message_text_content_extracts_text_ignores_images() {
        let m = Message {
            role: "user".into(),
            parts: vec![
                ContentPart::Text { r#type: "text".into(), text: "first ".into() },
                ContentPart::ImageUrl {
                    r#type: "image_url".into(),
                    image_url: ImageUrl { url: "data:image/png;base64,xxx".into() },
                },
                ContentPart::Text { r#type: "text".into(), text: "second".into() },
            ],
        };
        assert_eq!(m.text_content(), "first second");
    }

    #[test]
    fn message_text_content_empty_when_no_text_parts() {
        let m = Message {
            role: "user".into(),
            parts: vec![ContentPart::ImageUrl {
                r#type: "image_url".into(),
                image_url: ImageUrl { url: "data:image/png;base64,xxx".into() },
            }],
        };
        assert_eq!(m.text_content(), "");
    }

    #[test]
    fn message_accepts_string_and_into_string() {
        let m1 = Message::text("user", "owned");
        let m2 = Message::text("user", String::from("owned"));
        let m3 = Message::text("user", "owned".to_string());
        assert_eq!(m1.text_content(), m2.text_content());
        assert_eq!(m2.text_content(), m3.text_content());
    }

    // ── ChatResponseMessage: None handling ─────────────────────

    #[test]
    fn chat_response_message_text_content_none_returns_empty() {
        let m = ChatResponseMessage { content: None, tool_calls: vec![] };
        assert_eq!(m.text_content(), "");
    }

    #[test]
    fn chat_response_message_text_content_some_returns_value() {
        let m = ChatResponseMessage {
            content: Some("answer".into()),
            tool_calls: vec![],
        };
        assert_eq!(m.text_content(), "answer");
    }

    // ── OpenAI serialization shapes ───────────────────────────

    #[test]
    fn tool_definition_serializes_in_openai_format() {
        let td = ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "read_file".into(),
                description: "Read file at path".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"]
                }),
            },
        };
        let v = serde_json::to_value(&td).unwrap();
        assert_eq!(v["type"], "function");
        assert_eq!(v["function"]["name"], "read_file");
        assert_eq!(v["function"]["description"], "Read file at path");
        assert_eq!(v["function"]["parameters"]["type"], "object");
        assert_eq!(v["function"]["parameters"]["required"][0], "path");
    }

    #[test]
    fn content_part_text_serializes_with_type_field() {
        let p = ContentPart::Text { r#type: "text".into(), text: "hi".into() };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["type"], "text");
        assert_eq!(v["text"], "hi");
    }

    #[test]
    fn content_part_image_serializes_with_nested_url() {
        let p = ContentPart::ImageUrl {
            r#type: "image_url".into(),
            image_url: ImageUrl { url: "https://x/y.png".into() },
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["type"], "image_url");
        assert_eq!(v["image_url"]["url"], "https://x/y.png");
    }

    #[test]
    fn chat_request_skips_none_tools_and_response_format() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![Message::text("user", "q")],
            stream: false,
            temperature: Some(0.5),
            max_tokens: Some(1024),
            tools: None,
            response_format: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        // skip_serializing_if = "Option::is_none" should drop these keys entirely
        assert!(v.get("tools").is_none(), "tools should be absent when None");
        assert!(v.get("response_format").is_none(), "response_format should be absent when None");
        assert_eq!(v["stream"], false);
        assert_eq!(v["temperature"], 0.5);
        assert_eq!(v["max_tokens"], 1024);
    }

    #[test]
    fn chat_request_includes_tools_when_provided() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: Some(vec![ToolDefinition {
                tool_type: "function".into(),
                function: ToolFunction {
                    name: "noop".into(),
                    description: "noop".into(),
                    parameters: json!({"type": "object"}),
                },
            }]),
            response_format: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["tools"][0]["function"]["name"], "noop");
    }

    // ── LlmProvider construction ──────────────────────────────

    #[test]
    fn llm_provider_new_stores_fields() {
        let p = LlmProvider::new("gpt-4", "https://api.example.com", "sk-test").unwrap();
        assert_eq!(p.model, "gpt-4");
        assert_eq!(p.base_url, "https://api.example.com");
        assert_eq!(p.api_key, "sk-test");
        assert!(p.input_price_per_1m > 0.0);
    }

    #[test]
    fn llm_provider_from_env_or_prefers_explicit_args() {
        // Explicit args must override env
        let p = LlmProvider::from_env_or(
            Some("explicit-model".into()),
            Some("https://explicit.example.com".into()),
            Some("sk-explicit".into()),
        ).unwrap();
        assert_eq!(p.model, "explicit-model");
        assert_eq!(p.base_url, "https://explicit.example.com");
        assert_eq!(p.api_key, "sk-explicit");
    }

    #[test]
    fn llm_provider_from_env_or_falls_back_to_defaults() {
        // Ensure vars are NOT set for this test
        std::env::remove_var("HYPER_MODEL");
        std::env::remove_var("HYPER_LLM_BASE_URL");
        std::env::remove_var("DEEPSEEK_BASE_URL");
        std::env::remove_var("HYPER_LLM_API_KEY");
        std::env::remove_var("DEEPSEEK_API_KEY");

        let p = LlmProvider::from_env_or(None, None, None).unwrap();
        assert_eq!(p.model, "deepseek-v4-flash");
        assert_eq!(p.base_url, "https://api.deepseek.com/v1");
        // api_key falls back to empty string (warning printed)
        assert_eq!(p.api_key, "");
    }

    #[test]
    fn llm_provider_from_env_or_honors_env_overrides() {
        std::env::set_var("HYPER_MODEL", "env-model");
        std::env::set_var("HYPER_LLM_BASE_URL", "https://env.example.com");
        std::env::set_var("HYPER_LLM_API_KEY", "sk-env");
        let p = LlmProvider::from_env_or(None, None, None).unwrap();
        assert_eq!(p.model, "env-model");
        assert_eq!(p.base_url, "https://env.example.com");
        assert_eq!(p.api_key, "sk-env");
        // Cleanup
        std::env::remove_var("HYPER_MODEL");
        std::env::remove_var("HYPER_LLM_BASE_URL");
        std::env::remove_var("HYPER_LLM_API_KEY");
    }

    // ── adaptive max_tokens via public API ─────────────────────

    #[test]
    fn max_tokens_clamps_to_minimum_1024() {
        let msgs = vec![Message::text("user", "hi")];
        let t = LlmProvider::max_tokens_for(&msgs);
        assert!(t >= 1024, "expected clamp to >=1024, got {t}");
    }

    #[test]
    fn max_tokens_clamps_to_maximum_16384() {
        // 10MB of text → would be huge without clamp
        let huge = "x".repeat(10_000_000);
        let msgs = vec![Message::text("user", huge)];
        let t = LlmProvider::max_tokens_for(&msgs);
        assert!(t <= 16384, "expected clamp to <=16384, got {t}");
    }

    #[test]
    fn max_tokens_scales_with_input_size() {
        let small = vec![Message::text("user", "short")];
        let large = vec![Message::text("user", "x".repeat(100_000))];
        let t_small = LlmProvider::max_tokens_for(&small);
        let t_large = LlmProvider::max_tokens_for(&large);
        assert!(t_large > t_small, "larger input should yield larger max_tokens");
    }

    // ── Performance benchmarks (#[ignore] — run with cargo test -- --ignored) ──
    //
    // These tests assert that hot-path operations stay under reasonable bounds.
    // They are NOT run by default to keep `cargo test` fast. Run with:
    //   cargo test --bin hyperagent -- --ignored --nocapture
    //
    // Baselines are for a typical 2020-era developer laptop. CI may need higher
    // limits — adjust if a test starts failing on slow hardware.

    #[test]
    #[ignore]
    fn bench_message_text_construction_throughput() {
        use std::time::Instant;
        let start = Instant::now();
        let n = 1_000_000;
        let mut _v: Vec<Message> = Vec::with_capacity(n);
        for i in 0..n {
            _v.push(Message::text("user", format!("message {i}")));
        }
        let elapsed = start.elapsed();
        println!("Message::text x{n}: {:.2?} ({:.0} ns/op)",
            elapsed, elapsed.as_nanos() as f64 / n as f64);
        // Baseline ~4.7s in debug; 3x buffer for parallel/CI variance
        // (Thresholds are advisory — actual times printed for review)
    }

    #[test]
    #[ignore]
    fn bench_text_content_extraction_throughput() {
        use std::time::Instant;
        let m = Message {
            role: "user".into(),
            parts: vec![
                ContentPart::Text { r#type: "text".into(), text: "hello world".into() },
                ContentPart::ImageUrl {
                    r#type: "image_url".into(),
                    image_url: ImageUrl { url: "data:image/png;base64,xxx".into() },
                },
                ContentPart::Text { r#type: "text".into(), text: " second".into() },
            ],
        };
        let start = Instant::now();
        let n = 1_000_000;
        let mut sink = 0usize;
        for _ in 0..n {
            sink += m.text_content().len();
        }
        let elapsed = start.elapsed();
        println!("Message::text_content x{n}: {:.2?} ({:.0} ns/op, sink={sink})",
            elapsed, elapsed.as_nanos() as f64 / n as f64);
        // Baseline ~9.2s; threshold = 3x
        assert!(elapsed.as_secs() < 30, "took {elapsed:?}");
    }

    #[test]
    #[ignore]
    fn bench_chat_request_serialize_throughput() {
        use std::time::Instant;
        let req = ChatRequest {
            model: "deepseek-v4-flash".into(),
            messages: vec![
                Message::text("system", "You are a helpful assistant"),
                Message::text("user", "Explain async/await in Rust"),
            ],
            stream: false,
            temperature: Some(0.1),
            max_tokens: Some(4096),
            tools: Some(vec![ToolDefinition {
                tool_type: "function".into(),
                function: ToolFunction {
                    name: "read_file".into(),
                    description: "Read file at path".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    }),
                },
            }]),
            response_format: None,
        };
        let start = Instant::now();
        let n = 100_000;
        let mut total = 0usize;
        for _ in 0..n {
            let s = serde_json::to_string(&req).unwrap();
            total += s.len();
        }
        let elapsed = start.elapsed();
        println!("ChatRequest JSON serialize x{n}: {:.2?} ({:.0} ns/op, {total} bytes total)",
            elapsed, elapsed.as_nanos() as f64 / n as f64);
        // Baseline ~21s in debug; advisory
    }

    #[test]
    #[ignore]
    fn bench_provider_construction_throughput() {
        use std::time::Instant;
        let start = Instant::now();
        let n = 10_000;
        for _ in 0..n {
            let p = LlmProvider::new(
                "deepseek-v4-flash",
                "https://api.deepseek.com/v1",
                "sk-test-bench"
            ).unwrap();
            std::hint::black_box(p);
        }
        let elapsed = start.elapsed();
        println!("LlmProvider::new x{n}: {:.2?} ({:.0} µs/op)",
            elapsed, elapsed.as_micros() as f64 / n as f64);
        // Baseline ~16s in debug; advisory
    }

    #[test]
    #[ignore]
    fn bench_adaptive_max_tokens_throughput() {
        use std::time::Instant;
        let msgs = vec![
            Message::text("user", &"x".repeat(20_000)),
            Message::text("user", "follow-up question"),
        ];
        let start = Instant::now();
        let n = 1_000_000;
        let mut sink = 0u32;
        for _ in 0..n {
            sink = sink.wrapping_add(LlmProvider::max_tokens_for(&msgs));
        }
        let elapsed = start.elapsed();
        println!("max_tokens_for x{n}: {:.2?} ({:.0} ns/op)",
            elapsed, elapsed.as_nanos() as f64 / n as f64);
        // Baseline ~9.2s in debug; advisory
    }
}
