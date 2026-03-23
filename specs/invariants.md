# Invariants

## Security

**INV-SEC-1: No unauthenticated remote execution.** Every remote exec request must carry a valid credential. Kith-daemon rejects missing, expired, or invalid credentials.

**INV-SEC-2: Policy enforcement is at the daemon, not the model.** The LLM cannot bypass policy. Enforced in Rust, not in the system prompt.

**INV-SEC-3: The model never sees raw credentials.** Credential injection happens below the InferenceBackend's visibility.

**INV-SEC-4: Audit completeness.** Every state-changing action produces an audit entry: who, what, when, where, outcome.

**INV-SEC-5: Audit immutability.** Audit entries cannot be modified or deleted through the kith interface.

## Consistency

**INV-CON-1: CRDT convergence.** Given sufficient connectivity, all mesh members converge to the same event set.

**INV-CON-2: Commit window atomicity.** A pending change is fully committed or fully rolled back. No partial commits.

**INV-CON-3: Capability reports are eventually fresh.** Stale reports are timestamped so the agent can reason about freshness.

**INV-CON-4: Vector index is a view, not truth.** Rebuildable from source data. Loss is recoverable.

## Operational

**INV-OPS-1: Pass-through adds zero latency.** No LLM inference, no network calls for pass-through commands. <5ms overhead.

**INV-OPS-2: Inference failure degrades to bash.** If the InferenceBackend is unreachable, kith shell becomes a plain terminal.

**INV-OPS-3: Mesh partition doesn't prevent local operation.** Local exec, observation, and audit continue during partition.

**INV-OPS-4: No tool wrappers.** The agent uses standard Unix commands via PTY. No Rust-native equivalents.

**INV-OPS-5: Model-agnostic operation.** No component outside kith-shell's InferenceBackend implementations contains model-specific logic. Swapping models is a config change.

## Data

**INV-DAT-1: Events are append-only.** Add-wins OR-Set semantics. No modification.

**INV-DAT-2: Event access is scope-gated.** CRDT syncs full events across the mesh. Access to event content is filtered at query time based on the caller's scope. All mesh members are trusted at the transport level (WireGuard).

**INV-DAT-3: Embedding consistency.** All mesh members use the same embedding model version.
