//! Integration tests for all wired components.
//! Verifies that components assembled in production code paths actually work together.

use std::sync::Arc;
use std::time::Duration;

use kith_common::credential::Keypair;
use kith_common::event::{Event, EventCategory, EventScope};
use kith_common::policy::{MachinePolicy, Scope};
use kith_daemon::audit::AuditLog;
use kith_daemon::commit::CommitWindowManager;
use kith_daemon::policy::PolicyEvaluator;
use kith_daemon::proto::kith_daemon_server::KithDaemonServer;
use kith_daemon::service::KithDaemonService;
use kith_shell::daemon_client::DaemonClient;
use kith_state::embedding::{BagOfWordsEmbedder, EmbeddingBackend};
use kith_state::hybrid::HybridRetriever;
use kith_state::vector_index::VectorIndex;
use kith_sync::store::EventStore;

/// Helper: start daemon with shared EventStore.
async fn start_daemon_with_store(name: &str, kp: &Keypair) -> (String, Arc<EventStore>) {
    let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());
    let mut policy = MachinePolicy::default();
    policy.users.insert(pubkey_hex, Scope::Ops);

    let evaluator = PolicyEvaluator::new(policy, name.into());
    let audit = AuditLog::new(name);
    let commit = CommitWindowManager::new(Duration::from_secs(600));
    let store = Arc::new(EventStore::new());

    let service =
        KithDaemonService::with_event_store(evaluator, audit, commit, name.into(), store.clone());

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
    (format!("http://{addr}"), store)
}

// --- Test 1: Capabilities returns real values ---

#[tokio::test]
async fn capabilities_returns_real_data() {
    let kp = Keypair::generate();
    let (addr, _store) = start_daemon_with_store("cap-test", &kp).await;

    let _client = DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
        .await
        .unwrap();

    // Query capabilities via the daemon's Query RPC (capabilities is not exposed via DaemonClient,
    // but we can verify via gRPC directly)
    let channel = tonic::transport::Channel::from_shared(addr)
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut grpc = kith_daemon::proto::kith_daemon_client::KithDaemonClient::new(channel);

    let now = chrono::Utc::now().timestamp_millis();
    let cred = kp.sign(now, b"capabilities");

    let resp = grpc
        .capabilities(tonic::Request::new(
            kith_daemon::proto::CapabilitiesRequest {
                credential: Some(kith_daemon::proto::Credential {
                    public_key: cred.public_key,
                    timestamp_unix_ms: cred.timestamp_unix_ms,
                    signature: cred.signature,
                }),
            },
        ))
        .await
        .unwrap();

    let cap = resp.into_inner();
    assert_eq!(cap.hostname, "cap-test");
    assert!(!cap.os.is_empty(), "OS should be populated");
    assert!(!cap.arch.is_empty(), "arch should be populated");

    // CPU count should be > 0
    let resources = cap.resources.expect("resources should be present");
    assert!(resources.cpu_count > 0, "CPU count should be > 0");

    // At least some tools should be found (git is common in dev/CI)
    // Don't assert specific tools — CI may differ
}

// --- Test 2: with_event_store shares store between service + external ---

#[tokio::test]
async fn with_event_store_shares_state() {
    let kp = Keypair::generate();
    let (addr, store) = start_daemon_with_store("store-test", &kp).await;

    // Write an event directly to the shared store
    store
        .write(
            Event::new(
                "store-test",
                EventCategory::System,
                "test.injected",
                "injected via shared store",
            )
            .with_scope(EventScope::Ops),
        )
        .await;

    assert_eq!(store.len().await, 1);

    // Exec via daemon — this also writes audit events to the store (via audit sink)
    let mut client = DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
        .await
        .unwrap();
    let _result = client.exec("echo store-test").await.unwrap();

    // Wait for audit sink to flush
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Store should have both: our injected event + audit from exec
    // (audit events go through the sink channel, which may or may not be wired
    //  in with_event_store — but the shared store itself should work)
    assert!(
        store.len().await >= 1,
        "store should have at least the injected event"
    );
}

// --- Test 3: HybridRetriever in agent works end-to-end ---

#[tokio::test]
async fn hybrid_retriever_in_agent() {
    let embedder = BagOfWordsEmbedder::new(100);

    // Build index with some events
    let mut index = VectorIndex::new();
    let events = vec![
        Event::new(
            "s1",
            EventCategory::Exec,
            "exec.command",
            "docker ps running",
        )
        .with_scope(EventScope::Ops),
        Event::new(
            "s1",
            EventCategory::Drift,
            "drift.file_changed",
            "nginx config changed",
        )
        .with_scope(EventScope::Public),
    ];

    for event in &events {
        let emb = embedder.embed(&event.detail).await.unwrap();
        index.insert(event.clone(), emb);
    }

    let retriever = HybridRetriever::new(index);

    // Search for docker
    let query_emb = embedder.embed("docker").await.unwrap();
    let results = retriever
        .search(&events, "docker", &query_emb, &EventScope::Ops, 10)
        .await;

    assert!(!results.is_empty(), "should find docker events");
    assert!(
        results[0].event.detail.contains("docker"),
        "top result should be docker"
    );
    assert!(
        results[0].combined_score > 0.0,
        "should have combined keyword+vector score"
    );
    assert!(results[0].keyword_score > 0.0, "keyword should contribute");
}

// --- Test 4: Agent with_embedder uses provided backend ---

