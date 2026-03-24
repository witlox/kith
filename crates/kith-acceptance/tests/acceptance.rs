//! BDD acceptance tests for kith.
//! Uses cucumber-rs to run Gherkin feature files.

#![allow(unused_variables, unused_imports, dead_code)]

use std::collections::{HashMap, HashSet};

use cucumber::World;
use kith_common::credential::Keypair;
use kith_common::drift::{DriftVector, DriftWeights};
use kith_common::event::{Event, EventCategory, EventScope};
use kith_common::inference::*;
use kith_common::policy::{MachinePolicy, PolicyDecision, Scope};
use kith_daemon::audit::AuditLog;
use kith_daemon::commit::CommitWindowManager;
use kith_daemon::drift::{DriftEvaluator, ObserverEvent};
use kith_mesh::peer::PeerRegistry;
use kith_shell::classify::{InputClass, InputClassifier};
use kith_shell::mock_backend::MockInferenceBackend;
use kith_sync::store::EventStore;

mod steps;

#[derive(World)]
#[world(init = Self::new)]
pub struct KithWorld {
    // --- Drift ---
    pub drift_evaluator: DriftEvaluator,
    pub drift_vector: DriftVector,
    pub drift_weights: DriftWeights,

    // --- Policy ---
    pub policy: MachinePolicy,
    pub per_machine_scopes: HashMap<(String, String), Scope>, // (user, machine) -> scope
    pub last_policy_decision: Option<PolicyDecision>,

    // --- Commit windows ---
    pub commit_mgr: CommitWindowManager,
    pub last_pending_id: Option<String>,
    pub last_commit_result: Option<bool>,
    pub expired_ids: Vec<String>,

    // --- Exec ---
    pub last_exec_result: Option<kith_daemon::exec::ExecResult>,
    pub last_exec_error: Option<String>,

    // --- Shell / classification ---
    pub classifier: InputClassifier,
    pub last_classification: Option<InputClass>,
    pub inference_reachable: bool,
    pub mock_backend: MockInferenceBackend,
    pub backend_was_called: bool,

    // --- Mesh ---
    pub peer_registry: PeerRegistry,
    pub mesh_events: Vec<kith_mesh::peer::MeshEvent>,

    // --- Sync / state ---
    pub event_store: EventStore,
    pub retrieval_results: Vec<kith_state::retrieval::RetrievalResult>,

    // --- Context ---
    pub current_machine: String,
    pub current_user: Option<String>,
    pub current_backend_name: String,
    pub keypair: Option<Keypair>,

    // --- Audit ---
    pub audit_log: AuditLog,

    // --- Notifications ---
    pub notifications: Vec<String>,

    // --- Tracking ---
    pub last_drift_event: Option<ObserverEvent>,
    pub expected_services: Vec<String>,
    pub ops_events_written: bool,
}

impl std::fmt::Debug for KithWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KithWorld")
            .field("current_machine", &self.current_machine)
            .field("current_user", &self.current_user)
            .finish_non_exhaustive()
    }
}

impl KithWorld {
    fn new() -> Self {
        let blacklist = vec![
            "/tmp/**".into(),
            "/var/log/**".into(),
            "/proc/**".into(),
            "/sys/**".into(),
            "/dev/**".into(),
            "/run/user/**".into(),
        ];
        Self {
            drift_evaluator: DriftEvaluator::new(blacklist.clone(), DriftWeights::default()),
            drift_vector: DriftVector::default(),
            drift_weights: DriftWeights::default(),

            policy: MachinePolicy::default(),
            per_machine_scopes: HashMap::new(),
            last_policy_decision: None,

            commit_mgr: CommitWindowManager::new(std::time::Duration::from_secs(600)),
            last_pending_id: None,
            last_commit_result: None,
            expired_ids: Vec::new(),

            last_exec_result: None,
            last_exec_error: None,

            classifier: InputClassifier::from_path_env(),
            last_classification: None,
            inference_reachable: true,
            mock_backend: MockInferenceBackend::new("mock-default"),
            backend_was_called: false,

            peer_registry: PeerRegistry::new(3600),
            mesh_events: Vec::new(),

            event_store: EventStore::new(),
            retrieval_results: Vec::new(),

            current_machine: "test-machine".into(),
            current_user: None,
            current_backend_name: "mock-default".into(),
            keypair: None,

            audit_log: AuditLog::new("test-machine"),

            notifications: Vec::new(),

            last_drift_event: None,
            expected_services: Vec::new(),
            ops_events_written: false,
        }
    }
}

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(KithWorld::cucumber().run("features/"));
}
