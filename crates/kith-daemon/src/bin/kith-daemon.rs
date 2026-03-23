//! kith-daemon binary — starts the gRPC server.
//! Configuration via environment variables for container use.

use std::net::SocketAddr;
use std::time::Duration;

use kith_common::policy::{MachinePolicy, Scope};
use kith_daemon::audit::AuditLog;
use kith_daemon::commit::CommitWindowManager;
use kith_daemon::policy::PolicyEvaluator;
use kith_daemon::proto::kith_daemon_server::KithDaemonServer;
use kith_daemon::service::KithDaemonService;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

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

    // Build policy from env
    let mut policy = MachinePolicy::default();
    policy.tofu = tofu;

    // Add authorized users from KITH_USERS env: "pubkey1:ops,pubkey2:viewer"
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

    let evaluator = PolicyEvaluator::new(policy, machine_name.clone());
    let audit = AuditLog::new(&machine_name);
    let commit = CommitWindowManager::new(Duration::from_secs(commit_window_secs));
    let service = KithDaemonService::new(evaluator, audit, commit, machine_name.clone());

    info!(%listen_addr, %machine_name, "kith-daemon starting");

    tonic::transport::Server::builder()
        .add_service(KithDaemonServer::new(service))
        .serve(listen_addr)
        .await?;

    Ok(())
}
