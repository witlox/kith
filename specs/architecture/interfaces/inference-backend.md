# InferenceBackend Interface

The trait that makes kith model-agnostic. Defined in kith-common, implemented in kith-shell.

---

## Trait Definition

```rust
/// The abstraction over LLM providers. Any model with tool calling
/// and streaming works. Implementations are in kith-shell.
#[async_trait]
pub trait InferenceBackend: Send + Sync {
    /// Send a conversation to the model and receive a streaming response.
    /// Tools are passed as JSON schemas. The model decides which to call.
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &InferenceConfig,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, InferenceError>> + Send>>, InferenceError>;

    /// Check if the backend is reachable.
    async fn health_check(&self) -> Result<(), InferenceError>;

    /// Backend display name (for status/logging).
    fn name(&self) -> &str;
}
```

## Supporting Types

```rust
/// A message in the conversation.
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
    ToolResult { tool_call_id: String, output: String },
}

/// A tool the model can call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}

/// A tool call produced by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Streaming response chunk.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Text content delta.
    TextDelta(String),
    /// Reasoning/thinking content (optional, model-dependent).
    ThinkingDelta(String),
    /// A complete tool call (emitted when fully parsed from stream).
    ToolCall(ToolCall),
    /// End of response.
    Done { usage: Option<UsageStats> },
}

/// Token usage statistics (optional).
#[derive(Debug, Clone)]
pub struct UsageStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Per-request inference configuration.
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// Timeout for the complete request.
    pub timeout: std::time::Duration,
}

/// Inference errors.
#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("backend unreachable: {0}")]
    Unreachable(String),
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    #[error("rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },
    #[error("context window exceeded: {used} tokens used, {limit} limit")]
    ContextOverflow { used: u64, limit: u64 },
    #[error("malformed response: {0}")]
    MalformedResponse(String),
    #[error("timeout after {0:?}")]
    Timeout(std::time::Duration),
    #[error("backend error: {0}")]
    BackendError(String),
}
```

## Implementations

### OpenAiCompatBackend (kith-shell)

Covers: vLLM, SGLang, Ollama, LM Studio, OpenAI, any OpenAI-compatible API.

```rust
pub struct OpenAiCompatBackend {
    endpoint: Url,
    model: String,
    api_key: Option<String>,
    client: reqwest::Client,
}
```

### AnthropicBackend (kith-shell)

Covers: Claude models via the Anthropic Messages API.

```rust
pub struct AnthropicBackend {
    api_key: String,
    model: String,
    client: reqwest::Client,
}
```

## Configuration

```toml
# ~/.config/kith/config.toml

[inference]
# Backend type: "openai-compatible" or "anthropic"
backend = "openai-compatible"
endpoint = "http://gpu-server:8000/v1"
model = "minimax-m2.5"
# api_key_env = "OPENAI_API_KEY"  # optional, read from env var

# Or for hosted:
# backend = "anthropic"
# model = "claude-sonnet-4-20250514"
# api_key_env = "ANTHROPIC_API_KEY"
```

## Design Decisions

- **Trait in kith-common, implementations in kith-shell:** prevents model-specific code from leaking into daemon/mesh/sync/state (INV-OPS-5).
- **StreamChunk::ThinkingDelta:** optional — present for models with interleaved thinking, absent for others. Shell renders if present, ignores if absent (inference-backend.feature: thinking scenario).
- **ToolCall emitted complete, not streaming:** tool calls are buffered in the stream parser until fully received, then emitted as a single ToolCall chunk. This simplifies dispatch.
- **No model-specific enums:** the trait doesn't have a `Provider` enum. New backends are just new structs implementing the trait. Adding a provider requires zero changes to existing code.
