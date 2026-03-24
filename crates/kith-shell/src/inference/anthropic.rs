//! Anthropic Messages API InferenceBackend implementation.
//! Covers Claude models via API key authentication.

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

use kith_common::error::InferenceError;
use kith_common::inference::*;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicBackend {
    api_key: String,
    model: String,
    client: Client,
}

impl AnthropicBackend {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: Client::new(),
        }
    }

    /// Build from config, reading API key from env var.
    pub fn from_config(model: &str, api_key_env: &str) -> Result<Self, InferenceError> {
        let api_key = std::env::var(api_key_env)
            .map_err(|_| InferenceError::AuthFailed(format!("env var {api_key_env} not set")))?;
        Ok(Self::new(api_key, model.into()))
    }

    fn build_request(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &InferenceConfig,
    ) -> AnthropicRequest {
        let mut system_prompt = None;
        let mut api_messages = Vec::new();

        for msg in messages {
            match &msg.role {
                Role::System => {
                    if let MessageContent::Text(t) = &msg.content {
                        system_prompt = Some(t.clone());
                    }
                }
                Role::User => {
                    if let MessageContent::Text(t) = &msg.content {
                        api_messages.push(AnthropicMessage {
                            role: "user".into(),
                            content: AnthropicContent::Text(t.clone()),
                        });
                    }
                }
                Role::Assistant => match &msg.content {
                    MessageContent::Text(t) => {
                        api_messages.push(AnthropicMessage {
                            role: "assistant".into(),
                            content: AnthropicContent::Text(t.clone()),
                        });
                    }
                    MessageContent::ToolCalls(tcs) => {
                        let blocks: Vec<AnthropicContentBlock> = tcs.iter().map(|tc| {
                            AnthropicContentBlock::ToolUse {
                                id: tc.id.clone(),
                                name: tc.name.clone(),
                                input: tc.arguments.clone(),
                            }
                        }).collect();
                        api_messages.push(AnthropicMessage {
                            role: "assistant".into(),
                            content: AnthropicContent::Blocks(blocks),
                        });
                    }
                    _ => {}
                },
                Role::Tool { tool_call_id } => {
                    let output = match &msg.content {
                        MessageContent::Text(t) => t.clone(),
                        MessageContent::ToolResult { output, .. } => output.clone(),
                        _ => String::new(),
                    };
                    api_messages.push(AnthropicMessage {
                        role: "user".into(),
                        content: AnthropicContent::Blocks(vec![
                            AnthropicContentBlock::ToolResult {
                                tool_use_id: tool_call_id.clone(),
                                content: output,
                            }
                        ]),
                    });
                }
            }
        }

        let api_tools: Option<Vec<AnthropicTool>> = if tools.is_empty() {
            None
        } else {
            Some(tools.iter().map(|t| AnthropicTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.parameters.clone(),
            }).collect())
        };

        AnthropicRequest {
            model: self.model.clone(),
            max_tokens: config.max_tokens.unwrap_or(4096),
            system: system_prompt,
            messages: api_messages,
            tools: api_tools,
            stream: true,
            temperature: config.temperature,
        }
    }
}

#[async_trait]
impl InferenceBackend for AnthropicBackend {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &InferenceConfig,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, InferenceError>> + Send>>, InferenceError> {
        let body = self.build_request(messages, tools, config);

        let response = self.client.post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .timeout(config.timeout)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    InferenceError::Timeout(config.timeout)
                } else if e.is_connect() {
                    InferenceError::Unreachable(e.to_string())
                } else {
                    InferenceError::BackendError(e.to_string())
                }
            })?;

        let status = response.status();
        if status.as_u16() == 429 {
            let retry_after = response.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(1000);
            return Err(InferenceError::RateLimited { retry_after_ms: retry_after });
        }
        if status.as_u16() == 401 {
            return Err(InferenceError::AuthFailed("invalid API key".into()));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            if body.contains("context_length_exceeded") || body.contains("max_tokens") {
                return Err(InferenceError::ContextOverflow { used: 0, limit: 0 });
            }
            return Err(InferenceError::BackendError(format!("HTTP {status}: {body}")));
        }

        let byte_stream = response.bytes_stream();
        let stream = parse_anthropic_sse_stream(byte_stream);
        Ok(Box::pin(stream))
    }

    async fn health_check(&self) -> Result<(), InferenceError> {
        // Anthropic doesn't have a /health endpoint. Send a minimal request.
        let response = self.client.post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": self.model,
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| InferenceError::Unreachable(e.to_string()))?;

        if response.status().is_success() || response.status().as_u16() == 200 {
            Ok(())
        } else {
            Err(InferenceError::Unreachable(format!("HTTP {}", response.status())))
        }
    }

    fn name(&self) -> &str {
        &self.model
    }
}

