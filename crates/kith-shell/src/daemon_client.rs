//! gRPC client for connecting to a kith-daemon.
//! Implements the remote(), apply(), commit(), rollback() native tools.

use kith_common::credential::Keypair;
use kith_common::error::KithError;
use kith_daemon::proto;
use kith_daemon::proto::kith_daemon_client::KithDaemonClient;
use tonic::transport::Channel;
use tracing::info;

/// Client for a single kith-daemon instance.
pub struct DaemonClient {
    client: KithDaemonClient<Channel>,
    keypair: Keypair,
    host: String,
}

/// Result of a remote command execution.
#[derive(Debug, Clone)]
pub struct RemoteExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl DaemonClient {
    /// Connect to a daemon at the given address.
    pub async fn connect(addr: &str, keypair: Keypair) -> Result<Self, KithError> {
        let uri = if addr.starts_with("http") {
            addr.to_string()
        } else {
            format!("http://{addr}")
        };

        let channel = Channel::from_shared(uri)
            .map_err(|e| KithError::MeshError(format!("invalid address: {e}")))?
            .connect()
            .await
            .map_err(|e| KithError::MachineUnreachable(format!("{addr}: {e}")))?;

        Ok(Self {
            client: KithDaemonClient::new(channel),
            keypair,
            host: addr.to_string(),
        })
    }

    /// Build a proto Credential for a request.
    fn make_credential(&self, request_hash: &[u8]) -> proto::Credential {
        let now = chrono::Utc::now().timestamp_millis();
        let cred = self.keypair.sign(now, request_hash);
        proto::Credential {
            public_key: cred.public_key,
            timestamp_unix_ms: cred.timestamp_unix_ms,
            signature: cred.signature,
        }
    }

