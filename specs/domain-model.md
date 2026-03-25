# Domain Model

## Overview

Kith is an intent-driven distributed shell environment. It provides a reasoning layer (LLM) over a mesh of machines, where the agent uses standard Unix tools for operations and adds native tools only for capabilities that don't exist in Unix: remote execution, fleet queries, semantic retrieval, and transactional state changes.

The LLM backend is pluggable — any model with tool calling and streaming works.

## Core Concepts

### Kith Shell
The user's terminal interface. A PTY wrapper that classifies input as pass-through (literal command, zero added latency) or intent (routed to the LLM via the InferenceBackend for reasoning and execution). Maintains conversation context enriched by the vector index and fleet state.

### Kith Daemon
A lightweight daemon on each mesh member. Provides: authenticated remote command execution, local state observation, capability reporting, drift detection, and immutable audit logging. Background service, not an init system.

### InferenceBackend
The abstraction that makes Kith model-agnostic. A trait with implementations for different LLM providers (OpenAI-compatible, Anthropic, direct vLLM/SGLang). The kith shell calls InferenceBackend for all reasoning; the rest of the system never touches the LLM directly.

### Mesh
WireGuard encrypted P2P transport + Nostr decentralized signaling. No coordination server.

### Sync Layer
cr-sqlite with CRDT merge semantics. Each kith-daemon writes events to local SQLite. Replicated across peers. Eventually consistent, partition-tolerant.

### State Layer
Vector index over operational state, built from synced cr-sqlite events + local observations. The agent queries this to retrieve its own context — memory hierarchy emerges from retrieval relevance.

### Commit Window
State changes enter "pending" state. User reviews and commits, or changes auto-revert after the window expires.

### Drift
Difference between expected and actual state on a machine. Detected by kith-daemon. Surfaced as events, not silently corrected.

### Capability Report
Structured description of what a machine can do. Published by each kith-daemon, synced via CRDT.

### Tool Registry
Local index of available Unix tools, discovered by scanning PATH directories at shell startup and on-demand rescan. Each tool entry includes: name, absolute path, category (vcs, container, language, build, server, other), and optional version string. The registry serves two purposes: (1) input classification (pass-through if first token matches a known tool) and (2) system prompt enrichment (the LLM knows what tools are available without guessing). The daemon maintains its own tool registry for capability reports, re-scanned periodically.

### Tool Category
A functional grouping for discovered tools. Categories: vcs, container, language, build, server, database, editor, network, monitoring, other. Assigned by matching tool names against a known-tools table. Unknown tools are categorized as "other".

### Native Tool
A tool in kith shell's own API: remote(), fleet_query(), retrieve(), apply(), commit(), rollback(), todo(). Everything else is standard Unix via PTY.

### Audit Trail
Immutable append-only log of every action. Source material for the vector index.

### Policy
Per-machine, per-user access rules enforced by kith-daemon in Rust code, not in the LLM prompt.

## Relationships

```
User → Kith Shell → [pass-through] → local bash
                   → [intent] → InferenceBackend → tool dispatch
                                  → local bash
                                  → remote(host) → Mesh → Kith Daemon → exec
                                  → fleet_query() → Sync Layer (cr-sqlite)
                                  → retrieve() → State Layer (vector index)
                                  → apply(host) → Kith Daemon → pending change
                                  → commit/rollback → Kith Daemon → finalize/revert

Kith Daemon → State Observer → events → cr-sqlite → Sync Layer → peers
Kith Daemon → Policy Engine → allow/deny (per request)
Kith Daemon → Audit Log → cr-sqlite → vector index
```
