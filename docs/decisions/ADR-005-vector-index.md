# ADR-005: Embedded Vector Index for Operational State Retrieval

## Status: Accepted

## Context

The agent needs semantic retrieval over operational history: "why is staging broken?" should surface relevant deployment events, error logs, and past reasoning traces from the synced cr-sqlite store.

The design conversation established that the L1/L2/L3 memory hierarchy should emerge from retrieval relevance rather than being statically designed. The vector space is the agent's self-managed memory.

## Decision

kith-state maintains an embedded vector index (in-process, not a separate service) built from cr-sqlite events.

- **Embedding source**: subscribe to cr-sqlite event stream. Embed each event's detail + metadata.
- **Embedding model**: configurable. Default to a small, fast local model (e.g., all-MiniLM-L6-v2 or similar). API-based embedding as an option for better quality.
- **Index implementation**: start with an in-process library (usearch, lance, or hnswlib via FFI). No external vector database.
- **Hybrid retrieval**: vector similarity for semantic queries + structured SQL for exact matches (port numbers, PIDs, file paths, timestamps).
- **Version tracking**: embedding model version recorded per entry. Distance comparisons only between same-version entries. Model update triggers re-indexing.

## Consequences

**Positive:**
- No external service to run — embedded in the kith process
- Hybrid retrieval covers both semantic ("why is this broken") and exact ("what's on port 3000") queries
- Index is a materialized view — rebuildable from cr-sqlite at any time (INV-CON-4)
- L1/L2/L3 emerges from relevance scores, not static config

**Negative:**
- Embedding quality on operational data is unproven (ASM-STO-2) — terse log lines may not cluster well
- Requires an embedding model dependency (either local model or API call)
- Re-indexing on model update is potentially slow for large event stores

**Trade-offs vs. alternatives:**
- External vector DB (Qdrant, Milvus): more features, another service to run — overkill for personal infra
- No vector search (SQL only): loses semantic "find related things I forgot about" capability
- Full-text search (SQLite FTS5): keyword matching but no semantic understanding

## Validation

Embed 1 week of real operational data. Run 50 retrieval queries spanning both semantic and exact-match types. Measure recall@5 (target: >70% per ASM-STO-2). If recall is poor, increase weight of structured retrieval relative to vector.
