# Configuration

Kith reads configuration from `~/.config/kith/config.toml`.

## Full Example

```toml
[inference]
backend = "openai-compatible"
endpoint = "http://gpu-server:8000/v1"
model = "qwen3-coder"
# api_key_env = "OPENAI_API_KEY"

[inference.anthropic]
endpoint = "https://api.anthropic.com/v1"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"

[embedding]
backend = "api"
endpoint = "http://gpu-server:8000/v1"
model = "bge-small-en-v1.5"
dimensions = 384
# api_key_env = "EMBEDDING_API_KEY"

[mesh]
identifier = "my-mesh-2026"
wireguard_interface = "kith0"
listen_port = 51820
mesh_cidr = "kith-mesh"
nostr_relays = ["wss://relay.damus.io"]
```

## Sections

### `[inference]`

| Key | Description | Default |
|-----|-------------|---------|
| `backend` | Backend type: `openai-compatible` or `anthropic` | `openai-compatible` |
| `endpoint` | API endpoint URL | — |
| `model` | Model identifier | — |
| `api_key_env` | Environment variable containing the API key | — |

### `[embedding]`

| Key | Description | Default |
|-----|-------------|---------|
| `backend` | `bag-of-words` (local) or `api` (remote) | `bag-of-words` |
| `endpoint` | OpenAI-compatible `/v1/embeddings` endpoint | — |
| `model` | Embedding model name | — |
| `dimensions` | Vector dimensions | 1000 (bag-of-words) |

### `[mesh]`

| Key | Description | Default |
|-----|-------------|---------|
| `identifier` | Mesh name (used for Nostr event filtering) | — |
| `wireguard_interface` | WireGuard interface name | `kith0` |
| `listen_port` | WireGuard listen port | `51820` |
| `mesh_cidr` | IPv6 ULA prefix or IPv4 CIDR | — |
| `nostr_relays` | List of Nostr relay WebSocket URLs | — |
