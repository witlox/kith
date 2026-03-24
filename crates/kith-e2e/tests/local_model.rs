//! E2e test with a real local LLM via Ollama in Docker.
//!
//! MANUAL ONLY — not in CI. Requires Docker + ~500MB model download.
//!
//! Run with:
//!   cargo test -p kith-e2e --features local-model --test local_model -- --nocapture
//!
//! First run pulls ollama image + qwen3.5:0.8b model (~500MB).
//! Subsequent runs use cached layers.

#![cfg(feature = "local-model")]

use std::time::Duration;

use futures::StreamExt;
use kith_common::inference::*;
use kith_shell::inference::OpenAiCompatBackend;
use kith_shell::tools::native_tools;
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt, core::WaitFor};

const MODEL: &str = "qwen3.5:0.8b";

fn ollama_image() -> GenericImage {
    GenericImage::new("ollama/ollama", "latest")
        .with_exposed_port(11434.into())
        .with_wait_for(WaitFor::message_on_stderr("Listening on"))
}

/// Pull the model inside the running Ollama container.
async fn pull_model(endpoint: &str) {
    eprintln!("pulling {MODEL} (may take a few minutes on first run)...");
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{endpoint}/api/pull"))
        .json(&serde_json::json!({"name": MODEL, "stream": false}))
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .expect("pull request should send");

    assert!(
        resp.status().is_success(),
        "model pull failed: {}",
        resp.status()
    );
    let _ = resp.text().await;
    eprintln!("{MODEL} ready");
}

/// Test: real model produces a text response via OpenAI-compatible API.
#[tokio::test]
async fn real_model_text_response() {
    let container = ollama_image().start().await.expect("ollama should start");

    let port = container.get_host_port_ipv4(11434).await.unwrap();
    let endpoint = format!("http://127.0.0.1:{port}");

    pull_model(&endpoint).await;

    // Use Ollama's OpenAI-compatible endpoint
    let backend = OpenAiCompatBackend::new(format!("{endpoint}/v1"), MODEL.into(), None);

    let messages = vec![Message {
        role: Role::User,
        content: MessageContent::Text(
            "What is 2+2? Reply with just the number, nothing else.".into(),
        ),
    }];

    let config = InferenceConfig {
        temperature: Some(0.0),
        max_tokens: Some(16),
        timeout: Duration::from_secs(60),
    };

    let result = backend.complete(&messages, &[], &config).await;

    match result {
        Ok(mut stream) => {
            let mut text = String::new();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(StreamChunk::TextDelta(t)) => text.push_str(&t),
                    Ok(StreamChunk::ThinkingDelta(_)) => {}
                    Ok(StreamChunk::Done { .. }) => break,
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("stream chunk error (non-fatal): {e}");
                        break;
                    }
                }
            }

            eprintln!("model response: {text:?}");
            assert!(!text.is_empty(), "should have text response");
            assert!(
                text.contains('4'),
                "response should contain '4', got: {text}"
            );
        }
        Err(e) => {
            panic!("complete failed: {e}");
        }
    }
}

/// Test: real model with tools available.
#[tokio::test]
async fn real_model_with_tools() {
    let container = ollama_image().start().await.expect("ollama should start");

    let port = container.get_host_port_ipv4(11434).await.unwrap();
    let endpoint = format!("http://127.0.0.1:{port}");

    pull_model(&endpoint).await;

    let backend = OpenAiCompatBackend::new(format!("{endpoint}/v1"), MODEL.into(), None);

    let messages = vec![
        Message {
            role: Role::System,
            content: MessageContent::Text(
                "You are a shell agent. Use the remote tool to execute commands on remote machines.".into(),
            ),
        },
        Message {
            role: Role::User,
            content: MessageContent::Text(
                "Run 'ls' on staging-1".into(),
            ),
        },
    ];

    let tools = native_tools();
    let config = InferenceConfig {
        temperature: Some(0.0),
        max_tokens: Some(128),
        timeout: Duration::from_secs(60),
    };

    let result = backend.complete(&messages, &tools, &config).await;

    match result {
        Ok(mut stream) => {
            let mut text = String::new();
            let mut tool_calls = Vec::new();

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(StreamChunk::TextDelta(t)) => text.push_str(&t),
                    Ok(StreamChunk::ToolCall(tc)) => tool_calls.push(tc),
                    Ok(StreamChunk::ThinkingDelta(_)) => {}
                    Ok(StreamChunk::Done { .. }) => break,
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("stream chunk error (non-fatal): {e}");
                        break;
                    }
                }
            }

            let has_output = !text.is_empty() || !tool_calls.is_empty();
            assert!(has_output, "model should produce text or tool calls");

            eprintln!("text: {text:?}");
            eprintln!("tool_calls: {tool_calls:?}");

            // With a 0.8B model, tool calls may or may not work reliably.
            // The test validates the InferenceBackend works end-to-end,
            // not that the model makes perfect decisions.
            if !tool_calls.is_empty() {
                eprintln!("model produced tool call: {}", tool_calls[0].name);
                assert!(!tool_calls[0].name.is_empty());
            } else {
                eprintln!("model produced text response (no tool call — expected for tiny model)");
            }
        }
        Err(e) => {
            panic!("complete failed: {e}");
        }
    }
}
