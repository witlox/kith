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

struct SysInfo {
    tools: Vec<String>,
    services: Vec<String>,
    memory_total: u64,
    memory_available: u64,
    disk_total: u64,
    disk_available: u64,
    cpu_count: u32,
}

fn sysinfo() -> SysInfo {
    // Real PATH scan via ToolRegistry
    let registry = kith_common::tool_registry::ToolRegistry::scan();
    let tools: Vec<String> = registry
        .to_capability_tools()
        .into_iter()
        .map(|(name, cat, ver)| {
            if let Some(v) = ver {
                format!("{name} ({cat}, {v})")
            } else {
                format!("{name} ({cat})")
            }
        })
        .collect();

    // Check running services (basic: ps-based)
    let services: Vec<String> = std::process::Command::new("ps")
        .args(["aux"])
        .output()
        .ok()
        .map(|o| {
            let output = String::from_utf8_lossy(&o.stdout);
            let mut found = Vec::new();
            for svc in ["docker", "nginx", "postgres", "redis", "sshd"] {
                if output.contains(svc) {
                    found.push(svc.to_string());
                }
            }
            found
        })
        .unwrap_or_default();

    // CPU count
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);

    // Memory info (platform-specific)
    let (memory_total, memory_available) = get_memory_info();

    // Disk info for root partition
    let (disk_total, disk_available) = get_disk_info("/");

    SysInfo {
        tools,
        services,
        memory_total,
        memory_available,
        disk_total,
        disk_available,
        cpu_count,
    }
}

fn get_memory_info() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            let parse_kb = |key: &str| -> u64 {
                content
                    .lines()
                    .find(|l| l.starts_with(key))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
                    * 1024
            };
            return (parse_kb("MemTotal:"), parse_kb("MemAvailable:"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
        {
            let total = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<u64>()
                .unwrap_or(0);
            return (total, 0); // available not easily accessible on macOS
        }
    }
    (0, 0)
}

fn get_disk_info(path: &str) -> (u64, u64) {
    if let Ok(output) = std::process::Command::new("df").args(["-k", path]).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = text.lines().nth(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 4 {
                let total = fields[1].parse::<u64>().unwrap_or(0) * 1024;
                let available = fields[3].parse::<u64>().unwrap_or(0) * 1024;
                return (total, available);
            }
        }
    }
    (0, 0)
}

/// Maximum number of concurrent command executions.
const MAX_CONCURRENT_EXEC: usize = 16;

/// Replay protection: tracks recently seen credential signatures.
struct ReplayGuard {
    seen: std::collections::HashSet<[u8; 64]>,
    timestamps: Vec<(i64, [u8; 64])>,
}

impl ReplayGuard {
    fn new() -> Self {
        Self {
            seen: std::collections::HashSet::new(),
            timestamps: Vec::new(),
        }
    }

    /// Check if signature was already seen. If not, record it.
    /// Evicts entries older than 60 seconds.
    fn check_and_record(&mut self, signature: &[u8], timestamp_ms: i64) -> bool {
        let sig: [u8; 64] = match signature.try_into() {
            Ok(s) => s,
            Err(_) => return false, // malformed, will fail signature check anyway
        };

        // Evict old entries
        let cutoff = timestamp_ms - 60_000;
        self.timestamps.retain(|(ts, s)| {
            if *ts < cutoff {
                self.seen.remove(s);
                false
            } else {
                true
            }
        });

        if self.seen.contains(&sig) {
            return false; // replay
        }

        self.seen.insert(sig);
        self.timestamps.push((timestamp_ms, sig));
        true
    }
}

pub struct KithDaemonService {
    policy: Arc<PolicyEvaluator>,
    audit: Arc<Mutex<AuditLog>>,
    commit_mgr: Arc<Mutex<CommitWindowManager>>,
    tx_mgr: Arc<Mutex<crate::containment::TransactionManager>>,
    event_store: Arc<kith_sync::store::EventStore>,
    exec_semaphore: Arc<tokio::sync::Semaphore>,
    replay_guard: Arc<Mutex<ReplayGuard>>,
    machine_name: String,
}

