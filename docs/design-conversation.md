# Design Conversation Summary

Source material for the analyst profile. Captures the evolution and key decisions.

## Evolution

1. **Fork OpenCode, swap model** → rejected (too much inherited complexity)
2. **Fork Gemini CLI** → better base (Qwen already proved the path) but still a coding assistant
3. **Replace the shell** → the agent IS the shell; Unix tools stay, agent orchestrates
4. **Distributed mesh** → multiple machines, unified context, local+remote execution
5. **Self-managed memory via vector space** → embed everything, retrieval determines relevance
6. **CRDT-synced state** → cr-sqlite, eventually consistent, partition-tolerant
7. **Pact patterns, not pact code** → borrow drift/commit/audit/capability patterns for small fleet
8. **No tool wrappers** → model uses cat/grep/sed directly; ingest daemon + overlayfs handle observability and rollback
9. **Nostr + WireGuard** → no coordination server; signed events for discovery, P2P tunnels for transport
10. **Model-agnostic** → InferenceBackend trait; bootstrap with hosted APIs, self-host when ready
11. **Named "Kith"** → your machines, your familiar territory

## Key Decisions

- **D1:** Unix tools are the tools. No wrappers. ~2K system prompt vs Claude Code's ~27K.
- **D2:** Native tools only for new capabilities: remote, fleet_query, retrieve, apply/commit/rollback, todo.
- **D3:** Containment at OS level (cgroups, overlayfs, namespaces), not application level.
- **D4:** Pact patterns adapted: acknowledged drift, commit windows, immutable audit, capability reporting, no-SSH.
- **D5:** Content stays at origin. CRDT syncs pointers, not content. Retrieval is authenticated.
- **D6:** Thin system prompt because infrastructure handles safety.
- **D7:** Graceful degradation at every layer.
- **D8:** Model-agnostic via InferenceBackend trait. No model-specific logic outside trait implementations.

## Open Questions for Analyst

1. How should the input classifier work? Heuristics or model-based?
2. Exact commit window UX for local vs. remote changes?
3. Tool registry handling of conflicting versions across machines?
4. Vector index retention policy?
5. Multi-user scenarios on the same machine?
6. Multi-machine plans when one target is unreachable — partial or all-or-nothing?
7. Interactive programs (vim, top, less) via pass-through?
8. InferenceBackend selection UX — how does the user switch models mid-session?
9. System prompt templating per backend — how are model-specific prompt tweaks managed?
