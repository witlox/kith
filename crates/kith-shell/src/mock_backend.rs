//! Mock InferenceBackend for testing. Returns canned responses.

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::Stream;
use futures::stream;

use kith_common::error::InferenceError;
use kith_common::inference::*;

/// Mock backend that returns pre-configured responses.
pub struct MockInferenceBackend {
    name: String,
    responses: Arc<Mutex<Vec<MockResponse>>>,
    calls: Arc<Mutex<Vec<MockCall>>>,
    healthy: Arc<Mutex<bool>>,
}

/// A recorded call to the mock backend.
#[derive(Debug, Clone)]
pub struct MockCall {
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
}

/// A pre-configured response.
#[derive(Debug, Clone)]
pub enum MockResponse {
    Text(String),
    ToolCall(ToolCall),
    Error(String),
}

impl MockInferenceBackend {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            responses: Arc::new(Mutex::new(Vec::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
            healthy: Arc::new(Mutex::new(true)),
        }
    }

    /// Queue a response to be returned on the next call.
    pub fn queue_response(&self, response: MockResponse) {
        self.responses.lock().unwrap().push(response);
    }

    /// Queue a text response.
    pub fn queue_text(&self, text: impl Into<String>) {
        self.queue_response(MockResponse::Text(text.into()));
    }

    /// Queue a tool call response.
    pub fn queue_tool_call(&self, name: impl Into<String>, args: serde_json::Value) {
        self.queue_response(MockResponse::ToolCall(ToolCall {
            id: format!("call_{}", uuid::Uuid::new_v4()),
            name: name.into(),
            arguments: args,
        }));
    }

    /// Get recorded calls.
    pub fn calls(&self) -> Vec<MockCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Set health status.
    pub fn set_healthy(&self, healthy: bool) {
        *self.healthy.lock().unwrap() = healthy;
    }
}

#[async_trait]
impl InferenceBackend for MockInferenceBackend {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        _config: &InferenceConfig,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<StreamChunk, InferenceError>> + Send>>,
        InferenceError,
    > {
        if !*self.healthy.lock().unwrap() {
            return Err(InferenceError::Unreachable("mock unhealthy".into()));
        }

        self.calls.lock().unwrap().push(MockCall {
            messages: messages.to_vec(),
            tools: tools.to_vec(),
        });

        let response = self
            .responses
            .lock()
            .unwrap()
            .pop()
            .unwrap_or(MockResponse::Text("mock response".into()));

        let chunks: Vec<Result<StreamChunk, InferenceError>> = match response {
            MockResponse::Text(text) => vec![
                Ok(StreamChunk::TextDelta(text)),
                Ok(StreamChunk::Done { usage: None }),
            ],
            MockResponse::ToolCall(tc) => vec![
                Ok(StreamChunk::ToolCall(tc)),
                Ok(StreamChunk::Done { usage: None }),
            ],
            MockResponse::Error(msg) => vec![Err(InferenceError::BackendError(msg))],
        };

        Ok(Box::pin(stream::iter(chunks)))
    }

    async fn health_check(&self) -> Result<(), InferenceError> {
        if *self.healthy.lock().unwrap() {
            Ok(())
        } else {
            Err(InferenceError::Unreachable("mock unhealthy".into()))
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn mock_returns_queued_text() {
        let backend = MockInferenceBackend::new("test");
        backend.queue_text("hello world");

        let mut stream = backend
            .complete(&[], &[], &InferenceConfig::default())
            .await
            .unwrap();

        let chunk = stream.next().await.unwrap().unwrap();
        assert!(matches!(chunk, StreamChunk::TextDelta(t) if t == "hello world"));

        let done = stream.next().await.unwrap().unwrap();
        assert!(matches!(done, StreamChunk::Done { .. }));
    }

    #[tokio::test]
    async fn mock_returns_tool_call() {
        let backend = MockInferenceBackend::new("test");
        backend.queue_tool_call(
            "remote",
            serde_json::json!({"host": "staging-1", "command": "docker ps"}),
        );

        let mut stream = backend
            .complete(&[], &[], &InferenceConfig::default())
            .await
            .unwrap();

        let chunk = stream.next().await.unwrap().unwrap();
        assert!(matches!(chunk, StreamChunk::ToolCall(tc) if tc.name == "remote"));
    }

    #[tokio::test]
    async fn mock_records_calls() {
        let backend = MockInferenceBackend::new("test");
        backend.queue_text("ok");

        let msg = Message {
            role: Role::User,
            content: MessageContent::Text("hello".into()),
        };
        let _ = backend
            .complete(&[msg], &[], &InferenceConfig::default())
            .await
            .unwrap();

        let calls = backend.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].messages.len(), 1);
    }

    #[tokio::test]
    async fn mock_unhealthy_returns_error() {
        let backend = MockInferenceBackend::new("test");
        backend.set_healthy(false);

        let result = backend
            .complete(&[], &[], &InferenceConfig::default())
            .await;
        assert!(result.is_err());
        assert!(matches!(result, Err(InferenceError::Unreachable(_))));
    }

    #[tokio::test]
    async fn mock_health_check() {
        let backend = MockInferenceBackend::new("test");
        assert!(backend.health_check().await.is_ok());

        backend.set_healthy(false);
        assert!(backend.health_check().await.is_err());
    }

    #[tokio::test]
    async fn mock_name() {
        let backend = MockInferenceBackend::new("claude-mock");
        assert_eq!(backend.name(), "claude-mock");
    }

    #[tokio::test]
    async fn mock_default_response_when_queue_empty() {
        let backend = MockInferenceBackend::new("test");
        // No queued response — should return default "mock response"
        let mut stream = backend
            .complete(&[], &[], &InferenceConfig::default())
            .await
            .unwrap();

        let chunk = stream.next().await.unwrap().unwrap();
        assert!(matches!(chunk, StreamChunk::TextDelta(t) if t == "mock response"));
    }
}
