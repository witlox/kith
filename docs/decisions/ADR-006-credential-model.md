# ADR-006: Ed25519 Keypair Credentials with TOFU Trust

## Status: Accepted (resolves F-01, F-02)

## Context

The adversary review found two critical gaps:
- F-01: No credential format defined — `user_identity` was a plain string
- F-02: Scope was self-asserted by the caller, allowing privilege escalation

Kith needs authentication that is: simple (no OIDC server for a 3-10 machine mesh), self-contained (no external dependencies), and cryptographically sound.

Pact uses OIDC/mTLS because HPC environments have identity providers. Kith's personal/small-team mesh doesn't.

## Decision

### Credential Format: Ed25519 Signed Requests

Each user generates an Ed25519 keypair on first run (`kith init`). The public key is the user's identity. The private key never leaves the user's machine.

Every gRPC request carries:
1. The user's public key (identity)
2. A timestamp (to prevent replay)
3. An Ed25519 signature over `(public_key || timestamp || request_payload_hash)`

The daemon verifies:
1. Signature is valid for the given public key
2. Timestamp is within ±30 seconds of daemon's clock (replay window)
3. Public key is in the machine's `MachinePolicy.users` map

### Trust Establishment: TOFU + Manual Verification

**First connection (Trust On First Use):**
- User runs `kith trust <machine>` which connects to the daemon and exchanges public keys
- The daemon's admin must pre-add the user's public key to `MachinePolicy.users` in the daemon config
- Alternatively: `MachinePolicy.tofu = true` allows the daemon to accept unknown keys with `viewer` scope on first contact, requiring admin promotion to `ops`

**Key distribution:**
- User's public key is displayed by `kith whoami`
- Admin adds it to daemon config: `users = { "ed25519:<pubkey>" = "ops" }`
- Or: admin runs `kith-daemon add-user <pubkey> --scope ops`

### Scope is Server-Determined

Scope is **never** in the request. The daemon looks up the authenticated public key in `MachinePolicy.users` and determines scope from the policy. The gRPC request carries only the credential (pubkey + timestamp + signature), not the claimed scope.

## Proto Changes

```protobuf
message Credential {
  bytes public_key = 1;        // Ed25519 public key (32 bytes)
  int64 timestamp_unix_ms = 2; // milliseconds since epoch
  bytes signature = 3;         // Ed25519 signature (64 bytes)
}

message ExecRequest {
  string command = 1;
  Credential credential = 2;   // replaces user_identity + scope
}
```

All request messages get a `Credential` field replacing `user_identity` and `scope`.

## Consequences

**Positive:**
- No external identity provider needed
- Cryptographically strong authentication (Ed25519 is battle-tested)
- Scope is server-authoritative — no self-assertion possible
- Keys are small (32 bytes) and signatures fast
- TOFU allows zero-config onboarding for personal use

**Negative:**
- Key management is manual (no automatic rotation, revocation is "remove from config")
- Clock skew >30s breaks authentication (mitigated by F-07: document NTP assumption)
- No user-friendly identity (public keys are hex strings, not emails)

**Acceptable for scope:** This is personal/small-team infrastructure. The trust model is "I know my machines and I know my team." OIDC would be overkill. If the mesh grows beyond ~10 users, revisit.
