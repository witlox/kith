# Daemon Interfaces

gRPC service definition and internal trait interfaces for kith-daemon.

---

## gRPC Service (proto/kith/daemon/v1/daemon.proto)

See `proto/kith/daemon/v1/daemon.proto` for the canonical proto definition.

Key design points (ADR-006):
- All requests carry a `Credential` (Ed25519 pubkey + timestamp + signature)
- **Scope is never in the request.** Daemon looks up scope from `MachinePolicy.users` using the authenticated public key
- Credential validation: verify signature, check timestamp within ±30s, lookup pubkey in policy

## Internal Traits

### PolicyEvaluator

```rust
/// Per-machine, per-user policy enforcement. Enforced in Rust, not prompt.
/// Scope is looked up from MachinePolicy, never passed by the caller (ADR-006).
pub trait PolicyEvaluator: Send + Sync {
    /// Authenticate credential and check if the identity permits the action.
    /// Returns Deny if credential is invalid or scope insufficient.
    fn evaluate(&self, credential: &Credential, action: &Action) -> PolicyDecision;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Ops,
    Viewer,
}

#[derive(Debug, Clone)]
pub enum Action {
    Exec { command: String },
    Query,
    Apply { command: String },
    Commit,
    Rollback,
    Events,
    Capabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
}
```

### StateObserver

```rust
/// Watches machine state for changes. Produces ObserverEvents.
pub trait StateObserver: Send + Sync {
    /// Start observing. Events sent on the channel.
    async fn start(&self, tx: mpsc::Sender<ObserverEvent>) -> Result<(), KithError>;
    /// Stop observing.
    async fn stop(&self) -> Result<(), KithError>;
}

#[derive(Debug, Clone)]
pub struct ObserverEvent {
    pub category: DriftCategory,
    pub path: String,
    pub detail: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftCategory {
    Files,
    Services,
    Network,
    Packages,
}
```

### CommitWindowManager

```rust
/// Manages pending changes with auto-rollback on expiry.
pub trait CommitWindowManager: Send + Sync {
    /// Open a new pending change. Returns pending_id.
    fn open(&mut self, command: &str, duration: std::time::Duration) -> Result<String, KithError>;
    /// Commit a single pending change.
    fn commit(&mut self, pending_id: &str) -> Result<(), KithError>;
    /// Commit all pending changes atomically (F-06).
    fn commit_all(&mut self) -> Result<(), KithError>;
    /// Rollback a single pending change.
    fn rollback(&mut self, pending_id: &str) -> Result<(), KithError>;
    /// Rollback all pending changes.
    fn rollback_all(&mut self) -> Result<(), KithError>;
    /// Check for expired windows and auto-rollback.
    fn tick(&mut self) -> Vec<String>; // returns IDs of auto-rolled-back changes
    /// List pending changes.
    fn pending(&self) -> Vec<PendingChange>;
}

#[derive(Debug, Clone)]
pub struct PendingChange {
    pub id: String,
    pub command: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}
```
