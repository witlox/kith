# Kith

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)]()

---

An intent-driven, distributed shell environment powered by LLM inference. Kith replaces the traditional terminal workflow with a reasoning layer that operates across a mesh of machines — executing locally or remotely, maintaining persistent operational context, and enforcing policy-scoped containment on every action.

The Unix philosophy stays intact: standard tools, standard commands, standard pipes. The agent is the orchestrator that used to be you.

## Design Principles

- **Unix tools are the tools** — no proprietary wrappers around cat/grep/sed; the model uses standard commands directly
- **Intent-driven, not command-driven** — express what you want; the agent composes and executes
- **Escape hatch always available** — prefix with `run:` to bypass the agent and execute literally
- **Pact-patterned infrastructure** — acknowledged drift, optimistic concurrency with commit windows, immutable audit log
- **Distributed by default** — mesh of kith-daemons connected via WireGuard, synced via CRDTs
- **Containment as a primitive** — every agent action is policy-scoped and audited; blast radius is explicit
- **Model-managed context** — vector space over operational state; the agent retrieves its own memory
- **Model-agnostic** — any LLM with tool calling and streaming works; bootstrap with hosted APIs, self-host when ready

## Architecture

```
Intent Interface     kith shell (PTY wrapper + LLM inference)
Reasoning            Any LLM with tool calling (self-hosted or API)
State Layer          cr-sqlite (CRDT sync) + vector index
Mesh                 WireGuard (Nostr signaling) + kith-daemon (gRPC)
Containment          cgroups v2 + overlayfs + policy engine
Platform             Linux (full) / macOS (agent + remote only)
```

Borrows patterns from [pact](https://github.com/witlox/pact) (promise-based HPC config management), adapted for small-fleet personal/team infrastructure.

## Components

### kith-daemon (every machine in the mesh)

Lightweight daemon providing authenticated remote execution, state observation, capability reporting, drift detection, and audit logging. Not PID 1 — runs as a background service alongside the existing OS.

### kith shell (your terminal)

PTY wrapper with LLM inference. Classifies input as pass-through (literal commands, zero latency) or intent (routed to the model for planning and execution). Maintains conversation context enriched by the vector index.

### sync layer (cr-sqlite)

Each kith-daemon writes events to local SQLite. CRDT merge semantics sync state across the mesh. Eventually consistent, partition-tolerant. The vector index is a local materialized view over the merged event log.

### mesh network (WireGuard + Nostr)

Machines discover each other via signed events on Nostr relays. Direct P2P WireGuard tunnels for transport. No coordination server required. Optional DERP relay for NAT-hostile environments.

## Model Support

Kith is model-agnostic. Any LLM backend that supports tool calling and streaming works:

| Backend | Use Case |
|---------|----------|
| **Claude (Opus/Sonnet)** via Anthropic API | Bootstrap, high-quality reasoning |
| **GPT-5.x** via OpenAI API | Bootstrap, large context window |
| **Gemini 3** via Google API | Bootstrap, 1M context |
| **MiniMax M2.5** via vLLM/SGLang | Self-hosted, interleaved thinking, MIT license |
| **Qwen3-Coder** via vLLM/SGLang | Self-hosted, Apache 2.0 |
| **DeepSeek V3.2** via vLLM/SGLang | Self-hosted, thinking-with-tools |
| **Any OpenAI-compatible endpoint** | Local models via Ollama, LM Studio, etc. |

Configure in `~/.config/kith/config.toml`:

```toml
[inference]
endpoint = "https://api.anthropic.com/v1"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"
```

## Native Tools

| Tool | Purpose |
|------|---------|
| `remote(host, command)` | Execute on a remote machine via kith-daemon |
| `fleet_query(query)` | Query synced state across the mesh |
| `retrieve(query)` | Semantic search over operational history |
| `apply(host, change)` | Make a change with commit window semantics |
| `commit` / `rollback` | Finalize or revert pending changes |
| `todo` | Agent self-managed task tracking |

Everything else — file ops, git, builds, tests, processes — is standard Unix.

## Quick Start

```bash
# Build
cargo build --release -p kith-shell --bin kith
cargo build --release -p kith-daemon --bin kith-daemon

# Initialize (generates keypair + default config)
./target/release/kith --init

# Start daemon on each machine
RUST_LOG=info ./target/release/kith-daemon

# Start shell (interactive)
./target/release/kith

# Or with a specific backend
ANTHROPIC_API_KEY=sk-... ./target/release/kith --backend anthropic

# Or single command
./target/release/kith "echo hello"
```

Interactive usage:
```
kith> ls -la                              # pass-through (bash via PTY)
kith> what's the state of things?         # intent (routed to LLM)
kith> run: docker ps                      # escape hatch (forced bash)
kith> exit
```

Configuration: `~/.config/kith/config.toml`
```toml
[inference]
backend = "openai-compatible"
endpoint = "http://gpu-server:8000/v1"
model = "qwen3-coder"
# api_key_env = "OPENAI_API_KEY"

[mesh]
identifier = "my-mesh-2026"
wireguard_interface = "kith0"
listen_port = 51820
mesh_cidr = "kith-mesh"
nostr_relays = ["wss://relay.damus.io"]
```

## Related Projects

- [pact](https://github.com/witlox/pact) — Promise-based HPC config management (pattern source)
- [lattice](https://github.com/witlox/lattice) — HPC job scheduling
- [sovra](https://github.com/witlox/sovra) — Federated key management

## License

[Apache-2.0](LICENSE)

## Citation

```
@software{kith,
  title={Kith: Intent-driven distributed shell with LLM reasoning},
  author={Pim Witlox},
  year={2026},
  url={https://github.com/witlox/kith}
}
```
