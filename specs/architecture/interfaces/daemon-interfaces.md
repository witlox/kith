# Daemon Interfaces

gRPC service definition and internal trait interfaces for kith-daemon.

---

## gRPC Service (proto/kith/daemon/v1/daemon.proto)

```protobuf
syntax = "proto3";
package kith.daemon.v1;

service KithDaemon {
  // Execute a command on this machine. Output streams back.
  rpc Exec(ExecRequest) returns (stream ExecOutput);

  // Query machine state (non-destructive).
  rpc Query(QueryRequest) returns (QueryResponse);

  // Apply a change with commit window semantics.
  rpc Apply(ApplyRequest) returns (ApplyResponse);

  // Commit a pending change.
  rpc Commit(CommitRequest) returns (CommitResponse);

  // Rollback a pending change.
  rpc Rollback(RollbackRequest) returns (RollbackResponse);

  // Stream events since a timestamp.
  rpc Events(EventsRequest) returns (stream Event);

  // Get capability report for this machine.
  rpc Capabilities(CapabilitiesRequest) returns (CapabilityReport);
}

message ExecRequest {
  string command = 1;
  string user_identity = 2;   // who is requesting
  string scope = 3;           // "ops" or "viewer"
}

message ExecOutput {
  oneof output {
    string stdout = 1;
    string stderr = 2;
  }
  int32 exit_code = 3;        // only set on final message
  bool done = 4;
}

message QueryRequest {
  enum QueryType {
    STATE = 0;       // processes, services, disk, network
    DRIFT = 1;       // current drift vector
    PENDING = 2;     // pending changes
  }
  QueryType query_type = 1;
  string user_identity = 2;
  string scope = 3;
}

message QueryResponse {
  string json_payload = 1;    // structured state as JSON
}

message ApplyRequest {
  string command = 1;
  string user_identity = 2;
  string scope = 3;
  uint32 commit_window_seconds = 4;  // 0 = use default
}

message ApplyResponse {
  string pending_id = 1;
  uint64 expires_at_unix = 2;
}

message CommitRequest {
  string pending_id = 1;
  string user_identity = 2;
}

message CommitResponse {
  bool success = 1;
  string message = 2;
}

message RollbackRequest {
  string pending_id = 1;
  string user_identity = 2;
}

message RollbackResponse {
  bool success = 1;
  string message = 2;
}

message EventsRequest {
  uint64 since_unix = 1;
  string user_identity = 2;
  string scope = 3;
}

message CapabilitiesRequest {
  string user_identity = 1;
}
```

## Internal Traits

### PolicyEvaluator

```rust
/// Per-machine, per-user policy enforcement. Enforced in Rust, not prompt.
pub trait PolicyEvaluator: Send + Sync {
    /// Check if identity+scope permits the given action.
    fn evaluate(&self, identity: &str, scope: &Scope, action: &Action) -> PolicyDecision;
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
    /// Commit a pending change.
    fn commit(&mut self, pending_id: &str) -> Result<(), KithError>;
    /// Rollback a pending change.
    fn rollback(&mut self, pending_id: &str) -> Result<(), KithError>;
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
