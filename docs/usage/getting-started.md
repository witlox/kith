# Getting Started

## Install from Release

Download the latest release for your platform from [GitHub Releases](https://github.com/witlox/kith/releases):

```bash
# Linux x86_64 (shell + daemon with Nostr + WireGuard)
curl -LO https://github.com/witlox/kith/releases/latest/download/kith-linux-x86_64.tar.gz
tar xzf kith-linux-x86_64.tar.gz
sudo mv kith kith-daemon /usr/local/bin/

# Linux aarch64
curl -LO https://github.com/witlox/kith/releases/latest/download/kith-linux-aarch64.tar.gz
tar xzf kith-linux-aarch64.tar.gz
sudo mv kith kith-daemon /usr/local/bin/

# macOS Apple Silicon (shell only)
curl -LO https://github.com/witlox/kith/releases/latest/download/kith-macos-arm64.tar.gz
tar xzf kith-macos-arm64.tar.gz
sudo mv kith /usr/local/bin/

# macOS Intel
curl -LO https://github.com/witlox/kith/releases/latest/download/kith-macos-x86_64.tar.gz
tar xzf kith-macos-x86_64.tar.gz
sudo mv kith /usr/local/bin/
```

Verify the checksum:

```bash
curl -LO https://github.com/witlox/kith/releases/latest/download/kith-linux-x86_64.tar.gz.sha256
shasum -a 256 -c kith-linux-x86_64.tar.gz.sha256
```

## Build from Source

Prerequisites:
- Rust stable toolchain (1.85+, edition 2024)
- Protocol Buffers compiler (`protoc`)
- Docker (optional, for container tests and daemon images)

```bash
git clone https://github.com/witlox/kith.git && cd kith
just release
# or: cargo build --release -p kith-shell --bin kith && cargo build --release -p kith-daemon --bin kith-daemon
```

## Initialize

Generate a keypair and default configuration:

```bash
kith --init
```

This creates `~/.config/kith/config.toml` and an Ed25519 keypair for mesh authentication.

## Start the Daemon

On each Linux machine that should be part of the mesh:

```bash
RUST_LOG=info kith-daemon
```

The daemon exposes a gRPC interface for remote execution, state queries, drift detection, and event sync. macOS machines run the shell only and connect to Linux daemons.

## Start the Shell

```bash
# Interactive mode
kith

# With a specific backend
ANTHROPIC_API_KEY=sk-... kith --backend anthropic

# Single command
kith "what's the state of things?"
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
