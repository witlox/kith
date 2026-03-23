//! Policy evaluator — authenticates credentials and checks scope.
//! Scope is server-determined from MachinePolicy (ADR-006, F-02).

use kith_common::credential::{self, Credential};
use kith_common::error::KithError;
use kith_common::policy::{ActionCategory, MachinePolicy, PolicyDecision, Scope};

/// Authenticates a credential and evaluates policy in one step.
/// Returns (pubkey_hex, scope, decision).
pub struct PolicyEvaluator {
    policy: MachinePolicy,
    machine_name: String,
    max_clock_skew_ms: i64,
}

impl PolicyEvaluator {
    pub fn new(policy: MachinePolicy, machine_name: String) -> Self {
        Self {
            policy,
            machine_name,
            max_clock_skew_ms: 30_000,
        }
    }

    /// Authenticate credential and check if the action is permitted.
    pub fn evaluate(
        &self,
        cred: &Credential,
        request_hash: &[u8],
        action: &ActionCategory,
    ) -> Result<PolicyDecision, KithError> {
        let now_ms = chrono::Utc::now().timestamp_millis();

        // Step 1: Verify credential signature + freshness
        let pubkey = credential::verify_credential(cred, request_hash, now_ms, self.max_clock_skew_ms)?;
        let pubkey_hex = credential::pubkey_to_hex(&pubkey);

        // Step 2: Look up scope from policy (never from request)
        let scope = match self.policy.scope_for(&pubkey_hex) {
            Some(s) => s,
            None => {
                return Ok(PolicyDecision::Deny {
                    reason: format!(
                        "unknown identity on {}: {}",
                        self.machine_name,
                        &pubkey_hex[..16]
                    ),
                });
            }
        };

        // Step 3: Evaluate action against scope
        Ok(MachinePolicy::evaluate(&scope, action))
    }

    /// Get the scope for an already-authenticated pubkey.
    pub fn scope_for(&self, pubkey_hex: &str) -> Option<Scope> {
        self.policy.scope_for(pubkey_hex)
    }

    pub fn machine_name(&self) -> &str {
        &self.machine_name
    }

    pub fn policy(&self) -> &MachinePolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kith_common::credential::Keypair;

    fn setup() -> (Keypair, PolicyEvaluator) {
        let kp = Keypair::generate();
        let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());

        let mut policy = MachinePolicy::default();
        policy.users.insert(pubkey_hex, Scope::Ops);

        let evaluator = PolicyEvaluator::new(policy, "staging-1".into());
        (kp, evaluator)
    }

    fn viewer_setup() -> (Keypair, PolicyEvaluator) {
        let kp = Keypair::generate();
        let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());

        let mut policy = MachinePolicy::default();
        policy.users.insert(pubkey_hex, Scope::Viewer);

        let evaluator = PolicyEvaluator::new(policy, "staging-1".into());
        (kp, evaluator)
    }

    #[test]
    fn ops_user_allowed_exec() {
        let (kp, eval) = setup();
        let now = chrono::Utc::now().timestamp_millis();
        let hash = b"exec-docker-ps";
        let cred = kp.sign(now, hash);

        let result = eval.evaluate(&cred, hash, &ActionCategory::Exec).unwrap();
        assert_eq!(result, PolicyDecision::Allow);
    }

    #[test]
    fn viewer_denied_exec() {
        let (kp, eval) = viewer_setup();
        let now = chrono::Utc::now().timestamp_millis();
        let hash = b"exec-docker-ps";
        let cred = kp.sign(now, hash);

        let result = eval.evaluate(&cred, hash, &ActionCategory::Exec).unwrap();
        assert!(matches!(result, PolicyDecision::Deny { .. }));
    }

    #[test]
    fn viewer_allowed_query() {
        let (kp, eval) = viewer_setup();
        let now = chrono::Utc::now().timestamp_millis();
        let hash = b"query";
        let cred = kp.sign(now, hash);

        let result = eval.evaluate(&cred, hash, &ActionCategory::Query).unwrap();
        assert_eq!(result, PolicyDecision::Allow);
    }

    #[test]
    fn unknown_user_denied() {
        let unknown_kp = Keypair::generate();
        let (_, eval) = setup(); // eval doesn't know unknown_kp
        let now = chrono::Utc::now().timestamp_millis();
        let hash = b"exec";
        let cred = unknown_kp.sign(now, hash);

        let result = eval.evaluate(&cred, hash, &ActionCategory::Exec).unwrap();
        assert!(matches!(result, PolicyDecision::Deny { reason } if reason.contains("unknown identity")));
    }

    #[test]
    fn expired_credential_rejected() {
        let (kp, eval) = setup();
        let old = chrono::Utc::now().timestamp_millis() - 60_000;
        let hash = b"exec";
        let cred = kp.sign(old, hash);

        let result = eval.evaluate(&cred, hash, &ActionCategory::Exec);
        assert!(matches!(result, Err(KithError::CredentialsExpired)));
    }

    #[test]
    fn tampered_credential_rejected() {
        let (kp, eval) = setup();
        let now = chrono::Utc::now().timestamp_millis();
        let hash = b"exec";
        let mut cred = kp.sign(now, hash);
        cred.signature[0] ^= 0xFF;

        let result = eval.evaluate(&cred, hash, &ActionCategory::Exec);
        assert!(matches!(result, Err(KithError::InvalidCredential(_))));
    }

    #[test]
    fn wrong_request_hash_rejected() {
        let (kp, eval) = setup();
        let now = chrono::Utc::now().timestamp_millis();
        let cred = kp.sign(now, b"original");

        let result = eval.evaluate(&cred, b"tampered", &ActionCategory::Exec);
        assert!(matches!(result, Err(KithError::InvalidCredential(_))));
    }

    #[test]
    fn tofu_gives_viewer_scope() {
        let kp = Keypair::generate();
        let mut policy = MachinePolicy::default();
        policy.tofu = true;

        let eval = PolicyEvaluator::new(policy, "staging-1".into());
        let now = chrono::Utc::now().timestamp_millis();
        let hash = b"query";
        let cred = kp.sign(now, hash);

        // TOFU allows queries (viewer scope)
        let result = eval.evaluate(&cred, hash, &ActionCategory::Query).unwrap();
        assert_eq!(result, PolicyDecision::Allow);

        // TOFU denies exec (viewer scope)
        let hash2 = b"exec";
        let cred2 = kp.sign(now, hash2);
        let result = eval.evaluate(&cred2, hash2, &ActionCategory::Exec).unwrap();
        assert!(matches!(result, PolicyDecision::Deny { .. }));
    }
}
