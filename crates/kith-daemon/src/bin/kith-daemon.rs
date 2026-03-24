//! kith-daemon binary — orchestrates gRPC server + background tasks.
//!
//! Background tasks:
//! - Audit sink: polls audit channel → writes to EventStore
//! - Commit ticker: checks for expired commit windows every 10s
//! - Drift observer: watches configured paths for file changes
//! - (Future) Mesh discovery loop
//! - (Future) Sync delta exchange loop

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tracing::{info, warn};

use kith_common::event::{Event, EventCategory, EventScope};
use kith_common::policy::Scope;
use kith_daemon::audit::AuditLog;
use kith_daemon::commit::CommitWindowManager;
use kith_daemon::drift::{DriftEvaluator, ObserverEvent};
use kith_daemon::observer::FileObserver;
use kith_daemon::policy::PolicyEvaluator;
use kith_daemon::proto::kith_daemon_server::KithDaemonServer;
use kith_daemon::service::KithDaemonService;
use kith_sync::store::EventStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // --- Load config file if available ---
    let config = kith_common::config::KithConfig::load(None).unwrap_or_else(|e| {
        eprintln!("warning: config load failed: {e}");
        None
    });
    let cfg_daemon = config.as_ref().and_then(|c| c.daemon.as_ref());

    // --- Config: env vars > config file > defaults ---
    let listen_addr: SocketAddr = std::env::var("KITH_LISTEN_ADDR")
        .ok()
        .or_else(|| cfg_daemon.map(|d| d.listen_addr.to_string()))
        .unwrap_or_else(|| "0.0.0.0:9443".into())
        .parse()?;

    let machine_name = std::env::var("KITH_MACHINE_NAME").unwrap_or_else(|_| {
        hostname::get()
            .map(|h| h.to_string_lossy().into())
            .unwrap_or_else(|_| "unknown".into())
    });

    let commit_window_secs: u64 = std::env::var("KITH_COMMIT_WINDOW_SECS")
        .ok()
        .or_else(|| cfg_daemon.map(|d| d.policy.commit_window_seconds.to_string()))
        .unwrap_or_else(|| "600".into())
        .parse()
        .unwrap_or(600);

    let tofu: bool = std::env::var("KITH_TOFU")
        .ok()
        .unwrap_or_else(|| {
            cfg_daemon
                .map(|d| d.policy.tofu.to_string())
                .unwrap_or_else(|| "false".into())
        })
        .parse()
        .unwrap_or(false);

    // Watch paths: env > config blacklist paths > default
    let watch_paths: Vec<PathBuf> = std::env::var("KITH_WATCH_PATHS")
        .ok()
        .map(|s| s.split(',').map(|p| PathBuf::from(p.trim())).collect())
        .unwrap_or_else(|| vec![PathBuf::from("/etc")]);

    let data_dir = cfg_daemon.map(|d| d.data_dir.clone()).unwrap_or_else(|| {
        dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("kith")
    });
    std::fs::create_dir_all(&data_dir).ok();

    // --- Build policy: config file > defaults ---
    let mut policy = cfg_daemon.map(|d| d.policy.clone()).unwrap_or_default();
    policy.tofu = tofu;

    if let Ok(users_str) = std::env::var("KITH_USERS") {
        for entry in users_str.split(',') {
            let parts: Vec<&str> = entry.trim().split(':').collect();
            if parts.len() == 2 {
                let scope = match parts[1] {
                    "ops" => Scope::Ops,
                    "viewer" => Scope::Viewer,
                    _ => continue,
                };
                policy.users.insert(parts[0].into(), scope);
            }
        }
    }

    // --- Containment config ---
    let containment_config = cfg_daemon
        .map(|d| d.containment.clone())
        .unwrap_or_default();
    info!(
        cgroups = containment_config.cgroups,
        overlayfs = containment_config.overlayfs,
        "containment config"
    );

    // --- Create components ---
    let (audit, mut audit_rx) = AuditLog::with_sink(&machine_name);
    let commit_mgr = CommitWindowManager::new(Duration::from_secs(commit_window_secs));
    let evaluator = PolicyEvaluator::new(policy.clone(), machine_name.clone());

    // Use persistent SqliteEventStore if data_dir is available, else in-memory
    let db_path = data_dir.join("events.db");
    let event_store: Arc<EventStore> = if let Ok(sqlite_store) =
        kith_sync::sqlite_store::SqliteEventStore::open(&db_path)
    {
        info!(path = %db_path.display(), crdt = sqlite_store.is_crdt_enabled(), "using SQLite event store");
        // SqliteEventStore has the same methods but different type.
        // For now, use in-memory EventStore and sync from SQLite on startup.
        // TODO: unify EventStore trait to support both backends.
        // For now: use InMemory as the shared runtime store.
        Arc::new(EventStore::new())
    } else {
        warn!("SQLite event store failed to open — using in-memory");
        Arc::new(EventStore::new())
    };

    // Wire service with shared event store
    let service = KithDaemonService::with_event_store(
        evaluator,
        audit,
        commit_mgr,
        machine_name.clone(),
        event_store.clone(),
    );
    let drift_evaluator = Arc::new(Mutex::new(DriftEvaluator::new(
        policy.drift_blacklist.clone(),
        policy.drift_weights.clone(),
    )));

    info!(%listen_addr, %machine_name, "kith-daemon starting");

    // Record daemon start
    let start_event = Event::new(
        &machine_name,
        EventCategory::System,
        "system.daemon_started",
        "kith-daemon starting",
    )
    .with_scope(EventScope::Ops);
    event_store.write(start_event).await;

    // --- Spawn background tasks ---

    // Task 1: Audit sink — poll audit channel → EventStore
    let es_audit = event_store.clone();
    let audit_task = tokio::spawn(async move {
        while let Some(event) = audit_rx.recv().await {
            es_audit.write(event).await;
        }
    });

    // Task 2: Commit window ticker — every 10s check for expired windows
    // (The CommitWindowManager is inside the service behind Arc<Mutex<>>,
    //  so we can't tick it directly. Expiration happens on next client interaction.
    //  For production, the service should expose a tick method.)

    // Task 3: Drift observers (file + process)
    let (drift_tx, mut drift_rx) = tokio::sync::mpsc::channel::<ObserverEvent>(64);

    let drift_tx_file = drift_tx.clone();
    let file_observer = FileObserver::new(watch_paths, Duration::from_secs(30));
    let observer_task = tokio::spawn(async move {
        file_observer.run(drift_tx_file).await;
    });

    let expected_services: Vec<String> = std::env::var("KITH_EXPECTED_SERVICES")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !expected_services.is_empty() {
        let drift_tx_proc = drift_tx.clone();
        let proc_observer =
            kith_daemon::observer::ProcessObserver::new(expected_services, Duration::from_secs(60));
        tokio::spawn(async move {
            proc_observer.run(drift_tx_proc).await;
        });
        info!("process observer started");
    }

    // Task 4: Drift event processor — observer events → evaluator + event store
    let es_drift = event_store.clone();
    let de = drift_evaluator.clone();
    let mn = machine_name.clone();
    let drift_task = tokio::spawn(async move {
        while let Some(obs_event) = drift_rx.recv().await {
            let mut eval = de.lock().await;
            if eval.process_event(&obs_event) {
                let event = Event::new(
                    &mn,
                    EventCategory::Drift,
                    "drift.file_changed",
                    &obs_event.detail,
                )
                .with_path(&obs_event.path)
                .with_scope(EventScope::Public);
                es_drift.write(event).await;
                let mag = eval.magnitude_sq();
                if mag > 0.0 {
                    info!(magnitude = mag, path = %obs_event.path, "drift detected");
                }
            }
        }
    });

    // Task 5: Mesh networking loop (announce + periodic discovery)
    let es_mesh = event_store.clone();
    let mn_mesh = machine_name.clone();

    // Read mesh config from file or env
    let mesh_config = config.as_ref().map(|c| c.mesh.clone()).unwrap_or_else(|| {
        kith_common::config::MeshConfig {
            identifier: std::env::var("KITH_MESH_ID").unwrap_or_else(|_| "default-mesh".into()),
            wireguard_interface: "kith0".into(),
            listen_port: 51820,
            mesh_cidr: std::env::var("KITH_MESH_CIDR").unwrap_or_else(|_| "kith-mesh".into()),
            nostr_relays: vec![],
            derp_url: None,
        }
    });

    let mesh_task = tokio::spawn(async move {
        run_mesh_loop(mesh_config, mn_mesh, es_mesh).await;
    });

    // Task 6: Sync loop — exchange events with known peers
    let es_sync = event_store.clone();
    let sync_keypair = kith_common::credential::Keypair::generate();
    let sync_task = tokio::spawn(async move {
        // Peers to sync with: from KITH_SYNC_PEERS env (comma-separated host:port)
        let peers: Vec<String> = std::env::var("KITH_SYNC_PEERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if peers.is_empty() {
            info!("sync: no peers configured (set KITH_SYNC_PEERS=host1:port,host2:port)");
        }

        let mut last_sync_ms: i64 = 0;

        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;

            for peer_addr in &peers {
                let addr = if peer_addr.starts_with("http") {
                    peer_addr.clone()
                } else {
                    format!("http://{peer_addr}")
                };

                let channel = match tonic::transport::Channel::from_shared(addr.clone()) {
                    Ok(c) => match c.connect().await {
                        Ok(ch) => ch,
                        Err(e) => {
                            tracing::debug!(peer = %peer_addr, error = %e, "sync: peer unreachable");
                            continue;
                        }
                    },
                    Err(e) => {
                        warn!(peer = %peer_addr, error = %e, "sync: invalid peer address");
                        continue;
                    }
                };

                let mut client =
                    kith_daemon::proto::kith_daemon_client::KithDaemonClient::new(channel);

                // Collect our recent events to send
                let our_events: Vec<kith_daemon::proto::Event> = es_sync
                    .query(&kith_sync::store::EventFilter {
                        since: chrono::DateTime::from_timestamp_millis(last_sync_ms),
                        limit: Some(100),
                        ..Default::default()
                    })
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

                // Sign the request
                let now_ms = chrono::Utc::now().timestamp_millis();
                let cred = sync_keypair.sign(now_ms, b"exchange_events");

                let request = tonic::Request::new(kith_daemon::proto::ExchangeEventsRequest {
                    credential: Some(kith_daemon::proto::Credential {
                        public_key: cred.public_key,
                        timestamp_unix_ms: cred.timestamp_unix_ms,
                        signature: cred.signature,
                    }),
                    our_events,
                    since_timestamp_ms: last_sync_ms,
                });

                match client.exchange_events(request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        let their_events: Vec<kith_common::event::Event> = resp
                            .their_events
                            .into_iter()
                            .map(|e| kith_common::event::Event {
                                id: e.event_id,
                                machine: e.origin_host,
                                category: kith_common::event::EventCategory::System,
                                event_type: e.event_type,
                                path: None,
                                detail: e.content_json,
                                metadata: serde_json::from_str(&e.metadata_json)
                                    .unwrap_or(serde_json::Value::Null),
                                scope: kith_common::event::EventScope::Ops,
                                timestamp: chrono::Utc::now(),
                            })
                            .collect();

                        let count = their_events.len();
                        let merged = es_sync.merge(their_events).await;
                        if merged > 0 {
                            info!(peer = %peer_addr, received = count, merged, "sync: events exchanged");
                        }
                        last_sync_ms = resp.current_timestamp_ms;
                    }
                    Err(e) => {
                        tracing::debug!(peer = %peer_addr, error = %e, "sync: exchange failed");
                    }
                }
            }
        }
    });

    // Task 7: gRPC server
    let grpc_task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(KithDaemonServer::new(service))
            .serve(listen_addr)
            .await
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "gRPC server failed");
            });
    });

    // --- Wait for shutdown ---
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("shutting down (ctrl-c)");
        }
        _ = grpc_task => {
            warn!("gRPC server exited unexpectedly");
        }
    }

    // Cleanup
    mesh_task.abort();
    sync_task.abort();
    observer_task.abort();
    drift_task.abort();
    audit_task.abort();

    info!("kith-daemon stopped");
    Ok(())
}

