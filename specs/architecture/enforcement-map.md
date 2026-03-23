# Enforcement Map

Maps every invariant to its enforcement point — where validation happens, what rejects violations.

---

## Security Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| INV-SEC-1 | No unauthenticated remote exec | `KithDaemonService` gRPC interceptor | Extract credential from request metadata. Reject if missing/expired/invalid. | `tonic::Status::UNAUTHENTICATED` |
| INV-SEC-2 | Policy at daemon, not model | `PolicyEvaluator::evaluate()` in kith-daemon | Called in every gRPC handler before action execution. Model requests pass through same path. | `PolicyDecision::Deny` → `PERMISSION_DENIED` |
| INV-SEC-3 | Model never sees raw credentials | `kith-shell` credential handling | Credentials injected into gRPC metadata below InferenceBackend visibility. Tool arguments never contain credentials. | Structural — no API passes credentials through the model |
| INV-SEC-4 | Audit completeness | `kith-daemon` event writing | Every gRPC handler writes an Event (with who/what/when/where/outcome) to cr-sqlite before returning. | Structural — event write is in handler code path |
| INV-SEC-5 | Audit immutability | cr-sqlite schema | Events table is append-only. No UPDATE or DELETE exposed in SyncEngine API. CRDT uses add-wins semantics. | Structural — no mutation API exists |

## Consistency Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| INV-CON-1 | CRDT convergence | cr-sqlite merge semantics | Given sufficient connectivity, all peers converge to same event set. cr-sqlite guarantees this. | Protocol guarantee |
| INV-CON-2 | Commit window atomicity | `CommitWindowManager::commit()` / `rollback()` | Pending change is fully committed or fully rolled back. No partial state. | Atomic operation |
| INV-CON-3 | Capability reports eventually fresh | CapabilityReport timestamp | `updated_at` field allows agent to reason about staleness. Stale data doesn't break anything — just reduces quality. | Informational timestamp |
| INV-CON-4 | Vector index is a view | kith-state design | Index built from cr-sqlite events. Can be rebuilt from scratch at any time. | Rebuild on corruption |

## Operational Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| INV-OPS-1 | Pass-through zero latency | `kith-shell` input classifier | Pass-through commands go directly to PTY bash child process. No InferenceBackend call, no network. | Structural — classification happens before inference |
| INV-OPS-2 | Inference failure → bash | `kith-shell` fallback logic | On InferenceError::Unreachable or Timeout, shell enters pass-through mode. Shows notification. | Automatic degradation |
| INV-OPS-3 | Partition doesn't prevent local ops | `kith-daemon` design | Daemon operates on local state. cr-sqlite works locally during partition. Sync resumes on reconnect. | By design — no remote dependency for local ops |
| INV-OPS-4 | No tool wrappers | `kith-shell` system prompt + tool definitions | Only 7 native tools defined (remote, fleet_query, retrieve, apply, commit, rollback, todo). Everything else is bash. | System prompt instructs model to use Unix commands |
| INV-OPS-5 | Model-agnostic | InferenceBackend trait | No model-specific code outside `kith-shell/src/inference/`. Trait defined in kith-common. | Structural — trait boundary enforces this |

## Data Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| INV-DAT-1 | Events append-only | cr-sqlite + SyncEngine API | Add-wins OR-Set semantics. No update/delete in event table. | Structural — no mutation API |
| INV-DAT-2 | Event access is scope-gated | Query handlers in kith-daemon + kith-shell | CRDT syncs full events. Query/retrieval filters by caller's scope (from MachinePolicy). All mesh members trusted at transport level (WireGuard). | Scope-filtered results |
| INV-DAT-3 | Embedding consistency | kith-state config | Embedding model version recorded in metadata. Distance comparisons only between same-version embeddings. | Version check at query time |

---

## Enforcement Categories

| Category | Count | Invariants | Description |
|----------|-------|------------|-------------|
| **Structural** | 7 | INV-SEC-3, INV-SEC-4, INV-SEC-5, INV-OPS-1, INV-OPS-4, INV-OPS-5, INV-DAT-1 | Impossible to violate by design |
| **Runtime logic** | 5 | INV-SEC-1, INV-SEC-2, INV-OPS-2, INV-OPS-3, INV-DAT-2 | Active enforcement in code |
| **Protocol guarantee** | 1 | INV-CON-1 | Guaranteed by cr-sqlite CRDT |
| **Operational** | 2 | INV-CON-2, INV-DAT-3 | Atomic ops / version checks |
| **Informational** | 2 | INV-CON-3, INV-CON-4 | Staleness is visible, not prevented |
