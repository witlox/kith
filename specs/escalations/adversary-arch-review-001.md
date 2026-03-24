> All findings in this report have been resolved. See git history for fixes.

# Adversary Architecture Review — Findings Report

**Date:** 2026-03-23
**Mode:** Architecture review (pre-implementation)
**Scope:** All specs + architecture documents

---

## F-01: Trust model is implicit — no credential format specified

**Severity: Critical**

The gRPC proto passes `user_identity` as a plain string. The daemon interfaces show `PolicyEvaluator::evaluate(&self, identity: &str, scope: &Scope, action: &Action)`. But nowhere in the specs or architecture is it defined:

- What a credential actually *is* (JWT? mTLS cert? pre-shared key? SSH key signature?)
- How trust is established between kith-shell and kith-daemon
- How the daemon *validates* a credential (what it checks, what it trusts)
- How credentials are provisioned (first-time setup, key distribution)

INV-SEC-1 says "every request must carry a valid credential" but the architecture doesn't define what "valid" means. The `ExecRequest.user_identity` field is a string — an implementer could put anything in there, including an unverified username.

**Impact:** Without a specified credential format, the implementer will either invent an ad-hoc scheme (likely insecure) or punt the problem.

**Recommendation:** The architect should define: credential format (recommend: Ed25519 signed challenge-response or mTLS), trust establishment (recommend: TOFU — trust on first use — with manual key verification), and credential lifecycle (issuance, rotation, revocation). Doesn't need to be as complex as pact's OIDC — but it needs to exist.

---

## F-02: Scope is self-asserted in the gRPC request

**Severity: Critical**

`ExecRequest` has `string scope = 3` — the *caller* declares their own scope. The daemon's `PolicyEvaluator` receives this scope and checks it. But nothing prevents a malicious caller from asserting `scope = "ops"` when they should only have `"viewer"`.

The `MachinePolicy.users` map maps identity→scope, which is correct. But the daemon interface shows the scope coming from the *request*, not from the policy lookup.

**Impact:** A caller who knows a valid identity string could escalate privileges by self-asserting ops scope.

**Recommendation:** Remove scope from gRPC request messages. The daemon should look up the caller's scope from `MachinePolicy.users` based on the authenticated identity. Scope is a property of the identity on this machine, not a request parameter.

---

## F-03: INV-DAT-2 contradicted by sync design

**Severity: High**

INV-DAT-2 says "content stays at origin — CRDT syncs metadata and pointers." But the cr-sqlite schema in sync-interfaces.md has `detail TEXT` and `metadata TEXT` columns that sync to all peers. The Event struct has `detail: String` and `metadata: serde_json::Value` which contain actual content (command text, error messages, file paths).

The design conversation's intent was that *sensitive content* stays at origin with only pointers synced. But the current architecture syncs full event content to all peers. An exec event contains the command that was run. A drift event contains the file path. These sync everywhere.

**Impact:** Sensitive operational data (commands, paths, error details) replicates to all mesh members regardless of the event's scope tag. Scope filtering happens at query time, not at sync time. A compromised peer has access to all synced content.

**Recommendation:** Either: (a) accept that content syncs everywhere and update INV-DAT-2 to reflect reality (content syncs, access is filtered at query time), or (b) split events into a synced metadata portion (id, machine, category, timestamp, scope) and a content portion that stays at origin and is fetched on demand. Option (a) is simpler and likely appropriate for a personal/small-team mesh where all machines are trusted.

---

## F-04: Nostr mesh identifier is security-critical but unspecified

**Severity: High**

The mesh identifier in mesh-interfaces.md is described as a "pre-shared secret" that limits peer discovery. It's a tag on Nostr events. But:

- Nostr events are *public*. Anyone monitoring the relay can see events tagged with your mesh identifier.
- The mesh identifier is a string in config (`identifier = "my-mesh-2026"`). If it's guessable, an attacker can discover your machines' WireGuard endpoints and public keys.
- There's no authentication of Nostr events beyond the Nostr keypair. The mesh identifier doesn't prove membership — it's just a filter.

**Impact:** An attacker who knows (or guesses) the mesh identifier can: (1) discover all mesh members' public IPs and WireGuard keys, (2) publish fake peer events to inject a rogue machine into the mesh discovery.