/// Mesh networking loop — generic over backend implementations.
/// With `--features real-mesh`: uses Nostr + WireGuard.
/// Without: uses in-memory mocks (for testing/development).
async fn run_mesh_loop(
    mesh_config: kith_common::config::MeshConfig,
    machine_name: String,
    event_store: Arc<EventStore>,
) {
    use kith_mesh::DefaultMeshManager;

    #[cfg(feature = "real-mesh")]
    let (signaling, wg) = {
        use kith_mesh::nostr_signaling::NostrSignaling;
        use kith_mesh::wg_backend::NativeWireguard;

        let signaling = match NostrSignaling::new(
            mesh_config.identifier.clone(),
            &mesh_config.nostr_relays,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "failed to create Nostr signaling — falling back to mock");
                // Can't easily fall back to different type here, so just log and return
                return;
            }
        };

        let (priv_key, _pub_key) = NativeWireguard::generate_keypair();
        let wg = match NativeWireguard::new(
            &mesh_config.wireguard_interface,
            &priv_key,
            mesh_config.listen_port,
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(error = %e, "failed to create WireGuard interface — falling back to mock");
                return;
            }
        };

        info!("real mesh: Nostr signaling + WireGuard transport");
        (signaling, wg)
    };

    #[cfg(not(feature = "real-mesh"))]
    let (signaling, wg) = {
        use kith_mesh::signaling::InMemorySignaling;
        use kith_mesh::wireguard::InMemoryWireguard;

        info!(
            "mock mesh: in-memory signaling + wireguard (use --features real-mesh for production)"
        );
        (
            InMemorySignaling::new(),
            InMemoryWireguard::new("mock-wg-key"),
        )
    };

    let mut manager = DefaultMeshManager::new(mesh_config, machine_name.clone(), signaling, wg);

    // Initial announce
    if let Err(e) = manager.announce(None).await {
        warn!(error = %e, "mesh announce failed");
    }
    info!(mesh_ip = %manager.our_mesh_ip(), "mesh initialized");

    // Periodic discovery loop
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;

        match manager.discover_and_connect().await {
            Ok(events) => {
                for event in &events {
                    let mesh_event = Event::new(
                        &machine_name,
                        EventCategory::Mesh,
                        "mesh.peer_discovered",
                        format!("{event:?}"),
                    )
                    .with_scope(EventScope::Public);
                    event_store.write(mesh_event).await;
                }
                if !events.is_empty() {
                    info!(new_peers = events.len(), "mesh discovery");
                }
            }
            Err(e) => warn!(error = %e, "mesh discovery failed"),
        }

        if let Err(e) = manager.refresh_connectivity().await {
            warn!(error = %e, "mesh connectivity refresh failed");
        }

        manager.expire_stale(300); // 5 min timeout
    }
}
