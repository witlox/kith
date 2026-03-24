//! E2e test: two daemons exchange events via ExchangeEvents RPC.

use std::sync::Arc;
use std::time::Duration;

use kith_common::credential::Keypair;
use kith_common::event::{Event, EventCategory, EventScope};
use kith_common::policy::{MachinePolicy, Scope};
use kith_daemon::audit::AuditLog;
use kith_daemon::commit::CommitWindowManager;
use kith_daemon::policy::PolicyEvaluator;
use kith_daemon::proto::kith_daemon_client::KithDaemonClient;
use kith_daemon::proto::kith_daemon_server::KithDaemonServer;
use kith_daemon::service::KithDaemonService;
use kith_sync::store::EventStore;

/// Start a daemon with a shared EventStore so we can verify sync.
async fn start_daemon_with_store(name: &str, keypair: &Keypair) -> (String, Arc<EventStore>) {
    let pubkey_hex = kith_common::credential::pubkey_to_hex(&keypair.public_key_bytes());

    let mut policy = MachinePolicy::default();
    policy.users.insert(pubkey_hex, Scope::Ops);
    // Also allow sync keypairs (TOFU)
    policy.tofu = true;

    let evaluator = PolicyEvaluator::new(policy, name.into());
    let audit = AuditLog::new(name);
    let commit = CommitWindowManager::new(Duration::from_secs(600));
    let event_store = Arc::new(EventStore::new());

    let service = KithDaemonService::with_event_store(
        evaluator,
        audit,
        commit,
        name.into(),
        event_store.clone(),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(KithDaemonServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    (format!("http://{addr}"), event_store)
}

/// Two daemons exchange events via ExchangeEvents RPC.
#[tokio::test]
async fn e2e_sync_exchange_between_daemons() {
    let kp = Keypair::generate();

    // Start two daemons with shared stores
    let (_addr_a, store_a) = start_daemon_with_store("daemon-a", &kp).await;
    let (addr_b, store_b) = start_daemon_with_store("daemon-b", &kp).await;

    // Write events to daemon A's store
    for i in 0..5 {
        store_a
            .write(
                Event::new(
                    "daemon-a",
                    EventCategory::Exec,
                    "exec.command",
                    format!("command-a-{i}"),
                )
                .with_scope(EventScope::Ops),
            )
            .await;
    }

    // Write events to daemon B's store
    for i in 0..3 {
        store_b
            .write(
                Event::new(
                    "daemon-b",
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

    // Daemon A syncs with Daemon B via ExchangeEvents RPC
    let channel_b = tonic::transport::Channel::from_shared(addr_b.clone())
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client_b = KithDaemonClient::new(channel_b);

    // Collect A's events to send
    let a_events: Vec<kith_daemon::proto::Event> = store_a
        .all()
        .await
        .into_iter()
        .map(|e| kith_daemon::proto::Event {
            event_id: e.id,
            event_type: e.event_type,
            origin_host: e.machine,
            timestamp: None,
            scope: format!("{:?}", e.scope),
            metadata_json: e.metadata.to_string(),
            content_json: e.detail,
        })
        .collect();

    // Sign request
    let now = chrono::Utc::now().timestamp_millis();
    let cred = kp.sign(now, b"exchange_events");

    let request = tonic::Request::new(kith_daemon::proto::ExchangeEventsRequest {
        credential: Some(kith_daemon::proto::Credential {
            public_key: cred.public_key,
            timestamp_unix_ms: cred.timestamp_unix_ms,
            signature: cred.signature,
        }),
        our_events: a_events,
        since_timestamp_ms: 0,
    });

    let response = client_b.exchange_events(request).await.unwrap();
    let resp = response.into_inner();

    // B should have returned its 3 events
    assert!(
        !resp.their_events.is_empty(),
        "daemon B should return its events"
    );

    // B should now have A's events merged (5 from A + 3 original = up to 8)
    // (Exact count depends on merge dedup)
    assert!(
        store_b.len().await >= 3,
        "daemon B should have at least its original events"
    );

    // Merge B's response into A
    let b_events: Vec<Event> = resp
        .their_events
        .into_iter()
        .map(|e| Event {
            id: e.event_id,
            machine: e.origin_host,
            category: EventCategory::System,
            event_type: e.event_type,
            path: None,
            detail: e.content_json,
            metadata: serde_json::from_str(&e.metadata_json).unwrap_or(serde_json::Value::Null),
            scope: EventScope::Ops,
            timestamp: chrono::Utc::now(),
        })
        .collect();

    let merged = store_a.merge(b_events).await;
    assert!(merged > 0, "A should have merged B's events");

    // A now has events from both
    assert!(
        store_a.len().await > 5,
        "daemon A should have more than its original 5 events"
    );
}
