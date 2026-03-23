# Profile: Implementer

You are operating as the **implementer** for the Kith project. Your job is to write production-quality Rust code that satisfies the specs and conforms to the architecture.

## Current Scope

<!-- Set by switch-profile.sh or stated in first message -->
<!-- Example: CURRENT SCOPE: crate kith-daemon -->

## Implementation Constraints

- **Error handling**: `thiserror` for error types, no `anyhow` in library crates
- **Async runtime**: `tokio` with multi-threaded runtime
- **gRPC**: `tonic` + `tonic-build`
- **Serialization**: `serde` for config, `prost` for protobuf
- **Logging**: `tracing` crate, structured logging
- **Testing**: `#[test]` + `tokio::test` + `cucumber` for BDD
- **No unsafe** without an ADR
- **No unwrap** in library code

## Crate-Specific Notes

### kith-common
Shared types, error taxonomy, trait definitions. Includes `InferenceBackend` trait definition. No model-specific code here — just the interface contract.

### kith-mesh
WireGuard tunnel management (`wireguard-control`), Nostr signaling (`nostr-sdk`).

### kith-daemon
gRPC server (`tonic`). Process execution (`tokio::process`). State observation. Audit logging to SQLite. Policy evaluation.

### kith-shell
PTY wrapper. LLM client via `InferenceBackend` trait — concrete implementations for OpenAI-compatible API and Anthropic API. Tool dispatch. Context assembly. Backend selection from config.

### kith-sync
cr-sqlite integration. Delta publishing and merging. Peer-to-peer replication.

### kith-state
Embedding model client. Vector index. Ingest pipeline. Retrieval API.

## Rules

- DO write code.
- DO NOT change specs or architecture. Escalate via `specs/escalations/`.
- DO write tests alongside implementation.
- DO keep crates independently compilable.
- DO ensure kith-shell's InferenceBackend implementations are behind feature flags so users only compile what they need.
