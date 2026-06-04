//! Browser Automation — Chrome DevTools Protocol (CDP) integration
//!
//! Launches Chrome/Chromium in headless mode with remote debugging,
//! then controls it via CDP over WebSocket.
//!
//! Capabilities:
//! - Navigate to URLs
//! - Take screenshots (base64 or file)
//! - Click elements by CSS selector
//! - Get page text content
//! - Execute JavaScript
//!
//! Usage:
//!   /browser open <url>    — Open URL and capture screenshot
//!   /browser screenshot    — Take screenshot of current page
//!   /browser click <sel>   — Click element by CSS selector
//!   /browser source        — Get page text content
//!   /browser eval <js>     — Execute JavaScript
//!   /browser close         — Close browser session
//!   /browser status        — Show connection info

use anyhow::{Context, Result};
use base64::Engine;
use futures::stream::StreamExt;
use futures::sink::SinkExt;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// Finding Chrome/Chromium on the system
fn find_chrome() -> Option<PathBuf> {
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/snap/bin/chromium",
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    ];
    for path in &candidates {
        let p = std::path::Path::new(path);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    // Try `which` command
    for name in &["google-chrome", "chromium", "chromium-browser", "chrome"] {
        if let Ok(output) = Command::new("which").arg(name).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }
    None
}

/// CDP Message
#[derive(Debug)]
struct CdpMessage {
    id: u64,
    method: String,
    params: Value,
}

/// CDP Response from the browser
#[derive(Debug)]
struct CdpResponse {
    id: u64,
    result: Value,
}

/// Browser connection state
pub struct Browser {
    chrome_process: Option<Child>,
    ws_url: String,
    target_id: String,
    // WebSocket sender - used to send CDP commands
    sender: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<CdpMessage>>>>,
    // Response receiver
    responses: Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value>>>>>,
    next_id: Arc<Mutex<u64>>,
    port: u16,
    chrome_path: PathBuf,
    data_dir: PathBuf,
    connected: bool,
}

