//! In-memory event store. Production will use cr-sqlite (ADR-001).
//! This provides the SyncEngine interface that all consumers depend on,
//! backed by an in-memory vec for now. cr-sqlite wiring is additive.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{RwLock, broadcast};

use kith_common::event::{Event, EventCategory, EventScope};

/// Filter criteria for event queries.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub since: Option<DateTime<Utc>>,
    pub machine: Option<String>,
    pub category: Option<EventCategory>,
    pub event_type: Option<String>,
    pub scope: Option<EventScope>,
    pub limit: Option<usize>,
}

/// In-memory event store with broadcast subscription.
/// Satisfies the SyncEngine interface. cr-sqlite replaces the vec later.
pub struct EventStore {
    events: Arc<RwLock<Vec<Event>>>,
    tx: broadcast::Sender<Event>,
}

impl EventStore {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
            tx,
        }
    }

    /// Write an event to the store.
    pub async fn write(&self, event: Event) {
        self.events.write().await.push(event.clone());
        let _ = self.tx.send(event);
    }

    /// Query events matching filter criteria.
    pub async fn query(&self, filter: &EventFilter) -> Vec<Event> {
        let events = self.events.read().await;
        let mut results: Vec<&Event> = events
            .iter()
            .filter(|e| {
                if let Some(ref since) = filter.since
                    && e.timestamp < *since {
                        return false;
                    }
                if let Some(ref machine) = filter.machine
                    && e.machine != *machine {
                        return false;
                    }
                if let Some(ref category) = filter.category
                    && e.category != *category {
                        return false;
                    }
                if let Some(ref event_type) = filter.event_type
                    && e.event_type != *event_type {
                        return false;
                    }
                if let Some(ref scope) = filter.scope {
                    match scope {
                        EventScope::Ops => {} // ops sees everything
                        EventScope::Public => {
                            if e.scope != EventScope::Public {
                                return false;
                            }
                        }
                    }
                }
                true
            })
            .collect();

        // Sort by timestamp descending (newest first)
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        results.into_iter().cloned().collect()
    }

    /// Get all events (unfiltered).
    pub async fn all(&self) -> Vec<Event> {
        self.events.read().await.clone()
    }

    /// Count events.
    pub async fn len(&self) -> usize {
        self.events.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.events.read().await.is_empty()
    }

    /// Subscribe to new events.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    /// Merge events from a peer (simulates CRDT merge).
    /// Deduplicates by event ID.
    pub async fn merge(&self, peer_events: Vec<Event>) -> usize {
        let mut store = self.events.write().await;
        let existing_ids: std::collections::HashSet<String> =
            store.iter().map(|e| e.id.clone()).collect();

        let mut merged = 0;
        for event in peer_events {
            if !existing_ids.contains(&event.id) {
                let _ = self.tx.send(event.clone());
                store.push(event);
                merged += 1;
            }
        }
        merged
    }
}

impl Default for EventStore {
    fn default() -> Self {
        Self::new()
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
    async fn write_and_query_all() {
        let store = EventStore::new();
        store.write(drift_event("staging-1", "/etc/config")).await;
        store.write(exec_event("staging-1", "docker ps")).await;

        assert_eq!(store.len().await, 2);
        let all = store.all().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn filter_by_machine() {
        let store = EventStore::new();
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
        let store = EventStore::new();
        store.write(drift_event("staging-1", "/a")).await;
        store.write(exec_event("staging-1", "cmd")).await;

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
    async fn filter_by_scope_public_excludes_ops() {
        let store = EventStore::new();
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

        // Ops sees everything
        let ops = store
            .query(&EventFilter {
                scope: Some(EventScope::Ops),
                ..Default::default()
            })
            .await;
        assert_eq!(ops.len(), 2);
    }

    #[tokio::test]
    async fn filter_with_limit() {
        let store = EventStore::new();
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
    async fn subscribe_receives_new_events() {
        let store = EventStore::new();
        let mut rx = store.subscribe();

        store.write(drift_event("staging-1", "/etc/config")).await;

        let event = rx.try_recv().unwrap();
        assert_eq!(event.machine, "staging-1");
    }

    #[tokio::test]
    async fn merge_deduplicates_by_id() {
        let store = EventStore::new();
        let event = drift_event("staging-1", "/a");

        store.write(event.clone()).await;
        assert_eq!(store.len().await, 1);

        // Merge same event again — should be deduplicated
        let merged = store.merge(vec![event]).await;
        assert_eq!(merged, 0);
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn merge_adds_new_events() {
        let store = EventStore::new();
        store.write(drift_event("staging-1", "/a")).await;

        let new_event = drift_event("prod-1", "/b");
        let merged = store.merge(vec![new_event]).await;
        assert_eq!(merged, 1);
        assert_eq!(store.len().await, 2);
    }

    #[tokio::test]
    async fn merge_notifies_subscribers() {
        let store = EventStore::new();
        let mut rx = store.subscribe();

        let new_event = drift_event("staging-1", "/a");
        store.merge(vec![new_event]).await;

        let received = rx.try_recv().unwrap();
        assert_eq!(received.machine, "staging-1");
    }

    #[tokio::test]
    async fn filter_by_event_type() {
        let store = EventStore::new();
        store.write(drift_event("s", "/a")).await;
        store
            .write(
                Event::new(
                    "s",
                    EventCategory::Drift,
                    "drift.service_stopped",
                    "nginx stopped",
                )
                .with_scope(EventScope::Public),
            )
            .await;

        let results = store
            .query(&EventFilter {
                event_type: Some("drift.service_stopped".into()),
                ..Default::default()
            })
            .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, "drift.service_stopped");
    }

    #[tokio::test]
    async fn empty_store() {
        let store = EventStore::new();
        assert!(store.is_empty().await);
        assert_eq!(store.len().await, 0);
        let results = store.query(&EventFilter::default()).await;
        assert!(results.is_empty());
    }
}
