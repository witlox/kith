# Fidelity Index

Last scan: 2026-03-24

## How to read this file

Tracks what the BDD test suite ACTUALLY verifies vs what the scenarios CLAIM is tested.

**Step depth:**
- **REAL**: Step function contains assertions or calls real code
- **STUB**: Step function is empty `{}` — matches the scenario but verifies nothing

## Feature Fidelity

| Feature | Scenarios | Total Steps | Real Steps | Stub Steps | Confidence |
|---------|-----------|-------------|------------|------------|------------|
| drift-detection | 10 | 39 | 31 | 8 | **HIGH** |
| policy-enforcement | 10 | 32 | 23 | 9 | **HIGH** |
| commit-windows | 7 | 27 | 17 | 10 | **MODERATE** |
| local-execution | 8 | 23 | 13 | 10 | **MODERATE** |
| mesh-networking | 5 | 20 | 11 | 9 | **MODERATE** |
| remote-execution | 4 | 16 | 11 | 5 | **MODERATE** |
| state-and-retrieval | 5 | 12 | 9 | 3 | **HIGH** |
| inference-backend | 10 | 44 | 11 | 33 | **LOW** |
| **Total** | **59** | **213** | **126 (59%)** | **87 (41%)** | |

## Stub Hotspots

**inference-backend (33 stubs):** Most stubs are infrastructure assertions ("tokens stream to terminal", "tool call boundaries detected", "reasoning is rendered") that require a real PTY or rendering layer to verify. These are correct at the spec level but unverifiable without the terminal UI.

**commit-windows (10 stubs):** Stubs around overlayfs/backup mechanics ("overlay is merged", "backup is restored") that require OS-level containment to verify.

**mesh-networking (9 stubs):** Stubs around WireGuard tunnel state ("tunnel established", "e2e encrypted", "traffic routes through relay") that require real network infrastructure.

## Priority for Stub Conversion

1. **Audit trail stubs** (policy.rs: audit_allowed, audit_denial, audit_rejection) — these guard INV-SEC-4 and should call real AuditLog
2. **Streaming stubs** (inference_backend, remote_execution) — need mock streaming to verify
3. **Containment stubs** (commit_windows) — need OS-level testing, defer to e2e/containers

## Unit/Integration/E2e Test Coverage

| Crate | Tests | Depth |
|-------|-------|-------|
| kith-common | 39 | THOROUGH — all types tested with roundtrips |
| kith-daemon | 43 | THOROUGH — real crypto, policy eval, gRPC service |
| kith-shell | 63 | THOROUGH — classifier, agent loop, real HTTP backend types |
| kith-mesh | 34 | THOROUGH — registry, IP allocation, manager integration |
| kith-sync | 23 | THOROUGH — in-memory + SQLite with persistence test |
| kith-state | 25 | THOROUGH — keyword + vector + hybrid retrieval |
| kith-e2e | 29 | THOROUGH — real gRPC, multi-daemon, chaos, containers |
| **Total** | **256** | |
