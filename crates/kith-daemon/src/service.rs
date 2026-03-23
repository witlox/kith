//! gRPC service implementation — wires policy, exec, audit, commit together.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tonic::{Request, Response, Status};
use tracing::info;

use crate::audit::AuditLog;
use crate::commit::CommitWindowManager;
use crate::exec;
use crate::policy::PolicyEvaluator;
use crate::proto;
use crate::proto::kith_daemon_server::KithDaemon;
use kith_common::credential::Credential;
use kith_common::policy::{ActionCategory, PolicyDecision};

pub struct KithDaemonService {
    policy: Arc<PolicyEvaluator>,
    audit: Arc<Mutex<AuditLog>>,
    commit_mgr: Arc<Mutex<CommitWindowManager>>,
    machine_name: String,
}

impl KithDaemonService {
    pub fn new(
        policy: PolicyEvaluator,
        audit: AuditLog,
        commit_mgr: CommitWindowManager,
        machine_name: String,
    ) -> Self {
        Self {
            policy: Arc::new(policy),
            audit: Arc::new(Mutex::new(audit)),
            commit_mgr: Arc::new(Mutex::new(commit_mgr)),
            machine_name,
        }
    }

    /// Extract credential from proto message and build request hash.
    fn extract_credential(
        cred: Option<&proto::Credential>,
        request_context: &[u8],
    ) -> Result<(Credential, Vec<u8>), Status> {
        let c = cred.ok_or_else(|| Status::unauthenticated("credential required"))?;
        Ok((
            Credential {
                public_key: c.public_key.clone(),
                timestamp_unix_ms: c.timestamp_unix_ms,
                signature: c.signature.clone(),
            },
            request_context.to_vec(),
        ))
    }

    /// Authenticate + authorize, returning the pubkey hex on success.
    fn auth(
        &self,
        cred: Option<&proto::Credential>,
        request_context: &[u8],
        action: &ActionCategory,
    ) -> Result<String, Status> {
        let (credential, hash) = Self::extract_credential(cred, request_context)?;

        match self.policy.evaluate(&credential, &hash, action) {
            Ok(PolicyDecision::Allow) => {
                let pubkey_hex =
                    kith_common::credential::pubkey_to_hex(&credential.public_key.as_slice().try_into().unwrap_or([0u8; 32]));
                Ok(pubkey_hex)
            }
            Ok(PolicyDecision::Deny { reason }) => {
                Err(Status::permission_denied(reason))
            }
            Err(kith_common::error::KithError::CredentialsExpired) => {
                Err(Status::unauthenticated("credentials expired"))
            }
            Err(kith_common::error::KithError::InvalidCredential(msg)) => {
                Err(Status::unauthenticated(msg))
            }
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }
}

#[tonic::async_trait]
impl KithDaemon for KithDaemonService {
    type ExecStream = tokio_stream::wrappers::ReceiverStream<Result<proto::ExecOutput, Status>>;

