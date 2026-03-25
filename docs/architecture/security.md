# Security

## Credential Model (ADR-006)

All authentication uses Ed25519 keypairs with TOFU (Trust On First Use) semantics.

### Key Generation

`kith --init` generates a keypair stored in `~/.config/kith/`. The public key is shared with daemons for authorization.

### Request Authentication

Every gRPC request includes a signed credential:

```
signature = Ed25519_sign(secret_key, pubkey || timestamp_ms || request_hash)
```

The daemon verifies:
1. Signature is valid
2. Timestamp is within acceptable drift window (replay protection)
3. Public key is in the machine's policy file with appropriate scope

### Scope Model

Scope is **server-determined** — the daemon assigns permissions based on the public key:

| Scope | Permissions |
|-------|------------|
| **Ops** | Exec, Apply, Commit, Rollback, Query, Events, Capabilities |
| **Viewer** | Query, Events, Capabilities |

### No SSH

Kith deliberately avoids SSH. All remote execution goes through the daemon's gRPC interface with credential-based auth. This provides:
- Uniform audit trail (every action logged)
- Policy enforcement at the daemon level
- No shell escape — commands are arguments to `Exec`, not interactive sessions

## Mesh Security

- WireGuard provides encrypted, authenticated tunnels between peers
- Nostr events are Ed25519-signed (same keypair)
- No coordination server — no single point of trust
