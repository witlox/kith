//! E2e scenario 3: drift detection -> event store -> sync -> fleet query
//! E2e scenario 6: partition and recovery — independent ops, CRDT merge, no data loss

use kith_common::drift::{DriftCategory, DriftWeights};
use kith_common::event::{Event, EventCategory, EventScope};
use kith_daemon::drift::{DriftEvaluator, ObserverEvent};
use kith_state::retrieval::KeywordRetriever;
use kith_sync::store::{EventFilter, EventStore};

/// Helper: create a drift event from an observer event.
fn drift_to_event(machine: &str, obs: &ObserverEvent) -> Event {
    let event_type = match obs.category {
        DriftCategory::Files => "drift.file_changed",
        DriftCategory::Services => "drift.service_stopped",
        DriftCategory::Network => "drift.port_closed",
        DriftCategory::Packages => "drift.package_installed",
    };
    Event::new(machine, EventCategory::Drift, event_type, &obs.detail)
        .with_path(&obs.path)
        .with_scope(EventScope::Public)
        .with_metadata(serde_json::json!({
            "category": format!("{:?}", obs.category),
            "path": obs.path,
        }))
}

// ---------------------------------------------------------------------------
// Scenario 3: Drift detection -> event store -> fleet query -> retrieval
// ---------------------------------------------------------------------------

/// Drift detected on staging-1, synced to dev-mac's event store,
/// agent on dev-mac retrieves via fleet_query / retrieve.
#[tokio::test]
async fn e2e_drift_to_sync_to_retrieval() {
    // staging-1's drift evaluator detects changes
    let blacklist = vec!["/tmp/**".into(), "/var/log/**".into()];
    let mut evaluator = DriftEvaluator::new(blacklist, DriftWeights::default());

    let obs_event = ObserverEvent {
        category: DriftCategory::Files,
        path: "/etc/nginx/conf.d/api.conf".into(),
        detail: "nginx config modified manually".into(),
        timestamp: chrono::Utc::now(),
    };

    // Drift evaluator processes the event (not blacklisted)
    assert!(evaluator.process_event(&obs_event));
    assert!(evaluator.magnitude_sq() > 0.0);

    // staging-1 writes drift event to its local event store
    let staging_store = EventStore::new();
    let drift_event = drift_to_event("staging-1", &obs_event);
    staging_store.write(drift_event).await;

    // Simulate sync: staging-1's events merge into dev-mac's store
    let dev_store = EventStore::new();
    let staging_events = staging_store.all().await;
    let merged = dev_store.merge(staging_events).await;
    assert_eq!(merged, 1);

    // dev-mac can now query fleet state
    let drift_events = dev_store
        .query(&EventFilter {
            category: Some(EventCategory::Drift),
            machine: Some("staging-1".into()),
            ..Default::default()
        })
        .await;
    assert_eq!(drift_events.len(), 1);
    assert_eq!(drift_events[0].machine, "staging-1");

    // Agent on dev-mac retrieves via semantic search
    let all = dev_store.all().await;
    let results = KeywordRetriever::search(&all, "nginx config", &EventScope::Ops, 10);
    assert_eq!(results.len(), 1);
    assert!(results[0].event.detail.contains("nginx"));
}

/// Multiple drift categories accumulate and are queryable.
#[tokio::test]
async fn e2e_drift_multiple_categories() {
    let mut evaluator = DriftEvaluator::new(vec![], DriftWeights::default());
    let store = EventStore::new();

    let events = vec![
        ObserverEvent {
            category: DriftCategory::Files,
            path: "/etc/config".into(),
            detail: "config changed".into(),
            timestamp: chrono::Utc::now(),
        },
        ObserverEvent {
            category: DriftCategory::Services,
            path: "nginx".into(),
            detail: "nginx stopped".into(),
            timestamp: chrono::Utc::now(),
        },
        ObserverEvent {
            category: DriftCategory::Network,
            path: "port:8080".into(),
            detail: "port 8080 closed".into(),
            timestamp: chrono::Utc::now(),
        },
    ];

    for obs in &events {
        evaluator.process_event(obs);
        store.write(drift_to_event("staging-1", obs)).await;
    }

    // Drift vector has all 3 categories
    let dv = evaluator.drift_vector();
    assert_eq!(dv.files, 1.0);
    assert_eq!(dv.services, 1.0);
    assert_eq!(dv.network, 1.0);

    // All 3 events in store
    assert_eq!(store.len().await, 3);

    // Can search for specific category
    let all = store.all().await;
    let nginx_results = KeywordRetriever::search(&all, "nginx stopped", &EventScope::Ops, 10);
    assert_eq!(nginx_results.len(), 1);
}

