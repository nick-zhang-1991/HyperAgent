//! Web search tool for HyperAgent
//!
//! Performs web searches using the DuckDuckGo API (no API key needed).
//! Usage: `hyper search <query>`

use anyhow::Result;
use serde::Deserialize;

/// Search result item
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SearchResult {
    title: String,
    link: String,
    snippet: String,
}

/// Perform a web search using DuckDuckGo's instant answer API (free, no key needed)
pub async fn search(query: &str, max_results: usize) -> Result<Vec<(String, String, String)>> {
    let url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        urlencoding(query)
    );

    let client = reqwest::Client::builder()
        .user_agent("HyperAgent/1.0")
        .build()?;

    let resp = client.get(&url).send().await?;
    let body: serde_json::Value = resp.json().await?;

    let mut results = Vec::new();

    // Extract abstract text
    if let Some(abstract_text) = body["AbstractText"].as_str() {
        if !abstract_text.is_empty() {
            let source = body["AbstractSource"].as_str().unwrap_or("DuckDuckGo");
            let url_str = body["AbstractURL"].as_str().unwrap_or("");
            results.push((
                format!("{source} - {abstract_text:.100}"),
                url_str.to_string(),
                abstract_text.to_string(),
            ));
        }
    }

    // Extract related topics
    if let Some(topics) = body["RelatedTopics"].as_array() {
        for topic in topics {
            if results.len() >= max_results {
                break;
            }
            if let Some(text) = topic["Text"].as_str() {
                if let Some(url_str) = topic["FirstURL"].as_str() {
                    results.push((
                        text.to_string(),
                        url_str.to_string(),
                        text.to_string(),
                    ));
                }
            }
        }
    }

    Ok(results)
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

/// Display search results to stdout
pub fn display_results(results: &[(String, String, String)]) {
    if results.is_empty() {
        println!("   No results found.");
        return;
    }

    println!("\n📊 Search Results:");
    println!("{}", "-".repeat(60));
    for (i, (title, url, snippet)) in results.iter().enumerate() {
        println!("  {}. {}", i + 1, title);
        println!("     📎 {}", url);
        println!("     💬 {:.200}", snippet);
        println!();
    }
}
