# Model Support

Kith is model-agnostic. Any LLM backend that supports tool calling and streaming works through the `InferenceBackend` trait.

## Supported Backends

| Backend | Use Case |
|---------|----------|
| **Claude (Opus/Sonnet)** via Anthropic API | High-quality reasoning, extended thinking |
| **GPT-5.x** via OpenAI API | Large context window |
| **Gemini 3** via Google API | 1M context |
| **MiniMax M2.5** via vLLM/SGLang | Self-hosted, interleaved thinking, MIT license |
| **Qwen3-Coder** via vLLM/SGLang | Self-hosted, Apache 2.0 |
| **DeepSeek V3.2** via vLLM/SGLang | Self-hosted, thinking-with-tools |
| **Any OpenAI-compatible endpoint** | Local models via Ollama, LM Studio, etc. |

## Backend Configuration

### Anthropic (Claude)

```toml
[inference]
backend = "anthropic"
endpoint = "https://api.anthropic.com/v1"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"
```

### OpenAI-Compatible (covers GPT, Gemini, self-hosted)

```toml
[inference]
backend = "openai-compatible"
endpoint = "https://api.openai.com/v1"
model = "gpt-4.1"
api_key_env = "OPENAI_API_KEY"
```

### Self-Hosted (Ollama, vLLM, SGLang)

```toml
[inference]
backend = "openai-compatible"
endpoint = "http://gpu-server:8000/v1"
model = "qwen3-coder"
```

No API key needed for local endpoints.

## Embedding Backends

For vector-based semantic retrieval, kith supports:

| Backend | Config | Notes |
|---------|--------|-------|
| Bag-of-words | `backend = "bag-of-words"` | Local, no GPU needed, vocab-versioned |
| API embeddings | `backend = "api"` | Any OpenAI-compatible `/v1/embeddings` endpoint |

## Graceful Degradation

If the inference backend is unreachable, kith degrades to pass-through mode — all input is executed as shell commands. The agent resumes when connectivity returns.
