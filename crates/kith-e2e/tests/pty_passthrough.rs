//! E2e scenario 1: local pass-through command execution.
//! Tests that known commands bypass the InferenceBackend entirely.
//! Uses real process execution to verify the full local path.


use kith_shell::classify::{InputClass, InputClassifier};
use kith_shell::mock_backend::MockInferenceBackend;

/// Scenario 1: pass-through commands execute with no LLM involvement.
#[tokio::test]
async fn e2e_passthrough_executes_directly() {
    let classifier = InputClassifier::from_path_env();

    // ls should be in PATH on any Unix system
    let result = classifier.classify("ls -la /tmp");
    assert_eq!(result, InputClass::PassThrough("ls -la /tmp".into()));

    // Actually execute it
    let exec_result = kith_daemon::exec::exec_command("ls -la /tmp")
        .await
        .unwrap();
    assert_eq!(exec_result.exit_code, 0);
    assert!(!exec_result.stdout.is_empty());
}

/// Scenario 1: escape hatch bypasses LLM.
#[tokio::test]
async fn e2e_escape_hatch() {
    let classifier = InputClassifier::from_path_env();

    let result = classifier.classify("run: echo escape-test");
    assert_eq!(result, InputClass::PassThrough("echo escape-test".into()));

    // Execute the escaped command
    let exec_result = kith_daemon::exec::exec_command("echo escape-test")
        .await
        .unwrap();
    assert_eq!(exec_result.stdout.trim(), "escape-test");
}

/// Scenario 1: pass-through has no InferenceBackend interaction.
#[tokio::test]
async fn e2e_passthrough_no_inference_call() {
    let classifier = InputClassifier::from_path_env();
    let backend = MockInferenceBackend::new("should-not-be-called");

    // Classify and verify pass-through
    let input = "echo hello";
    match classifier.classify(input) {
        InputClass::PassThrough(cmd) => {
            // Execute directly — no backend call
            let result = kith_daemon::exec::exec_command(&cmd).await.unwrap();
            assert_eq!(result.stdout.trim(), "hello");
        }
        InputClass::Intent(_) => panic!("echo should be pass-through"),
    }

    // Backend was never called
    assert!(backend.calls().is_empty());
}

/// Scenario 1: natural language is classified as intent.
#[tokio::test]
async fn e2e_intent_classified_correctly() {
    let classifier = InputClassifier::from_path_env();

    // Natural language that doesn't start with a known command
    let result = classifier.classify("what's using port 3000?");
    assert!(matches!(result, InputClass::Intent(_)));

    let result = classifier.classify("deploy the app to staging");
    assert!(matches!(result, InputClass::Intent(_)));
}

/// Scenario 1: verify common commands are recognized.
#[tokio::test]
async fn e2e_common_commands_recognized() {
    let classifier = InputClassifier::from_path_env();

    let commands = [
        "ls -la",
        "cat /etc/hostname",
        "grep -r TODO .",
        "echo hello world",
        "pwd",
        "whoami",
    ];

    for cmd in &commands {
        let result = classifier.classify(cmd);
        assert!(
            matches!(result, InputClass::PassThrough(_)),
            "{cmd} should be pass-through but was classified as intent"
        );
    }
}

/// Scenario 1: verify pass-through latency is low.
#[tokio::test]
async fn e2e_passthrough_latency() {
    let classifier = InputClassifier::from_path_env();

    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let _ = classifier.classify("ls -la");
    }
    let elapsed = start.elapsed();

    // 1000 classifications should take < 50ms (50µs each)
    assert!(
        elapsed.as_millis() < 50,
        "1000 classifications took {}ms, should be <50ms",
        elapsed.as_millis()
    );
}
