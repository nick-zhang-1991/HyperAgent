//! Web search module — allows HyperAgent to search the web for answers.
//!
//! Uses DuckDuckGo Lite HTML endpoint (no API key required, privacy-respecting).
//! Extracts text results from the HTML using lightweight regex parsing.
//!
//! Public API:
//! - [`search`] — submit query, get `Vec<SearchResult>` back
//! - [`display_results`] — print results to stdout in a human-readable format

use anyhow::{Context, Result};
use regex::Regex;
use std::sync::OnceLock;

/// A single web search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Search the web using DuckDuckGo Lite (no API key required).
///
/// `max_results` caps the number of returned entries (DDG Lite usually returns
/// 8-10 results at most, so values above 10 are effectively ignored).
pub async fn search(query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
    let client = crate::llm::provider::shared_client();
    let url = format!("https://lite.duckduckgo.com/lite/?q={}", urlencode(query));

    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .header("User-Agent", "HyperAgent/1.0 (coding agent; +https://github.com/nousresearch/hyperagent)")
        .send()
        .await
        .context("Failed to fetch search results from DuckDuckGo")?;

    let status = resp.status();
    let html = resp.text().await.context("Failed to read response body")?;

    if !status.is_success() {
        anyhow::bail!("DuckDuckGo returned HTTP {status}");
    }

    Ok(extract_results(&html, max_results))
}

/// Print search results to stdout in a human-friendly format.
pub fn display_results(results: &[SearchResult]) {
    if results.is_empty() {
        println!("   No search results found.");
        return;
    }
    for (i, r) in results.iter().enumerate() {
        println!("  {}. {}", i + 1, r.title);
        println!("     {}", r.url);
        println!("     {}", r.snippet);
        println!();
    }
}

// ── Internal helpers ────────────────────────────────────────────

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            other => format!("%{:02X}", other as u8),
        })
        .collect()
}

fn re_script_style() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<script[^>]*>.*?</script>|<style[^>]*>.*?</style>").unwrap())
}

fn re_tags() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^>]+>").unwrap())
}

/// Extract search results from DuckDuckGo Lite HTML.
fn extract_results(html: &str, max: usize) -> Vec<SearchResult> {
    let blocks: Vec<&str> = html.split("<tr class=\"result\"").collect();

    if blocks.len() <= 1 {
        return fallback_results(html, max);
    }

    let mut results = Vec::with_capacity(max.min(10));

    for block in blocks.iter().skip(1) {
        if results.len() >= max {
            break;
        }

        let title = extract_tag_content(block, "a", "result-link")
            .or_else(|| extract_link_text(block))
            .unwrap_or_default();
        let url = extract_href(block).unwrap_or_default();
        let snippet = extract_tag_content(block, "td", "result-snippet")
            .or_else(|| extract_snippet_raw(block))
            .unwrap_or_default();

        if !title.is_empty() {
            results.push(SearchResult { title, url, snippet });
        }
    }

    if results.is_empty() {
        return fallback_results(html, max);
    }
    results
}

/// Fallback: strip HTML tags and return any readable text as a single result.
fn fallback_results(html: &str, _max: usize) -> Vec<SearchResult> {
    let cleaned = re_script_style().replace_all(html, "");
    let cleaned = re_tags().replace_all(&cleaned, "\n");

    let mut lines: Vec<&str> = cleaned
        .split('\n')
        .map(|l| l.trim())
        .filter(|l| l.len() > 30)
        .collect();
    lines.dedup();
    lines.truncate(5);

    let body = if lines.is_empty() {
        "(No structured results extracted.)".to_string()
    } else {
        lines.join("\n")
    };

    vec![SearchResult {
        title: "DuckDuckGo Results".into(),
        url: String::new(),
        snippet: body,
    }]
}