impl KithDaemonService {
    pub fn new(
        policy: PolicyEvaluator,
        audit: AuditLog,
        commit_mgr: CommitWindowManager,
        machine_name: String,
    ) -> Self {
        let backup_dir = std::env::temp_dir().join("kith-containment");
        Self {
            policy: Arc::new(policy),
            audit: Arc::new(Mutex::new(audit)),
            commit_mgr: Arc::new(Mutex::new(commit_mgr)),
            tx_mgr: Arc::new(Mutex::new(crate::containment::TransactionManager::new(
                backup_dir,
            ))),
            event_store: Arc::new(kith_sync::store::EventStore::new()),
            exec_semaphore: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_EXEC)),
            replay_guard: Arc::new(Mutex::new(ReplayGuard::new())),
            machine_name,
        }
    }

    /// Create with an external EventStore (for sharing with background tasks).
    pub fn with_event_store(
        policy: PolicyEvaluator,
        audit: AuditLog,
        commit_mgr: CommitWindowManager,
        machine_name: String,
        event_store: Arc<kith_sync::store::EventStore>,
    ) -> Self {
        let backup_dir = std::env::temp_dir().join("kith-containment");
        Self {
            policy: Arc::new(policy),
            audit: Arc::new(Mutex::new(audit)),
            commit_mgr: Arc::new(Mutex::new(commit_mgr)),
            tx_mgr: Arc::new(Mutex::new(crate::containment::TransactionManager::new(
                backup_dir,
            ))),
            event_store,
            exec_semaphore: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_EXEC)),
            replay_guard: Arc::new(Mutex::new(ReplayGuard::new())),
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
    async fn auth(
        &self,
        cred: Option<&proto::Credential>,
        request_context: &[u8],
        action: &ActionCategory,
    ) -> Result<String, Status> {
        let (user, _scope) = self.auth_with_scope(cred, request_context, action).await?;
        Ok(user)
    }

    /// Authenticate + authorize, returning (pubkey_hex, scope) on success.
    async fn auth_with_scope(
        &self,
        cred: Option<&proto::Credential>,
        request_context: &[u8],
        action: &ActionCategory,
    ) -> Result<(String, kith_common::policy::Scope), Status> {
        let (credential, hash) = Self::extract_credential(cred, request_context)?;

        // Replay protection: reject duplicate signatures within the freshness window
        {
            let mut guard = self.replay_guard.lock().await;
            if !guard.check_and_record(&credential.signature, credential.timestamp_unix_ms) {
                return Err(Status::already_exists("credential replay detected"));
            }
        }

        match self.policy.evaluate(&credential, &hash, action) {
            Ok(PolicyDecision::Allow) => {
                let pubkey_hex = kith_common::credential::pubkey_to_hex(
                    &credential
                        .public_key
                        .as_slice()
                        .try_into()
                        .unwrap_or([0u8; 32]),
                );
                let scope = self
                    .policy
                    .scope_for(&pubkey_hex)
                    .unwrap_or(kith_common::policy::Scope::Viewer);
                Ok((pubkey_hex, scope))
            }
            Ok(PolicyDecision::Deny { reason }) => Err(Status::permission_denied(reason)),
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
        let user = self
            .auth(
                req.credential.as_ref(),
                req.command.as_bytes(),
                &ActionCategory::Exec,
            )
            .await?;

        info!(user = %user, command = %req.command, "exec authorized");

        let command = req.command.clone();
        let audit = self.audit.clone();
        let _machine = self.machine_name.clone();
        let semaphore = self.exec_semaphore.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(32);

        tokio::spawn(async move {
            let _permit = match semaphore.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    let _ = tx
                        .send(Err(Status::resource_exhausted(
                            "too many concurrent executions",
                        )))
                        .await;
                    return;
                }
            };
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

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn query(
        &self,
        request: Request<proto::QueryRequest>,
    ) -> Result<Response<proto::StateResponse>, Status> {
        let req = request.into_inner();
        let _user = self
            .auth(req.credential.as_ref(), b"query", &ActionCategory::Query)
            .await?;

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
        let user = self
            .auth(
                req.credential.as_ref(),
                req.command.as_bytes(),
                &ActionCategory::Apply,
            )
            .await?;

        let duration = if req.commit_window_seconds > 0 {
            Some(Duration::from_secs(req.commit_window_seconds as u64))
        } else {
            None
        };

        let pending_id = self.commit_mgr.lock().await.open(&req.command, duration);

        // Begin containment transaction (backup files before change)
        // For now, no specific paths — the transaction tracks the pending_id
        if let Err(e) = self.tx_mgr.lock().await.begin(pending_id.clone(), &[]) {
            tracing::warn!(error = %e, "containment: transaction begin failed (continuing without)");
        }

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
        let user = self
            .auth(
                req.credential.as_ref(),
                req.change_id.as_bytes(),
                &ActionCategory::Commit,
            )
            .await?;

        match self.commit_mgr.lock().await.commit(&req.change_id) {
            Ok(_) => {
                // Commit containment transaction (make changes permanent)
                let _ = self.tx_mgr.lock().await.commit(&req.change_id);
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
        let user = self
            .auth(
                req.credential.as_ref(),
                req.change_id.as_bytes(),
                &ActionCategory::Rollback,
            )
            .await?;

        match self.commit_mgr.lock().await.rollback(&req.change_id) {
            Ok(_) => {
                // Rollback containment transaction (revert changes)
                let _ = self.tx_mgr.lock().await.rollback(&req.change_id);
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
        let (_user, scope) = self
            .auth_with_scope(req.credential.as_ref(), b"events", &ActionCategory::Events)
            .await?;

        // Return audit log entries filtered by caller's scope
        let audit = self.audit.lock().await;
        let event_scope = match scope {
            kith_common::policy::Scope::Ops => kith_common::event::EventScope::Ops,
            kith_common::policy::Scope::Viewer => kith_common::event::EventScope::Public,
        };
        let entries: Vec<_> = audit
            .entries_for_scope(&event_scope)
            .into_iter()
            .cloned()
            .collect();

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

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn capabilities(
        &self,
        request: Request<proto::CapabilitiesRequest>,
    ) -> Result<Response<proto::CapabilityReport>, Status> {
        let req = request.into_inner();
        let _user = self
            .auth(
                req.credential.as_ref(),
                b"capabilities",
                &ActionCategory::Capabilities,
            )
            .await?;

        // Gather real system info
        let sys_info = sysinfo();

        Ok(Response::new(proto::CapabilityReport {
            hostname: self.machine_name.clone(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            installed_tools: sys_info.tools,
            running_services: sys_info.services,
            resources: Some(proto::ResourceInfo {
                memory_total_bytes: sys_info.memory_total,
                memory_available_bytes: sys_info.memory_available,
                disk_total_bytes: sys_info.disk_total,
                disk_available_bytes: sys_info.disk_available,
                cpu_count: sys_info.cpu_count,
                gpus: vec![],
            }),
            reported_at: None,
        }))
    }

    async fn exchange_events(
        &self,
        request: Request<proto::ExchangeEventsRequest>,
    ) -> Result<Response<proto::ExchangeEventsResponse>, Status> {
        let req = request.into_inner();
        let (_user, scope) = self
            .auth_with_scope(
                req.credential.as_ref(),
                b"exchange_events",
                &ActionCategory::Events,
            )
            .await?;

        // Merge incoming events into our store
        let incoming: Vec<kith_common::event::Event> = req
            .our_events
            .iter()
            .map(|e| kith_common::event::Event {
                id: e.event_id.clone(),
                machine: e.origin_host.clone(),
                category: kith_common::event::EventCategory::System, // simplified
                event_type: e.event_type.clone(),
                path: None,
                detail: e.content_json.clone(),
                metadata: serde_json::from_str(&e.metadata_json).unwrap_or(serde_json::Value::Null),
                scope: kith_common::event::EventScope::Ops,
                timestamp: chrono::Utc::now(),
            })
            .collect();

        let merged = self.event_store.merge(incoming).await;
        if merged > 0 {
            info!(merged, "sync: merged events from peer");
        }

        // Return our events since their timestamp, filtered by caller's scope
        let since = req.since_timestamp_ms;
        let event_scope = match scope {
            kith_common::policy::Scope::Ops => None, // Ops sees everything
            kith_common::policy::Scope::Viewer => Some(kith_common::event::EventScope::Public),
        };
        let our_events: Vec<proto::Event> = self
            .event_store
            .query(&kith_sync::store::EventFilter {
                since: chrono::DateTime::from_timestamp_millis(since),
                scope: event_scope,
                ..Default::default()
            })
            .await
            .into_iter()
            .map(|e| proto::Event {
                event_id: e.id,
                event_type: e.event_type,
                origin_host: e.machine,
                timestamp: None,
                scope: format!("{:?}", e.scope),
                metadata_json: e.metadata.to_string(),
                content_json: e.detail,
            })
            .collect();

        Ok(Response::new(proto::ExchangeEventsResponse {
            their_events: our_events,
            current_timestamp_ms: chrono::Utc::now().timestamp_millis(),
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
