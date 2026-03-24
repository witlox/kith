//! Policy types. Per-machine, per-user scope. Intentionally simple.
//! Scope is server-determined from MachinePolicy (ADR-006, F-02).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::drift::DriftWeights;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Scope {
    Ops,
    Viewer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachinePolicy {
    pub users: HashMap<String, Scope>,
    pub default_scope: Option<Scope>,
    pub commit_window_seconds: u32,
    pub drift_blacklist: Vec<String>,
    pub drift_weights: DriftWeights,
    /// Allow unknown keys with viewer scope on first contact (TOFU).
    pub tofu: bool,
}

impl Default for MachinePolicy {
    fn default() -> Self {
        Self {
            users: HashMap::new(),
            default_scope: None,
            commit_window_seconds: 600,
            drift_blacklist: vec![
                "/tmp/**".into(),
                "/var/log/**".into(),
                "/proc/**".into(),
                "/sys/**".into(),
                "/dev/**".into(),
                "/run/user/**".into(),
            ],
            drift_weights: DriftWeights::default(),
            tofu: false,
        }
    }
}

impl MachinePolicy {
    /// Look up the scope for a given public key (hex-encoded).
    /// Returns None if the key is unknown and no default/TOFU is configured.
    pub fn scope_for(&self, pubkey_hex: &str) -> Option<Scope> {
        if let Some(scope) = self.users.get(pubkey_hex) {
            return Some(scope.clone());
        }
        if self.tofu {
            return Some(Scope::Viewer);
        }
        self.default_scope.clone()
    }

    /// Check if a scope permits a given action category.
    pub fn evaluate(scope: &Scope, action: &ActionCategory) -> PolicyDecision {
        match (scope, action) {
            // Viewers can only query and read events
            (
                Scope::Viewer,
                ActionCategory::Query | ActionCategory::Events | ActionCategory::Capabilities,
            ) => PolicyDecision::Allow,
            (Scope::Viewer, _) => PolicyDecision::Deny {
                reason: format!("viewer scope cannot perform {action:?}"),
            },
            // Ops can do everything
            (Scope::Ops, _) => PolicyDecision::Allow,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ActionCategory {
    Exec,
    Query,
    Apply,
    Commit,
    Rollback,
    Events,
    Capabilities,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_with_user(pubkey: &str, scope: Scope) -> MachinePolicy {
        let mut p = MachinePolicy::default();
        p.users.insert(pubkey.into(), scope);
        p
    }

    #[test]
    fn ops_user_can_do_everything() {
        let scope = Scope::Ops;
        let actions = [
            ActionCategory::Exec,
            ActionCategory::Query,
            ActionCategory::Apply,
            ActionCategory::Commit,
            ActionCategory::Rollback,
            ActionCategory::Events,
            ActionCategory::Capabilities,
        ];
        for action in &actions {
            assert_eq!(
                MachinePolicy::evaluate(&scope, action),
                PolicyDecision::Allow,
                "ops should allow {action:?}"
            );
        }
    }

    #[test]
    fn viewer_can_query_and_read() {
        let scope = Scope::Viewer;
        assert_eq!(
            MachinePolicy::evaluate(&scope, &ActionCategory::Query),
            PolicyDecision::Allow
        );
        assert_eq!(
            MachinePolicy::evaluate(&scope, &ActionCategory::Events),
            PolicyDecision::Allow
        );
        assert_eq!(
            MachinePolicy::evaluate(&scope, &ActionCategory::Capabilities),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn viewer_cannot_execute() {
        let scope = Scope::Viewer;
        let denied = [
            ActionCategory::Exec,
            ActionCategory::Apply,
            ActionCategory::Commit,
            ActionCategory::Rollback,
        ];
        for action in &denied {
            assert!(
                matches!(
                    MachinePolicy::evaluate(&scope, action),
                    PolicyDecision::Deny { .. }
                ),
                "viewer should deny {action:?}"
            );
        }
    }

    #[test]
    fn scope_lookup_known_user() {
        let policy = policy_with_user("abc123", Scope::Ops);
        assert_eq!(policy.scope_for("abc123"), Some(Scope::Ops));
    }

    #[test]
    fn scope_lookup_unknown_user_no_default() {
        let policy = MachinePolicy::default();
        assert_eq!(policy.scope_for("unknown"), None);
    }

    #[test]
    fn scope_lookup_unknown_user_with_tofu() {
        let mut policy = MachinePolicy::default();
        policy.tofu = true;
        assert_eq!(policy.scope_for("unknown"), Some(Scope::Viewer));
    }

    #[test]
    fn scope_lookup_unknown_user_with_default() {
        let mut policy = MachinePolicy::default();
        policy.default_scope = Some(Scope::Viewer);
        assert_eq!(policy.scope_for("unknown"), Some(Scope::Viewer));
    }

    #[test]
    fn known_user_overrides_tofu() {
        let mut policy = MachinePolicy::default();
        policy.tofu = true;
        policy.users.insert("known".into(), Scope::Ops);
        assert_eq!(policy.scope_for("known"), Some(Scope::Ops));
    }

    #[test]
    fn default_policy_has_standard_blacklist() {
        let policy = MachinePolicy::default();
        assert!(policy.drift_blacklist.contains(&"/tmp/**".to_string()));
        assert!(policy.drift_blacklist.contains(&"/proc/**".to_string()));
    }
}
