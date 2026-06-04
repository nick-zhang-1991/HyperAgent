//! Vision & Multi-modal support — image analysis via LLM vision APIs
//!
//! Supports:
//! - Screenshot analysis (understand what's on screen)
//! - Image file analysis (diagrams, UI mockups, handwritten notes)
//! - Base64 image encoding for LLM vision API
//!
//! Integrates with computer_use screenshot capabilities for a
//! "see → understand → act" loop.

use anyhow::{anyhow, Result};
use base64::Engine;
use std::path::Path;

/// Analyze an image file using the LLM's vision capabilities
pub fn analyze_image(
    image_path: &Path,
    prompt: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<String> {
    // Read and encode image to base64
    let image_data = std::fs::read(image_path)
        .map_err(|e| anyhow!("Failed to read image {}: {e}", image_path.display()))?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(&image_data);

    // Detect mime type from extension
    let mime = match image_path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    };

    let data_url = format!("data:{mime};base64,{encoded}");

    // Build the vision API request body
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {"type": "image_url", "image_url": {"url": data_url}}
                ]
            }
        ],
        "max_tokens": 4096,
    });

    // Send request
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| anyhow!("Vision API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow!("Vision API returned {status}: {text}"));
    }

    let json: serde_json::Value = resp
        .json()
        .map_err(|e| anyhow!("Failed to parse vision response: {e}"))?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("(no content in response)")
        .to_string();

    Ok(content)
}

/// Take a screenshot and analyze it
pub fn analyze_screenshot(prompt: &str, api_key: &str, base_url: &str, model: &str) -> Result<String> {
    // Use computer_use to take screenshot
    let shot_path = std::path::PathBuf::from("/tmp/hyper-vision-shot.png");
    let result = crate::computer_use::ComputerUse::screenshot(Some(&shot_path));
    if !result.success {
        return Err(anyhow!("Screenshot failed: {}", result.message));
    }
    analyze_image(&shot_path, prompt, api_key, base_url, model)
}

/// Encode an image to base64 data URL (for use in system prompts)
pub fn image_to_data_url(path: &Path) -> Result<String> {
    let data = std::fs::read(path)
        .map_err(|e| anyhow!("Failed to read {path:?}: {e}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Ok(format!("data:{mime};base64,{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_image_to_data_url_png() {
        // Create a minimal valid PNG (1x1 pixel)
        let png_data = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
            0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41,
            0x54, 0x08, 0xD7, 0x63, 0x60, 0x60, 0x60, 0x00,
            0x00, 0x00, 0x04, 0x00, 0x01, 0x27, 0x34, 0x27,
            0x0E, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
            0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let path = std::env::temp_dir().join("test-1px.png");
        std::fs::write(&path, &png_data).unwrap();
        let url = image_to_data_url(&path).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_image_to_data_url_nonexistent() {
        let result = image_to_data_url(&PathBuf::from("/nonexistent/test.png"));
        assert!(result.is_err());
    }

    #[test]
    fn test_analyze_image_missing_file() {
        let result = analyze_image(
            &PathBuf::from("/nonexistent.png"),
            "What's in this image?",
            "fake-key",
            "http://localhost:11434/v1",
            "llava",
        );
        assert!(result.is_err());
    }
}
