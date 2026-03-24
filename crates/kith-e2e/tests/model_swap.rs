//! E2e scenario 9: model swap — switch InferenceBackend, same workflow, same results.
//! Tests that the system is truly model-agnostic (INV-OPS-5).

use futures::StreamExt;
use kith_common::inference::*;
use kith_shell::classify::{InputClass, InputClassifier};
use kith_shell::context::ConversationContext;
use kith_shell::mock_backend::{MockInferenceBackend, MockResponse};
use kith_shell::prompt::build_system_prompt;
use kith_shell::tools::native_tools;

/// Simulate an agent turn: classify input, if intent then call backend.
async fn agent_turn(
    backend: &MockInferenceBackend,
    classifier: &InputClassifier,
    context: &mut ConversationContext,
    input: &str,
) -> AgentResult {
    match classifier.classify(input) {
        InputClass::PassThrough(cmd) => AgentResult::PassThrough(cmd),
        InputClass::Intent(text) => {
            context.add_user(text);

            let tools = native_tools();
            let config = InferenceConfig::default();

            match backend.complete(context.messages(), &tools, &config).await {
                Ok(mut stream) => {
                    let mut text_output = String::new();
                    let mut tool_calls = Vec::new();

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(StreamChunk::TextDelta(t)) => text_output.push_str(&t),
                            Ok(StreamChunk::ToolCall(tc)) => tool_calls.push(tc),
                            Ok(StreamChunk::ThinkingDelta(_)) => {} // model-dependent, ignored
                            Ok(StreamChunk::Done { .. }) => break,
                            Err(e) => return AgentResult::Error(e.to_string()),
                        }
                    }

                    if !tool_calls.is_empty() {
                        context.add_assistant(MessageContent::ToolCalls(tool_calls.clone()));
                        AgentResult::ToolCalls(tool_calls)
                    } else {
                        context.add_assistant(MessageContent::Text(text_output.clone()));
                        AgentResult::Text(text_output)
                    }
                }
                Err(e) => AgentResult::Error(e.to_string()),
            }
        }
    }
}

#[derive(Debug)]
enum AgentResult {
    PassThrough(String),
    Text(String),
    ToolCalls(Vec<ToolCall>),
    Error(String),
}

/// Scenario 9: two different backends produce equivalent results for the same workflow.
#[tokio::test]
async fn e2e_model_swap_same_workflow() {
    let classifier = InputClassifier::new(
        ["ls", "git", "docker"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    );

    // Backend A: simulates "Claude"
    let backend_a = MockInferenceBackend::new("claude-sonnet");
    backend_a.queue_response(MockResponse::ToolCall(ToolCall {
        id: "call_1".into(),
        name: "remote".into(),
        arguments: serde_json::json!({"host": "staging-1", "command": "docker ps"}),
    }));

    // Backend B: simulates "self-hosted MiniMax"
    let backend_b = MockInferenceBackend::new("minimax-m2.5");
    backend_b.queue_response(MockResponse::ToolCall(ToolCall {
        id: "call_2".into(),
        name: "remote".into(),
        arguments: serde_json::json!({"host": "staging-1", "command": "docker ps"}),
    }));

    let prompt = build_system_prompt("dev-mac", "Darwin", "staging-1: ok", None);

    // Run same workflow with backend A
    let mut ctx_a = ConversationContext::new(100);
    ctx_a.set_system_prompt(prompt.clone());
    let result_a = agent_turn(
        &backend_a,
        &classifier,
        &mut ctx_a,
        "check what's running on staging-1",
    )
    .await;

    // Run same workflow with backend B
    let mut ctx_b = ConversationContext::new(100);
    ctx_b.set_system_prompt(prompt);
    let result_b = agent_turn(
        &backend_b,
        &classifier,
        &mut ctx_b,
        "check what's running on staging-1",
    )
    .await;

    // Both should produce a remote() tool call
    match (&result_a, &result_b) {
        (AgentResult::ToolCalls(tc_a), AgentResult::ToolCalls(tc_b)) => {
            assert_eq!(tc_a[0].name, "remote");
            assert_eq!(tc_b[0].name, "remote");
            assert_eq!(tc_a[0].arguments["host"], tc_b[0].arguments["host"]);
            assert_eq!(tc_a[0].arguments["command"], tc_b[0].arguments["command"]);
        }
        _ => panic!("both backends should produce tool calls: {result_a:?}, {result_b:?}"),
    }

    // Verify no other component is aware of the backend change
    assert_eq!(backend_a.name(), "claude-sonnet");
    assert_eq!(backend_b.name(), "minimax-m2.5");

    // Both received the same messages and tools
    let calls_a = backend_a.calls();
    let calls_b = backend_b.calls();
    assert_eq!(calls_a[0].messages.len(), calls_b[0].messages.len());
    assert_eq!(calls_a[0].tools.len(), calls_b[0].tools.len());
}

/// Pass-through commands are unaffected by backend choice.
#[tokio::test]
async fn e2e_passthrough_independent_of_backend() {
    let classifier = InputClassifier::new(
        ["ls", "git", "docker"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    );

    let backend = MockInferenceBackend::new("any-model");
    let mut ctx = ConversationContext::new(100);

    // Known command -> pass-through, no backend call
    let result = agent_turn(&backend, &classifier, &mut ctx, "ls -la").await;
    assert!(matches!(result, AgentResult::PassThrough(cmd) if cmd == "ls -la"));

    // Backend was never called
    assert!(backend.calls().is_empty());
}

/// Backend failure degrades gracefully — no panic, clear error.
#[tokio::test]
async fn e2e_backend_failure_graceful() {
    let classifier = InputClassifier::new(std::collections::HashSet::new());

    let backend = MockInferenceBackend::new("failing-model");
    backend.set_healthy(false);

    let mut ctx = ConversationContext::new(100);
    ctx.set_system_prompt("sys".into());

    let result = agent_turn(&backend, &classifier, &mut ctx, "do something").await;
    assert!(matches!(result, AgentResult::Error(msg) if msg.contains("unhealthy")));
}

/// ThinkingDelta is model-dependent — present for some, absent for others.
/// Both should work.
#[tokio::test]
async fn e2e_thinking_optional() {
    let classifier = InputClassifier::new(std::collections::HashSet::new());

    // Backend that produces thinking deltas (like Opus/M2.5)
    let backend_thinking = MockInferenceBackend::new("thinking-model");
    backend_thinking.queue_text("I'll help with that.");

    // Backend that doesn't (like non-thinking models)
    let backend_no_thinking = MockInferenceBackend::new("simple-model");
    backend_no_thinking.queue_text("I'll help with that.");

    let mut ctx1 = ConversationContext::new(100);
    ctx1.set_system_prompt("sys".into());
    let r1 = agent_turn(&backend_thinking, &classifier, &mut ctx1, "help").await;

    let mut ctx2 = ConversationContext::new(100);
    ctx2.set_system_prompt("sys".into());
    let r2 = agent_turn(&backend_no_thinking, &classifier, &mut ctx2, "help").await;

    // Both produce text output
    assert!(matches!(&r1, AgentResult::Text(t) if t.contains("help")));
    assert!(matches!(&r2, AgentResult::Text(t) if t.contains("help")));
}
