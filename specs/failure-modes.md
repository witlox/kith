# Failure Modes

## FM-1: LLM inference unavailable
**Severity:** Medium — **Mitigation:** Graceful degradation to bash (INV-OPS-2). Detect timeout >5s, fall back to pass-through.

## FM-2: LLM hallucinates destructive command
**Severity:** High — **Mitigation:** Commit windows (pending, not immediate), policy enforcement (daemon rejects), containment (overlayfs isolates).

## FM-3: Mesh partition
**Severity:** Low — **Mitigation:** Local ops continue (INV-OPS-3). Remote tools return "unreachable." Sync resumes on reconnection.

## FM-4: cr-sqlite divergence after long partition
**Severity:** Medium — **Mitigation:** Batched delta sync. Async vector index processing. Freshness indicators on stale data.

## FM-5: Vector index returns irrelevant context
**Severity:** Medium — **Mitigation:** Hybrid retrieval (vector + structured). Agent falls back to live observation.

## FM-6: Commit window expires unintentionally
**Severity:** Medium — **Mitigation:** TUI notification, audible alert, "extend" command. Track expiry rates.

## FM-7: Nostr relay unavailability
**Severity:** Low — **Mitigation:** 5+ diverse relays. Cache last-known endpoints. Fallback to static WireGuard config.

## FM-8: Kith-daemon crash
**Severity:** Medium — **Mitigation:** systemd auto-restart. Mesh detects absence via heartbeat. Agent reports "unreachable."

## FM-9: Credential compromise
**Severity:** Critical — **Mitigation:** Short-lived credentials. Revocation via Nostr signaling. Audit trail preserves evidence.

## FM-10: Context window exhaustion
**Severity:** Medium — **Mitigation:** Compaction at ~80% budget. Vector index provides retrieval of forgotten context.

## FM-11: Embedding model version mismatch
**Severity:** Medium — **Mitigation:** Version recorded in metadata. Same-version distances only. Model update triggers re-indexing.

## FM-12: InferenceBackend returns unexpected format
**Severity:** Medium — **Mitigation:** Each backend implementation validates response shape. Malformed responses logged and retried once. If retry fails, surface error to user, don't pass garbage to tool dispatch.
