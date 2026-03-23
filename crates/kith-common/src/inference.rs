//! InferenceBackend trait — the abstraction that makes kith model-agnostic.
//! Defined here in kith-common; implementations live in kith-shell (INV-OPS-5).

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::error::InferenceError;

#[async_trait]
pub trait InferenceBackend: Send + Sync {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &InferenceConfig,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<StreamChunk, InferenceError>> + Send>>,
        InferenceError,
    >;

    async fn health_check(&self) -> Result<(), InferenceError>;

    fn name(&self) -> &str;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool { tool_call_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
    ToolCalls(Vec<ToolCall>),
    ToolResult {
        tool_call_id: String,
        output: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum StreamChunk {
    TextDelta(String),
    ThinkingDelta(String),
    ToolCall(ToolCall),
    Done { usage: Option<UsageStats> },
}

#[derive(Debug, Clone)]
pub struct UsageStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct InferenceConfig {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout: std::time::Duration,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            temperature: None,
            max_tokens: None,
            timeout: std::time::Duration::from_secs(120),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_serialization() {
        let msg = Message {
            role: Role::User,
            content: MessageContent::Text("hello".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed.role, Role::User));
        assert!(matches!(parsed.content, MessageContent::Text(s) if s == "hello"));
    }

    #[test]
    fn tool_call_serialization() {
        let tc = ToolCall {
            id: "call_1".into(),
            name: "remote".into(),
            arguments: serde_json::json!({"host": "staging-1", "command": "docker ps"}),
        };
        let json = serde_json::to_string(&tc).unwrap();
        let parsed: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "remote");
    }

    #[test]
    fn tool_definition_serialization() {
        let td = ToolDefinition {
            name: "remote".into(),
            description: "Execute on a remote machine".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "host": {"type": "string"},
                    "command": {"type": "string"}
                }
            }),
        };
        let json = serde_json::to_string(&td).unwrap();
        assert!(json.contains("remote"));
    }

    #[test]
    fn default_inference_config() {
        let c = InferenceConfig::default();
        assert_eq!(c.timeout, std::time::Duration::from_secs(120));
        assert!(c.temperature.is_none());
        assert!(c.max_tokens.is_none());
    }

    #[test]
    fn stream_chunk_variants() {
        let _ = StreamChunk::TextDelta("hello".into());
        let _ = StreamChunk::ThinkingDelta("reasoning...".into());
        let _ = StreamChunk::ToolCall(ToolCall {
            id: "1".into(),
            name: "remote".into(),
            arguments: serde_json::Value::Null,
        });
        let _ = StreamChunk::Done {
            usage: Some(UsageStats {
                input_tokens: 100,
                output_tokens: 50,
            }),
        };
    }
}
