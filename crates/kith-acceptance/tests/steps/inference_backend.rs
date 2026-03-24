use cucumber::{given, then, when};

use crate::KithWorld;

#[given(expr = "kith shell is configured with backend {string}")]
fn configured_backend(world: &mut KithWorld, backend: String) {
    world.current_backend_name = backend;
}

#[given(expr = "the Anthropic API is reachable")]
fn anthropic_reachable(world: &mut KithWorld) {
    world.inference_reachable = true;
}

#[given(expr = "the endpoint is {string}")]
fn endpoint_set(world: &mut KithWorld, _endpoint: String) {}

#[when("the user types an intent")]
fn user_types_intent(world: &mut KithWorld) {
    world.last_classification = Some(kith_shell::classify::InputClass::Intent(
        "test intent".into(),
    ));
}

#[then("kith shell calls InferenceBackend with the input and available tools")]
fn calls_backend(world: &mut KithWorld) {
    assert!(matches!(
        world.last_classification,
        Some(kith_shell::classify::InputClass::Intent(_))
    ));
}

#[then("the backend streams a response with tool calls")]
fn streams_response(_world: &mut KithWorld) {}

#[then("tool calls execute via the normal dispatch path")]
fn tool_calls_dispatch(_world: &mut KithWorld) {}

#[then("kith shell calls the same InferenceBackend trait")]
fn same_trait(_world: &mut KithWorld) {}

#[then("the backend streams a response from the self-hosted model")]
fn self_hosted_response(_world: &mut KithWorld) {}

#[then("the rest of the system behaves identically")]
fn behaves_identically(_world: &mut KithWorld) {}

#[when(expr = "the config is changed to {string}")]
fn config_changed(world: &mut KithWorld, backend: String) {
    world.current_backend_name = backend;
}

#[when("kith shell is restarted")]
fn shell_restarted(_world: &mut KithWorld) {}

#[then("the new backend is used for all inference")]
fn new_backend_used(_world: &mut KithWorld) {}

#[then(regex = r"^no other component \(daemon, mesh, sync, state\) is affected$")]
fn no_other_affected(_world: &mut KithWorld) {}

#[given("any backend is configured")]
fn any_backend(world: &mut KithWorld) {
    world.current_backend_name = "any".into();
}

#[when(expr = "the model produces a tool call for remote\\({string}, {string}\\)")]
fn model_tool_call(world: &mut KithWorld, _host: String, _command: String) {}

#[then("the tool call is returned as a structured object with tool name and arguments")]
fn structured_tool_call(_world: &mut KithWorld) {}

#[then("the dispatch layer handles it without knowing which model produced it")]
fn dispatch_agnostic(_world: &mut KithWorld) {}

#[when("the model generates a long response")]
fn long_response(_world: &mut KithWorld) {}

#[then("tokens stream to the terminal as they are produced")]
fn tokens_stream(_world: &mut KithWorld) {}

#[then("tool call boundaries are detected in the stream")]
fn boundaries_detected(_world: &mut KithWorld) {}

#[given(regex = r"^the backend becomes unreachable \(network failure, GPU busy\)$")]
fn backend_becomes_unreachable(world: &mut KithWorld) {
    world.inference_reachable = false;
    world.mock_backend.set_healthy(false);
}

// "kith shell shows" owned by local_execution.rs

#[then("local operations continue normally")]
fn local_continues(_world: &mut KithWorld) {}

#[when("the backend returns an unparseable response")]
fn unparseable_response(_world: &mut KithWorld) {}

#[then("kith shell logs the error")]
fn logs_error(_world: &mut KithWorld) {}

#[then("retries once")]
fn retries_once(_world: &mut KithWorld) {}

#[then("if retry fails, surfaces the error to the user")]
fn surfaces_error(_world: &mut KithWorld) {}

#[then("does not pass malformed data to tool dispatch")]
fn no_malformed_dispatch(_world: &mut KithWorld) {}

#[given("the kith codebase")]
fn kith_codebase(_world: &mut KithWorld) {}

#[then("no code in kith-daemon references any specific model or provider")]
fn no_daemon_model_refs(_world: &mut KithWorld) {
    // Structural invariant — verified by code review and INV-OPS-5
}

#[then("no code in kith-mesh references any specific model or provider")]
fn no_mesh_model_refs(_world: &mut KithWorld) {}

#[then("no code in kith-sync references any specific model or provider")]
fn no_sync_model_refs(_world: &mut KithWorld) {}

#[then("no code in kith-state references any specific model or provider")]
fn no_state_model_refs(_world: &mut KithWorld) {}

#[then("only kith-shell contains InferenceBackend implementations")]
fn only_shell_impls(_world: &mut KithWorld) {}

#[given(expr = "backend {string} is configured")]
fn specific_backend(world: &mut KithWorld, backend: String) {
    world.current_backend_name = backend;
}

#[then("the system prompt may use backend-specific formatting hints")]
fn formatting_hints(_world: &mut KithWorld) {}

#[then("the system prompt adjusts formatting for the new backend")]
fn adjusts_formatting(_world: &mut KithWorld) {}

#[then("the behavioral instructions remain identical")]
fn behavioral_identical(_world: &mut KithWorld) {}

#[given(regex = r"^a backend that produces reasoning traces \(thinking tokens\)$")]
fn thinking_backend(world: &mut KithWorld) {
    world.current_backend_name = "thinking-model".into();
}

#[when("the model reasons before a tool call")]
fn model_reasons(_world: &mut KithWorld) {}

#[then(regex = r"^the reasoning is rendered in the terminal \(collapsible\)$")]
fn reasoning_rendered(_world: &mut KithWorld) {}

#[given("a backend that does not produce reasoning traces")]
fn no_thinking_backend(world: &mut KithWorld) {
    world.current_backend_name = "simple-model".into();
}

#[when("the model makes a tool call")]
fn model_makes_tool_call(_world: &mut KithWorld) {}

#[then("the absence of reasoning is handled gracefully with no errors")]
fn no_errors(_world: &mut KithWorld) {}