**Recommendation:** (a) Encrypt the Nostr event content with a symmetric key derived from the mesh identifier (so observers can't read the WireGuard keys). (b) The WireGuard allowed-peers list should be configured statically or verified out-of-band on first connection (TOFU), not blindly accepted from Nostr. (c) Document the threat model: Nostr provides discovery, not authentication. WireGuard key verification is the trust boundary.

---

## F-05: DriftVector magnitude formula doesn't compute Euclidean norm

**Severity: Medium**

The `DriftVector::magnitude()` function sums squared weighted values but doesn't take the square root:

```rust
pub fn magnitude(&self, weights: &DriftWeights) -> f64 {
    (self.files * weights.files).powi(2)
        + (self.services * weights.services).powi(2)
        + (self.network * weights.network).powi(2)
        + (self.packages * weights.packages).powi(2)
}
```

This returns the squared magnitude, not the magnitude. Pact's implementation does the same (returns sum of squares). This is *consistent* but the name `magnitude` is misleading — it returns a squared distance. The drift-detection.feature says "drift magnitude is 4.0" for 2 files + 1 service, but with default weights (files=1.0, services=2.0): `(2*1)^2 + (1*2)^2 = 4 + 4 = 8`, not 4.

**Impact:** The feature spec scenario gives the wrong expected value (4.0 vs 8.0), or the formula doesn't match the intent. Inconsistency between spec and implementation will confuse the implementer.

**Recommendation:** Decide: is magnitude the squared norm (consistent with pact, cheaper to compute, fine for comparison) or the Euclidean norm (sqrt, more intuitive)? Fix the feature spec's expected value to match whichever is chosen. Document the choice.

---

## F-06: Commit window "atomicity" for multiple changes is underspecified

**Severity: Medium**

commit-windows.feature has: "Multiple pending changes committed atomically — Given pending changes exist for file-a.py and file-b.py, When the user types commit, Then both are committed atomically."

But `CommitWindowManager::commit()` takes a single `pending_id`. There's no batch commit API. The feature implies all pending changes commit together, but the trait interface only supports one at a time.

Also: what happens if the first commit succeeds but the second fails (e.g., file was deleted between apply and commit)? "Atomically" implies all-or-nothing, but the architecture doesn't provide a transaction mechanism across multiple pending changes.

**Impact:** The implementer has no way to implement the atomic multi-change commit scenario with the current interface.

**Recommendation:** Either: (a) add a `commit_all(&mut self) -> Result<(), KithError>` to CommitWindowManager that commits all pending changes atomically (overlayfs makes this natural — one overlay per commit-set, not per-change), or (b) weaken the feature spec to "committed sequentially" and accept that partial failure is possible. Option (a) is better and aligns with how overlayfs works.

---

## F-07: Clock skew across mesh members not addressed

**Severity: Medium**

Events have `timestamp: chrono::DateTime<chrono::Utc>`. Events from different machines are ordered by timestamp. But there's no NTP requirement, no clock skew tolerance, and no vector clock or logical clock.

If machine A's clock is 30 seconds ahead of machine B's, events will appear out of causal order. The agent asking "what happened in the last 5 minutes?" could miss events or include wrong ones.

**Impact:** Incorrect temporal reasoning by the agent. For operational queries this is usually tolerable (events are roughly ordered). For commit window expiry, clock skew between the machine running the daemon and the machine running the shell could cause unexpected early/late expiry.

**Recommendation:** (a) Document the assumption that mesh members have reasonably synced clocks (NTP). Add to assumptions.md. (b) Commit window expiry should be tracked by the daemon's local clock only — never compared across machines. (c) Consider adding a Lamport timestamp alongside wall-clock time for causal ordering within the event stream.

---

## F-08: No scenario for Nostr replay attacks or stale peer events

**Severity: Medium**

Nostr events are signed and timestamped. But mesh-networking.feature has no scenario for: an attacker replaying an old peer event with a stale endpoint. If a machine's IP changes and an attacker replays the old Nostr event, other peers might try to connect to the old (potentially attacker-controlled) IP.

Parameterized replaceable events (kind 30078) mitigate this — newer events replace older ones. But an attacker who controls a relay could serve the old event to new subscribers.

**Impact:** Potential misdirection of WireGuard tunnel setup. WireGuard's cryptographic handshake protects against connecting to a machine that doesn't hold the correct private key, so the actual risk is denial-of-service (tunnels point at wrong IP) rather than interception.

**Recommendation:** (a) WireGuard key verification is the actual trust boundary — document this explicitly. (b) Add a scenario to mesh-networking.feature: "Given a stale peer event is served by a relay, Then the WireGuard handshake fails, And the peer is marked unreachable." (c) Consider a maximum age for peer events — ignore events older than N minutes.

---

## F-09: macOS commit window implementation gap

**Severity: Medium**

ADR-002 says macOS uses "copy-based snapshots instead of overlayfs" for commit windows. But:

- The commit-windows.feature exclusively references overlayfs ("applied via overlayfs overlay", "overlay is merged", "overlay is discarded")
- No feature scenario covers macOS-specific behavior
- The `ContainmentConfig` has `overlayfs: bool` but no config for the copy-based fallback

**Impact:** Implementer has clear guidance for Linux but ambiguous guidance for macOS. macOS is explicitly the dev box platform — commit windows on local file edits are a core developer workflow.

**Recommendation:** (a) Add macOS scenarios to commit-windows.feature (e.g., "Given kith shell is running on macOS, When the agent edits a file, Then the change is applied via file copy snapshot"). (b) Specify the copy-based mechanism: copy original to `.kith-backup/`, edit in place, rollback restores from backup. (c) Document limitations: copy-based snapshots don't handle directory trees as cleanly as overlayfs.

---

## F-10: Input classification (pass-through vs. intent) is unspecified

**Severity: Medium**

INV-OPS-1 says pass-through has zero latency. The module map says kith-shell has a `classify/` submodule. But no spec, feature file, or architecture document defines *how* input is classified.

- What makes `ls -la` pass-through but `find all Python files that import requests` intent?
- Is classification regex-based? First-word lookup? Model-based (which would violate the zero-latency invariant)?
- What about ambiguous input like `git push origin main` (could be literal or intent)?
- The escape hatch (`run:` prefix) is defined but the default classifier is not.

**Impact:** The implementer must invent the classification heuristic. A bad heuristic either sends too much to the model (latency) or too little (the agent never activates).

**Recommendation:** Escalate to analyst or architect to define classification rules. Suggested approach: if input starts with a known command (from PATH scan), treat as pass-through. If it looks like natural language (no leading command match), treat as intent. `run:` forces pass-through. `ask:` (or similar) could force intent. Document in a feature file.

---

## F-11: Audit events written "before returning" may lose data on crash

**Severity: Low**

The enforcement map says INV-SEC-4 is enforced by "event write in handler code path." But cr-sqlite writes to a local SQLite database. If the daemon crashes between executing the command and writing the audit event, the action happened but wasn't audited.

**Impact:** Audit trail has a theoretical completeness gap on daemon crash. For personal/small-team infrastructure this is likely acceptable.

**Recommendation:** Write the audit event *before* executing the action (intent audit), then write the outcome event after. Or accept the gap and document it in failure-modes.md.

---

## F-12: InferenceBackend trait — `complete()` has no cancellation mechanism

**Severity: Low**

The `complete()` method returns a `Stream`. But there's no way to signal cancellation to the backend if the user presses Ctrl+C mid-response. The stream can be dropped, but HTTP connections may linger.

**Impact:** Resource leak on cancellation. For self-hosted inference, abandoned requests may continue consuming GPU time.

**Recommendation:** Consider adding a `CancellationToken` parameter to `complete()`, or document that implementations should use `reqwest`'s built-in timeout/abort on drop.

---

## Summary

| Severity | Count | IDs |
|----------|-------|-----|
| Critical | 2 | F-01, F-02 |
| High | 2 | F-03, F-04 |
| Medium | 5 | F-05, F-06, F-07, F-08, F-09 |
| Low | 2 | F-11, F-12 |

**Blocking implementation:** F-01 and F-02 must be resolved before the implementer starts kith-daemon. Without a credential format and with self-asserted scopes, the security model is broken.

**Should resolve before implementation:** F-03 (clarify INV-DAT-2), F-04 (Nostr threat model), F-06 (batch commit API), F-10 (input classification).

**Can resolve during implementation:** F-05, F-07, F-08, F-09, F-11, F-12.

---

## Arch Mode Checklist Assessment

| Check | Status | Notes |
|-------|--------|-------|
| Every invariant has enforcement point | **Pass** | enforcement-map covers all 17 |
| InferenceBackend doesn't leak model assumptions | **Pass** | ThinkingDelta is optional, no model enums |
| Trust model is explicit | **Fail** | F-01: credential format undefined |
| Containment model actually contains | **Pass with caveat** | F-09: macOS path underspecified |
| Sync handles partition/skew/concurrent/disappear | **Pass with caveat** | F-07: clock skew unaddressed |
| Vector index handles stale/version/growth | **Pass** | INV-CON-4, INV-DAT-3 cover this |
| Nostr handles relay unavail/replay/impersonation | **Partial** | F-04, F-08: threat model gaps |
| Commit window handles expiry/concurrent/emergency | **Partial** | F-06: multi-commit atomicity gap |
| macOS limitations documented with fallbacks | **Partial** | F-09: feature specs Linux-only |
| No model-specific logic outside InferenceBackend | **Pass** | Verified across all specs |