/// Parse Anthropic SSE stream into StreamChunks.
fn parse_anthropic_sse_stream(
    byte_stream: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<StreamChunk, InferenceError>> + Send {
    use futures::StreamExt;

    let mut current_tool_id = String::new();
    let mut current_tool_name = String::new();
    let mut current_tool_input = String::new();

    futures::stream::unfold(
        (Box::pin(byte_stream), String::new(), current_tool_id, current_tool_name, current_tool_input),
        |(mut stream, mut buf, mut tool_id, mut tool_name, mut tool_input)| async move {
            loop {
                if let Some(pos) = buf.find("\n\n") {
                    let event_block = buf[..pos].to_string();
                    buf = buf[pos + 2..].to_string();

                    // Parse event type and data
                    let mut event_type = String::new();
                    let mut data = String::new();
                    for line in event_block.lines() {
                        if let Some(et) = line.strip_prefix("event: ") {
                            event_type = et.trim().to_string();
                        } else if let Some(d) = line.strip_prefix("data: ") {
                            data = d.trim().to_string();
                        }
                    }

                    match event_type.as_str() {
                        "content_block_start" => {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                                if let Some(cb) = v.get("content_block") {
                                    if cb.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                        tool_id = cb.get("id").and_then(|i| i.as_str()).unwrap_or("").into();
                                        tool_name = cb.get("name").and_then(|n| n.as_str()).unwrap_or("").into();
                                        tool_input.clear();
                                    }
                                }
                            }
                        }
                        "content_block_delta" => {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                                if let Some(delta) = v.get("delta") {
                                    let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    match delta_type {
                                        "text_delta" => {
                                            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                                if !text.is_empty() {
                                                    return Some((
                                                        Ok(StreamChunk::TextDelta(text.into())),
                                                        (stream, buf, tool_id, tool_name, tool_input),
                                                    ));
                                                }
                                            }
                                        }
                                        "thinking_delta" => {
                                            if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                                                if !thinking.is_empty() {
                                                    return Some((
                                                        Ok(StreamChunk::ThinkingDelta(thinking.into())),
                                                        (stream, buf, tool_id, tool_name, tool_input),
                                                    ));
                                                }
                                            }
                                        }
                                        "input_json_delta" => {
                                            if let Some(partial) = delta.get("partial_json").and_then(|p| p.as_str()) {
                                                tool_input.push_str(partial);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        "content_block_stop" => {
                            if !tool_name.is_empty() {
                                let tc = ToolCall {
                                    id: std::mem::take(&mut tool_id),
                                    name: std::mem::take(&mut tool_name),
                                    arguments: serde_json::from_str(&tool_input)
                                        .unwrap_or(serde_json::Value::Null),
                                };
                                tool_input.clear();
                                return Some((
                                    Ok(StreamChunk::ToolCall(tc)),
                                    (stream, buf, tool_id, tool_name, tool_input),
                                ));
                            }
                        }
                        "message_stop" => {
                            return Some((
                                Ok(StreamChunk::Done { usage: None }),
                                (stream, buf, tool_id, tool_name, tool_input),
                            ));
                        }
                        "message_delta" => {
                            // Could extract usage stats here
                        }
                        _ => {}
                    }
                    continue;
                }

                match stream.next().await {
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(InferenceError::BackendError(e.to_string())),
                            (stream, buf, tool_id, tool_name, tool_input),
                        ));
                    }
                    None => {
                        return Some((
                            Ok(StreamChunk::Done { usage: None }),
                            (stream, buf, tool_id, tool_name, tool_input),
                        ));
                    }
                }
            }
        },
    )
}

// --- Anthropic API types ---

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_conversion_system_extracted() {
        let backend = AnthropicBackend::new("test-key".into(), "claude-sonnet".into());
        let messages = vec![
            Message { role: Role::System, content: MessageContent::Text("sys prompt".into()) },
            Message { role: Role::User, content: MessageContent::Text("hello".into()) },
        ];
        let req = backend.build_request(&messages, &[], &InferenceConfig::default());
        assert_eq!(req.system.unwrap(), "sys prompt");
        assert_eq!(req.messages.len(), 1); // system not in messages
        assert_eq!(req.messages[0].role, "user");
    }

    #[test]
    fn tool_calls_converted() {
        let backend = AnthropicBackend::new("test-key".into(), "claude-sonnet".into());
        let messages = vec![
            Message {
                role: Role::Assistant,
                content: MessageContent::ToolCalls(vec![ToolCall {
                    id: "tc_1".into(),
                    name: "remote".into(),
                    arguments: serde_json::json!({"host": "staging"}),
                }]),
            },
        ];
        let req = backend.build_request(&messages, &[], &InferenceConfig::default());
        let serialized = serde_json::to_string(&req).unwrap();
        assert!(serialized.contains("tool_use"));
        assert!(serialized.contains("remote"));
    }

    #[test]
    fn tool_result_converted() {
        let backend = AnthropicBackend::new("test-key".into(), "claude-sonnet".into());
        let messages = vec![
            Message {
                role: Role::Tool { tool_call_id: "tc_1".into() },
                content: MessageContent::ToolResult {
                    tool_call_id: "tc_1".into(),
                    output: "done".into(),
                },
            },
        ];
        let req = backend.build_request(&messages, &[], &InferenceConfig::default());
        // Tool results are sent as user messages with tool_result blocks
        assert_eq!(req.messages[0].role, "user");
        let serialized = serde_json::to_string(&req).unwrap();
        assert!(serialized.contains("tool_result"));
    }

    #[test]
    fn tools_serialized_with_input_schema() {
        let backend = AnthropicBackend::new("test-key".into(), "claude-sonnet".into());
        let tools = vec![ToolDefinition {
            name: "remote".into(),
            description: "exec remote".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "host": { "type": "string" } }
            }),
        }];
        let req = backend.build_request(&[], &tools, &InferenceConfig::default());
        assert!(req.tools.is_some());
        let serialized = serde_json::to_string(&req).unwrap();
        assert!(serialized.contains("input_schema"));
    }

    #[test]
    fn from_config_missing_env_errors() {
        let result = AnthropicBackend::from_config("model", "NONEXISTENT_KEY_12345");
        assert!(result.is_err());
    }

    #[test]
    fn request_has_correct_defaults() {
        let backend = AnthropicBackend::new("key".into(), "claude-sonnet".into());
        let req = backend.build_request(
            &[Message { role: Role::User, content: MessageContent::Text("hi".into()) }],
            &[],
            &InferenceConfig::default(),
        );
        assert_eq!(req.max_tokens, 4096);
        assert!(req.stream);
        assert_eq!(req.model, "claude-sonnet");
    }
}
