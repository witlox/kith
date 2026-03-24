//! SQLite-backed EventStore. Persistent, supports cr-sqlite extension for CRDT sync.
//!
//! cr-sqlite is loaded as a runtime extension if available. Without it,
//! the store works as plain SQLite (no CRDT merge, but still persistent).

use std::path::Path;
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use rusqlite::{Connection, params};
use tokio::sync::{Mutex, broadcast};
use tracing::{info, warn};

use kith_common::event::{Event, EventCategory, EventScope};

use crate::store::EventFilter;

/// SQLite-backed event store. Thread-safe via Mutex.
pub struct SqliteEventStore {
    conn: Arc<Mutex<Connection>>,
    tx: broadcast::Sender<Event>,
    crsql_available: bool,
}

impl SqliteEventStore {
    /// Open or create a SQLite database at the given path.
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Create an in-memory SQLite database (for testing).
    pub fn in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, rusqlite::Error> {
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                machine TEXT NOT NULL,
                category TEXT NOT NULL,
                event_type TEXT NOT NULL,
                path TEXT,
                detail TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                scope TEXT NOT NULL DEFAULT 'Ops',
                timestamp_ms INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_machine ON events(machine);
            CREATE INDEX IF NOT EXISTS idx_events_category ON events(category);
            CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp_ms);",
        )?;

        // Try to load cr-sqlite extension
        let crsql_available = Self::try_load_crsqlite(&conn);

        let (tx, _) = broadcast::channel(256);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            tx,
            crsql_available,
        })
    }

    fn try_load_crsqlite(conn: &Connection) -> bool {
        // cr-sqlite extension paths to try
        let paths = [
            "crsqlite",
            "./crsqlite",
            "/usr/local/lib/crsqlite",
            "/usr/lib/crsqlite",
        ];

        // SAFETY: load_extension requires unsafe per rusqlite's FFI contract.
        // The extension paths are hardcoded constants — no user input reaches here.
        // Extension loading is disabled immediately after loading to prevent
        // further dynamic library loads through SQL injection.
        unsafe {
            for path in &paths {
                if conn.load_extension_enable().is_ok() {
                    if conn.load_extension(path, None::<&str>).is_ok() {
                        // Enable CRDT on events table
                        if conn.execute_batch("SELECT crsql_as_crr('events');").is_ok() {
                            info!("cr-sqlite loaded — CRDT sync enabled");
                            let _ = conn.load_extension_disable();
                            return true;
                        }
                    }
                    let _ = conn.load_extension_disable();
                }
            }
        }

        warn!("cr-sqlite not available — running without CRDT sync");
        false
    }

    pub fn is_crdt_enabled(&self) -> bool {
        self.crsql_available
    }

    /// Write an event to the store.
    pub async fn write(&self, event: Event) {
        let conn = self.conn.lock().await;
        let timestamp_ms = event.timestamp.timestamp_millis();

        let result = conn.execute(
            "INSERT OR IGNORE INTO events (id, machine, category, event_type, path, detail, metadata, scope, timestamp_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                event.id,
                event.machine,
                format!("{:?}", event.category),
                event.event_type,
                event.path,
                event.detail,
                event.metadata.to_string(),
                format!("{:?}", event.scope),
                timestamp_ms,
            ],
        );

        if let Err(e) = result {
            tracing::error!(error = %e, "failed to write event to SQLite");
            return;
        }

        let _ = self.tx.send(event);
    }

    /// Query events matching filter criteria.
    pub async fn query(&self, filter: &EventFilter) -> Vec<Event> {
        let conn = self.conn.lock().await;

        let mut sql = String::from(
            "SELECT id, machine, category, event_type, path, detail, metadata, scope, timestamp_ms FROM events WHERE 1=1",
        );
        let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref since) = filter.since {
            sql.push_str(" AND timestamp_ms >= ?");
            bind_values.push(Box::new(since.timestamp_millis()));
        }
        if let Some(ref machine) = filter.machine {
            sql.push_str(" AND machine = ?");
            bind_values.push(Box::new(machine.clone()));
        }
        if let Some(ref category) = filter.category {
            sql.push_str(" AND category = ?");
            bind_values.push(Box::new(format!("{category:?}")));
        }
        if let Some(ref event_type) = filter.event_type {
            sql.push_str(" AND event_type = ?");
            bind_values.push(Box::new(event_type.clone()));
        }
        if let Some(ref scope) = filter.scope {
            match scope {
                EventScope::Public => {
                    sql.push_str(" AND scope = 'Public'");
                }
                EventScope::Ops => {} // ops sees everything
            }
        }

        sql.push_str(" ORDER BY timestamp_ms DESC");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        let refs: Vec<&dyn rusqlite::types::ToSql> =
            bind_values.iter().map(|b| b.as_ref()).collect();

        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "failed to prepare query");
                return Vec::new();
            }
        };

        let rows = stmt.query_map(refs.as_slice(), |row| Ok(row_to_event(row)));

        match rows {
            Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                tracing::error!(error = %e, "failed to query events");
                Vec::new()
            }
        }
    }

    /// Get all events.
    pub async fn all(&self) -> Vec<Event> {
        self.query(&EventFilter::default()).await
    }

    /// Count events.
    pub async fn len(&self) -> usize {
        let conn = self.conn.lock().await;
        conn.query_row("SELECT COUNT(*) FROM events", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize
    }

    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Subscribe to new events.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    /// Merge events (deduplicates by ID via INSERT OR IGNORE).
    pub async fn merge(&self, events: Vec<Event>) -> usize {
        let conn = self.conn.lock().await;
        let mut merged = 0;

        for event in events {
            let timestamp_ms = event.timestamp.timestamp_millis();
            let result = conn.execute(
                "INSERT OR IGNORE INTO events (id, machine, category, event_type, path, detail, metadata, scope, timestamp_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    event.id,
                    event.machine,
                    format!("{:?}", event.category),
                    event.event_type,
                    event.path,
                    event.detail,
                    event.metadata.to_string(),
                    format!("{:?}", event.scope),
                    timestamp_ms,
                ],
            );

            if let Ok(changes) = result
                && changes > 0
            {
                let _ = self.tx.send(event);
                merged += 1;
            }
        }

        merged
    }

    /// Get cr-sqlite changes for sync (if CRDT enabled).
    pub async fn get_changes_since(&self, version: i64) -> Vec<u8> {
        if !self.crsql_available {
            return Vec::new();
        }
        let conn = self.conn.lock().await;
        let mut stmt = match conn.prepare("SELECT * FROM crsql_changes WHERE db_version > ?") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows: Vec<String> =
            match stmt.query_map(params![version], |row| Ok(format!("{:?}", row))) {
                Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
        serde_json::to_vec(&rows).unwrap_or_default()
    }
}