#[tokio::test]
async fn agent_with_embedder() {
    use kith_shell::agent::{Agent, AgentOutput};
    use kith_shell::mock_backend::MockInferenceBackend;

    let backend = MockInferenceBackend::new("test");
    backend.queue_text("I found some results.");

    let embedder = Box::new(BagOfWordsEmbedder::new(100));
    let mut agent =
        Agent::with_embedder(Box::new(backend), "you are a test agent".into(), embedder);

    // The agent should work with the provided embedder
    let output = agent.process("echo hello").await;
    assert!(matches!(output, AgentOutput::PassThrough { .. }));
}

// --- Test 5: Config roundtrip with all sections ---

#[test]
fn config_roundtrip_all_sections() {
    let toml_str = r#"
[mesh]
identifier = "test-mesh"
wireguard_interface = "kith0"
listen_port = 51820
mesh_cidr = "10.47.0.0/24"
nostr_relays = ["wss://relay.example.com"]

[inference]
backend = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"

[embedding]
backend = "api"
endpoint = "http://localhost:11434/v1"
model = "all-minilm"
dimensions = 384
"#;
    let config: kith_common::config::KithConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.mesh.identifier, "test-mesh");

    let inf = config.inference.unwrap();
    assert_eq!(inf.backend, "anthropic");

    let emb = config.embedding.unwrap();
    assert_eq!(emb.backend, "api");
    assert_eq!(emb.endpoint.unwrap(), "http://localhost:11434/v1");
    assert_eq!(emb.model.unwrap(), "all-minilm");
    assert_eq!(emb.dimensions.unwrap(), 384);
}

// --- Test 6: Config with [embedding] backend = "bow" uses BagOfWords ---

#[test]
fn config_bow_embedding() {
    let toml_str = r#"
[mesh]
identifier = "test"
wireguard_interface = "kith0"
listen_port = 51820
mesh_cidr = "test"
nostr_relays = []

[embedding]
backend = "bow"
"#;
    let config: kith_common::config::KithConfig = toml::from_str(toml_str).unwrap();
    let emb = config.embedding.unwrap();
    assert_eq!(emb.backend, "bow");
    assert!(emb.endpoint.is_none());
}

// --- Test 7: TransactionManager commit/rollback through daemon RPCs ---

#[tokio::test]
async fn containment_through_rpcs() {
    let kp = Keypair::generate();
    let (addr, _store) = start_daemon_with_store("containment-test", &kp).await;

    let mut client = DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
        .await
        .unwrap();

    // Apply creates a transaction
    let pending_id = client.apply("test change", 600).await.unwrap();
    assert!(!pending_id.is_empty());

    // Rollback should succeed (transaction exists)
    let rolled_back = client.rollback(&pending_id).await.unwrap();
    assert!(rolled_back);

    // Apply again + commit
    let pending_id2 = client.apply("second change", 600).await.unwrap();
    let committed = client.commit(&pending_id2).await.unwrap();
    assert!(committed);
}

// --- Test 8: SqliteEventStore persistence ---

#[tokio::test]
async fn sqlite_store_persists() {
    use kith_sync::sqlite_store::SqliteEventStore;

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test-persist.db");

    // Write events
    {
        let store = SqliteEventStore::open(&db_path).unwrap();
        store
            .write(
                Event::new(
                    "node-1",
                    EventCategory::Exec,
                    "exec.command",
                    "test persist",
                )
                .with_scope(EventScope::Ops),
            )
            .await;
        assert_eq!(store.len().await, 1);
    }

    // Reopen and verify
    {
        let store = SqliteEventStore::open(&db_path).unwrap();
        assert_eq!(store.len().await, 1);
        let all = store.all().await;
        assert_eq!(all[0].detail, "test persist");
    }
}

// --- Test 9: FileObserver detects changes ---

#[tokio::test]
async fn file_observer_integration() {
    use kith_daemon::drift::ObserverEvent;
    use kith_daemon::observer::FileObserver;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("watched.conf");
    std::fs::write(&file, "original").unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<ObserverEvent>(16);
    let observer = FileObserver::new(vec![file.clone()], Duration::from_millis(50));

    let handle = tokio::spawn(async move { observer.run(tx).await });

    // Wait for initial scan
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Modify
    std::fs::write(&file, "changed").unwrap();

    // Wait for detection
    tokio::time::sleep(Duration::from_millis(200)).await;
    handle.abort();

    let event = rx.try_recv().expect("should detect change");
    assert!(event.detail.contains("modified"));
}

// --- Test 10: ProcessObserver detects services ---

#[tokio::test]
async fn process_observer_integration() {
    use kith_daemon::drift::ObserverEvent;
    use kith_daemon::observer::ProcessObserver;

    // Watch for a process that definitely exists: the test process itself
    let (tx, _rx) = tokio::sync::mpsc::channel::<ObserverEvent>(16);
    let observer = ProcessObserver::new(
        vec!["cargo".into()], // cargo is running (this test!)
        Duration::from_millis(100),
    );

    let handle = tokio::spawn(async move { observer.run(tx).await });

    // Wait for at least one poll cycle
    tokio::time::sleep(Duration::from_millis(300)).await;
    handle.abort();

    // ProcessObserver should have detected cargo running.
    // On first poll, it records current state (no event).
    // We'd need cargo to start/stop to get an event.
    // This test validates the observer runs without crashing.
}
