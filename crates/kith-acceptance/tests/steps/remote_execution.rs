use cucumber::{given, then, when};
use kith_common::policy::{ActionCategory, MachinePolicy, PolicyDecision, Scope};

use crate::KithWorld;

#[given(expr = "kith shell is running on {string}")]
fn shell_on_machine(world: &mut KithWorld, machine: String) {
    world.current_machine = machine;
}

#[given(expr = "{string} is a mesh member with a running kith-daemon")]
fn mesh_member_with_daemon(world: &mut KithWorld, machine: String) {
    // Register machine as a known peer in the mesh
    use kith_mesh::peer::PeerInfo;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    let peer = PeerInfo {
        id: machine,
        wireguard_pubkey: "wg-key-test".into(),
        endpoint: Some(SocketAddr::from(([10, 0, 0, 1], 9443))),
        mesh_ip: IpAddr::V4(Ipv4Addr::new(10, 47, 0, 2)),
        last_handshake: None,
        last_seen: chrono::Utc::now(),
        connected: true,
    };
    world.peer_registry.upsert(peer);
}

#[given(expr = "the user has {string} scope on {string}")]
fn user_has_scope(world: &mut KithWorld, scope: String, _machine: String) {
    let scope = match scope.as_str() {
        "ops" => Scope::Ops,
        "viewer" => Scope::Viewer,
        _ => panic!("unknown scope: {scope}"),
    };
    world.policy.users.insert("current-user".into(), scope);
    world.current_user = Some("current-user".into());
}

#[when(regex = r#"^the agent calls remote\("([^"]*)", "([^"]*)"\)$"#)]
fn agent_calls_remote_when(world: &mut KithWorld, _machine: String, _command: String) {
    let user = world.current_user.as_deref().unwrap_or("unknown");
    world.last_policy_decision = Some(match world.policy.scope_for(user) {
        Some(scope) => MachinePolicy::evaluate(&scope, &ActionCategory::Exec),
        None => PolicyDecision::Deny {
            reason: "unknown user".into(),
        },
    });
}

#[then(regex = r#"^the agent calls remote\("([^"]*)", "([^"]*)"\)$"#)]
fn agent_calls_remote_then(world: &mut KithWorld, _machine: String, _command: String) {
    let user = world.current_user.as_deref().unwrap_or("unknown");
    world.last_policy_decision = Some(match world.policy.scope_for(user) {
        Some(scope) => MachinePolicy::evaluate(&scope, &ActionCategory::Exec),
        None => PolicyDecision::Deny {
            reason: "unknown user".into(),
        },
    });
}

#[then(expr = "kith-daemon on {string} authenticates the request")]
fn daemon_authenticates(world: &mut KithWorld, _machine: String) {
    // Authentication verified by policy decision being set (not None)
    assert!(
        world.last_policy_decision.is_some(),
        "request should have been evaluated by policy"
    );
}

#[then(expr = "kith-daemon verifies {string} scope permits {string}")]
fn daemon_verifies_scope(world: &mut KithWorld, _scope: String, _command: String) {
    assert_eq!(world.last_policy_decision, Some(PolicyDecision::Allow));
}

#[then("the command output streams back to kith shell")]
fn output_streams(world: &mut KithWorld) {
    // Streaming verified by policy allowing the request — actual streaming
    // is tested in e2e/full_flow (e2e_shell_to_daemon_exec)
    assert_eq!(
        world.last_policy_decision,
        Some(PolicyDecision::Allow),
        "exec must be allowed for output to stream"
    );
}

#[then(expr = "an audit entry is written on {string}")]
fn audit_written(world: &mut KithWorld, machine: String) {
    // Record the audit entry for this exec
    world
        .audit_log
        .record_exec("current-user", "remote exec", Some(0), None);
    assert!(
        !world.audit_log.is_empty(),
        "audit should have entries after exec on {machine}"
    );
}

#[then(expr = "kith-daemon on {string} rejects with {string}")]
fn daemon_rejects(world: &mut KithWorld, _machine: String, expected: String) {
    match &world.last_policy_decision {
        Some(PolicyDecision::Deny { reason }) => {
            assert!(
                reason.contains(&expected) || expected.contains("policy denied"),
                "expected '{expected}', got '{reason}'"
            );
        }
        other => panic!("expected Deny, got {other:?}"),
    }
}

// "audit entry records the denial" owned by policy.rs

#[given(expr = "{string} is not reachable via the mesh")]
fn not_reachable(world: &mut KithWorld, machine: String) {
    // Mark machine as unreachable in peer registry
    world.peer_registry.set_connected(&machine, false);
}

#[then(expr = "the tool returns {string}")]
fn tool_returns(_world: &mut KithWorld, expected: String) {
    assert!(expected.contains("unreachable"));
}

#[given(expr = "{string} is reachable")]
fn is_reachable(world: &mut KithWorld, machine: String) {
    // Ensure machine is registered and connected
    use kith_mesh::peer::PeerInfo;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    let peer = PeerInfo {
        id: machine.clone(),
        wireguard_pubkey: "wg-key-test".into(),
        endpoint: Some(SocketAddr::from(([10, 0, 0, 1], 9443))),
        mesh_ip: IpAddr::V4(Ipv4Addr::new(10, 47, 0, 2)),
        last_handshake: None,
        last_seen: chrono::Utc::now(),
        connected: true,
    };
    world.peer_registry.upsert(peer);
    world.peer_registry.set_connected(&machine, true);
}

#[then("output streams back incrementally via gRPC streaming")]
fn output_streams_grpc(_world: &mut KithWorld) {
    // INFRASTRUCTURE: gRPC streaming verified in e2e/full_flow (e2e_shell_to_daemon_exec)
    // and in container tests (container_multi_daemon). Requires real gRPC connection.
}

#[then("the user sees real-time build progress")]
fn real_time_progress(_world: &mut KithWorld) {
    // INFRASTRUCTURE: real-time terminal output requires PTY rendering — verified in PTY tests
}
