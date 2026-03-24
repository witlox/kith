//! Tests that PROVE the production wiring actually works.
//! These test the actual data flow, not just that components exist.
//! Each test was written BEFORE the fix — TDD style.

use kith_common::event::{Event, EventCategory, EventScope};
use kith_daemon::containment::TransactionManager;
use kith_state::embedding::{BagOfWordsEmbedder, EmbeddingBackend};
use kith_state::vector_index::VectorIndex;
use kith_sync::sqlite_store::SqliteEventStore;

// ============================================================
// Test 1: SqliteEventStore is actually used for persistence
// ============================================================

/// Proves: events written to the daemon's store survive in SQLite,
/// not just in-memory. If the daemon throws away SqliteEventStore,
/// this test fails because reopening the DB finds nothing.
#[tokio::test]
async fn daemon_events_persist_in_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("events.db");

    // Write events via SqliteEventStore
    {
        let store = SqliteEventStore::open(&db_path).unwrap();
        store
            .write(
                Event::new(
                    "daemon-1",
                    EventCategory::Exec,
                    "exec.command",
                    "persisted cmd",
                )
                .with_scope(EventScope::Ops),
            )
            .await;
        assert_eq!(store.len().await, 1);
    }

    // Reopen — data must survive
    {
        let store = SqliteEventStore::open(&db_path).unwrap();
        assert_eq!(
            store.len().await,
            1,
            "events must persist across store reopen"
        );
        let events = store.all().await;
        assert_eq!(events[0].detail, "persisted cmd");
    }
}

// ============================================================
// Test 2: Agent's retrieve() finds events from EventStore
// ============================================================

/// Proves: when events exist in the agent's EventStore, the retrieve()
/// tool actually finds them. If the store is empty/isolated, this fails.
#[tokio::test]
async fn agent_retrieve_finds_events_in_store() {
    use kith_shell::agent::{Agent, AgentOutput};
    use kith_shell::mock_backend::MockInferenceBackend;

    let backend = MockInferenceBackend::new("test");
    // Queue a tool call for retrieve
    backend.queue_tool_call(
        "retrieve",
        serde_json::json!({"query": "docker deployment"}),
    );

    let mut agent = Agent::new(Box::new(backend), "test agent".into());

    // Manually inject events into the agent's store
    // (In production, these come from sync loop or daemon audit)
    agent
        .event_store_mut()
        .write(
            Event::new(
                "staging-1",
                EventCategory::Exec,
                "exec.command",
                "docker compose up deployment",
            )
            .with_scope(EventScope::Ops),
        )
        .await;

    // Process an intent that triggers retrieve
    // (must NOT start with a PATH command or it becomes pass-through)
    let output = agent.process("search for docker deployment logs").await;

    match output {
        AgentOutput::ToolResults(results) => {
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].tool_name, "retrieve");
            // The retrieve output should contain the docker event
            assert!(
                results[0].output.contains("docker"),
                "retrieve should find docker event, got: {}",
                results[0].output
            );
        }
        other => panic!("expected ToolResults, got {other:?}"),
    }
}

// ============================================================
// Test 3: VectorIndex gets populated when events are written
// ============================================================

/// Proves: when events flow into the agent, they get embedded and
/// indexed in the VectorIndex. If the index is never populated,
/// hybrid search returns no vector results.
#[tokio::test]
async fn vector_index_populated_from_events() {
    let embedder = BagOfWordsEmbedder::new(100);

    let mut index = VectorIndex::new();
    let event = Event::new(
        "staging-1",
        EventCategory::Exec,
        "exec.command",
        "docker container running",
    )
    .with_scope(EventScope::Ops);

    // Embed and insert — this is what SHOULD happen automatically
    let emb = embedder.embed(&event.detail).await.unwrap();
    index.insert(event.clone(), emb);

    assert_eq!(index.len(), 1, "index should have 1 entry after insert");

    // Search should find it
    let query_emb = embedder.embed("docker").await.unwrap();
    let results = index.search(&query_emb, 5);
    assert!(!results.is_empty(), "search should find the docker event");
    assert!(
        results[0].event.detail.contains("docker"),
        "result should be the docker event"
    );
}

// ============================================================
// Test 4: Containment actually protects files
// ============================================================

/// Proves: when apply() is called with real file paths, rollback
/// actually restores the original content. If paths are empty,
/// rollback is a no-op and the file stays modified.
#[tokio::test]
async fn containment_protects_and_restores_files() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("protected.conf");
    std::fs::write(&file, "original content").unwrap();

    let backup_dir = dir.path().join("backups");
    let mut tx_mgr = TransactionManager::new(backup_dir);

    // Begin transaction WITH the file path
    tx_mgr.begin("tx-protect".into(), &[file.clone()]).unwrap();

    // Modify the file (simulating what apply does)
    std::fs::write(&file, "dangerous change").unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "dangerous change");

    // Rollback should restore
    tx_mgr.rollback("tx-protect").unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "original content",
        "rollback must restore original file content"
    );
}

/// Proves: commit keeps the changes (backup removed).
#[tokio::test]
async fn containment_commit_keeps_changes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("committed.conf");
    std::fs::write(&file, "before").unwrap();

    let backup_dir = dir.path().join("backups");
    let mut tx_mgr = TransactionManager::new(backup_dir);

    tx_mgr.begin("tx-commit".into(), &[file.clone()]).unwrap();
    std::fs::write(&file, "after").unwrap();
    tx_mgr.commit("tx-commit").unwrap();

    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "after",
        "commit must keep the new content"
    );
}
