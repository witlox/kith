# Failure Modes

## FM-1: LLM Inference Unavailable

**Severity:** Medium

**Behavior:** Shell degrades to pass-through mode. All input is executed as bash commands. Agent resumes when connectivity returns.

**Detection:** Timeout >5 seconds or connection refused from backend.

## FM-2: LLM Hallucinates Destructive Command

**Severity:** High

**Mitigations:**
- Commit windows: changes are pending, not immediate
- Policy enforcement: daemon rejects unauthorized actions
- Containment: overlayfs/copy transactions isolate changes
- Audit: every action recorded before execution

## FM-3: Mesh Partition

**Severity:** Low

**Behavior:** Local operations continue. Remote tools return "unreachable." Event sync pauses and resumes on reconnection. cr-sqlite CRDT merge resolves conflicts automatically.

## FM-4: Daemon Unreachable

**Severity:** Low

**Behavior:** Shell falls back to local execution. `remote` tool returns error. `apply`/`commit`/`rollback` unavailable. `retrieve` and `fleet_query` use local event store only.

## FM-5: Event Store Corruption

**Severity:** Medium

**Mitigation:** In-memory store is ephemeral — restart recovers from SQLite. SQLite uses WAL mode for crash safety. CRDT merge can reconstruct from peer replicas.

## FM-6: Credential Compromise

**Severity:** High

**Mitigation:** Remove the compromised public key from all daemon policy files. TOFU model means no CA to revoke — removal is immediate and per-machine.

## FM-7: Nostr Relay Unavailable

**Severity:** Low

**Behavior:** Existing WireGuard tunnels continue working. New peer discovery paused. Multiple relays configured for redundancy.
