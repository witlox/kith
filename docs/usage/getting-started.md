# Getting Started

## Prerequisites

- Rust stable toolchain (1.85+, edition 2024)
- Protocol Buffers compiler (`protoc`)
- Docker (optional, for container tests and daemon images)

## Build

```bash
# Build everything
cargo build --workspace

# Build release binaries
cargo build --release -p kith-shell --bin kith
cargo build --release -p kith-daemon --bin kith-daemon
```

Or with just:

```bash
just build       # debug
just release     # optimized
```

## Initialize

Generate a keypair and default configuration:

```bash
./target/release/kith --init
```

This creates `~/.config/kith/config.toml` and a Ed25519 keypair for mesh authentication.

## Start the Daemon

On each machine that should be part of the mesh:

```bash
RUST_LOG=info ./target/release/kith-daemon
```

The daemon exposes a gRPC interface for remote execution, state queries, drift detection, and event sync.

## Start the Shell

```bash
# Interactive mode
./target/release/kith

# With a specific backend
ANTHROPIC_API_KEY=sk-... ./target/release/kith --backend anthropic

# Single command
./target/release/kith "what's the state of things?"
```

## Interactive Usage

```
kith> ls -la                              # pass-through (bash via PTY)
kith> what's the state of things?         # intent (routed to LLM)
kith> run: docker ps                      # escape hatch (forced bash)
kith> exit
```

The shell classifies each input:
- Commands found in `$PATH` → executed directly (zero latency)
- Everything else → routed to the LLM for planning and execution
- `run:` prefix → forced pass-through regardless of classification
