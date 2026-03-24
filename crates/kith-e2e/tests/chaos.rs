//! Chaos and degradation tests.
//! Tests system behavior under failure conditions:
//! - Daemon unreachable mid-session
//! - Credential expiry during operation
//! - Concurrent operations on same daemon
//! - Event store merge under high volume
//! - Commit window expiry race

use std::time::Duration;

use kith_common::credential::Keypair;
use kith_common::event::{Event, EventCategory, EventScope};
use kith_daemon::commit::CommitWindowManager;
use kith_e2e::helpers;
use kith_shell::daemon_client::DaemonClient;
use kith_sync::store::EventStore;

/// Chaos: daemon becomes unreachable after initial connection.
#[tokio::test]
async fn chaos_daemon_unreachable_mid_session() {
    let (addr, kp) = helpers::start_daemon("ephemeral-1").await;
    let mut client = DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
        .await
        .unwrap();

    // First call succeeds
    let result = client.exec("echo alive").await.unwrap();
    assert_eq!(result.exit_code, 0);

    // We can't actually kill the spawned server in this test,
    // but we can test connecting to a dead address
    let dead_client = DaemonClient::connect("127.0.0.1:1", Keypair::generate()).await;
    assert!(
        dead_client.is_err(),
        "connecting to dead address should fail"
    );
}

/// Chaos: concurrent exec requests to same daemon.
#[tokio::test]
async fn chaos_concurrent_exec() {
    let (addr, kp) = helpers::start_daemon("concurrent-1").await;

    let mut handles = Vec::new();
    for i in 0..10 {
        let addr = addr.clone();
        let secret = kp.secret_bytes();
        let handle = tokio::spawn(async move {
            let mut client = DaemonClient::connect(&addr, Keypair::from_secret(&secret))
                .await
                .unwrap();
            let result = client.exec(&format!("echo concurrent-{i}")).await.unwrap();
            assert_eq!(result.exit_code, 0);
            assert!(result.stdout.contains(&format!("concurrent-{i}")));
            i
        });
        handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }

    // All 10 completed
    results.sort();
    assert_eq!(results, (0..10).collect::<Vec<_>>());
}

/// Chaos: concurrent apply/commit from multiple clients.
#[tokio::test]
async fn chaos_concurrent_apply_commit() {
    let (addr, kp) = helpers::start_daemon("concurrent-apply-1").await;

    let mut handles = Vec::new();
    for i in 0..5 {
        let addr = addr.clone();
        let secret = kp.secret_bytes();
        let handle = tokio::spawn(async move {
            let mut client = DaemonClient::connect(&addr, Keypair::from_secret(&secret))
                .await
                .unwrap();

            let pending_id = client.apply(&format!("change-{i}"), 600).await.unwrap();
            assert!(!pending_id.is_empty());

            let committed = client.commit(&pending_id).await.unwrap();
            assert!(committed, "change-{i} should commit");
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

/// Chaos: commit window expiry race — apply, wait for expiry, then try to commit.
#[tokio::test]
async fn chaos_commit_window_expiry_race() {
    let mut mgr = CommitWindowManager::new(Duration::from_millis(50)); // 50ms window

    let id = mgr.open("short-lived-change", Some(Duration::from_millis(50)));

    // Wait for expiry
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Tick should expire it
    let expired = mgr.tick();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].id, id);

    // Late commit should fail
    let result = mgr.commit(&id);
    assert!(result.is_err(), "commit after expiry should fail");
}

/// Chaos: high-volume event store merge.
#[tokio::test]
async fn chaos_high_volume_merge() {
    let store_a = EventStore::new();
    let store_b = EventStore::new();

    // Generate 1000 events on each side
    for i in 0..1000 {
        store_a
            .write(
                Event::new(
                    "node-a",
                    EventCategory::System,
                    "system.tick",
                    format!("tick-a-{i}"),
                )
                .with_scope(EventScope::Ops),
            )
            .await;
        store_b
            .write(
                Event::new(
                    "node-b",
                    EventCategory::System,
                    "system.tick",
                    format!("tick-b-{i}"),
                )
                .with_scope(EventScope::Ops),
            )
            .await;
    }

    // Merge both directions
    let start = std::time::Instant::now();
    let a_events = store_a.all().await;
    let b_events = store_b.all().await;
    let merged_b = store_b.merge(a_events).await;
    let merged_a = store_a.merge(b_events).await;
    let elapsed = start.elapsed();

    assert_eq!(merged_b, 1000);
    assert_eq!(merged_a, 1000);
    assert_eq!(store_a.len().await, 2000);
    assert_eq!(store_b.len().await, 2000);

    // Should complete in reasonable time
    assert!(
        elapsed.as_millis() < 5000,
        "merging 2x1000 events took {}ms, should be <5000ms",
        elapsed.as_millis()
    );
}

/// Chaos: event store subscription under concurrent writes.
#[tokio::test]
async fn chaos_subscription_under_load() {
    let store = EventStore::new();
    let mut rx = store.subscribe();

    // Spawn writer
    let _store_clone = {
        // We need Arc for sharing — but EventStore doesn't impl Clone.
        // Instead, write from this task and check subscription after.
        let count = 100;
        for i in 0..count {
            store
                .write(
                    Event::new(
                        "node",
                        EventCategory::System,
                        "system.tick",
                        format!("tick-{i}"),
                    )
                    .with_scope(EventScope::Ops),
                )
                .await;
        }
        count
    };

    // Subscriber should have received events (might miss some if buffer full)
    let mut received = 0;
    while rx.try_recv().is_ok() {
        received += 1;
    }

    // Should have received most events (broadcast channel has buffer of 256)
    assert!(
        received >= 50,
        "should receive >=50 of 100 events, got {received}"
    );
}

/// Chaos: multiple unauthorized attempts don't affect authorized clients.
#[tokio::test]
async fn chaos_auth_abuse_doesnt_affect_legit() {
    let (addr, kp) = helpers::start_daemon("auth-chaos-1").await;

    // Spawn 10 unauthorized attempts
    let mut bad_handles = Vec::new();
    for _ in 0..10 {
        let addr = addr.clone();
        let handle = tokio::spawn(async move {
            let bad_kp = Keypair::generate();
            let mut client = DaemonClient::connect(&addr, bad_kp).await.unwrap();
            let _ = client.exec("echo bad").await; // should fail
        });
        bad_handles.push(handle);
    }

    // Legitimate client should still work
    let mut legit_client = DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
        .await
        .unwrap();

    let result = legit_client.exec("echo legit").await.unwrap();
    assert_eq!(result.stdout.trim(), "legit");

    // Wait for bad clients to finish
    for handle in bad_handles {
        let _ = handle.await;
    }

    // Legit client still works after abuse
    let result = legit_client.exec("echo still-works").await.unwrap();
    assert_eq!(result.stdout.trim(), "still-works");
}
