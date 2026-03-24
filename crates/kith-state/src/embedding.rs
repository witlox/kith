//! Embedding backend trait + bag-of-words baseline.
//! Real embedding model (API or local) plugs in via the trait.

use async_trait::async_trait;

/// A vector embedding of text.
#[derive(Debug, Clone)]
pub struct Embedding {
    pub values: Vec<f32>,
    pub model_version: String,
}

/// Trait for embedding text into vectors.
#[async_trait]
pub trait EmbeddingBackend: Send + Sync {
    /// Embed a single text. Returns a vector.
    async fn embed(&self, text: &str) -> Result<Embedding, String>;

    /// Embed multiple texts (batch). Default: sequential.
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Embedding>, String> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// Dimensionality of embeddings.
    fn dimensions(&self) -> usize;

    /// Model version string (for INV-DAT-3 consistency check).
    fn model_version(&self) -> &str;
}

/// Bag-of-words embedding — simple baseline, no external model needed.
/// Builds a vocabulary from seen terms, encodes as term-frequency vectors.
/// Good enough for keyword-heavy operational data. Replace with real model later.
pub struct BagOfWordsEmbedder {
    vocabulary: std::sync::Mutex<Vec<String>>,
    max_vocab: usize,
}

impl BagOfWordsEmbedder {
    pub fn new(max_vocab: usize) -> Self {
        Self {
            vocabulary: std::sync::Mutex::new(Vec::new()),
            max_vocab,
        }
    }

    fn tokenize(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
            .filter(|s| s.len() > 1)
            .map(String::from)
            .collect()
    }

    fn get_or_add_index(&self, term: &str) -> Option<usize> {
        let mut vocab = self.vocabulary.lock().unwrap();
        if let Some(pos) = vocab.iter().position(|t| t == term) {
            return Some(pos);
        }
        if vocab.len() < self.max_vocab {
            let idx = vocab.len();
            vocab.push(term.to_string());
            return Some(idx);
        }
        None // vocabulary full
    }
}

impl Default for BagOfWordsEmbedder {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[async_trait]
impl EmbeddingBackend for BagOfWordsEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding, String> {
        let tokens = Self::tokenize(text);
        let dim = self.max_vocab;
        let mut values = vec![0.0f32; dim];

        for token in &tokens {
            if let Some(idx) = self.get_or_add_index(token) {
                values[idx] += 1.0;
            }
        }

        // L2 normalize
        let norm: f32 = values.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut values {
                *v /= norm;
            }
        }

        Ok(Embedding {
            values,
            model_version: "bow-v1".into(),
        })
    }

    fn dimensions(&self) -> usize {
        self.max_vocab
    }

    fn model_version(&self) -> &str {
        "bow-v1"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bow_embed_produces_vector() {
        let embedder = BagOfWordsEmbedder::new(100);
        let emb = embedder.embed("docker ps running containers").await.unwrap();
        assert_eq!(emb.values.len(), 100);
        assert_eq!(emb.model_version, "bow-v1");
        // Should have non-zero values for the tokens
        assert!(emb.values.iter().any(|v| *v > 0.0));
    }

    #[tokio::test]
    async fn bow_similar_texts_closer() {
        let embedder = BagOfWordsEmbedder::new(100);
        let e1 = embedder.embed("docker container running").await.unwrap();
        let e2 = embedder.embed("docker container stopped").await.unwrap();
        let e3 = embedder.embed("nginx config file changed").await.unwrap();

        let sim_12 = cosine_similarity(&e1.values, &e2.values);
        let sim_13 = cosine_similarity(&e1.values, &e3.values);

        assert!(
            sim_12 > sim_13,
            "docker texts should be more similar ({sim_12}) than docker vs nginx ({sim_13})"
        );
    }

    #[tokio::test]
    async fn bow_normalized() {
        let embedder = BagOfWordsEmbedder::new(100);
        let emb = embedder.embed("test normalization").await.unwrap();
        let norm: f32 = emb.values.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "should be unit norm, got {norm}");
    }

    #[tokio::test]
    async fn bow_empty_text() {
        let embedder = BagOfWordsEmbedder::new(100);
        let emb = embedder.embed("").await.unwrap();
        assert!(emb.values.iter().all(|v| *v == 0.0));
    }

    #[tokio::test]
    async fn bow_batch() {
        let embedder = BagOfWordsEmbedder::new(100);
        let results = embedder
            .embed_batch(&["hello world", "foo bar"])
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|v| v * v).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }
        dot / (norm_a * norm_b)
    }
}