impl Browser {
    /// Launch Chrome and connect to CDP
    pub async fn launch(port: u16) -> Result<Self> {
        let chrome_path = find_chrome()
            .ok_or_else(|| anyhow::anyhow!("Chrome/Chromium not found. Install Chrome or set PATH."))?;

        let data_dir = std::env::temp_dir().join(format!("hyper-chrome-{}", std::process::id()));

        println!("   🚀 Launching Chrome (headless) on port {}...", port);
        let chrome_process = Command::new(&chrome_path)
            .args([
                "--headless=new",
                "--disable-gpu",
                "--no-sandbox",
                "--disable-dev-shm-usage",
                &format!("--remote-debugging-port={}", port),
                &format!("--user-data-dir={}", data_dir.display()),
                "--window-size=1280,720",
                "--disable-extensions",
                "--mute-audio",
                "--disable-sync",
                "--no-first-run",
                "--disable-default-apps",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to launch Chrome")?;

        // Wait for Chrome to start listening
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        // Get WebSocket URL from devtools endpoint
        let ws_url = Self::get_ws_url(port).await?;

        let sender = Arc::new(Mutex::new(None));
        let responses: Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(Mutex::new(1u64));

        let mut browser = Self {
            chrome_process: Some(chrome_process),
            ws_url: ws_url.clone(),
            target_id: String::new(),
            sender,
            responses,
            next_id,
            port,
            chrome_path,
            data_dir,
            connected: false,
        };

        // Connect WebSocket
        browser.connect_ws().await?;
        browser.connected = true;

        // Enable necessary CDP domains
        browser.send_command("Page.enable", serde_json::json!({})).await?;
        browser.send_command("DOM.enable", serde_json::json!({})).await?;
        browser.send_command("Runtime.enable", serde_json::json!({})).await?;

        println!("   ✅ Browser ready ({})", browser.ws_url);
        Ok(browser)
    }

    /// Get the WebSocket debug URL from Chrome's HTTP endpoint
    async fn get_ws_url(port: u16) -> Result<String> {
        let url = format!("http://localhost:{}/json/version", port);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        for attempt in 0..10 {
            match client.get(&url).send().await {
                Ok(resp) => {
                    let body: Value = resp.json().await?;
                    if let Some(ws_url) = body["webSocketDebuggerUrl"].as_str() {
                        return Ok(ws_url.to_string());
                    }
                }
                Err(_) => {}
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        anyhow::bail!("Chrome did not start debugging endpoint within 5s")
    }

    /// Connect WebSocket to Chrome's CDP endpoint
    async fn connect_ws(&self) -> Result<()> {
        let url = self.ws_url.clone();
        let (ws_stream, _) = connect_async(&url)
            .await
            .context("Failed to connect to Chrome WebSocket")?;

        let (mut write, mut read) = ws_stream.split();

        let responses = self.responses.clone();
        let sender = self.sender.clone();

        // Spawn reader task
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                            if let Some(id) = parsed["id"].as_u64() {
                                let mut map = responses.lock().unwrap();
                                if let Some(tx) = map.remove(&id) {
                                    let _ = tx.send(Ok(parsed["result"].clone()));
                                }
                            }
                            // Ignore event messages (no "id" field)
                        }
                    }
                    _ => {} // Ping/pong handled by tungstenite internally
                }
            }
        });

        // Spawn writer task
        let sender_clone = sender.clone();
        tokio::spawn(async move {
            let mut write = write;
            let mut rx = {
                let mut lock = sender_clone.lock().unwrap();
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<CdpMessage>();
                *lock = Some(tx);
                rx
            };

            while let Some(msg) = rx.recv().await {
                let payload = serde_json::json!({
                    "id": msg.id,
                    "method": msg.method,
                    "params": msg.params,
                });
                let text = serde_json::to_string(&payload).unwrap_or_default();
                if let Err(e) = write.send(Message::Text(text)).await {
                    eprintln!("   ⚠️  CDP send error: {e}");
                    break;
                }
            }
        });

        Ok(())
    }

    /// Send a CDP command and wait for response
    pub async fn send_command(&self, method: &str, params: Value) -> Result<Value> {
        let mut next_id = self.next_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        drop(next_id);

        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Value>>();
        {
            let mut map = self.responses.lock().unwrap();
            map.insert(id, tx);
        }

        {
            let sender_opt = self.sender.lock().unwrap();
            if let Some(ref sender) = *sender_opt {
                let msg = CdpMessage {
                    id,
                    method: method.to_string(),
                    params,
                };
                let _ = sender.send(msg);
            } else {
                anyhow::bail!("CDP sender not initialized");
            }
        }

        // Wait for response with timeout
        tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .context("CDP command timed out")?
            .context("CDP response channel closed")?
    }

    /// Navigate to a URL
    pub async fn navigate(&self, url: &str) -> Result<String> {
        let full_url = if !url.starts_with("http://") && !url.starts_with("https://") {
            format!("https://{}", url)
        } else {
            url.to_string()
        };

        println!("   🌐 Navigating to: {}", full_url);
        let result = self
            .send_command("Page.navigate", serde_json::json!({"url": full_url}))
            .await?;

        // Wait for page to load
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

        let frame_id = result["frameId"].as_str().unwrap_or("unknown").to_string();
        Ok(frame_id)
    }

    /// Take a screenshot, return base64 PNG data
    pub async fn screenshot_base64(&self) -> Result<String> {
        let result = self
            .send_command("Page.captureScreenshot", serde_json::json!({"format": "png"}))
            .await?;

        result["data"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("No screenshot data in response"))
    }

    /// Take a screenshot and save to file
    pub async fn screenshot_file(&self, path: &str) -> Result<PathBuf> {
        let base64_data = self.screenshot_base64().await?;
        let png_data = base64::engine::general_purpose::STANDARD
            .decode(&base64_data)
            .context("Failed to decode base64 screenshot")?;

        let save_path = PathBuf::from(path);
        let mut file = std::fs::File::create(&save_path)?;
        file.write_all(&png_data)?;
        Ok(save_path)
    }

    /// Get page text content (without HTML tags)
    pub async fn page_text(&self) -> Result<String> {
        // Use document.body.innerText
        let result = self
            .send_command(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "document.body?.innerText || document.documentElement?.innerText || ''",
                    "returnByValue": true,
                }),
            )
            .await?;

        result["result"]["value"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("No text content"))
    }

    /// Get page HTML
    pub async fn page_html(&self) -> Result<String> {
        let result = self
            .send_command(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "document.documentElement?.outerHTML || ''",
                    "returnByValue": true,
                }),
            )
            .await?;

        result["result"]["value"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("No HTML content"))
    }

    /// Get page title
    pub async fn page_title(&self) -> Result<String> {
        let result = self
            .send_command(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "document.title",
                    "returnByValue": true,
                }),
            )
            .await?;

        result["result"]["value"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("No title"))
    }

    /// Click an element by CSS selector
    pub async fn click(&self, selector: &str) -> Result<()> {
        // First, find the element via DOM.querySelector
        let doc_result = self
            .send_command("DOM.getDocument", serde_json::json!({"depth": 0}))
            .await?;
        let doc_node_id = doc_result["root"]["nodeId"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("No document node"))?;

        let query_result = self
            .send_command(
                "DOM.querySelector",
                serde_json::json!({"nodeId": doc_node_id, "selector": selector}),
            )
            .await?;

        let element_id = query_result["nodeId"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("Element '{selector}' not found"))?;

        // Get the element's box model to find the click position
        let box_result = self
            .send_command(
                "DOM.getBoxModel",
                serde_json::json!({"nodeId": element_id}),
            )
            .await?;

        let content = box_result["model"]["content"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No box model for element"))?;

        if content.len() < 2 {
            anyhow::bail!("Element has no dimensions");
        }

        let x = content[0].as_f64().unwrap_or(0.0);
        let y = content[1].as_f64().unwrap_or(0.0);

        // Click via Input.dispatchMouseEvent
        self.send_command(
            "Input.dispatchMouseEvent",
            serde_json::json!({
                "type": "mousePressed",
                "x": x + 2.0,
                "y": y + 2.0,
                "button": "left",
                "clickCount": 1,
            }),
        )
        .await?;

        self.send_command(
            "Input.dispatchMouseEvent",
            serde_json::json!({
                "type": "mouseReleased",
                "x": x + 2.0,
                "y": y + 2.0,
                "button": "left",
                "clickCount": 1,
            }),
        )
        .await?;

        // Wait a bit for any navigation/update
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(())
    }

    /// Execute JavaScript in the page context
    pub async fn evaluate_js(&self, js: &str) -> Result<Value> {
        let result = self
            .send_command(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": js,
                    "returnByValue": true,
                }),
            )
            .await?;

        Ok(result["result"]["value"].clone())
    }

    /// Check if browser is still connected
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Close the browser
    pub async fn close(&mut self) -> Result<()> {
        self.connected = false;

        // Kill Chrome process
        if let Some(mut child) = self.chrome_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        // Clean up data directory
        let data_dir = self.data_dir.clone();
        tokio::task::spawn_blocking(move || {
            let _ = std::fs::remove_dir_all(&data_dir);
        });

        println!("   🚫 Browser closed");
        Ok(())
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(mut child) = self.chrome_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let data_dir = self.data_dir.clone();
        std::thread::spawn(move || {
            let _ = std::fs::remove_dir_all(&data_dir);
        });
    }
}

