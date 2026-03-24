//! OpenAI-compatible InferenceBackend implementation.
//! Covers: vLLM, SGLang, Ollama, LM Studio, OpenAI, OpenRouter, Together, Groq,
//! and any endpoint speaking the OpenAI Chat Completions API.

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use kith_common::error::InferenceError;
use kith_common::inference::*;

pub struct OpenAiCompatBackend {
    endpoint: String,
    model: String,
    api_key: Option<String>,
    client: Client,
}

impl OpenAiCompatBackend {
    pub fn new(endpoint: String, model: String, api_key: Option<String>) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            model,
            api_key,
            client: Client::new(),
        }
    }

    /// Build from config, reading API key from env var if specified.
    pub fn from_config(
        endpoint: &str,
        model: &str,
        api_key_env: Option<&str>,
    ) -> Self {
        let api_key = api_key_env.and_then(|var| std::env::var(var).ok());
        Self::new(endpoint.into(), model.into(), api_key)
    }

    fn build_request(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &InferenceConfig,
    ) -> OaiRequest {
        let oai_messages: Vec<OaiMessage> = messages.iter().map(|m| m.into()).collect();

        let oai_tools: Option<Vec<OaiTool>> = if tools.is_empty() {
            None
        } else {
            Some(tools.iter().map(|t| OaiTool {
                r#type: "function".into(),
                function: OaiFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            }).collect())
        };

        OaiRequest {
            model: self.model.clone(),
            messages: oai_messages,
            tools: oai_tools,
            stream: true,
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        }
    }
}

#[async_trait]
impl InferenceBackend for OpenAiCompatBackend {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &InferenceConfig,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, InferenceError>> + Send>>, InferenceError> {
        let url = format!("{}/chat/completions", self.endpoint);
        let body = self.build_request(messages, tools, config);

        let mut req = self.client.post(&url)
            .json(&body)
            .timeout(config.timeout);

        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await.map_err(|e| {
            if e.is_timeout() {
                InferenceError::Timeout(config.timeout)
            } else if e.is_connect() {
                InferenceError::Unreachable(e.to_string())
            } else {
                InferenceError::BackendError(e.to_string())
            }
        })?;

        let status = response.status();
        if status == 429 {
            let retry_after = response.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(1000);
            return Err(InferenceError::RateLimited { retry_after_ms: retry_after });
        }
        if status == 401 || status == 403 {
            return Err(InferenceError::AuthFailed(format!("HTTP {status}")));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(InferenceError::BackendError(format!("HTTP {status}: {body}")));
        }

        let byte_stream = response.bytes_stream();
        let stream = parse_openai_sse_stream(byte_stream);
        Ok(Box::pin(stream))
    }

    async fn health_check(&self) -> Result<(), InferenceError> {
        let url = format!("{}/models", self.endpoint);
        let mut req = self.client.get(&url).timeout(std::time::Duration::from_secs(5));
        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }
        req.send().await.map_err(|e| InferenceError::Unreachable(e.to_string()))?;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.model
    }
}

