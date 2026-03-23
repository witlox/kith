//! Step definitions for policy-enforcement.feature

use cucumber::{given, then, when};
use kith_common::policy::{ActionCategory, MachinePolicy, PolicyDecision, Scope};

use crate::KithWorld;

#[given(expr = "user {string} has {string} scope on {string}")]
fn set_user_scope(world: &mut KithWorld, user: String, scope: String, _machine: String) {
    let scope = match scope.as_str() {
        "ops" => Scope::Ops,
        "viewer" => Scope::Viewer,
        _ => panic!("unknown scope: {scope}"),
    };
    world.policy.users.insert(user, scope);
}

#[when(expr = "the agent sends an exec request for {string} as {string}")]
fn send_exec_request(world: &mut KithWorld, _command: String, user: String) {
    match world.policy.scope_for(&user) {
        Some(scope) => {
            world.last_policy_decision =
                Some(MachinePolicy::evaluate(&scope, &ActionCategory::Exec));
        }
        None => {
            world.last_policy_decision = Some(PolicyDecision::Deny {
                reason: "unknown user".into(),
            });
        }
    }
}

#[when(expr = "the agent sends a query request as {string}")]
fn send_query_request(world: &mut KithWorld, user: String) {
    match world.policy.scope_for(&user) {
        Some(scope) => {
            world.last_policy_decision =
                Some(MachinePolicy::evaluate(&scope, &ActionCategory::Query));
        }
        None => {
            world.last_policy_decision = Some(PolicyDecision::Deny {
                reason: "unknown user".into(),
            });
        }
    }
}

#[then("kith-daemon allows the execution")]
fn assert_allowed(world: &mut KithWorld) {
    assert_eq!(
        world.last_policy_decision,
        Some(PolicyDecision::Allow),
        "expected Allow, got {:?}",
        world.last_policy_decision
    );
}

#[then(expr = "kith-daemon rejects with {string}")]
fn assert_rejected_with(world: &mut KithWorld, expected_reason: String) {
    match &world.last_policy_decision {
        Some(PolicyDecision::Deny { reason }) => {
            assert!(
                reason.contains(&expected_reason),
                "expected reason containing '{expected_reason}', got '{reason}'"
            );
        }
        other => panic!("expected Deny, got {other:?}"),
    }
}

#[then("kith-daemon returns the machine state")]
fn assert_query_allowed(world: &mut KithWorld) {
    assert_eq!(world.last_policy_decision, Some(PolicyDecision::Allow));
}

#[then("it succeeds")]
fn assert_succeeds(world: &mut KithWorld) {
    assert_eq!(world.last_policy_decision, Some(PolicyDecision::Allow));
}

#[then("it is denied")]
fn assert_denied(world: &mut KithWorld) {
    assert!(
        matches!(world.last_policy_decision, Some(PolicyDecision::Deny { .. })),
        "expected Deny, got {:?}",
        world.last_policy_decision
    );
}
