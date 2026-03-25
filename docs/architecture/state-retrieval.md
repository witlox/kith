# State & Retrieval

## Overview

Kith maintains operational state as an event log. The agent retrieves context from this log using hybrid search — combining keyword matching with vector similarity.

## Event Model

Every operation produces an `Event`:

```rust
struct Event {
    id: String,
    machine: String,
    category: EventCategory,  // Drift, Exec, Apply, Commit, Rollback, Policy, Mesh, Capability, System
    event_type: String,
    path: Option<String>,
    detail: String,
    metadata: serde_json::Value,
    scope: EventScope,         // Public or Ops
    timestamp: DateTime<Utc>,
}
```

## Storage

- **In-memory EventStore** — fast, used by the shell agent
- **SqliteEventStore** — persistent, used by the daemon with cr-sqlite CRDT extensions
- Events are synced between stores via `merge()` (deduplicated by ID)

## Retrieval Pipeline

### Keyword Search

`KeywordRetriever` scores events by term frequency overlap with the query, filtered by scope.

### Vector Search

`VectorIndex` stores embeddings for operational events and finds nearest neighbors by cosine similarity.

### Hybrid Search

`HybridRetriever` combines both:
1. Keyword results (normalized scores)
2. Vector results (cosine similarity)
3. Combined score: `0.5 * keyword + 0.5 * vector`

## Selective Embedding

Not all events are worth embedding. Only operational events get indexed:

| Embedded | Skipped |
|----------|---------|
| Exec, Drift, Apply, Commit, Rollback | System, Mesh, Capability, Policy |

This keeps the vector index focused on actionable operational history.

## Embedding Backends

| Backend | Description |
|---------|-------------|
| `BagOfWordsEmbedder` | Local, vocabulary-versioned, 1000-dim sparse vectors |
| `ApiEmbeddingBackend` | Remote, any OpenAI-compatible `/v1/embeddings` endpoint |
