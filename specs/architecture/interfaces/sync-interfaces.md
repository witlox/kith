# Sync Interfaces

cr-sqlite CRDT replication interfaces.

---

## SyncEngine

```rust
/// Manages cr-sqlite replication between peers.
pub trait SyncEngine: Send + Sync {
    /// Initialize the local database with CRDT tables.
    async fn init(&self) -> Result<(), KithError>;

    /// Write an event to the local store.
    async fn write_event(&self, event: &Event) -> Result<(), KithError>;

    /// Read events matching criteria.
    async fn query_events(&self, filter: &EventFilter) -> Result<Vec<Event>, KithError>;

    /// Get changes since a given version for a specific peer.
    async fn get_changes_since(&self, peer_id: &str, version: i64) -> Result<Vec<u8>, KithError>;

    /// Apply changes received from a peer.
    async fn apply_changes(&self, peer_id: &str, changes: &[u8]) -> Result<(), KithError>;

    /// Subscribe to new local events (for embedding pipeline).
    fn subscribe(&self) -> broadcast::Receiver<Event>;
}

/// Filter criteria for event queries.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    pub machine: Option<String>,
    pub category: Option<String>,
    pub scope: Option<Scope>,
    pub limit: Option<u32>,
}
```

## Replication Protocol

Peer-to-peer delta exchange over the WireGuard mesh:

1. Peer A connects to Peer B's sync endpoint
2. A sends its last-known version for B
3. B responds with all changes since that version (cr-sqlite changeset)
4. A applies the changeset locally
5. Symmetric: B also requests A's changes

Sync runs on a configurable interval (default: 5 seconds). On reconnect after partition, full delta exchange catches up.

## SQLite Schema

```sql
-- Core events table (CRDT-compatible via cr-sqlite)
CREATE TABLE events (
    id TEXT PRIMARY KEY,            -- UUID
    machine TEXT NOT NULL,          -- origin hostname
    category TEXT NOT NULL,         -- event category
    event_type TEXT NOT NULL,       -- specific event type
    path TEXT,                      -- affected path (if applicable)
    detail TEXT,                    -- human-readable detail
    metadata TEXT,                  -- JSON metadata
    scope TEXT NOT NULL DEFAULT 'ops', -- access scope
    timestamp INTEGER NOT NULL,     -- unix timestamp (ms)

    -- cr-sqlite will add its own tracking columns
    UNIQUE(id)
);

-- Capability reports (latest per machine)
CREATE TABLE capabilities (
    machine TEXT PRIMARY KEY,
    report TEXT NOT NULL,           -- JSON CapabilityReport
    updated_at INTEGER NOT NULL
);

-- Drift state (latest per machine)
CREATE TABLE drift_state (
    machine TEXT PRIMARY KEY,
    drift_vector TEXT NOT NULL,     -- JSON DriftVector
    magnitude REAL NOT NULL,
    updated_at INTEGER NOT NULL
);
```
