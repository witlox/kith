# Dependency Graph

Module dependencies with justification. Each dependency traces to a spec requirement.

---

## Compile-Time Dependencies

```
kith-daemon ──▶ kith-common    (shared types: Event, DriftVector, CapabilityReport, Scope)
kith-daemon ──▶ kith-sync      (cr-sqlite event writes, sync state reads)
kith-daemon ──▶ kith-mesh      (mesh membership for peer awareness)
kith-shell  ──▶ kith-common    (InferenceBackend trait, shared types, tool definitions)
kith-sync   ──▶ kith-common    (Event type, sync config)
kith-state  ──▶ kith-common    (Event type, retrieval query/response types)
kith-state  ──▶ kith-sync      (event subscription for embedding pipeline)
kith-mesh   ──▶ kith-common    (MeshConfig, PeerInfo types)
```

## Runtime Dependencies

```
kith-shell ──gRPC──▶ kith-daemon   (Exec, Query, Apply, Commit, Rollback)
kith-shell ──SQL───▶ kith-sync     (fleet_query reads from local cr-sqlite)
kith-shell ──API───▶ kith-state    (retrieve: vector search)
kith-shell ──PTY───▶ local bash    (pass-through and model-composed commands)
kith-shell ──HTTP──▶ LLM endpoint  (InferenceBackend: any provider)

kith-daemon ──SQL──▶ kith-sync     (write events, read peer state)
kith-daemon ──WG───▶ kith-mesh     (encrypted transport to peers)

kith-mesh ──Nostr──▶ Nostr relays  (signaling: peer discovery, key exchange)
kith-mesh ──WG────▶ peer machines  (direct P2P WireGuard tunnels)

kith-sync ──TCP───▶ peer sync      (cr-sqlite delta replication over mesh)

kith-state ──SQL──▶ kith-sync      (subscribe to event stream for embedding)
```

## Justification Table

| Dependency | Justification |
|-----------|---------------|
| daemon → common | Daemon constructs Events, DriftVectors, CapabilityReports using shared types |
| daemon → sync | Daemon writes events to cr-sqlite, reads peer state for fleet queries (INV-CON-1) |
| daemon → mesh | Daemon needs mesh membership info for peer awareness |
| shell → common | Shell uses InferenceBackend trait, tool definitions, shared types |
| shell →(gRPC) daemon | Remote exec, apply/commit/rollback, capability queries (feature specs) |
| shell →(SQL) sync | Fleet query reads from local merged cr-sqlite (state-and-retrieval.feature) |
| shell →(API) state | Semantic retrieval over operational history (state-and-retrieval.feature) |
| shell →(HTTP) LLM | InferenceBackend calls for all reasoning (INV-OPS-5) |
| sync → common | Events and config types shared across crates |
| state → common | Event types for embedding, query/response types for retrieval API |
| state → sync | Subscribes to cr-sqlite event stream as embedding source |
| mesh → common | Mesh configuration types |
| mesh →(Nostr) relays | Peer discovery and signaling (mesh-networking.feature) |
| mesh →(WG) peers | Encrypted transport (mesh-networking.feature) |

---

## Cycle Analysis

**No cycles.** Dependency graph is a DAG:
- `kith-common` is a leaf (no internal dependencies)
- `kith-mesh` depends only on `kith-common`
- `kith-sync` depends on `kith-common` and `kith-mesh` (for transport)
- `kith-state` depends on `kith-common` and `kith-sync`
- `kith-daemon` depends on `kith-common`, `kith-sync`, and `kith-mesh`
- `kith-shell` depends on `kith-common` (compile-time only; runtime connections to daemon, sync, state)

```
kith-shell (binary)
    │ compile: kith-common
    │ runtime: kith-daemon (gRPC), kith-sync (SQL), kith-state (API)
    │
kith-daemon (binary)
    ├── kith-common
    ├── kith-sync
    │     ├── kith-common
    │     └── kith-mesh
    │           └── kith-common
    └── kith-mesh
          └── kith-common

kith-state (library/service)
    ├── kith-common
    └── kith-sync
          ├── kith-common
          └── kith-mesh
                └── kith-common
```

## God Module Check

| Module | Direct Compile Dependencies | Status |
|--------|---------------------------|--------|
| kith-common | 0 internal | OK — leaf |
| kith-mesh | 1 (common) | OK |
| kith-sync | 2 (common, mesh) | OK |
| kith-state | 2 (common, sync) | OK |
| kith-daemon | 3 (common, sync, mesh) | OK |
| kith-shell | 1 (common) | OK — runtime deps only |

No module exceeds 3 direct compile-time dependencies.
