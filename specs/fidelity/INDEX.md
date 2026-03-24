# Fidelity Index

Last scan: 2026-03-24 (post-stub elimination)

## Summary

| Metric | Count |
|--------|-------|
| Feature files | 8 |
| Scenarios | 59 (all pass) |
| Total step functions | 214 |
| Real assertions | 175 (82%) |
| Infrastructure markers | 39 (18%) |
| Empty stubs | **0** |
| Unit/integration/e2e tests | 271 |
| Container tests | 7 |
| Local model tests | 2 |

## Infrastructure Markers by Category

These steps have explicit `// INFRASTRUCTURE:` or `// VERIFIED:` comments
explaining why they can't be asserted at BDD level and where they ARE verified.

| Category | Count | Reason |
|----------|-------|--------|
| Terminal rendering | 6 | Requires PTY/terminal — verified in pty tests |
| OS-level containment | 6 | overlayfs/backup — verified in containment tests |
| Network infrastructure | 7 | WireGuard tunnels, DERP relay, NAT — verified in container tests |
| Structural invariants | 12 | INV-OPS-5 model-agnostic, code structure — verified by grep/review |
| Streaming/protocol | 5 | gRPC streaming, SSE parsing — verified in e2e tests |
| External service | 3 | Nostr relays, process lifecycle — verified in container/e2e |

## Unit Test Coverage by Crate

| Crate | Tests | Key Areas |
|-------|-------|-----------|
| kith-common | 39 | Types, crypto, drift, events, policy, inference trait |
| kith-daemon | 53 | Policy auth, drift, commit, audit, exec, gRPC, observer, containment |
| kith-shell | 68 | Classifier, tools, prompt, context, agent, PTY, backends |
| kith-mesh | 34 | Peer registry, signaling, WG, IP allocation, manager |
| kith-sync | 23 | In-memory + SQLite store, filtering, merge |
| kith-state | 25 | Keyword + vector + hybrid retrieval, embedding |
| kith-e2e | 29 | Full flow, chaos, drift-sync, model swap, PTY |
| **Total** | **271** | |
