//! BDD acceptance tests for kith.
//! Uses cucumber-rs to run Gherkin feature files.

use cucumber::World;
use kith_common::drift::{DriftVector, DriftWeights};
use kith_common::policy::{MachinePolicy, Scope};

mod steps;

#[derive(World, Debug)]
#[world(init = Self::new)]
pub struct KithWorld {
    // Real kith-common types — wired to real code, not mocks
    pub drift_vector: DriftVector,
    pub drift_weights: DriftWeights,
    pub policy: MachinePolicy,
    pub last_policy_decision: Option<kith_common::policy::PolicyDecision>,
    pub last_error: Option<String>,
}

impl KithWorld {
    fn new() -> Self {
        Self {
            drift_vector: DriftVector::default(),
            drift_weights: DriftWeights::default(),
            policy: MachinePolicy::default(),
            last_policy_decision: None,
            last_error: None,
        }
    }
}

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(KithWorld::cucumber().run("features/"));
}
