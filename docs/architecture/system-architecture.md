# System Architecture

## Layer Model

```
┌─────────────────────────────────────────────────┐
│  Intent Interface    kith shell (PTY + LLM)     │
├─────────────────────────────────────────────────┤
│  Reasoning           Any LLM with tool calling  │
├─────────────────────────────────────────────────┤
│  State Layer         cr-sqlite CRDT + vector    │
├─────────────────────────────────────────────────┤
│  Mesh                WireGuard + Nostr + gRPC   │
├─────────────────────────────────────────────────┤
│  Containment         cgroups v2 + overlayfs     │
├─────────────────────────────────────────────────┤
│  Platform            Linux (full) / macOS (agent)│
└─────────────────────────────────────────────────┘
```

## Crate Map

| Crate | Responsibility |
|-------|---------------|
| `kith-common` | Shared kernel: types, errors, config, traits (InferenceBackend), Ed25519 credentials |
| `kith-mesh` | Peer registry, WireGuard tunnel management, Nostr signaling |
| `kith-daemon` | gRPC service: exec, policy, audit, drift detection, commit windows, containment |
| `kith-shell` | PTY wrapper, LLM inference, tool dispatch, agent loop, conversation context |
| `kith-sync` | Event store (in-memory + SQLite), cr-sqlite CRDT replication |
| `kith-state` | Keyword retrieval, vector index, hybrid search, embedding backends |

## Data Flow

1. User types input into kith shell
2. `InputClassifier` determines: pass-through (PATH match) or intent
3. Pass-through → executed directly via PTY (zero latency)
4. Intent → sent to LLM with conversation context + native tool definitions
5. LLM responds with text or tool calls
6. Tool calls dispatched: `remote` → daemon gRPC, `retrieve` → hybrid search, `apply` → commit window
7. All actions audited to immutable event log
8. Events synced across mesh via CRDT replication

## Key Invariants

- **INV-OPS-1:** Every state-mutating action is audited before execution
- **INV-OPS-2:** Shell degrades to pass-through when inference is unavailable
- **INV-OPS-3:** Local operations continue during mesh partition
- **INV-OPS-4:** The model uses standard Unix tools — no wrappers
- **INV-SEC-1:** All remote calls are Ed25519-authenticated with server-determined scope
