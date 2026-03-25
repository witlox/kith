<p align="center">
  <img src="logo.png" alt="kith" width="600">
</p>

<p align="center">
  <a href="https://github.com/witlox/kith/actions/workflows/ci.yml"><img src="https://github.com/witlox/kith/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://codecov.io/gh/witlox/kith"><img src="https://codecov.io/gh/witlox/kith/graph/badge.svg" alt="codecov"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License"></a>
  <a href="https://doc.rust-lang.org/edition-guide/rust-2024/"><img src="https://img.shields.io/badge/rust-2024_edition-orange.svg" alt="Rust"></a>
  <a href="https://witlox.github.io/kith/"><img src="https://img.shields.io/badge/docs-mdbook-blue.svg" alt="Docs"></a>
</p>

An intent-driven distributed shell: a reasoning layer (LLM) over a mesh of machines. Kith replaces the traditional terminal workflow with an agent that operates across machines — executing locally or remotely, maintaining persistent operational context, and enforcing policy-scoped containment on every action.

The Unix philosophy stays intact: standard tools, standard commands, standard pipes. The agent is the orchestrator that used to be you.

## Quick Start

```bash
# Install from release (Linux x86_64)
curl -LO https://github.com/witlox/kith/releases/latest/download/kith-linux-x86_64.tar.gz
tar xzf kith-linux-x86_64.tar.gz
sudo mv kith kith-daemon /usr/local/bin/

# Or build from source
just release

# Initialize (generates keypair + default config)
kith --init

# Start daemon on each machine
RUST_LOG=info kith-daemon

# Start shell
kith

# Or with a specific backend
ANTHROPIC_API_KEY=sk-... kith --backend anthropic

# Or single command
kith "summarize the git log for this week"
```

See [Releases](https://github.com/witlox/kith/releases) for all platforms (Linux x86_64/aarch64, macOS arm64/x86_64).

Interactive usage:
```
kith> ls -la                              # pass-through (bash via PTY, zero latency)
kith> what's the state of things?         # intent (routed to LLM)
kith> run: docker ps                      # escape hatch (forced bash)
kith> exit
```

## Design Principles

- **Unix tools are the tools** — no wrappers around cat/grep/sed; the model uses standard commands directly
- **Intent-driven, not command-driven** — express what you want; the agent composes and executes
- **Escape hatch always available** — prefix with `run:` to bypass the agent
- **Distributed by default** — mesh of kith-daemons connected via WireGuard, synced via CRDTs
- **Containment as a primitive** — every action is policy-scoped and audited; blast radius is explicit
- **Model-agnostic** — any LLM with tool calling works; bootstrap with hosted APIs, self-host when ready
- **Pact-patterned** — acknowledged drift, commit windows, immutable audit log

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Intent Interface    kith shell (PTY + LLM)     │
├─────────────────────────────────────────────────┤
│  Reasoning           Any LLM with tool calling  │
├─────────────────────────────────────────────────┤
│  State Layer         cr-sqlite CRDT + vector    │
├─────────────────────────────────────────────────┤
│  Mesh                WireGuard + Nostr + gRPC   │
├─────────────────────────────────────────────────┤
│  Containment         cgroups v2 + overlayfs     │
├─────────────────────────────────────────────────┤
│  Platform            Linux (full) / macOS (agent)│
└─────────────────────────────────────────────────┘
```

| Crate | Role |
|-------|------|
| `kith-common` | Shared kernel: types, errors, config, traits, Ed25519 credentials |
| `kith-mesh` | Peer registry, WireGuard tunnels, Nostr signaling |
| `kith-daemon` | gRPC service: exec, policy, audit, drift, commit windows, containment |
| `kith-shell` | PTY wrapper, LLM inference, tool dispatch, agent loop |
| `kith-sync` | Event store (in-memory + SQLite), cr-sqlite CRDT replication |
| `kith-state` | Keyword retrieval, vector index, hybrid search, embeddings |

## Native Tools

7 tools for capabilities that don't exist in standard Unix. Everything else is bash.

| Tool | Purpose |
|------|---------|
| `remote(host, command)` | Execute on a remote machine via kith-daemon |
| `fleet_query(query)` | Query synced state across the mesh |
| `retrieve(query)` | Semantic search over operational history |
| `apply(host, command, paths?)` | Make a change with commit window; optionally back up paths |
| `commit(pending_id)` | Finalize pending changes |
| `rollback(pending_id)` | Revert pending changes |
| `todo(action, text?)` | Agent self-managed task tracking |

## Model Support

Any LLM backend with tool calling and streaming works through the `InferenceBackend` trait:

| Backend | Use Case |
|---------|----------|
| **Claude** via Anthropic API | High-quality reasoning, extended thinking |
| **GPT-4.1/5.x** via OpenAI API | Large context window |
| **Gemini 3** via Google API | 1M context |
| **Qwen3-Coder** via vLLM/SGLang | Self-hosted, Apache 2.0 |
| **DeepSeek V3.2** via vLLM/SGLang | Self-hosted, thinking-with-tools |
| **Any OpenAI-compatible endpoint** | Ollama, LM Studio, etc. |

## Configuration

`~/.config/kith/config.toml` — see [examples/](examples/) for starter configs.

```toml
[inference]
backend = "anthropic"
endpoint = "https://api.anthropic.com/v1"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"

[embedding]
backend = "api"
endpoint = "http://gpu-server:8000/v1"
model = "bge-small-en-v1.5"
dimensions = 384

[mesh]
identifier = "my-mesh-2026"
wireguard_interface = "kith0"
listen_port = 51820
mesh_cidr = "kith-mesh"
nostr_relays = ["wss://relay.damus.io"]
```

## Development

```bash
just check          # fmt + clippy + test
just test           # all tests (unit + integration + BDD)
just test-unit      # fast unit tests only
just test-bdd       # BDD acceptance tests
just test-e2e       # e2e tests
just test-containers # container e2e (requires docker)
just doc            # build mdbook documentation
just doc-serve      # serve docs locally with live reload
just version        # show computed release version
```

## Documentation

Full documentation is available at [witlox.github.io/kith](https://witlox.github.io/kith/) or build locally with `just doc`.

Architectural decisions are in [`docs/decisions/`](docs/decisions/).

## Related Projects

- [pact](https://github.com/witlox/pact) — Promise-based HPC config management (pattern source)
- [lattice](https://github.com/witlox/lattice) — HPC job scheduling
- [sovra](https://github.com/witlox/sovra) — Federated key management

## License

[Apache-2.0](LICENSE)

## Citation

```bibtex
@software{kith,
  title={Kith: Intent-driven distributed shell with LLM reasoning},
  author={Pim Witlox},
  year={2026},
  url={https://github.com/witlox/kith}
}
```