    /// Execute a command on the remote daemon (native tool: remote).
    pub async fn exec(&mut self, command: &str) -> Result<RemoteExecResult, KithError> {
        let cred = self.make_credential(command.as_bytes());

        let request = tonic::Request::new(proto::ExecRequest {
            command: command.into(),
            credential: Some(cred),
        });

        let response = self
            .client
            .exec(request)
            .await
            .map_err(|s| KithError::Internal(format!("exec failed: {s}")))?;

        let mut stream = response.into_inner();
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = -1;

        while let Some(item) = tokio_stream::StreamExt::next(&mut stream).await {
            let output = item.map_err(|s| KithError::Internal(format!("stream error: {s}")))?;

            match output.output {
                Some(proto::exec_output::Output::Stdout(data)) => {
                    stdout.push_str(&String::from_utf8_lossy(&data));
                }
                Some(proto::exec_output::Output::Stderr(data)) => {
                    stderr.push_str(&String::from_utf8_lossy(&data));
                }
                None => {}
            }

            if output.is_complete {
                exit_code = output.exit_code;
            }
        }

        info!(host = %self.host, command = %command, exit_code, "remote exec complete");

        Ok(RemoteExecResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    /// Apply a change with commit window (native tool: apply).
    pub async fn apply(
        &mut self,
        command: &str,
        commit_window_seconds: u32,
    ) -> Result<String, KithError> {
        let cred = self.make_credential(command.as_bytes());

        let request = tonic::Request::new(proto::ApplyRequest {
            command: command.into(),
            credential: Some(cred),
            commit_window_seconds,
        });

        let response = self
            .client
            .apply(request)
            .await
            .map_err(|s| KithError::Internal(format!("apply failed: {s}")))?;

        Ok(response.into_inner().change_id)
    }

    /// Commit a pending change (native tool: commit).
    pub async fn commit(&mut self, change_id: &str) -> Result<bool, KithError> {
        let cred = self.make_credential(change_id.as_bytes());

        let request = tonic::Request::new(proto::CommitRequest {
            change_id: change_id.into(),
            credential: Some(cred),
        });

        let response = self
            .client
            .commit(request)
            .await
            .map_err(|s| KithError::Internal(format!("commit failed: {s}")))?;

        Ok(response.into_inner().success)
    }

    /// Rollback a pending change (native tool: rollback).
    pub async fn rollback(&mut self, change_id: &str) -> Result<bool, KithError> {
        let cred = self.make_credential(change_id.as_bytes());

        let request = tonic::Request::new(proto::RollbackRequest {
            change_id: change_id.into(),
            credential: Some(cred),
        });

        let response = self
            .client
            .rollback(request)
            .await
            .map_err(|s| KithError::Internal(format!("rollback failed: {s}")))?;

        Ok(response.into_inner().success)
    }

    /// Query machine state.
    pub async fn query(&mut self) -> Result<String, KithError> {
        let cred = self.make_credential(b"query");

        let request = tonic::Request::new(proto::QueryRequest {
            credential: Some(cred),
            query_type: 0,
        });

        let response = self
            .client
            .query(request)
            .await
            .map_err(|s| KithError::Internal(format!("query failed: {s}")))?;

        Ok(response.into_inner().json_payload)
    }

    pub fn host(&self) -> &str {
        &self.host
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kith_common::policy::{MachinePolicy, Scope};
    use kith_daemon::audit::AuditLog;
    use kith_daemon::commit::CommitWindowManager;
    use kith_daemon::policy::PolicyEvaluator;
    use kith_daemon::proto::kith_daemon_server::KithDaemonServer;
    use kith_daemon::service::KithDaemonService;
    use std::time::Duration;

    /// Start a daemon in-process on a random port, return the address.
    async fn start_test_daemon(keypair: &Keypair) -> String {
        let pubkey_hex = kith_common::credential::pubkey_to_hex(&keypair.public_key_bytes());

        let mut policy = MachinePolicy::default();
        policy.users.insert(pubkey_hex, Scope::Ops);

        let evaluator = PolicyEvaluator::new(policy, "test-daemon".into());
        let audit = AuditLog::new("test-daemon");
        let commit = CommitWindowManager::new(Duration::from_secs(600));
        let service = KithDaemonService::new(evaluator, audit, commit, "test-daemon".into());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(KithDaemonServer::new(service))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        // Small delay for server startup
        tokio::time::sleep(Duration::from_millis(50)).await;

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn e2e_remote_exec() {
        let kp = Keypair::generate();
        let addr = start_test_daemon(&kp).await;

        let mut client = DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();

        let result = client.exec("echo integration-test").await.unwrap();
        assert_eq!(result.stdout.trim(), "integration-test");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn e2e_apply_commit_cycle() {
        let kp = Keypair::generate();
        let addr = start_test_daemon(&kp).await;

        let mut client = DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();

        let pending_id = client.apply("docker compose up", 600).await.unwrap();
        assert!(!pending_id.is_empty());

        let committed = client.commit(&pending_id).await.unwrap();
        assert!(committed);
    }

    #[tokio::test]
    async fn e2e_apply_rollback() {
        let kp = Keypair::generate();
        let addr = start_test_daemon(&kp).await;

        let mut client = DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();

        let pending_id = client.apply("risky change", 600).await.unwrap();
        let rolled_back = client.rollback(&pending_id).await.unwrap();
        assert!(rolled_back);
    }

    #[tokio::test]
    async fn e2e_query_state() {
        let kp = Keypair::generate();
        let addr = start_test_daemon(&kp).await;

        let mut client = DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();

        let state = client.query().await.unwrap();
        assert!(state.contains("test-daemon"));
        assert!(state.contains("ok"));
    }

    #[tokio::test]
    async fn e2e_unauthorized_exec() {
        let server_kp = Keypair::generate();
        let addr = start_test_daemon(&server_kp).await;

        // Connect with a DIFFERENT keypair — not authorized
        let unauthorized_kp = Keypair::generate();
        let mut client = DaemonClient::connect(&addr, unauthorized_kp).await.unwrap();

        let result = client.exec("echo should-fail").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("denied") || err.contains("unknown") || err.contains("PERMISSION_DENIED"),
            "expected auth error, got: {err}"
        );
    }

    #[tokio::test]
    async fn e2e_unreachable_daemon() {
        let kp = Keypair::generate();
        let result = DaemonClient::connect("127.0.0.1:1", kp).await;
        assert!(result.is_err());
    }
}