/// Blacklisted paths don't produce events.
#[tokio::test]
async fn e2e_drift_blacklist_prevents_events() {
    let mut evaluator = DriftEvaluator::new(
        vec!["/tmp/**".into(), "/var/log/**".into()],
        DriftWeights::default(),
    );
    let store = EventStore::new();

    let noisy = ObserverEvent {
        category: DriftCategory::Files,
        path: "/tmp/scratch/output".into(),
        detail: "temp file changed".into(),
        timestamp: chrono::Utc::now(),
    };

    // Blacklisted — evaluator returns false, no event written
    assert!(!evaluator.process_event(&noisy));
    // Don't write blacklisted events to store

    let real = ObserverEvent {
        category: DriftCategory::Files,
        path: "/etc/nginx/nginx.conf".into(),
        detail: "real config change".into(),
        timestamp: chrono::Utc::now(),
    };

    assert!(evaluator.process_event(&real));
    store.write(drift_to_event("staging-1", &real)).await;

    assert_eq!(store.len().await, 1);
}

// ---------------------------------------------------------------------------
// Scenario 6: Partition and recovery
// ---------------------------------------------------------------------------

/// Two stores operate independently during partition, then merge with no data loss.
#[tokio::test]
async fn e2e_partition_and_recovery() {
    let store_a = EventStore::new();
    let store_b = EventStore::new();

    // Both stores accumulate events independently (partition)
    for i in 0..5 {
        store_a
            .write(
                Event::new(
                    "dev-mac",
                    EventCategory::Exec,
                    "exec.command",
                    format!("command-a-{i}"),
                )
                .with_scope(EventScope::Ops),
            )
            .await;
    }

    for i in 0..3 {
        store_b
            .write(
                Event::new(
                    "staging-1",
                    EventCategory::Exec,
                    "exec.command",
                    format!("command-b-{i}"),
                )
                .with_scope(EventScope::Ops),
            )
            .await;
    }

    assert_eq!(store_a.len().await, 5);
    assert_eq!(store_b.len().await, 3);

    // Connectivity restored: bidirectional merge
    let a_events = store_a.all().await;
    let b_events = store_b.all().await;

    let merged_into_b = store_b.merge(a_events).await;
    let merged_into_a = store_a.merge(b_events).await;

    assert_eq!(merged_into_b, 5); // B got A's 5 events
    assert_eq!(merged_into_a, 3); // A got B's 3 events

    // Both stores now have all 8 events
    assert_eq!(store_a.len().await, 8);
    assert_eq!(store_b.len().await, 8);

    // No data loss — all events from both sides present
    let all_a = store_a.all().await;
    let a_machines: Vec<&str> = all_a.iter().map(|e| e.machine.as_str()).collect();
    assert!(a_machines.contains(&"dev-mac"));
    assert!(a_machines.contains(&"staging-1"));
}

/// Partition merge is idempotent — re-merging doesn't duplicate.
#[tokio::test]
async fn e2e_merge_idempotent() {
    let store_a = EventStore::new();
    let store_b = EventStore::new();

    store_a
        .write(
            Event::new("dev-mac", EventCategory::Exec, "exec.command", "cmd-1")
                .with_scope(EventScope::Ops),
        )
        .await;

    // First merge
    let events = store_a.all().await;
    let merged1 = store_b.merge(events).await;
    assert_eq!(merged1, 1);
    assert_eq!(store_b.len().await, 1);

    // Second merge — same events, should be deduplicated
    let events = store_a.all().await;
    let merged2 = store_b.merge(events).await;
    assert_eq!(merged2, 0);
    assert_eq!(store_b.len().await, 1);
}

/// Events accumulated during long partition all sync on reconnect.
#[tokio::test]
async fn e2e_long_partition_recovery() {
    let store_a = EventStore::new();
    let store_b = EventStore::new();

    // Simulate 24h of events on each side (100 events each)
    for i in 0..100 {
        store_a
            .write(
                Event::new(
                    "dev-mac",
                    EventCategory::System,
                    "system.heartbeat",
                    format!("heartbeat-a-{i}"),
                )
                .with_scope(EventScope::Ops),
            )
            .await;
        store_b
            .write(
                Event::new(
                    "staging-1",
                    EventCategory::System,
                    "system.heartbeat",
                    format!("heartbeat-b-{i}"),
                )
                .with_scope(EventScope::Ops),
            )
            .await;
    }

    // Merge both directions
    let a_events = store_a.all().await;
    let b_events = store_b.all().await;
    store_b.merge(a_events).await;
    store_a.merge(b_events).await;

    // Both have all 200
    assert_eq!(store_a.len().await, 200);
    assert_eq!(store_b.len().await, 200);
}
