# Security

## Credential Model (ADR-006)

All authentication uses Ed25519 keypairs with TOFU (Trust On First Use) semantics.

### Key Generation

`kith --init` generates a keypair stored in `~/.config/kith/`. The public key is shared with daemons for authorization. Key files are restricted to `0600` permissions.

### Request Authentication

Every gRPC request includes a signed credential:

```
payload   = pubkey (32B) || timestamp_ms (8B LE) || request_hash
signature = Ed25519_sign(secret_key, payload)
```

The daemon verifies:
1. Signature is valid (RFC 8032 via `ed25519-dalek`)
2. Timestamp is within ±30 second drift window
3. Signature has not been seen before (replay protection via seen-signature cache)
4. Public key is in the machine's policy file with appropriate scope

### Scope Model

Scope is **server-determined** — the daemon assigns permissions based on the public key:

| Scope | Permissions |
|-------|------------|
| **Ops** | Exec, Apply, Commit, Rollback, Query, Events (all), Capabilities |
| **Viewer** | Query, Events (public only), Capabilities |

Events and sync endpoints filter returned data by the caller's scope. Viewer-scoped users only see public-scoped events.

### No SSH

Kith deliberately avoids SSH. All remote execution goes through the daemon's gRPC interface with credential-based auth. This provides:
- Uniform audit trail (every action logged)
- Policy enforcement at the daemon level
- No shell escape — commands are arguments to `Exec`, not interactive sessions

## Mesh Security

- WireGuard provides encrypted, authenticated tunnels between peers
- Daemon gRPC listens on the WireGuard interface (not 0.0.0.0) — transport encryption is provided by WireGuard
- No coordination server — no single point of trust

## Containment

- Every `apply` goes through a commit window (pending → commit/rollback)
- Maximum 16 concurrent command executions per daemon (semaphore-enforced)
- CopyTransaction backs up files before modification; OverlayTransaction uses overlayfs on Linux
- Local pass-through commands (classified from PATH) execute without policy — the local user is trusted

---

## STRIDE Threat Model

Analysis performed 2026-03-25. Covers all security-relevant code paths.

### Spoofing

| ID | Severity | Finding | Status |
|----|----------|---------|--------|
| S-01 | Medium | gRPC transport is plaintext (no TLS) | **Mitigated by design:** all mesh traffic goes over WireGuard tunnels which provide encryption + authentication. Daemon should bind to WireGuard interface, not 0.0.0.0. |
| S-02 | Medium | TOFU has no key pinning — any unknown key gets Viewer on every connection | **Accepted:** TOFU is off by default. When on, grants only Viewer (read-only). True key pinning would require a separate key store. |
| S-03 | Medium | Credential replay within 30s window | **Mitigated:** ReplayGuard in daemon tracks seen signatures with 60s eviction. Duplicate signatures are rejected with `ALREADY_EXISTS`. |
| S-04 | Medium | Nostr signaling keys are ephemeral, not bound to kith Ed25519 identity | **Accepted risk:** Nostr is used for discovery only. WireGuard key exchange is the trust boundary. A rogue Nostr event leads to a WireGuard handshake failure, not a compromise. |

### Tampering

| ID | Severity | Finding | Status |
|----|----------|---------|--------|
| T-01 | Medium | Audit log is an in-memory Vec — no hash chain or immutability guarantees | **Accepted risk:** write-through to SQLite (INSERT OR IGNORE) provides durability. True immutability (hash chain, signed entries) is a future enhancement. Process-level access implies root. |
| T-02 | Medium | Events have no cryptographic signatures — forged events accepted during sync | **Accepted risk:** all sync peers are WireGuard-authenticated. Event signing is a future enhancement for multi-tenant deployments. |
| T-03 | Medium | Containment backups stored in `/tmp` — symlink attack risk on multi-user systems | **Accepted risk:** daemon typically runs as a dedicated user. Backup directory creation uses `create_dir_all` (not following symlinks on modern kernels). |
| T-04 | Low | Commit window has no per-user binding — any Ops user can commit/rollback another's change | **Accepted:** by design, Ops scope implies full trust for state management. |

