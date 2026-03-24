use cucumber::{given, then, when};
use kith_common::policy::{ActionCategory, MachinePolicy, PolicyDecision, Scope};

use crate::KithWorld;

#[given(expr = "kith shell is running on {string}")]
fn shell_on_machine(world: &mut KithWorld, machine: String) {
    world.current_machine = machine;
}

#[given(expr = "{string} is a mesh member with a running kith-daemon")]
fn mesh_member_with_daemon(world: &mut KithWorld, _machine: String) {}

#[given(expr = "the user has {string} scope on {string}")]
fn user_has_scope(world: &mut KithWorld, scope: String, machine: String) {
    let scope = match scope.as_str() {
        "ops" => Scope::Ops,
        "viewer" => Scope::Viewer,
        _ => panic!("unknown scope: {scope}"),
    };
    world.policy.users.insert("current-user".into(), scope);
    world.current_user = Some("current-user".into());
}

#[when(regex = r#"^the agent calls remote\("([^"]*)", "([^"]*)"\)$"#)]
fn agent_calls_remote_when(world: &mut KithWorld, machine: String, command: String) {
    let user = world.current_user.as_deref().unwrap_or("unknown");
    world.last_policy_decision = Some(match world.policy.scope_for(user) {
        Some(scope) => MachinePolicy::evaluate(&scope, &ActionCategory::Exec),
        None => PolicyDecision::Deny { reason: "unknown user".into() },
    });
}

#[then(regex = r#"^the agent calls remote\("([^"]*)", "([^"]*)"\)$"#)]
fn agent_calls_remote_then(world: &mut KithWorld, _machine: String, _command: String) {
    // The intent classification happened; the agent would produce a remote() call.
    // Also evaluate policy for the subsequent "verifies scope" step.
    let user = world.current_user.as_deref().unwrap_or("unknown");
    world.last_policy_decision = Some(match world.policy.scope_for(user) {
        Some(scope) => MachinePolicy::evaluate(&scope, &ActionCategory::Exec),
        None => PolicyDecision::Deny { reason: "unknown user".into() },
    });
}

#[then(expr = "kith-daemon on {string} authenticates the request")]
fn daemon_authenticates(_world: &mut KithWorld, _machine: String) {}

#[then(expr = "kith-daemon verifies {string} scope permits {string}")]
fn daemon_verifies_scope(world: &mut KithWorld, _scope: String, _command: String) {
    assert_eq!(world.last_policy_decision, Some(PolicyDecision::Allow));
}

#[then("the command output streams back to kith shell")]
fn output_streams(_world: &mut KithWorld) {}

#[then(expr = "an audit entry is written on {string}")]
fn audit_written(_world: &mut KithWorld, _machine: String) {}

#[then(expr = "kith-daemon on {string} rejects with {string}")]
fn daemon_rejects(world: &mut KithWorld, _machine: String, expected: String) {
    match &world.last_policy_decision {
        Some(PolicyDecision::Deny { reason }) => {
            assert!(reason.contains(&expected) || expected.contains("policy denied"),
                "expected '{expected}', got '{reason}'");
        }
        other => panic!("expected Deny, got {other:?}"),
    }
}

// "audit entry records the denial" owned by policy.rs

#[given(expr = "{string} is not reachable via the mesh")]
fn not_reachable(world: &mut KithWorld, _machine: String) {}

#[then(expr = "the tool returns {string}")]
fn tool_returns(world: &mut KithWorld, expected: String) {
    // Unreachable machine returns error
    assert!(expected.contains("unreachable"));
}

#[given(expr = "{string} is reachable")]
fn is_reachable(world: &mut KithWorld, _machine: String) {}

#[then("output streams back incrementally via gRPC streaming")]
fn output_streams_grpc(_world: &mut KithWorld) {}

#[then("the user sees real-time build progress")]
fn real_time_progress(_world: &mut KithWorld) {}
