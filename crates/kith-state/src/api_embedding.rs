//! API-based embedding backend — calls any OpenAI-compatible /v1/embeddings endpoint.
//! Works with: OpenAI, Ollama, vLLM, LiteLLM, Together, etc.
//! Enabled with: cargo build -p kith-state --features api-embeddings

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::embedding::{Embedding, EmbeddingBackend};

pub struct ApiEmbeddingBackend {
    endpoint: String,
    model: String,
    api_key: Option<String>,
    client: Client,
    dimensions: usize,
}

impl ApiEmbeddingBackend {
    /// Create a new API-based embedding backend.
    /// `dimensions` should match the model's output dimensionality.
    pub fn new(
        endpoint: String,
        model: String,
        api_key: Option<String>,
        dimensions: usize,
    ) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            model,
            api_key,
            client: Client::new(),
            dimensions,
        }
    }

    /// Common configurations.
    pub fn ollama(endpoint: &str, model: &str, dimensions: usize) -> Self {
        Self::new(endpoint.into(), model.into(), None, dimensions)
    }

    pub fn openai(model: &str, dimensions: usize) -> Self {
        let api_key = std::env::var("OPENAI_API_KEY").ok();
        Self::new(
            "https://api.openai.com/v1".into(),
            model.into(),
            api_key,
            dimensions,
        )
    }
}

#[async_trait]
impl EmbeddingBackend for ApiEmbeddingBackend {
    async fn embed(&self, text: &str) -> Result<Embedding, String> {
        let url = format!("{}/embeddings", self.endpoint);

        let body = EmbeddingRequest {
            model: self.model.clone(),
            input: text.into(),
        };

        let mut req = self.client.post(&url).json(&body);
        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req
            .send()
            .await
            .map_err(|e| format!("embedding request failed: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("embedding API error HTTP {status}: {body}"));
        }

        let result: EmbeddingResponse = response
            .json()
            .await
            .map_err(|e| format!("failed to parse embedding response: {e}"))?;

        let values = result
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| "no embedding data in response".to_string())?;

        debug!(model = %self.model, dims = values.len(), "embedding computed");

        Ok(Embedding {
            values,
            model_version: self.model.clone(),
        })
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Embedding>, String> {
        // Most APIs support batch input, but for simplicity use sequential.
        // Override if the API supports batch natively.
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_version(&self) -> &str {
        &self.model
    }
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    model: String,
    input: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_constructor() {
        let backend = ApiEmbeddingBackend::ollama("http://localhost:11434/v1", "all-minilm", 384);
        assert_eq!(backend.model, "all-minilm");
        assert_eq!(backend.dimensions(), 384);
        assert_eq!(backend.model_version(), "all-minilm");
        assert!(backend.api_key.is_none());
    }

    #[test]
    fn endpoint_trailing_slash() {
        let backend = ApiEmbeddingBackend::new(
            "http://localhost:8000/v1/".into(),
            "model".into(),
            None,
            768,
        );
        assert_eq!(backend.endpoint, "http://localhost:8000/v1");
    }
}
