//! End-to-end tests: full flow across multiple crates.

use kith_common::credential::Keypair;
use kith_common::event::{Event, EventCategory, EventScope};
use kith_e2e::helpers;
use kith_shell::daemon_client::DaemonClient;
use kith_state::retrieval::KeywordRetriever;
use kith_sync::store::EventStore;

/// E2e scenario 2: shell -> daemon -> exec -> streaming response
#[tokio::test]
async fn e2e_shell_to_daemon_exec() {
    let (addr, kp) = helpers::start_daemon("staging-1").await;
    let mut client =
        DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();

    let result = client.exec("echo e2e-test && echo done").await.unwrap();
    assert!(result.stdout.contains("e2e-test"));
    assert!(result.stdout.contains("done"));
    assert_eq!(result.exit_code, 0);
}

/// E2e scenario 4: apply -> commit cycle through shell client
#[tokio::test]
async fn e2e_apply_commit_via_shell() {
    let (addr, kp) = helpers::start_daemon("staging-1").await;
    let mut client =
        DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();

    // Apply
    let pending_id = client.apply("docker compose up -d", 600).await.unwrap();
    assert!(!pending_id.is_empty());

    // Commit
    assert!(client.commit(&pending_id).await.unwrap());

    // Double-commit should fail gracefully
    assert!(!client.commit(&pending_id).await.unwrap());
}

/// E2e scenario 4 variant: apply -> rollback
#[tokio::test]
async fn e2e_apply_rollback_via_shell() {
    let (addr, kp) = helpers::start_daemon("staging-1").await;
    let mut client =
        DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();

    let pending_id = client.apply("risky change", 600).await.unwrap();
    assert!(client.rollback(&pending_id).await.unwrap());
}

/// E2e scenario 7: unauthorized request -> daemon rejects -> audit
#[tokio::test]
async fn e2e_permission_enforcement() {
    let (addr, _authorized_kp) = helpers::start_daemon("prod-1").await;

    // Connect with unauthorized keypair
    let unauthorized = Keypair::generate();
    let mut client = DaemonClient::connect(&addr, unauthorized).await.unwrap();

    let result = client.exec("echo should-fail").await;
    assert!(result.is_err(), "unauthorized exec should fail");
}

/// E2e scenario 8: events from daemon flow into EventStore and are retrievable
#[tokio::test]
async fn e2e_event_store_retrieval() {
    // Simulate the full flow: daemon produces events, they go to EventStore,
    // agent retrieves via KeywordRetriever

    let store = EventStore::new();

    // Simulate daemon audit events being written to the store
    let exec_event = Event::new(
        "staging-1",
        EventCategory::Exec,
        "exec.command",
        "docker ps",
    )
    .with_metadata(serde_json::json!({"command": "docker ps", "exit_code": 0}))
    .with_scope(EventScope::Ops);

    let drift_event = Event::new(
        "staging-1",
        EventCategory::Drift,
        "drift.file_changed",
        "nginx config modified",
    )
    .with_path("/etc/nginx/conf.d/api.conf")
    .with_scope(EventScope::Public);

    store.write(exec_event).await;
    store.write(drift_event).await;

    // Retrieve all events
    let all = store.all().await;
    assert_eq!(all.len(), 2);

    // KeywordRetriever finds nginx-related events
    let results = KeywordRetriever::search(&all, "nginx", &EventScope::Ops, 10);
    assert_eq!(results.len(), 1);
    assert!(results[0].event.detail.contains("nginx"));

    // KeywordRetriever finds docker-related events
    let results = KeywordRetriever::search(&all, "docker", &EventScope::Ops, 10);
    assert_eq!(results.len(), 1);

    // Viewer scope can't see exec events (Ops-scoped)
    let results = KeywordRetriever::search(&all, "docker", &EventScope::Public, 10);
    assert!(results.is_empty());

    // Viewer scope can see drift events (Public-scoped)
    let results = KeywordRetriever::search(&all, "nginx", &EventScope::Public, 10);
    assert_eq!(results.len(), 1);
}

/// E2e: multiple daemons, shell connects to each
#[tokio::test]
async fn e2e_multi_daemon() {
    let (addr1, kp1) = helpers::start_daemon("staging-1").await;
    let (addr2, kp2) = helpers::start_daemon("prod-1").await;

    let mut client1 =
        DaemonClient::connect(&addr1, Keypair::from_secret(&kp1.secret_bytes()))
            .await
            .unwrap();
    let mut client2 =
        DaemonClient::connect(&addr2, Keypair::from_secret(&kp2.secret_bytes()))
            .await
            .unwrap();

    let state1 = client1.query().await.unwrap();
    let state2 = client2.query().await.unwrap();

    assert!(state1.contains("staging-1"));
    assert!(state2.contains("prod-1"));

    // Each daemon is independent
    let r1 = client1.exec("echo from-staging").await.unwrap();
    let r2 = client2.exec("echo from-prod").await.unwrap();

    assert!(r1.stdout.contains("from-staging"));
    assert!(r2.stdout.contains("from-prod"));
}
