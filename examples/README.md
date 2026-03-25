# Kith Examples

## Configuration Examples

| File | Description |
|------|-------------|
| [`config/dev-local.toml`](config/dev-local.toml) | Local Ollama, no mesh, bag-of-words embeddings |
| [`config/hosted-anthropic.toml`](config/hosted-anthropic.toml) | Claude API via Anthropic |
| [`config/hosted-openai.toml`](config/hosted-openai.toml) | GPT via OpenAI API with embeddings |
| [`config/self-hosted-vllm.toml`](config/self-hosted-vllm.toml) | Self-hosted vLLM/SGLang on GPU server |
| [`config/mesh-3node.toml`](config/mesh-3node.toml) | Three-node mesh with WireGuard + Nostr |

Copy any of these to `~/.config/kith/config.toml` and adjust.

## Script Examples

| Script | Description |
|--------|-------------|
| [`scripts/01-basic-usage.sh`](scripts/01-basic-usage.sh) | Pass-through vs intent, escape hatch, single command |
| [`scripts/02-remote-exec.sh`](scripts/02-remote-exec.sh) | Remote execution and fleet queries via daemon |
| [`scripts/03-apply-commit-rollback.sh`](scripts/03-apply-commit-rollback.sh) | Change management with commit windows |
| [`scripts/04-retrieval.sh`](scripts/04-retrieval.sh) | Semantic search over operational history |
| [`scripts/05-mesh-setup.sh`](scripts/05-mesh-setup.sh) | Initialize keypairs and connect machines |

## Starter Configs

Production-ready configs live in [`/config/`](../config/):

- [`minimal.toml`](../config/minimal.toml) — single machine, local Ollama
- [`production.toml`](../config/production.toml) — multi-machine mesh with hosted inference
