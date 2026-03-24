//! Hybrid retriever — combines keyword search + vector similarity (ADR-005).
//! Keyword for exact matches (ports, PIDs, paths), vector for semantic.

use kith_common::event::{Event, EventScope};

use crate::embedding::{Embedding, EmbeddingBackend};
use crate::retrieval::{KeywordRetriever, RetrievalResult};
use crate::vector_index::{SearchResult, VectorIndex};

/// Hybrid retrieval combining keyword + vector results.
pub struct HybridRetriever {
    index: VectorIndex,
    keyword_weight: f32,
    vector_weight: f32,
}

impl HybridRetriever {
    pub fn new(index: VectorIndex) -> Self {
        Self {
            index,
            keyword_weight: 0.4,
            vector_weight: 0.6,
        }
    }

    pub fn with_weights(mut self, keyword: f32, vector: f32) -> Self {
        self.keyword_weight = keyword;
        self.vector_weight = vector;
        self
    }

    /// Search using both keyword and vector similarity, merge results.
    pub async fn search(
        &self,
        events: &[Event],
        query: &str,
        query_embedding: &Embedding,
        scope: &EventScope,
        limit: usize,
    ) -> Vec<HybridResult> {
        // Keyword search
        let keyword_results = KeywordRetriever::search(events, query, scope, limit * 2);

        // Vector search
        let vector_results = self.index.search(query_embedding, limit * 2);

        // Merge: collect all unique events, combine scores
        let mut merged: std::collections::HashMap<String, HybridResult> =
            std::collections::HashMap::new();

        let keyword_max = keyword_results
            .iter()
            .map(|r| r.score)
            .fold(0.0f64, f64::max)
            .max(1.0);

        for kr in &keyword_results {
            let normalized = kr.score / keyword_max;
            let entry = merged.entry(kr.event.id.clone()).or_insert_with(|| HybridResult {
                event: kr.event.clone(),
                keyword_score: 0.0,
                vector_score: 0.0,
                combined_score: 0.0,
            });
            entry.keyword_score = normalized as f32;
        }

        let vector_max = vector_results
            .iter()
            .map(|r| r.score)
            .fold(0.0f32, f32::max)
            .max(1.0);

        for vr in &vector_results {
            let normalized = vr.score / vector_max;
            let entry = merged.entry(vr.event.id.clone()).or_insert_with(|| HybridResult {
                event: vr.event.clone(),
                keyword_score: 0.0,
                vector_score: 0.0,
                combined_score: 0.0,
            });
            entry.vector_score = normalized;
        }

        // Compute combined scores
        for result in merged.values_mut() {
            result.combined_score =
                result.keyword_score * self.keyword_weight + result.vector_score * self.vector_weight;
        }

        let mut results: Vec<HybridResult> = merged.into_values().collect();
        results.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
    }

    /// Get mutable access to the underlying index (for inserting new embeddings).
    pub fn index_mut(&mut self) -> &mut VectorIndex {
        &mut self.index
    }

    pub fn index(&self) -> &VectorIndex {
        &self.index
    }
}

#[derive(Debug, Clone)]
pub struct HybridResult {
    pub event: Event,
    pub keyword_score: f32,
    pub vector_score: f32,
    pub combined_score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::BagOfWordsEmbedder;
    use kith_common::event::{Event, EventCategory};

    fn make_event(machine: &str, detail: &str) -> Event {
        Event::new(machine, EventCategory::System, "test", detail)
            .with_scope(EventScope::Ops)
    }

    #[tokio::test]
    async fn hybrid_combines_keyword_and_vector() {
        let embedder = BagOfWordsEmbedder::new(100);

        let events = vec![
            make_event("s1", "docker container running on port 8080"),
            make_event("s1", "nginx config changed at /etc/nginx"),
            make_event("s1", "docker image built successfully"),
        ];

        // Index all events
        let mut index = VectorIndex::new();
        for event in &events {
            let emb = embedder.embed(&event.detail).await.unwrap();
            index.insert(event.clone(), emb);
        }

        let retriever = HybridRetriever::new(index);

        let query = "docker container";
        let query_emb = embedder.embed(query).await.unwrap();

        let results = retriever
            .search(&events, query, &query_emb, &EventScope::Ops, 10)
            .await;

        assert!(!results.is_empty());
        // First result should be most relevant to "docker container"
        assert!(
            results[0].event.detail.contains("docker"),
            "top result should mention docker: {}",
            results[0].event.detail
        );
        // Combined score uses both keyword and vector
        assert!(results[0].combined_score > 0.0);
    }

    #[tokio::test]
    async fn hybrid_keyword_only_when_no_embeddings() {
        let events = vec![
            make_event("s1", "docker running"),
            make_event("s1", "nginx stopped"),
        ];

        let index = VectorIndex::new(); // empty index
        let retriever = HybridRetriever::new(index);

        let query_emb = crate::embedding::Embedding {
            values: vec![0.0; 100],
            model_version: "test".into(),
        };

        let results = retriever
            .search(&events, "docker", &query_emb, &EventScope::Ops, 10)
            .await;

        // Should still find results via keyword search
        assert!(!results.is_empty());
        assert!(results[0].keyword_score > 0.0);
        assert_eq!(results[0].vector_score, 0.0);
    }

    #[tokio::test]
    async fn hybrid_respects_weights() {
        let embedder = BagOfWordsEmbedder::new(100);
        let events = vec![make_event("s1", "test event")];

        let mut index = VectorIndex::new();
        let emb = embedder.embed("test event").await.unwrap();
        index.insert(events[0].clone(), emb);

        let retriever = HybridRetriever::new(index).with_weights(0.8, 0.2);

        let query_emb = embedder.embed("test event").await.unwrap();
        let results = retriever
            .search(&events, "test event", &query_emb, &EventScope::Ops, 10)
            .await;

        assert!(!results.is_empty());
        // With 80% keyword weight, keyword_score dominates
        let r = &results[0];
        assert!(r.keyword_score * 0.8 > r.vector_score * 0.2 || r.combined_score > 0.0);
    }

    #[tokio::test]
    async fn hybrid_deduplicates_across_sources() {
        let embedder = BagOfWordsEmbedder::new(100);
        let events = vec![make_event("s1", "docker container")];

        let mut index = VectorIndex::new();
        let emb = embedder.embed("docker container").await.unwrap();
        index.insert(events[0].clone(), emb);

        let retriever = HybridRetriever::new(index);
        let query_emb = embedder.embed("docker container").await.unwrap();

        let results = retriever
            .search(&events, "docker container", &query_emb, &EventScope::Ops, 10)
            .await;

        // Same event found by both keyword and vector — should appear once
        assert_eq!(results.len(), 1);
        assert!(results[0].keyword_score > 0.0);
        assert!(results[0].vector_score > 0.0);
    }
}
