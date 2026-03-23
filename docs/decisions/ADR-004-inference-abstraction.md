# ADR-004: InferenceBackend Trait for Model-Agnostic Operation

## Status: Accepted

## Context

Kith must work with any LLM that supports tool calling and streaming. The design conversation explored locking to a single model (MiniMax M2.5) but concluded that the architecture should be model-agnostic, with model selection as a configuration choice.

Claude Code's approach (27K system prompt co-tuned with one model) was explicitly rejected. Kith pushes safety into infrastructure (daemon policy, containment) rather than the prompt, making the prompt thin (~2K tokens) and portable across models.

## Decision

Define an `InferenceBackend` trait in kith-common. Implement it in kith-shell for each provider family:

1. **OpenAiCompatBackend** — covers vLLM, SGLang, Ollama, LM Studio, OpenAI, and any OpenAI-compatible API endpoint
2. **AnthropicBackend** — covers Claude models via the Anthropic Messages API

No other component references any LLM provider or model. The trait boundary is the containment wall for model-specific code.

## Consequences

**Positive:**
- Bootstrap with hosted APIs (Claude, GPT) immediately, self-host later
- Model swap is a config change, not a code change
- Two implementations cover the entire current model landscape
- System prompt stays thin because safety is in infrastructure

**Negative:**
- Cannot leverage model-specific training optimizations (e.g., Anthropic's prompt priority hierarchy)
- Per-model system prompt templates are configuration, but still need empirical tuning
- Models without interleaved thinking produce different (possibly lower quality) agent behavior

**Design constraints:**
- StreamChunk::ThinkingDelta is optional — models that think produce it, others don't
- ToolCall is emitted complete (not streaming) to simplify dispatch
- No model-specific enums — adding a provider is just a new struct implementing the trait

## Validation

Test with at least two different backends: one hosted (Anthropic or OpenAI), one self-hosted via vLLM/SGLang. Run the same 20 multi-step tasks with each (ASM-MDL-3, ASM-MDL-4). Compare success rates and subjective quality.
