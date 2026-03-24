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
use kith_common::policy::{MachinePolicy, Scope};
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

    // --- Config from env ---
    let listen_addr: SocketAddr = std::env::var("KITH_LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:9443".into())
        .parse()?;

    let machine_name = std::env::var("KITH_MACHINE_NAME")
        .unwrap_or_else(|_| hostname::get().map(|h| h.to_string_lossy().into()).unwrap_or_else(|_| "unknown".into()));

    let commit_window_secs: u64 = std::env::var("KITH_COMMIT_WINDOW_SECS")
        .unwrap_or_else(|_| "600".into())
        .parse()
        .unwrap_or(600);

    let tofu: bool = std::env::var("KITH_TOFU")
        .unwrap_or_else(|_| "false".into())
        .parse()
        .unwrap_or(false);

    // Watch paths for drift (comma-separated)
    let watch_paths: Vec<PathBuf> = std::env::var("KITH_WATCH_PATHS")
        .unwrap_or_else(|_| "/etc".into())
        .split(',')
        .map(|s| PathBuf::from(s.trim()))
        .collect();

    // --- Build policy ---
    let mut policy = MachinePolicy::default();
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
    let start_event = Event::new(&machine_name, EventCategory::System, "system.daemon_started", "kith-daemon starting")
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
                let event = Event::new(&mn, EventCategory::Drift, "drift.file_changed", &obs_event.detail)
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

    // Task 5: gRPC server
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
    observer_task.abort();
    drift_task.abort();
    audit_task.abort();

    info!("kith-daemon stopped");
    Ok(())
}
