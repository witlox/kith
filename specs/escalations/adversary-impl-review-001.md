> All findings in this report have been resolved. See git history for fixes.

# Adversary Implementation Review — Findings Report

**Date:** 2026-03-23
**Mode:** Implementation review (post-implementation)
**Scope:** All 6 crates + test infrastructure

---

## FI-01: mesh_ip not allocated from mesh_cidr

**Severity: Medium**

`kith-mesh/src/manager.rs:45` has `mesh_ip: String::new(), // TODO: allocate from mesh_cidr`. The announce method publishes an empty mesh_ip to signaling. Peers receiving this will parse it as `0.0.0.0` (the unspecified address) for the WireGuard allowed-ips, which won't route correctly.

**Impact:** Mesh networking won't actually work until mesh IP allocation is implemented.

**Recommendation:** Implement a simple IP allocator from the configured CIDR. For a 3-10 machine mesh, a deterministic scheme (hash of machine_id mod subnet size) or sequential allocation with persistence is sufficient.

---

## FI-02: No gRPC service layer yet

**Severity: Medium**

kith-daemon has policy, drift, commit, audit, and exec modules — but no gRPC service that ties them together. The proto file is defined but no tonic server exists. The crate can't actually serve requests.

**Impact:** Daemon modules are individually tested but not integrated. The shell can't connect to a daemon yet.

**Recommendation:** This is expected given the per-crate implementation approach. The integrator phase should wire the gRPC service. Not a bug, but the gap should be tracked.

---

## FI-03: AuditLog is in-memory only — no persistence

**Severity: Medium**

`kith-daemon/src/audit.rs` stores events in a `Vec<Event>`. On daemon restart, all audit history is lost. INV-SEC-5 says "audit entries cannot be modified or deleted through the kith interface" — but a restart deletes everything.

**Impact:** Audit trail is volatile. Acceptable for the first implementation phase, but must be wired to cr-sqlite/EventStore before production use.

**Recommendation:** In the integrator phase, wire AuditLog to write events to kith-sync's EventStore (which will eventually be cr-sqlite backed). The in-memory vec becomes a write-through cache.

---

## FI-04: No timeout on command execution

**Severity: Medium**

`kith-daemon/src/exec.rs` runs `Command::new("sh").arg("-c").arg(command).output().await` with no timeout. A command like `yes` or `cat /dev/urandom` will hang forever.

**Impact:** Unbounded command execution can consume resources indefinitely.

**Recommendation:** Add a configurable timeout (default: 120s) using `tokio::time::timeout` wrapping the `.output().await`.

---

## FI-05: InMemorySignaling uses std::sync::Mutex in async context

**Severity: Low**

`kith-mesh/src/signaling.rs` uses `std::sync::Mutex` inside async trait implementations. This is technically safe (the lock is held for very short durations) but could block the tokio runtime if the lock is contended.

**Impact:** Negligible for test code. If InMemorySignaling is ever used as a production stub, this could become an issue.

**Recommendation:** Accept for now since this is test infrastructure. If promoted to production, switch to `tokio::sync::Mutex`.

---

## FI-06: EventStore scope filtering logic is inverted for query semantics

**Severity: Low**

In `kith-sync/src/store.rs`, the scope filter interprets `EventScope::Ops` as "show everything" and `EventScope::Public` as "show only public." This matches the invariant (INV-DAT-2: ops sees everything, viewers see filtered). But the filter parameter represents the *caller's* scope, which is confusing — passing `Ops` means "I am ops" not "filter to ops-only events."

**Impact:** Correct behavior but confusing API semantics. Could lead to bugs when integrating.

**Recommendation:** Rename the parameter or add a doc comment clarifying: "pass the caller's scope, not the desired event scope."

---

## FI-07: No integration between crates yet

**Severity: Info**

The 6 crates are independently tested but not wired together. This is expected — the integrator phase will:
- Wire daemon gRPC service using policy + drift + commit + audit + exec
- Wire shell to daemon via gRPC client
- Wire audit to EventStore for persistence
- Wire MeshManager into daemon startup
- Wire EventStore subscription to kith-state retrieval

---

## Impl Mode Checklist Assessment

| Check | Status | Notes |
|-------|--------|-------|
| No unwrap in library code | **Pass** | Only in mock/test infrastructure |
| All gRPC endpoints validate input | **N/A** | No gRPC server yet (FI-02) |
| All file/network operations have timeouts | **Fail** | FI-04: exec has no timeout |
| Tests exist for happy path AND error paths | **Pass** | Policy: valid/invalid/expired/tampered. Commit: found/not-found/expired. |
| No secrets in code, config, or logs | **Pass** | API keys read from env vars |
| Audit log entries complete | **Partial** | FI-03: in-memory only, no persistence |
| InferenceBackend impls tested with mock | **Pass** | MockInferenceBackend with queued responses |

---

## Summary

| Severity | Count | IDs |
|----------|-------|-----|
| Medium | 4 | FI-01, FI-02, FI-03, FI-04 |
| Low | 2 | FI-05, FI-06 |
| Info | 1 | FI-07 |

**No critical or high findings.** The medium findings are all expected gaps for the first implementation pass — they should be resolved during the integrator phase. FI-04 (exec timeout) should be fixed before integration.
