# Deployment

## Install from Release

Pre-built binaries are available on the [GitHub Releases](https://github.com/witlox/kith/releases) page:

### Linux (shell + daemon)

| Archive | Arch | Contents |
|---------|------|----------|
| [`kith-linux-x86_64.tar.gz`](https://github.com/witlox/kith/releases/latest/download/kith-linux-x86_64.tar.gz) | x86_64 | `kith` + `kith-daemon` (Nostr + WireGuard) |
| [`kith-linux-aarch64.tar.gz`](https://github.com/witlox/kith/releases/latest/download/kith-linux-aarch64.tar.gz) | aarch64 | `kith` + `kith-daemon` (Nostr + WireGuard) |

### macOS (shell only — connects to Linux daemons via gRPC)

| Archive | Arch |
|---------|------|
| [`kith-macos-arm64.tar.gz`](https://github.com/witlox/kith/releases/latest/download/kith-macos-arm64.tar.gz) | Apple Silicon |
| [`kith-macos-x86_64.tar.gz`](https://github.com/witlox/kith/releases/latest/download/kith-macos-x86_64.tar.gz) | Intel |

Each archive includes a `.sha256` checksum file for verification.

## Docker

Build the daemon image:

```bash
docker build -t kith-daemon .
```

Run:

```bash
docker run -d --name kith-daemon \
  -p 9443:9443 \
  -e RUST_LOG=info \
  kith-daemon
```

## Platform Differences

| Capability | Linux | macOS |
|-----------|-------|-------|
| kith shell | Yes | Yes |
| kith-daemon | Full (cgroups, overlayfs, namespaces) | Not supported (agent-only) |
| Containment | OverlayTransaction + CopyTransaction | CopyTransaction only |
| WireGuard | Kernel module or userspace (boringtun) | Userspace only |

## Mesh Setup

1. **Install** kith on each machine (release binary or from source)
2. **Initialize** keypairs on each machine: `kith --init`
3. **Exchange public keys** — add each machine's pubkey to the daemon policy files
4. **Configure** `~/.config/kith/config.toml` with Nostr relays and mesh identifier (see [Configuration](configuration.md))
5. **Start daemons** on Linux machines: `RUST_LOG=info kith-daemon`
6. **Connect** from any machine: `kith` — peers discover each other via Nostr and establish WireGuard tunnels automatically

## Systemd Service

```ini
[Unit]
Description=Kith Daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/kith-daemon
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo cp kith-daemon.service /etc/systemd/system/
sudo systemctl enable --now kith-daemon
```
