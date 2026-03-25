# Deployment

## Docker

Build the daemon image:

```bash
docker build -t kith-daemon .
```

Run:

```bash
docker run -d --name kith-daemon \
  -p 50051:50051 \
  -e RUST_LOG=info \
  kith-daemon
```

## Platform Differences

| Capability | Linux | macOS |
|-----------|-------|-------|
| kith shell | Yes | Yes |
| kith-daemon | Full (cgroups, overlayfs, namespaces) | Agent-only (connect to mesh) |
| Containment | OverlayTransaction + CopyTransaction | CopyTransaction only |
| WireGuard | Kernel module or userspace (boringtun) | Userspace only |

## Mesh Setup

1. **Generate keypairs** on each machine: `kith --init`
2. **Exchange public keys** and add to each daemon's policy file
3. **Configure Nostr relays** in `config.toml` for peer discovery
4. **Start daemons** — they will discover each other via Nostr and establish WireGuard tunnels

## Release Binaries

Pre-built binaries are available on the [GitHub Releases](https://github.com/witlox/kith/releases) page:

| Archive | Arch | Contents |
|---------|------|----------|
| `kith-x86_64.tar.gz` | x86_64 | kith (shell), kith-daemon |
| `kith-aarch64.tar.gz` | aarch64 | kith (shell), kith-daemon |
| `kith-x86_64-full.tar.gz` | x86_64 | Both binaries with nostr + wireguard features |
| `kith-aarch64-full.tar.gz` | aarch64 | Both binaries with nostr + wireguard features |