/// Manages browser lifecycle and provides REPL-friendly interface
pub struct BrowserManager {
    browser: Option<Browser>,
    port: u16,
}

impl BrowserManager {
    pub fn new() -> Self {
        Self {
            browser: None,
            port: 0,
        }
    }

    /// Get or launch the browser
    pub async fn get_or_launch(&mut self) -> Result<&Browser> {
        if self.browser.is_some() {
            // Check if still alive by pinging
            let result = self.browser.as_ref().unwrap().is_connected();
            if result {
                return Ok(self.browser.as_ref().unwrap());
            }
            // Dead browser — clear and re-launch
            println!("   🔄 Reconnecting browser...");
            self.browser = None;
        }

        self.port = Self::find_free_port(9222);
        let browser = Browser::launch(self.port).await?;
        Ok(self.browser.insert(browser))
    }

    /// Close the browser
    pub async fn close(&mut self) -> Result<()> {
        if let Some(mut browser) = self.browser.take() {
            browser.close().await?;
        }
        Ok(())
    }

    /// Helper: find a free port
    fn find_free_port(start: u16) -> u16 {
        for port in start..start + 100 {
            if is_port_available(port) {
                return port;
            }
        }
        start
    }

    /// Render status
    pub fn status(&self) -> String {
        match &self.browser {
            Some(b) => {
                let port = b.port;
                format!("   🟢 Browser connected (port {port})")
            }
            None => "   ⚪ No browser session active. Use /browser open <url> to start.".to_string(),
        }
    }
}

fn is_port_available(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        std::time::Duration::from_millis(100),
    )
    .is_err()
}
