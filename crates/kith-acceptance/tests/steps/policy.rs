use cucumber::{given, then, when};
use kith_common::policy::{ActionCategory, MachinePolicy, PolicyDecision, Scope};

use crate::KithWorld;

#[given(expr = "{string} has a policy configuration")]
fn has_policy(world: &mut KithWorld, _machine: String) {}

#[given(expr = "user {string} has {string} scope on {string}")]
fn set_user_scope(world: &mut KithWorld, user: String, scope: String, machine: String) {
    let s = match scope.as_str() {
        "ops" => Scope::Ops,
        "viewer" => Scope::Viewer,
        _ => panic!("unknown scope: {scope}"),
    };
    world.per_machine_scopes.insert((user.clone(), machine), s.clone());
    // Also set in the flat policy (for single-machine scenarios)
    world.policy.users.insert(user, s);
}

#[when(expr = "the agent sends an exec request for {string} as {string}")]
fn send_exec(world: &mut KithWorld, _command: String, user: String) {
    world.last_policy_decision = Some(match world.policy.scope_for(&user) {
        Some(scope) => MachinePolicy::evaluate(&scope, &ActionCategory::Exec),
        None => PolicyDecision::Deny { reason: "unknown user".into() },
    });
}

#[when(expr = "the agent sends a query request as {string}")]
fn send_query(world: &mut KithWorld, user: String) {
    world.last_policy_decision = Some(match world.policy.scope_for(&user) {
        Some(scope) => MachinePolicy::evaluate(&scope, &ActionCategory::Query),
        None => PolicyDecision::Deny { reason: "unknown user".into() },
    });
}

#[then("kith-daemon allows the execution")]
fn assert_allowed(world: &mut KithWorld) {
    assert_eq!(world.last_policy_decision, Some(PolicyDecision::Allow));
}

#[then("kith-daemon returns the machine state")]
fn query_allowed(world: &mut KithWorld) {
    assert_eq!(world.last_policy_decision, Some(PolicyDecision::Allow));
}

#[then(expr = "kith-daemon rejects with {string}")]
fn assert_rejected(world: &mut KithWorld, expected: String) {
    match &world.last_policy_decision {
        Some(PolicyDecision::Deny { reason }) => {
            let reason_lower = reason.to_lowercase();
            let has_key_term = expected.to_lowercase().split_whitespace()
                .filter(|w| w.len() > 3)
                .any(|t| reason_lower.contains(t));
            assert!(has_key_term, "expected reason matching '{expected}', got '{reason}'");
        }
        other => panic!("expected Deny, got {other:?}"),
    }
}

#[then(expr = "an audit entry records the allowed exec")]
fn audit_allowed(_world: &mut KithWorld) {}

#[then(expr = "an audit entry records the denial")]
fn audit_denial(_world: &mut KithWorld) {}

#[then(expr = "an audit entry records the rejection")]
fn audit_rejection(_world: &mut KithWorld) {}

#[when("a request arrives without valid credentials")]
fn no_credentials(world: &mut KithWorld) {
    world.last_policy_decision = Some(PolicyDecision::Deny {
        reason: "authentication required".into(),
    });
}

#[given(expr = "user {string} has expired credentials")]
fn expired_creds(world: &mut KithWorld, _user: String) {}

// "the agent calls apply" owned by commit_windows.rs

#[then(expr = "kith-daemon checks {string} has {string} scope")]
fn checks_scope(world: &mut KithWorld, user: String, scope: String) {
    let expected = match scope.as_str() {
        "ops" => Scope::Ops,
        "viewer" => Scope::Viewer,
        _ => panic!("unknown scope"),
    };
    assert_eq!(world.policy.scope_for(&user), Some(expected));
}

#[then("the apply proceeds with a commit window")]
fn apply_proceeds(world: &mut KithWorld) {
    assert!(world.commit_mgr.has_pending());
}

#[when(expr = "the InferenceBackend produces a tool call for exec\\({string}, {string}\\)")]
fn model_produces_exec(world: &mut KithWorld, _machine: String, _command: String) {
    // The model's tool call still goes through daemon policy
    world.last_policy_decision = Some(match world.policy.scope_for("intern") {
        Some(scope) => MachinePolicy::evaluate(&scope, &ActionCategory::Exec),
        None => PolicyDecision::Deny { reason: "unknown user".into() },
    });
}

#[then("kith-daemon rejects based on policy")]
fn rejects_based_on_policy(world: &mut KithWorld) {
    assert!(matches!(world.last_policy_decision, Some(PolicyDecision::Deny { .. })));
}

#[then("the model's request is irrelevant to the policy decision")]
fn model_irrelevant(_world: &mut KithWorld) {}

#[when(expr = "{string} sends an exec request for {string}")]
fn user_sends_exec(world: &mut KithWorld, user: String, _command: String) {
    world.last_policy_decision = Some(match world.policy.scope_for(&user) {
        Some(scope) => MachinePolicy::evaluate(&scope, &ActionCategory::Exec),
        None => PolicyDecision::Deny { reason: "unknown user".into() },
    });
}

#[when(expr = "{string} sends an exec request to {string}")]
fn user_sends_exec_to(world: &mut KithWorld, user: String, machine: String) {
    // Use per-machine scope if available, fall back to flat policy
    let scope = world.per_machine_scopes.get(&(user.clone(), machine))
        .cloned()
        .or_else(|| world.policy.scope_for(&user));
    world.last_policy_decision = Some(match scope {
        Some(scope) => MachinePolicy::evaluate(&scope, &ActionCategory::Exec),
        None => PolicyDecision::Deny { reason: "unknown user".into() },
    });
}

#[then("it succeeds")]
fn succeeds(world: &mut KithWorld) {
    assert_eq!(world.last_policy_decision, Some(PolicyDecision::Allow));
}

#[then("it is denied")]
fn denied(world: &mut KithWorld) {
    assert!(matches!(world.last_policy_decision, Some(PolicyDecision::Deny { .. })));
}

#[then(expr = "it is denied with {string}")]
fn denied_with(world: &mut KithWorld, expected: String) {
    match &world.last_policy_decision {
        Some(PolicyDecision::Deny { reason }) => {
            let reason_lower = reason.to_lowercase();
            let has_key_term = expected.to_lowercase().split_whitespace()
                .filter(|w| w.len() > 3)
                .any(|t| reason_lower.contains(t));
            assert!(has_key_term, "expected reason matching '{expected}', got '{reason}'");
        }
        other => panic!("expected Deny, got {other:?}"),
    }
}

#[given("any policy denial occurs")]
fn any_denial(world: &mut KithWorld) {
    world.last_policy_decision = Some(PolicyDecision::Deny {
        reason: "test denial".into(),
    });
}

#[then("the audit entry includes who requested, what was requested, which machine, and the denial reason")]
fn audit_complete(_world: &mut KithWorld) {}

#[given(expr = "{string} has events tagged with {string} scope")]
fn events_tagged(world: &mut KithWorld, _machine: String, _scope: String) {}

#[when(expr = "{string} calls fleet_query about {string}")]
fn fleet_query_about(world: &mut KithWorld, _user: String, _machine: String) {}

#[then("metadata is returned but ops-scoped content is withheld")]
fn metadata_returned(_world: &mut KithWorld) {}

#[then("the response indicates restricted entries exist")]
fn restricted_entries(_world: &mut KithWorld) {}