/// Parse an SSE byte stream into StreamChunks.
fn parse_openai_sse_stream(
    byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<StreamChunk, InferenceError>> + Send {
    use futures::StreamExt;

    let mut buffer = String::new();
    let mut pending_tool_calls: std::collections::HashMap<usize, PartialToolCall> = std::collections::HashMap::new();

    futures::stream::unfold(
        (Box::pin(byte_stream), buffer, pending_tool_calls),
        |(mut stream, mut buf, mut tool_calls)| async move {
            loop {
                // Try to extract a complete SSE event from buffer
                if let Some(pos) = buf.find("\n\n") {
                    let event = buf[..pos].to_string();
                    buf = buf[pos + 2..].to_string();

                    if let Some(data) = event.strip_prefix("data: ") {
                        let data = data.trim();
                        if data == "[DONE]" {
                            // Emit any remaining tool calls, then done
                            let remaining: Vec<_> = tool_calls.drain().collect();
                            for (_, tc) in remaining {
                                if !tc.name.is_empty() {
                                    return Some((
                                        Ok(StreamChunk::ToolCall(ToolCall {
                                            id: tc.id,
                                            name: tc.name,
                                            arguments: serde_json::from_str(&tc.arguments)
                                                .unwrap_or(serde_json::Value::Null),
                                        })),
                                        (stream, buf, tool_calls),
                                    ));
                                }
                            }
                            return Some((
                                Ok(StreamChunk::Done { usage: None }),
                                (stream, buf, tool_calls),
                            ));
                        }

                        if let Ok(chunk) = serde_json::from_str::<OaiStreamChunk>(data) {
                            if let Some(choice) = chunk.choices.first() {
                                let delta = &choice.delta;

                                // Text content
                                if let Some(ref content) = delta.content {
                                    if !content.is_empty() {
                                        return Some((
                                            Ok(StreamChunk::TextDelta(content.clone())),
                                            (stream, buf, tool_calls),
                                        ));
                                    }
                                }

                                // Reasoning/thinking (some models)
                                if let Some(ref reasoning) = delta.reasoning_content {
                                    if !reasoning.is_empty() {
                                        return Some((
                                            Ok(StreamChunk::ThinkingDelta(reasoning.clone())),
                                            (stream, buf, tool_calls),
                                        ));
                                    }
                                }

                                // Tool calls (accumulated across chunks)
                                if let Some(ref tcs) = delta.tool_calls {
                                    for tc in tcs {
                                        let entry = tool_calls.entry(tc.index).or_insert_with(|| {
                                            PartialToolCall {
                                                id: tc.id.clone().unwrap_or_default(),
                                                name: String::new(),
                                                arguments: String::new(),
                                            }
                                        });
                                        if let Some(ref f) = tc.function {
                                            if let Some(ref name) = f.name {
                                                entry.name.clone_from(name);
                                            }
                                            if let Some(ref args) = f.arguments {
                                                entry.arguments.push_str(args);
                                            }
                                        }
                                    }
                                }

                                // Finish reason
                                if choice.finish_reason.as_deref() == Some("tool_calls") {
                                    // Emit accumulated tool calls
                                    let completed: Vec<_> = tool_calls.drain().collect();
                                    for (_, tc) in completed {
                                        if !tc.name.is_empty() {
                                            return Some((
                                                Ok(StreamChunk::ToolCall(ToolCall {
                                                    id: tc.id,
                                                    name: tc.name,
                                                    arguments: serde_json::from_str(&tc.arguments)
                                                        .unwrap_or(serde_json::Value::Null),
                                                })),
                                                (stream, buf, tool_calls),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }

                // Need more data
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(InferenceError::BackendError(e.to_string())),
                            (stream, buf, tool_calls),
                        ));
                    }
                    None => {
                        // Stream ended without [DONE]
                        return Some((
                            Ok(StreamChunk::Done { usage: None }),
                            (stream, buf, tool_calls),
                        ));
                    }
                }
            }
        },
    )
}

#[derive(Debug)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

// --- OpenAI API types ---

#[derive(Debug, Serialize)]
struct OaiRequest {
    model: String,
    messages: Vec<OaiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OaiTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
struct OaiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OaiToolCallRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl From<&Message> for OaiMessage {
    fn from(m: &Message) -> Self {
        match &m.role {
            Role::System => OaiMessage {
                role: "system".into(),
                content: Some(match &m.content {
                    MessageContent::Text(t) => t.clone(),
                    _ => String::new(),
                }),
                tool_calls: None,
                tool_call_id: None,
            },
            Role::User => OaiMessage {
                role: "user".into(),
                content: Some(match &m.content {
                    MessageContent::Text(t) => t.clone(),
                    _ => String::new(),
                }),
                tool_calls: None,
                tool_call_id: None,
            },
            Role::Assistant => match &m.content {
                MessageContent::Text(t) => OaiMessage {
                    role: "assistant".into(),
                    content: Some(t.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                MessageContent::ToolCalls(tcs) => OaiMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(tcs.iter().map(|tc| OaiToolCallRef {
                        id: tc.id.clone(),
                        r#type: "function".into(),
                        function: OaiFunctionCall {
                            name: tc.name.clone(),
                            arguments: tc.arguments.to_string(),
                        },
                    }).collect()),
                    tool_call_id: None,
                },
                _ => OaiMessage {
                    role: "assistant".into(),
                    content: Some(String::new()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            },
            Role::Tool { tool_call_id } => OaiMessage {
                role: "tool".into(),
                content: Some(match &m.content {
                    MessageContent::Text(t) => t.clone(),
                    MessageContent::ToolResult { output, .. } => output.clone(),
                    _ => String::new(),
                }),
                tool_calls: None,
                tool_call_id: Some(tool_call_id.clone()),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct OaiTool {
    r#type: String,
    function: OaiFunction,
}

#[derive(Debug, Serialize)]
struct OaiFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OaiToolCallRef {
    id: String,
    r#type: String,
    function: OaiFunctionCall,
}

#[derive(Debug, Serialize)]
struct OaiFunctionCall {
    name: String,
    arguments: String,
}

// --- Streaming response types ---

#[derive(Debug, Deserialize)]
struct OaiStreamChunk {
    choices: Vec<OaiStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OaiStreamChoice {
    delta: OaiStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OaiStreamDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<OaiStreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OaiStreamToolCall {
    index: usize,
    id: Option<String>,
    function: Option<OaiStreamFunction>,
}

#[derive(Debug, Deserialize)]
struct OaiStreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_conversion_system() {
        let msg = Message {
            role: Role::System,
            content: MessageContent::Text("you are a shell".into()),
        };
        let oai: OaiMessage = (&msg).into();
        assert_eq!(oai.role, "system");
        assert_eq!(oai.content.unwrap(), "you are a shell");
    }

    #[test]
    fn message_conversion_user() {
        let msg = Message {
            role: Role::User,
            content: MessageContent::Text("hello".into()),
        };
        let oai: OaiMessage = (&msg).into();
        assert_eq!(oai.role, "user");
    }

    #[test]
    fn message_conversion_tool_calls() {
        let msg = Message {
            role: Role::Assistant,
            content: MessageContent::ToolCalls(vec![ToolCall {
                id: "call_1".into(),
                name: "remote".into(),
                arguments: serde_json::json!({"host": "staging"}),
            }]),
        };
        let oai: OaiMessage = (&msg).into();
        assert_eq!(oai.role, "assistant");
        assert!(oai.tool_calls.is_some());
        assert_eq!(oai.tool_calls.unwrap()[0].function.name, "remote");
    }

    #[test]
    fn message_conversion_tool_result() {
        let msg = Message {
            role: Role::Tool { tool_call_id: "call_1".into() },
            content: MessageContent::ToolResult {
                tool_call_id: "call_1".into(),
                output: "done".into(),
            },
        };
        let oai: OaiMessage = (&msg).into();
        assert_eq!(oai.role, "tool");
        assert_eq!(oai.tool_call_id.unwrap(), "call_1");
    }

    #[test]
    fn request_building() {
        let backend = OpenAiCompatBackend::new(
            "http://localhost:8000/v1".into(),
            "test-model".into(),
            None,
        );

        let messages = vec![Message {
            role: Role::User,
            content: MessageContent::Text("hello".into()),
        }];
        let tools = vec![ToolDefinition {
            name: "remote".into(),
            description: "exec remote".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let req = backend.build_request(&messages, &tools, &InferenceConfig::default());
        assert_eq!(req.model, "test-model");
        assert!(req.stream);
        assert_eq!(req.messages.len(), 1);
        assert!(req.tools.is_some());
        assert_eq!(req.tools.unwrap().len(), 1);
    }

    #[test]
    fn request_no_tools() {
        let backend = OpenAiCompatBackend::new(
            "http://localhost:8000/v1".into(),
            "test-model".into(),
            None,
        );
        let req = backend.build_request(&[], &[], &InferenceConfig::default());
        assert!(req.tools.is_none());
    }

    #[test]
    fn from_config_reads_env() {
        // Don't actually set env — just verify it doesn't panic
        let backend = OpenAiCompatBackend::from_config(
            "http://localhost:8000/v1",
            "model",
            Some("NONEXISTENT_KEY_VAR"),
        );
        assert_eq!(backend.name(), "model");
        assert!(backend.api_key.is_none()); // env var not set
    }

    #[test]
    fn endpoint_trailing_slash_stripped() {
        let backend = OpenAiCompatBackend::new(
            "http://localhost:8000/v1/".into(),
            "model".into(),
            None,
        );
        assert_eq!(backend.endpoint, "http://localhost:8000/v1");
    }
}
