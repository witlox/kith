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
    let _cfg_mesh = config.as_ref().map(|c| &c.mesh); // Used in Phase 2 (real mesh)

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

    // --- Create components ---
    let (audit, mut audit_rx) = AuditLog::with_sink(&machine_name);
    let commit_mgr = CommitWindowManager::new(Duration::from_secs(commit_window_secs));
    let evaluator = PolicyEvaluator::new(policy.clone(), machine_name.clone());
    let service = KithDaemonService::new(evaluator, audit, commit_mgr, machine_name.clone());

    let event_store = Arc::new(EventStore::new());
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

    // Task 3: Drift file observer
    let (drift_tx, mut drift_rx) = tokio::sync::mpsc::channel::<ObserverEvent>(64);
    let file_observer = FileObserver::new(watch_paths, Duration::from_secs(30));
    let observer_task = tokio::spawn(async move {
        file_observer.run(drift_tx).await;
    });

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
    let mesh_task = tokio::spawn(async move {
        use kith_mesh::DefaultMeshManager;
        use kith_mesh::signaling::InMemorySignaling;
        use kith_mesh::wireguard::InMemoryWireguard;

        let mesh_config = kith_common::config::MeshConfig {
            identifier: std::env::var("KITH_MESH_ID").unwrap_or_else(|_| "default-mesh".into()),
            wireguard_interface: "kith0".into(),
            listen_port: 51820,
            mesh_cidr: std::env::var("KITH_MESH_CIDR").unwrap_or_else(|_| "kith-mesh".into()),
            nostr_relays: vec![],
            derp_url: None,
        };

        // Use in-memory backends for now. Real Nostr + WireGuard
        // are available behind feature flags in kith-mesh.
        let signaling = InMemorySignaling::new();
        let wg = InMemoryWireguard::new("mock-wg-key");
        let mut manager = DefaultMeshManager::new(mesh_config, mn_mesh.clone(), signaling, wg);

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
                            &mn_mesh,
                            EventCategory::Mesh,
                            "mesh.peer_discovered",
                            format!("{event:?}"),
                        )
                        .with_scope(EventScope::Public);
                        es_mesh.write(mesh_event).await;
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
    });

    // Task 6: Sync loop (periodic merge with peers)
    let _es_sync = event_store.clone();
    let sync_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            // Sync is a no-op until peers are connected via real mesh.
            // When peers are available, this would:
            //   1. For each connected peer: fetch their events via gRPC
            //   2. Merge into local store: es_sync.merge(peer_events).await
            //   3. Send our new events to them
            // The EventStore.merge() handles dedup by event ID.
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