    async fn exec(
        &self,
        request: Request<proto::ExecRequest>,
    ) -> Result<Response<Self::ExecStream>, Status> {
        let req = request.into_inner();
        let user = self.auth(
            req.credential.as_ref(),
            req.command.as_bytes(),
            &ActionCategory::Exec,
        )?;

        info!(user = %user, command = %req.command, "exec authorized");

        let command = req.command.clone();
        let audit = self.audit.clone();
        let _machine = self.machine_name.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(32);

        tokio::spawn(async move {
            match exec::exec_command(&command).await {
                Ok(result) => {
                    // Send stdout
                    if !result.stdout.is_empty() {
                        let _ = tx
                            .send(Ok(proto::ExecOutput {
                                output: Some(proto::exec_output::Output::Stdout(
                                    result.stdout.into_bytes(),
                                )),
                                is_complete: false,
                                exit_code: 0,
                            }))
                            .await;
                    }
                    // Send stderr
                    if !result.stderr.is_empty() {
                        let _ = tx
                            .send(Ok(proto::ExecOutput {
                                output: Some(proto::exec_output::Output::Stderr(
                                    result.stderr.into_bytes(),
                                )),
                                is_complete: false,
                                exit_code: 0,
                            }))
                            .await;
                    }
                    // Send completion
                    let _ = tx
                        .send(Ok(proto::ExecOutput {
                            output: None,
                            is_complete: true,
                            exit_code: result.exit_code,
                        }))
                        .await;

                    audit
                        .lock()
                        .await
                        .record_exec(&user, &command, Some(result.exit_code), None);
                }
                Err(e) => {
                    let _ = tx.send(Err(Status::internal(e.to_string()))).await;
                    audit
                        .lock()
                        .await
                        .record_exec(&user, &command, None, Some(&e.to_string()));
                }
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn query(
        &self,
        request: Request<proto::QueryRequest>,
    ) -> Result<Response<proto::StateResponse>, Status> {
        let req = request.into_inner();
        let _user = self.auth(
            req.credential.as_ref(),
            b"query",
            &ActionCategory::Query,
        )?;

        // Return basic state info
        let payload = serde_json::json!({
            "hostname": self.machine_name,
            "status": "ok",
        });

        Ok(Response::new(proto::StateResponse {
            hostname: self.machine_name.clone(),
            json_payload: payload.to_string(),
            observed_at: None,
        }))
    }

    async fn apply(
        &self,
        request: Request<proto::ApplyRequest>,
    ) -> Result<Response<proto::PendingChange>, Status> {
        let req = request.into_inner();
        let user = self.auth(
            req.credential.as_ref(),
            req.command.as_bytes(),
            &ActionCategory::Apply,
        )?;

        let duration = if req.commit_window_seconds > 0 {
            Some(Duration::from_secs(req.commit_window_seconds as u64))
        } else {
            None
        };

        let pending_id = self.commit_mgr.lock().await.open(&req.command, duration);

        self.audit
            .lock()
            .await
            .record_change("change.applied", &pending_id, &user);

        info!(user = %user, pending_id = %pending_id, "change applied");

        Ok(Response::new(proto::PendingChange {
            change_id: pending_id,
            expires_at: None,
            preview: req.command,
        }))
    }

    async fn commit(
        &self,
        request: Request<proto::CommitRequest>,
    ) -> Result<Response<proto::CommitResult>, Status> {
        let req = request.into_inner();
        let user = self.auth(
            req.credential.as_ref(),
            req.change_id.as_bytes(),
            &ActionCategory::Commit,
        )?;

        match self.commit_mgr.lock().await.commit(&req.change_id) {
            Ok(_) => {
                self.audit
                    .lock()
                    .await
                    .record_change("change.committed", &req.change_id, &user);
                Ok(Response::new(proto::CommitResult {
                    success: true,
                    message: "committed".into(),
                }))
            }
            Err(e) => Ok(Response::new(proto::CommitResult {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn rollback(
        &self,
        request: Request<proto::RollbackRequest>,
    ) -> Result<Response<proto::RollbackResult>, Status> {
        let req = request.into_inner();
        let user = self.auth(
            req.credential.as_ref(),
            req.change_id.as_bytes(),
            &ActionCategory::Rollback,
        )?;

        match self.commit_mgr.lock().await.rollback(&req.change_id) {
            Ok(_) => {
                self.audit
                    .lock()
                    .await
                    .record_change("change.rolled_back", &req.change_id, &user);
                Ok(Response::new(proto::RollbackResult {
                    success: true,
                    message: "rolled back".into(),
                }))
            }
            Err(e) => Ok(Response::new(proto::RollbackResult {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    type EventsStream = tokio_stream::wrappers::ReceiverStream<Result<proto::Event, Status>>;

    async fn events(
        &self,
        request: Request<proto::EventsRequest>,
    ) -> Result<Response<Self::EventsStream>, Status> {
        let req = request.into_inner();
        let _user = self.auth(
            req.credential.as_ref(),
            b"events",
            &ActionCategory::Events,
        )?;

        // Return audit log entries as events
        let audit = self.audit.lock().await;
        let entries = audit.entries().to_vec();

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            for entry in entries {
                let _ = tx
                    .send(Ok(proto::Event {
                        event_id: entry.id,
                        event_type: entry.event_type,
                        origin_host: entry.machine,
                        timestamp: None,
                        scope: format!("{:?}", entry.scope),
                        metadata_json: entry.metadata.to_string(),
                        content_json: entry.detail,
                    }))
                    .await;
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn capabilities(
        &self,
        request: Request<proto::CapabilitiesRequest>,
    ) -> Result<Response<proto::CapabilityReport>, Status> {
        let req = request.into_inner();
        let _user = self.auth(
            req.credential.as_ref(),
            b"capabilities",
            &ActionCategory::Capabilities,
        )?;

        Ok(Response::new(proto::CapabilityReport {
            hostname: self.machine_name.clone(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            installed_tools: vec![],
            running_services: vec![],
            resources: None,
            reported_at: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::kith_daemon_server::KithDaemon;
    use kith_common::credential::Keypair;
    use kith_common::policy::{MachinePolicy, Scope};

    fn setup_service() -> (Keypair, KithDaemonService) {
        let kp = Keypair::generate();
        let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());

        let mut policy = MachinePolicy::default();
        policy.users.insert(pubkey_hex, Scope::Ops);

        let evaluator = PolicyEvaluator::new(policy, "test-machine".into());
        let audit = AuditLog::new("test-machine");
        let commit = CommitWindowManager::new(Duration::from_secs(600));

        let service = KithDaemonService::new(evaluator, audit, commit, "test-machine".into());
        (kp, service)
    }

    fn make_proto_cred(kp: &Keypair, request_hash: &[u8]) -> proto::Credential {
        let now = chrono::Utc::now().timestamp_millis();
        let cred = kp.sign(now, request_hash);
        proto::Credential {
            public_key: cred.public_key,
            timestamp_unix_ms: cred.timestamp_unix_ms,
            signature: cred.signature,
        }
    }

    #[tokio::test]
    async fn exec_authorized_returns_output() {
        let (kp, service) = setup_service();
        let cred = make_proto_cred(&kp, b"echo hello");

        let request = Request::new(proto::ExecRequest {
            command: "echo hello".into(),
            credential: Some(cred),
        });

        let response = service.exec(request).await.unwrap();
        let mut stream = response.into_inner();

        let mut got_stdout = false;
        let mut got_complete = false;

        while let Some(item) = tokio_stream::StreamExt::next(&mut stream).await {
            let output = item.unwrap();
            if let Some(proto::exec_output::Output::Stdout(data)) = &output.output {
                let text = String::from_utf8_lossy(data);
                assert!(text.contains("hello"));
                got_stdout = true;
            }
            if output.is_complete {
                assert_eq!(output.exit_code, 0);
                got_complete = true;
            }
        }

        assert!(got_stdout, "should have received stdout");
        assert!(got_complete, "should have received completion");
    }

    #[tokio::test]
    async fn exec_unauthorized_returns_permission_denied() {
        let (_, service) = setup_service();
        let unknown_kp = Keypair::generate();
        let cred = make_proto_cred(&unknown_kp, b"echo hello");

        let request = Request::new(proto::ExecRequest {
            command: "echo hello".into(),
            credential: Some(cred),
        });

        let result: Result<Response<_>, Status> = service.exec(request).await;
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn exec_no_credential_returns_unauthenticated() {
        let (_, service) = setup_service();

        let request = Request::new(proto::ExecRequest {
            command: "echo hello".into(),
            credential: None,
        });

        let result: Result<Response<_>, Status> = service.exec(request).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn query_returns_state() {
        let (kp, service) = setup_service();
        let cred = make_proto_cred(&kp, b"query");

        let request = Request::new(proto::QueryRequest {
            credential: Some(cred),
            query_type: 0,
        });

        let response = service.query(request).await.unwrap();
        let state = response.into_inner();
        assert_eq!(state.hostname, "test-machine");
        assert!(state.json_payload.contains("ok"));
    }

    #[tokio::test]
    async fn apply_commit_cycle() {
        let (kp, service) = setup_service();

        // Apply
        let cred = make_proto_cred(&kp, b"docker compose up");
        let apply_resp = service
            .apply(Request::new(proto::ApplyRequest {
                command: "docker compose up".into(),
                credential: Some(cred),
                commit_window_seconds: 600,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!apply_resp.change_id.is_empty());

        // Commit
        let cred2 = make_proto_cred(&kp, apply_resp.change_id.as_bytes());
        let commit_resp = service
            .commit(Request::new(proto::CommitRequest {
                change_id: apply_resp.change_id,
                credential: Some(cred2),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(commit_resp.success);
    }

    #[tokio::test]
    async fn exec_creates_audit_entry() {
        let (kp, service) = setup_service();
        let cred = make_proto_cred(&kp, b"echo audit-test");

        let request = Request::new(proto::ExecRequest {
            command: "echo audit-test".into(),
            credential: Some(cred),
        });

        let response: Response<_> = service.exec(request).await.unwrap();
        // Drain the stream
        let mut stream = response.into_inner();
        while let Some(_item) = tokio_stream::StreamExt::next(&mut stream).await {}

        // Small delay for the spawned audit task
        tokio::time::sleep(Duration::from_millis(50)).await;

        let audit = service.audit.lock().await;
        assert!(!audit.is_empty(), "audit should have entries");
        let last = audit.entries().last().unwrap();
        assert_eq!(last.event_type, "exec.command");
    }
}
