//! Shared test helpers for e2e tests.

use std::time::Duration;

use kith_common::credential::Keypair;
use kith_common::policy::{MachinePolicy, Scope};
use kith_daemon::audit::AuditLog;
use kith_daemon::commit::CommitWindowManager;
use kith_daemon::policy::PolicyEvaluator;
use kith_daemon::proto::kith_daemon_server::KithDaemonServer;
use kith_daemon::service::KithDaemonService;

/// Start a daemon in-process, return the address and the keypair authorized for ops.
pub async fn start_daemon(machine_name: &str) -> (String, Keypair) {
    let kp = Keypair::generate();
    let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());

    let mut policy = MachinePolicy::default();
    policy.users.insert(pubkey_hex, Scope::Ops);

    let evaluator = PolicyEvaluator::new(policy, machine_name.into());
    let audit = AuditLog::new(machine_name);
    let commit = CommitWindowManager::new(Duration::from_secs(600));
    let service = KithDaemonService::new(evaluator, audit, commit, machine_name.into());

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

    (format!("http://{addr}"), kp)
}