fn extract_tag_content(block: &str, tag: &str, class: &str) -> Option<String> {
    let pattern = format!(r#"<{tag}[^>]*class="[^"]*{}[^"]*"[^>]*>"#, regex::escape(class));
    let re = Regex::new(&pattern).ok()?;
    let tag_open = re.find(block)?;
    let after_open = &block[tag_open.end()..];
    let close_tag = format!("</{tag}>");
    let close_pos = after_open.find(&close_tag)?;
    let content = after_open[..close_pos].trim();
    if content.is_empty() { return None; }
    let cleaned = re_tags().replace_all(content, "");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() { None } else { Some(cleaned.to_string()) }
}

fn extract_link_text(block: &str) -> Option<String> {
    let re = Regex::new(r#"<a[^>]*>([^<]+)</a>"#).ok()?;
    let text = re.captures(block)?.get(1)?.as_str().trim();
    if text.is_empty() { None } else { Some(text.to_string()) }
}

fn extract_href(block: &str) -> Option<String> {
    let re = Regex::new(r#"href="([^"]+)""#).ok()?;
    let href = re.captures(block)?.get(1)?.as_str();
    Some(if href.starts_with("//") { format!("https:{href}") }
         else if href.starts_with('/') { format!("https://lite.duckduckgo.com{href}") }
         else { href.to_string() })
}

fn extract_snippet_raw(block: &str) -> Option<String> {
    let re = Regex::new(r#"</a>\s*<br\s*/?>\s*(.+?)(?:<br|<div|<tr|$)"#).ok()?;
    let text = re.captures(block)?.get(1)?.as_str().trim();
    let cleaned = re_tags().replace_all(text, "");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() { None } else { Some(cleaned.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_return_type() {
        // Verify the API contract: search returns Result<Vec<SearchResult>>
        let _: fn(&str, usize) -> _ = |q, m| {
            let _fut = search(q, m);
        };
    }

    #[test]
    fn test_urlencode() {
        assert_eq!(urlencode("hello world"), "hello+world");
        assert_eq!(urlencode("a/b"), "a%2Fb");
        assert_eq!(urlencode("rust + go"), "rust+%2B+go");
    }

    #[test]
    fn test_extract_results_structured_html() {
        let html = r#"<html><table><tr class="result">
            <td><a class="result-link" href="https://example.com">Example Title</a></td>
            <td class="result-snippet">This is a snippet for testing.</td>
        </tr><tr class="result">
            <td><a class="result-link" href="https://rust-lang.org">Rust Lang</a></td>
            <td class="result-snippet">A language empowering reliable software.</td>
        </tr></table></html>"#;
        let results = extract_results(html, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example Title");
        assert_eq!(results[0].url, "https://example.com");
        assert!(results[0].snippet.contains("snippet"));
        assert_eq!(results[1].title, "Rust Lang");
    }

    #[test]
    fn test_extract_results_limit() {
        let mut html = String::from("<html><table>");
        for i in 0..15 {
            html.push_str(&format!(
                r#"<tr class="result"><td><a class="result-link" href="https://ex{i}.com">Result {i}</a></td><td class="result-snippet">Snippet {i}</td></tr>"#
            ));
        }
        html.push_str("</table></html>");
        let results = extract_results(&html, 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_fallback() {
        let html = "<html><p>HyperAgent is a Rust-based CLI coding agent.</p></html>";
        let results = fallback_results(html, 5);
        assert_eq!(results.len(), 1);
        assert!(results[0].snippet.contains("HyperAgent"));
    }

    #[test]
    fn test_extract_href() {
        assert_eq!(extract_href(r#"<a href="https://x.com">x</a>"#), Some("https://x.com".into()));
        assert_eq!(extract_href(r#"<a href="//cdn.x.com/a.js">x</a>"#), Some("https://cdn.x.com/a.js".into()));
        assert_eq!(extract_href(r#"<a href="/lite/">x</a>"#), Some("https://lite.duckduckgo.com/lite/".into()));
    }

    #[test]
    fn test_display_results_empty() {
        display_results(&[]); // should not panic
    }
}
