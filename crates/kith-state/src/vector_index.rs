//! In-process vector index. Brute-force cosine similarity.
//! Fine for <100K events. Swap for ANN library (usearch, hnswlib) when needed.

use kith_common::event::Event;

use crate::embedding::Embedding;

/// An indexed entry: event ID + embedding vector.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub event_id: String,
    pub event: Event,
    pub embedding: Vec<f32>,
    pub model_version: String,
}

/// Brute-force vector index with cosine similarity search.
pub struct VectorIndex {
    entries: Vec<IndexEntry>,
}

impl VectorIndex {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add an entry to the index.
    pub fn insert(&mut self, event: Event, embedding: Embedding) {
        self.entries.push(IndexEntry {
            event_id: event.id.clone(),
            event,
            embedding: embedding.values,
            model_version: embedding.model_version,
        });
    }

    /// Search for the top-k most similar entries to the query vector.
    /// Only compares entries with matching model_version (INV-DAT-3).
    pub fn search(&self, query: &Embedding, k: usize) -> Vec<SearchResult> {
        let mut scored: Vec<SearchResult> = self
            .entries
            .iter()
            .filter(|e| e.model_version == query.model_version)
            .map(|entry| {
                let score = cosine_similarity(&query.values, &entry.embedding);
                SearchResult {
                    event: entry.event.clone(),
                    score,
                }
            })
            .collect();

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }

    /// Number of entries in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove all entries (e.g., for re-indexing after model version change).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub event: Event,
    pub score: f32,
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|v| v * v).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kith_common::event::{Event, EventCategory, EventScope};

    fn make_event(detail: &str) -> Event {
        Event::new("test", EventCategory::System, "test.event", detail)
            .with_scope(EventScope::Ops)
    }

    fn make_embedding(values: Vec<f32>) -> Embedding {
        Embedding {
            values,
            model_version: "test-v1".into(),
        }
    }

    #[test]
    fn insert_and_search() {
        let mut index = VectorIndex::new();

        index.insert(make_event("docker running"), make_embedding(vec![1.0, 0.0, 0.0]));
        index.insert(make_event("nginx config"), make_embedding(vec![0.0, 1.0, 0.0]));
        index.insert(make_event("docker stopped"), make_embedding(vec![0.9, 0.1, 0.0]));

        let query = make_embedding(vec![1.0, 0.0, 0.0]); // closest to "docker running"
        let results = index.search(&query, 2);

        assert_eq!(results.len(), 2);
        assert!(results[0].score > results[1].score);
        assert!(results[0].event.detail.contains("docker running"));
    }

    #[test]
    fn search_respects_model_version() {
        let mut index = VectorIndex::new();

        index.insert(
            make_event("old model"),
            Embedding {
                values: vec![1.0, 0.0],
                model_version: "v1".into(),
            },
        );
        index.insert(
            make_event("new model"),
            Embedding {
                values: vec![1.0, 0.0],
                model_version: "v2".into(),
            },
        );

        let query = Embedding {
            values: vec![1.0, 0.0],
            model_version: "v2".into(),
        };

        let results = index.search(&query, 10);
        assert_eq!(results.len(), 1); // only v2 entry
        assert!(results[0].event.detail.contains("new model"));
    }

    #[test]
    fn empty_index_returns_nothing() {
        let index = VectorIndex::new();
        let query = make_embedding(vec![1.0, 0.0]);
        assert!(index.search(&query, 10).is_empty());
    }

    #[test]
    fn clear_removes_all() {
        let mut index = VectorIndex::new();
        index.insert(make_event("a"), make_embedding(vec![1.0]));
        index.insert(make_event("b"), make_embedding(vec![0.0]));
        assert_eq!(index.len(), 2);
        index.clear();
        assert!(index.is_empty());
    }

    #[test]
    fn cosine_identical_vectors() {
        let sim = cosine_similarity(&[1.0, 0.0, 0.0], &[1.0, 0.0, 0.0]);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn cosine_orthogonal_vectors() {
        let sim = cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]);
        assert!(sim.abs() < 0.001);
    }

    #[test]
    fn cosine_opposite_vectors() {
        let sim = cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]);
        assert!((sim - (-1.0)).abs() < 0.001);
    }
}
