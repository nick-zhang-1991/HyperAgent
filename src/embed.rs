//! Embedding Provider — interface + implementations for text embeddings
//!
//! Supports:
//! - Ollama (local) — uses the ollama embeddings API via blocking HTTP
//! - Mock (for testing)
//!
//! Embeddings enhance code search with semantic understanding,
//! complementing the existing PageRank-based search.

use anyhow::Result;
use std::sync::Arc;

/// A text embedding provider
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a batch of texts. Returns a Vec of embedding vectors.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    /// Get the dimension of embeddings produced by this provider
    fn dimension(&self) -> usize;
}

/// Static dispatch: returns a boxed provider based on config
pub fn create_provider(config: &str) -> Option<Arc<dyn EmbeddingProvider>> {
    // config format: "ollama:model:url" or "mock:dim"
    if config.starts_with("ollama:") {
        let parts: Vec<&str> = config.splitn(3, ':').skip(1).collect();
        if parts.len() >= 1 {
            let model = parts[0];
            let url = parts.get(1).unwrap_or(&"http://localhost:11434");
            return Some(Arc::new(OllamaEmbedder::new(url, model)));
        }
    }
    if config.starts_with("mock:") {
        let dim: usize = config.split(':').nth(1).and_then(|s| s.parse().ok()).unwrap_or(4);
        return Some(Arc::new(MockEmbedder::new(dim)));
    }
    None
}

/// Ollama embedding provider (blocking HTTP)
pub struct OllamaEmbedder {
    url: String,
    model: String,
    dimension: usize,
    client: reqwest::blocking::Client,
}

impl OllamaEmbedder {
    pub fn new(url: &str, model: &str) -> Self {
        Self {
            url: url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dimension: match model {
                "nomic-embed-text" => 768,
                "mxbai-embed-large" => 1024,
                "llama3.2" => 3072,
                _ => 768,
            },
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl EmbeddingProvider for OllamaEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            let resp = self.client
                .post(format!("{}/api/embeddings", self.url))
                .json(&serde_json::json!({
                    "model": self.model,
                    "prompt": text,
                }))
                .send()?;

            let body: serde_json::Value = resp.json()?;
            let embedding: Vec<f32> = body["embedding"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("No embedding in response: {:?}", body))?
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();
            results.push(embedding);
        }
        Ok(results)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

/// Mock embedder for testing — deterministic pseudo-embeddings
pub struct MockEmbedder {
    dimension: usize,
}

impl MockEmbedder {
    pub fn new(dimension: usize) -> Self {
        Self { dimension }
    }
}

impl EmbeddingProvider for MockEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            let hash: u64 = text.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
            let mut vec = Vec::with_capacity(self.dimension);
            for i in 0..self.dimension {
                let val = ((hash.wrapping_mul(i as u64 + 1)) % 1000) as f32 / 1000.0;
                vec.push(val - 0.5);
            }
            results.push(vec);
        }
        Ok(results)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

/// Cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot / (norm_a * norm_b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_embedder() {
        let e = MockEmbedder::new(4);
        let vecs = e.embed(&["hello".into(), "world".into()]).unwrap();
        assert_eq!(vecs.len(), 2);
        assert_eq!(vecs[0].len(), 4);
        let vecs2 = e.embed(&["hello".into()]).unwrap();
        assert_eq!(vecs[0], vecs2[0]);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
        let c = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &c) - (-1.0)).abs() < 1e-6);
    }
}
