# Module Map

Module boundaries, responsibilities, and ownership. Each module maps to a Rust crate.

---

## kith-common

**Responsibility:** Shared kernel — types, config, errors, protobuf bindings used by all crates.

**Owns:**
- All domain types (DriftVector, DriftWeights, CapabilityReport, Event, Scope, etc.)
- Configuration structs (KithConfig, DaemonConfig, ShellConfig, MeshConfig)
- Error taxonomy (KithError enum)
- Protobuf-generated types (proto modules for daemon.proto)
- The `InferenceBackend` trait definition (trait lives here; implementations in kith-shell)

**Does NOT own:** Business logic, I/O, network, state management, InferenceBackend implementations.

**Justification:** Shared kernel pattern — common types prevent duplication and ensure wire compatibility. InferenceBackend trait lives here so kith-shell and future crates can reference it without circular deps.

---

## kith-mesh

**Responsibility:** WireGuard tunnel management and Nostr signaling. Establishes and maintains encrypted P2P connectivity between mesh members.

**Owns:**
- WireGuard peer configuration (add/remove peers, key management)
- Nostr event publishing and subscription (peer discovery)
- Mesh membership tracking (who's online, endpoint changes)
- NAT traversal coordination
- DERP relay fallback configuration

**Submodules:**
- `wireguard/` — tunnel lifecycle, peer add/remove via wireguard-control crate
- `signaling/` — Nostr client, event types, relay management via nostr-sdk crate
- `discovery/` — peer registry, heartbeat, mesh membership state

**Does NOT own:** Application-layer protocols (gRPC), sync, policy. Only provides connectivity.

**Justification:** Mesh networking is infrastructure that everything else sits on. Separated from kith-daemon because mesh code has different dependencies (nostr-sdk, wireguard-control) and can be tested independently.

---

## kith-daemon

**Responsibility:** Per-machine background service. Provides authenticated remote execution, state observation, drift detection, commit windows, capability reporting, and audit logging.

**Owns:**
- gRPC service implementation (Exec, Query, Apply, Commit, Rollback, Events, Capabilities)
- Policy enforcement (scope-based access control in Rust)
- State observation (inotify file watches, process polling, network state polling)
- Drift evaluator (4-category comparison: files, services, network, packages)
- Commit window manager (pending changes, auto-rollback on expiry)
- Capability reporter (OS, installed software, resources)
- Audit log (append-only local event store)
- Containment (cgroups v2, overlayfs — Linux only, graceful degradation on macOS)

**Submodules:**
- `service/` — gRPC service handlers (tonic)
- `policy/` — scope evaluation, credential validation
- `observer/` — inotify, process poller, network poller
- `drift/` — DriftEvaluator, blacklist filtering, weighted magnitude
- `commit/` — CommitWindow lifecycle, auto-rollback
- `capability/` — hardware/software detection, CapabilityReport builder
- `containment/` — cgroup v2 management, overlayfs transactions (Linux only, feature-gated)
- `audit/` — local append-only event log

**Does NOT own:** LLM inference, shell UI, mesh networking, sync protocol, vector index.

**Justification:** Daemon is the per-machine operational core. Every remote operation flows through it. Policy enforcement here (not in prompt) satisfies INV-SEC-2.

---

## kith-shell

**Responsibility:** The user's terminal interface. PTY wrapper that classifies input as pass-through or intent, routes intent to InferenceBackend, dispatches tool calls, maintains conversation context.

**Owns:**
- PTY wrapper (fork child bash, intercept I/O)
- Input classification (pass-through vs. intent, escape hatch detection)
- InferenceBackend implementations (OpenAI-compatible, Anthropic API)
- Tool dispatch (mapping model tool calls to native tool execution)
- Native tool implementations: remote(), fleet_query(), retrieve(), apply(), commit(), rollback(), todo()
- Conversation context management (system prompt assembly, compaction)
- System prompt templates (per-backend formatting, behavioral instructions)

**Submodules:**
- `pty/` — PTY fork, I/O streaming, pass-through path
- `classify/` — input classification heuristics
- `inference/` — InferenceBackend trait implementations
  - `openai_compat.rs` — OpenAI-compatible API (covers vLLM, SGLang, Ollama, etc.)
  - `anthropic.rs` — Anthropic API (Claude models)
- `tools/` — native tool implementations
- `context/` — conversation management, compaction, system prompt assembly
- `config/` — shell configuration, backend selection

**Does NOT own:** Daemon logic, mesh networking, sync, vector index computation. Calls these via gRPC/SQL/API.

**Justification:** Shell is the user-facing entry point. InferenceBackend implementations live here because only the shell interacts with LLMs (INV-OPS-5). All model-specific code is contained within `inference/`.

---

## kith-sync

**Responsibility:** cr-sqlite CRDT replication between kith-daemons. Eventually consistent state synchronization across the mesh.

**Owns:**
- cr-sqlite integration (SQLite with CRDT merge semantics)
- Delta replication protocol (peer-to-peer sync over mesh)
- Event table schema (CRDT-compatible append-only events)
- Sync scheduling (background sync interval, batch delta exchange)
- Partition detection and recovery (delta accumulation, merge on reconnect)

**Submodules:**
- `schema/` — SQLite table definitions, migrations
- `replication/` — delta exchange protocol, peer sync logic
- `merge/` — CRDT merge handling

**Does NOT own:** What events mean (that's kith-daemon's domain). Transport (uses kith-mesh). Vector indexing (that's kith-state).

**Justification:** Sync is a cross-cutting concern that all daemons participate in. Separated because cr-sqlite has its own lifecycle, schema, and testing requirements. ADR-001 documents the sync strategy.

---

## kith-state

**Responsibility:** Vector index over operational state. Built from synced cr-sqlite events. Provides semantic retrieval for the agent.

**Owns:**
- Embedding pipeline (subscribe to cr-sqlite events, embed, index)
- Vector index management (insert, query, index maintenance)
- Ingest daemon (local observations not captured by kith-daemon: shell history, PTY output)
- Retrieval API (semantic search, structured query, hybrid retrieval)

**Submodules:**
- `embed/` — embedding model integration (local or API-based)
- `index/` — vector index (in-process, e.g., usearch or lance)
- `ingest/` — event subscription, local observation capture
- `retrieval/` — query interface, ranking, filtering

**Does NOT own:** The events themselves (kith-sync owns the CRDT store). Mesh transport. LLM inference.

**Justification:** Vector retrieval is the agent's self-managed memory. Separated because the embedding model, index format, and retrieval strategy are independent concerns that evolve on their own schedule.

---

## Crate Summary

| Crate | Responsibility | Key Dependencies |
|-------|---------------|-----------------|
| kith-common | Shared types, config, errors, InferenceBackend trait | thiserror, serde, tonic (codegen) |
| kith-mesh | WireGuard + Nostr connectivity | wireguard-control, nostr-sdk |
| kith-daemon | Per-machine daemon, gRPC service | tonic, kith-common, kith-sync, kith-mesh |
| kith-shell | Terminal UI, LLM inference, tool dispatch | kith-common, kith-daemon (client), kith-state (client) |
| kith-sync | cr-sqlite CRDT replication | cr-sqlite, kith-common, kith-mesh |
| kith-state | Vector index, embedding, retrieval | kith-common, kith-sync (event subscription) |
