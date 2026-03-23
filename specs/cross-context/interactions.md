# Cross-Context Interactions

## Component Communication Map

```
kith-shell ──gRPC──→ kith-daemon (remote exec, query, apply, commit, rollback)
kith-shell ──SQL───→ kith-sync/cr-sqlite (fleet_query, manifest reads)
kith-shell ──API───→ kith-state/vector index (retrieve)
kith-shell ──PTY───→ local bash (pass-through and model-composed commands)
kith-shell ──trait─→ InferenceBackend (any LLM provider)

kith-daemon ──SQL──→ kith-sync/cr-sqlite (write events, read peer state)
kith-daemon ──WG───→ kith-mesh/WireGuard (encrypted transport)

kith-mesh ──Nostr──→ Nostr relays (signaling)
kith-mesh ──WG────→ peer mesh members (direct P2P tunnels)

kith-sync ──SQL───→ local SQLite (read/write events)
kith-sync ──TCP───→ peer sync layers (cr-sqlite delta replication)

kith-state ──SQL──→ kith-sync/cr-sqlite (subscribe to events)
kith-state ──PTY──→ ingest daemon (local observations)
```

## Data Flow Boundaries

- **Content stays at origin** (INV-DAT-2)
- **Credentials never enter model context** (INV-SEC-3)
- **Policy enforced at daemon** (INV-SEC-2)
- **Model accessed only via InferenceBackend** (INV-OPS-5)
