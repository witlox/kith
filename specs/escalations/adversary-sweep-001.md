> All findings in this report have been resolved. See git history for fixes.

# Adversary Full Sweep — Findings Report

**Date:** 2026-03-24
**Mode:** Implementation sweep (post-completion)
**Scope:** All 44 source files, 8160 lines across 8 crates

---

## FS-01: Identity key file written without restricted permissions

**Severity: High**

`kith-shell/src/bin/kith.rs:180` writes the Ed25519 private key with `std::fs::write()` which uses the default umask (typically 0644). The secret key is readable by other users on the system.

Pact solved this identically — ADR-006 specifies TOFU but doesn't address key file permissions. The fix from pact's auth crate: write with 0600 permissions explicitly.

**Recommendation:** After `std::fs::write(&key_path, kp.secret_bytes())`, set permissions to 0600:
```rust
#[cfg(unix)]
{ use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?; }
```

---

## FS-02: unsafe block for cr-sqlite extension loading

**Severity: Medium**

`kith-sync/src/sqlite_store.rs:79` uses `unsafe { conn.load_extension(...) }`. This is required by rusqlite's API for loading extensions, but:
- No ADR documents the unsafe usage
- The extension paths are hardcoded strings, not user-controlled (mitigates injection)
- If cr-sqlite has bugs, they're in the unsafe boundary

**Recommendation:** Document with a safety comment explaining why the unsafe is acceptable. The hardcoded paths prevent user-controlled library loading.

---

## FS-03: 68 stub BDD step definitions

**Severity: Medium**

Across the 6 step files, 68 step functions are empty `{}` bodies — they match but assert nothing. Examples: "an audit entry records the allowed exec", "gRPC connectivity is verified", "the connection remains end-to-end encrypted", "output streams back incrementally via gRPC streaming".

These give false confidence — the scenario passes but the step didn't verify anything.

**Impact:** 59 scenarios pass but not all 316 steps are meaningful assertions. Actual assertion depth is lower than the numbers suggest.

**Recommendation:** Track stub vs real steps (like pact's fidelity index). Prioritize converting stubs that guard critical paths: audit completeness, streaming output, encryption verification.

---

## FS-04: EventCategory serialized as Debug format in SQLite

**Severity: Low**

`sqlite_store.rs` stores EventCategory as `format!("{:?}", event.category)` producing strings like `"Drift"`, `"Exec"`. The parser matches these exact strings. This works but is fragile — if EventCategory variants are renamed, the parser silently falls through to `System`.

**Recommendation:** Use serde serialization (which the type already derives) instead of Debug formatting. Or add a test that roundtrips every EventCategory variant through SQLite.

---

## FS-05: gRPC Exec streams output but doesn't cap total size

**Severity: Low**

`kith-daemon/src/service.rs` Exec handler captures full stdout+stderr and sends as stream chunks. A command producing gigabytes of output (e.g., `cat /dev/urandom`) will consume unbounded memory.

The 120s timeout limits duration but not output volume.

**Recommendation:** Cap stdout/stderr accumulation at a configurable limit (e.g., 10MB). Truncate and signal truncation to the client.

---

## FS-06: Agent tool dispatch doesn't use DaemonClient for fleet_query/retrieve

**Severity: Low**

`kith-shell/src/agent.rs` fleet_query and retrieve tools query the local EventStore only. In a mesh, they should query the synced store which includes events from remote machines. Currently they only see events produced locally (or explicitly merged).

**Impact:** `fleet_query("what machines are in the mesh?")` returns nothing unless events were manually merged into the local store.

**Recommendation:** When daemon client is connected, fleet_query should also pull events from the daemon's synced store. Or: wire the EventStore in the agent to be the SqliteEventStore that participates in cr-sqlite sync.

---

## FS-07: BagOfWords embedder vocabulary is not persisted

**Severity: Low**

`BagOfWordsEmbedder` builds vocabulary in-memory. On restart, the vocabulary resets, and previously-computed embeddings have different vector representations than newly-computed ones. INV-DAT-3 requires embedding consistency, but the BoW model version is always "bow-v1" regardless of vocabulary state.

**Impact:** After restart, vector search against pre-restart embeddings returns wrong similarity scores.

**Recommendation:** Either persist the vocabulary alongside the vector index, or include a vocabulary hash in the model_version string. Or accept this as a known limitation of the baseline embedder (to be replaced by a real model).

---

## Summary

| Severity | Count | IDs |
|----------|-------|-----|
| High | 1 | FS-01 |
| Medium | 2 | FS-02, FS-03 |
| Low | 4 | FS-04, FS-05, FS-06, FS-07 |

**FS-01 (key file permissions) should be fixed immediately.** The rest are tracked improvements.

---

## Impl Mode Checklist

| Check | Status | Notes |
|-------|--------|-------|
| No unwrap in library code | **Pass** | Only in mocks + test helpers (acceptable) |
| All gRPC endpoints validate input | **Pass** | All RPCs call auth() first |
| All file/network operations have timeouts | **Pass** | exec: 120s, inference: configurable, gRPC: tonic defaults |
| Tests for happy path AND error paths | **Pass** | Policy: valid/invalid/expired/tampered. Commit: found/not-found/expired. Backends: unreachable/rate-limited |
| No secrets in code, config, or logs | **Pass with caveat** | FS-01: key file permissions |
| Audit log entries complete | **Partial** | FS-03: 68 stub steps don't verify audit |
| InferenceBackend tested with mock | **Pass** | MockInferenceBackend + 7 agent tests |
| No model-specific logic outside backends | **Pass** | Verified: daemon/mesh/sync/state have zero LLM references |