fn row_to_event(row: &rusqlite::Row) -> Event {
    let id: String = row.get(0).unwrap_or_default();
    let machine: String = row.get(1).unwrap_or_default();
    let category_str: String = row.get(2).unwrap_or_default();
    let event_type: String = row.get(3).unwrap_or_default();
    let path: Option<String> = row.get(4).ok();
    let detail: String = row.get(5).unwrap_or_default();
    let metadata_str: String = row.get(6).unwrap_or_default();
    let scope_str: String = row.get(7).unwrap_or_default();
    let timestamp_ms: i64 = row.get(8).unwrap_or(0);

    let category = match category_str.as_str() {
        "Drift" => EventCategory::Drift,
        "Exec" => EventCategory::Exec,
        "Apply" => EventCategory::Apply,
        "Commit" => EventCategory::Commit,
        "Rollback" => EventCategory::Rollback,
        "Policy" => EventCategory::Policy,
        "Mesh" => EventCategory::Mesh,
        "Capability" => EventCategory::Capability,
        _ => EventCategory::System,
    };

    let scope = match scope_str.as_str() {
        "Public" => EventScope::Public,
        _ => EventScope::Ops,
    };

    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_str).unwrap_or(serde_json::Value::Null);

    let timestamp = Utc
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .unwrap_or_else(Utc::now);

    Event {
        id,
        machine,
        category,
        event_type,
        path,
        detail,
        metadata,
        scope,
        timestamp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kith_common::event::Event;

    fn drift_event(machine: &str, path: &str) -> Event {
        Event::new(machine, EventCategory::Drift, "drift.file_changed", path)
            .with_path(path)
            .with_scope(EventScope::Public)
    }

    fn exec_event(machine: &str, command: &str) -> Event {
        Event::new(machine, EventCategory::Exec, "exec.command", command)
            .with_scope(EventScope::Ops)
    }

    #[tokio::test]
    async fn write_and_read() {
        let store = SqliteEventStore::in_memory().unwrap();
        store.write(drift_event("staging-1", "/etc/config")).await;
        store.write(exec_event("staging-1", "docker ps")).await;

        assert_eq!(store.len().await, 2);
        let all = store.all().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn filter_by_machine() {
        let store = SqliteEventStore::in_memory().unwrap();
        store.write(drift_event("staging-1", "/a")).await;
        store.write(drift_event("prod-1", "/b")).await;

        let results = store
            .query(&EventFilter {
                machine: Some("staging-1".into()),
                ..Default::default()
            })
            .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].machine, "staging-1");
    }

    #[tokio::test]
    async fn filter_by_category() {
        let store = SqliteEventStore::in_memory().unwrap();
        store.write(drift_event("s", "/a")).await;
        store.write(exec_event("s", "cmd")).await;

        let results = store
            .query(&EventFilter {
                category: Some(EventCategory::Drift),
                ..Default::default()
            })
            .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].category, EventCategory::Drift);
    }

    #[tokio::test]
    async fn filter_by_scope() {
        let store = SqliteEventStore::in_memory().unwrap();
        store.write(drift_event("s", "/a")).await; // Public
        store.write(exec_event("s", "cmd")).await; // Ops

        let public = store
            .query(&EventFilter {
                scope: Some(EventScope::Public),
                ..Default::default()
            })
            .await;
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].scope, EventScope::Public);
    }

    #[tokio::test]
    async fn filter_with_limit() {
        let store = SqliteEventStore::in_memory().unwrap();
        for i in 0..10 {
            store.write(drift_event("s", &format!("/path-{i}"))).await;
        }
        let results = store
            .query(&EventFilter {
                limit: Some(3),
                ..Default::default()
            })
            .await;
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn merge_deduplicates() {
        let store = SqliteEventStore::in_memory().unwrap();
        let event = drift_event("staging-1", "/a");
        store.write(event.clone()).await;

        let merged = store.merge(vec![event]).await;
        assert_eq!(merged, 0); // duplicate ignored
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn merge_adds_new() {
        let store = SqliteEventStore::in_memory().unwrap();
        store.write(drift_event("staging-1", "/a")).await;

        let merged = store.merge(vec![drift_event("prod-1", "/b")]).await;
        assert_eq!(merged, 1);
        assert_eq!(store.len().await, 2);
    }

    #[tokio::test]
    async fn subscribe_receives() {
        let store = SqliteEventStore::in_memory().unwrap();
        let mut rx = store.subscribe();
        store.write(drift_event("staging-1", "/a")).await;

        let event = rx.try_recv().unwrap();
        assert_eq!(event.machine, "staging-1");
    }

    #[tokio::test]
    async fn persistent_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        {
            let store = SqliteEventStore::open(&db_path).unwrap();
            store.write(drift_event("staging-1", "/etc/config")).await;
            store.write(exec_event("staging-1", "docker ps")).await;
            assert_eq!(store.len().await, 2);
        }

        // Reopen — data should persist
        {
            let store = SqliteEventStore::open(&db_path).unwrap();
            assert_eq!(store.len().await, 2);
            let all = store.all().await;
            assert_eq!(all[0].machine, "staging-1");
        }
    }

    #[tokio::test]
    async fn roundtrip_preserves_fields() {
        let store = SqliteEventStore::in_memory().unwrap();
        let event = Event::new(
            "staging-1",
            EventCategory::Drift,
            "drift.file_changed",
            "config modified",
        )
        .with_path("/etc/nginx/api.conf")
        .with_metadata(serde_json::json!({"change": "modified"}))
        .with_scope(EventScope::Public);

        let original_id = event.id.clone();
        store.write(event).await;

        let all = store.all().await;
        assert_eq!(all.len(), 1);
        let e = &all[0];
        assert_eq!(e.id, original_id);
        assert_eq!(e.machine, "staging-1");
        assert_eq!(e.category, EventCategory::Drift);
        assert_eq!(e.event_type, "drift.file_changed");
        assert_eq!(e.path.as_deref(), Some("/etc/nginx/api.conf"));
        assert_eq!(e.detail, "config modified");
        assert_eq!(e.metadata["change"], "modified");
        assert_eq!(e.scope, EventScope::Public);
    }

    #[tokio::test]
    async fn crdt_not_available_gracefully() {
        let store = SqliteEventStore::in_memory().unwrap();
        assert!(!store.is_crdt_enabled()); // cr-sqlite not installed in test env
        // Should still work as plain SQLite
        store.write(drift_event("s", "/a")).await;
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn every_event_category_roundtrips() {
        let store = SqliteEventStore::in_memory().unwrap();
        let categories = vec![
            (EventCategory::Drift, "drift.test"),
            (EventCategory::Exec, "exec.test"),
            (EventCategory::Apply, "apply.test"),
            (EventCategory::Commit, "commit.test"),
            (EventCategory::Rollback, "rollback.test"),
            (EventCategory::Policy, "policy.test"),
            (EventCategory::Mesh, "mesh.test"),
            (EventCategory::Capability, "capability.test"),
            (EventCategory::System, "system.test"),
        ];

        for (category, event_type) in &categories {
            let event = Event::new("test", category.clone(), *event_type, "roundtrip test")
                .with_scope(EventScope::Ops);
            store.write(event).await;
        }

        let all = store.all().await;
        assert_eq!(all.len(), categories.len());

        for (expected_cat, event_type) in &categories {
            let found = all
                .iter()
                .find(|e| e.event_type == *event_type)
                .unwrap_or_else(|| panic!("missing event type: {event_type}"));
            assert_eq!(
                &found.category, expected_cat,
                "category mismatch for {event_type}: expected {expected_cat:?}, got {:?}",
                found.category
            );
        }
    }
}
