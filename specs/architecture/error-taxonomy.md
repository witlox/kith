# Error Taxonomy

Per-module error types, gRPC status mapping, and user-facing messages.

---

## KithError (kith-common)

The shared error type. Defined via `thiserror`.

```rust
#[derive(Debug, thiserror::Error)]
pub enum KithError {
    // --- Auth/Policy ---
    #[error("authentication required")]
    Unauthenticated,
    #[error("policy denied: {reason}")]
    PolicyDenied { reason: String },
    #[error("credentials expired")]
    CredentialsExpired,

    // --- Lookup ---
    #[error("machine not found: {0}")]
    MachineNotFound(String),
    #[error("pending change not found: {0}")]
    PendingNotFound(String),

    // --- State ---
    #[error("commit window expired for {pending_id}")]
    CommitWindowExpired { pending_id: String },
    #[error("machine unreachable: {0}")]
    MachineUnreachable(String),

    // --- Drift ---
    #[error("drift detected on {machine}: {detail}")]
    DriftDetected { machine: String, detail: String },

    // --- Inference ---
    #[error("inference unavailable: {0}")]
    InferenceUnavailable(String),

    // --- Sync ---
    #[error("sync error: {0}")]
    SyncError(String),

    // --- Mesh ---
    #[error("mesh error: {0}")]
    MeshError(String),

    // --- Containment ---
    #[error("containment not available: {0}")]
    ContainmentUnavailable(String),

    // --- Transport ---
    #[error(transparent)]
    Transport(#[from] tonic::Status),

    // --- Internal ---
    #[error("internal error: {0}")]
    Internal(String),
}
```

## Error -> gRPC Status Mapping

| KithError Variant | gRPC Status Code | When |
|-------------------|-----------------|------|
| Unauthenticated | UNAUTHENTICATED | No credentials in request |
| PolicyDenied | PERMISSION_DENIED | Scope insufficient for action |
| CredentialsExpired | UNAUTHENTICATED | Credential validation failed |
| MachineNotFound | NOT_FOUND | Machine ID not in mesh |
| PendingNotFound | NOT_FOUND | Pending change ID doesn't exist |
| CommitWindowExpired | FAILED_PRECONDITION | Commit attempted after window expiry |
| MachineUnreachable | UNAVAILABLE | WireGuard tunnel down, peer offline |
| DriftDetected | OK (informational) | Not an error — returned as event data |
| InferenceUnavailable | UNAVAILABLE | Backend unreachable (shell degrades to bash) |
| SyncError | INTERNAL | cr-sqlite replication failure |
| MeshError | INTERNAL | WireGuard/Nostr failure |
| ContainmentUnavailable | FAILED_PRECONDITION | cgroups/overlayfs not available (macOS) |
| Internal | INTERNAL | Unexpected internal failure |

## User-Facing Messages

| Scenario | Message | Recovery Hint |
|----------|---------|---------------|
| Policy denial | "Access denied: viewer scope cannot execute commands on staging-1" | Contact machine owner for ops scope |
| Machine unreachable | "staging-1 unreachable" | Check mesh connectivity |
| Commit window expired | "Pending change abc123 expired — auto-rolled back" | Re-apply the change |
| Inference unavailable | "Inference unavailable — pass-through mode" | Check LLM endpoint connectivity |
| Containment unavailable | "Containment features not available on macOS" | Use Linux for full containment |

## InferenceError (kith-common)

Separate from KithError because inference errors have unique semantics (rate limiting, context overflow). See [inference-backend.md](interfaces/inference-backend.md) for the full type definition.

| InferenceError | Shell Behavior |
|----------------|---------------|
| Unreachable | Degrade to bash (INV-OPS-2) |
| RateLimited | Wait and retry once, then degrade |
| ContextOverflow | Trigger compaction, retry with shorter context |
| MalformedResponse | Log, retry once, surface error if retry fails (FM-12) |
| Timeout | Degrade to bash |
| AuthFailed | Surface to user, suggest checking API key config |

## Error Propagation Rules

1. **Daemon -> Shell**: gRPC Status codes. Shell maps to KithError for local handling or user display.
2. **Observer -> Daemon**: `mpsc` channel. Channel close = observer stopped gracefully.
3. **Sync -> Daemon**: KithError::SyncError. Daemon logs and continues — sync failures don't block local operations (INV-OPS-3).
4. **Mesh -> Daemon**: KithError::MeshError. Daemon continues with reduced peer awareness.
5. **InferenceBackend -> Shell**: InferenceError. Shell decides degradation strategy.

## Never Panic

All crates use `Result<T, KithError>` or `Result<T, InferenceError>`. Panics are bugs. The only acceptable `unwrap()` is in test code.