### Repudiation

| ID | Severity | Finding | Status |
|----|----------|---------|--------|
| R-01 | Low | Audit records user identity but not the original signed credential | **Accepted:** the credential was verified at request time. Storing signatures would enable cryptographic non-repudiation but adds storage cost. Future enhancement. |
| R-02 | Info | Every state-changing action is attributed to an identity | **Mitigated:** `auth()` is called before every RPC handler. Audit log records user, command, timestamp, and outcome. |

### Information Disclosure

| ID | Severity | Finding | Status |
|----|----------|---------|--------|
| I-01 | Low | Events endpoint previously returned all entries regardless of scope | **Mitigated:** `events()` now uses `entries_for_scope()` filtered by the caller's scope. Viewers see only public-scoped events. |
| I-02 | Low | `exchange_events` previously returned all scopes | **Mitigated:** query now includes scope filter. Viewer-scoped callers receive only public events. |
| I-03 | Low | Commands logged verbatim in audit (may contain inline secrets) | **Accepted design trade-off:** audit completeness requires recording exact commands. Users should use environment variables for secrets, not inline them in commands. |
| I-04 | Info | API keys in memory are not zeroized on drop | **Accepted:** process memory access implies root compromise. Zeroization via `zeroize` crate is a future hardening step. |
| I-05 | Medium | Daemon default listen address was `0.0.0.0:9443` | **Mitigated by documentation:** production deployments should bind to the WireGuard interface IP. Default configuration notes this. |

### Denial of Service

| ID | Severity | Finding | Status |
|----|----------|---------|--------|
| D-01 | Low | No concurrent exec limit previously | **Mitigated:** `exec_semaphore` limits to 16 concurrent command executions. Excess requests receive `RESOURCE_EXHAUSTED`. |
| D-02 | Medium | No gRPC rate limiting or connection limits | **Accepted risk:** tonic doesn't provide built-in rate limiting. Future: add tower middleware or a reverse proxy. WireGuard mesh limits exposure to authenticated peers. |
| D-03 | Medium | In-memory EventStore grows unbounded | **Accepted risk:** production deployments use SQLite with periodic compaction. In-memory store is for development/testing. Future: add configurable retention. |
| D-04 | Low | Broadcast channel fixed at 256 entries | **Accepted:** slow subscribers lag but do not block writes. Events are also persisted to the store. |

### Elevation of Privilege

| ID | Severity | Finding | Status |
|----|----------|---------|--------|
| E-01 | Low | `remote` tool previously fell back to local exec when no daemon was connected | **Mitigated:** removed local exec fallback. `remote` now returns "no daemon connected" error when daemon is absent, preventing policy bypass. |
| E-02 | Info | Local pass-through commands have no policy enforcement | **By design:** the local shell user is trusted. Pass-through is a PTY — equivalent to typing directly in a terminal. Policy enforcement is for remote operations. |
| E-03 | Info | Scope is always server-determined from MachinePolicy | **Mitigated:** correct by construction. Client never sends scope claims. |
| E-04 | Low | OverlayTransaction runs `mount`/`umount` as daemon process (root) | **Mitigated:** mount arguments are constructed from controlled paths (backup directories), not user input. No injection vector. |

### Summary

**Mitigated in this release:**
- Credential replay protection (S-03) — seen-signature cache with 60s eviction
- Events scope filtering (I-01, I-02) — both `events()` and `exchange_events()` respect caller scope
- Concurrent exec limit (D-01) — semaphore at 16
- Remote tool policy bypass (E-01) — no local fallback without daemon

**Accepted risks (no code fix, documented):**
- No TLS on gRPC (S-01) — WireGuard provides transport security
- No event signatures (T-02) — WireGuard authenticates peers
- No rate limiting (D-02) — mesh-only exposure limits attack surface
- Unbounded EventStore (D-03) — production uses SQLite

**Future hardening opportunities:**
- mTLS on gRPC (defense in depth)
- Event hash chain / signed audit entries (non-repudiation)
- Tower rate limiting middleware
- EventStore retention policy
- `zeroize` for in-memory secrets
- Nostr identity binding to kith Ed25519 key
