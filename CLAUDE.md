# Kith — Project Context

## What This Is

An intent-driven distributed shell: a reasoning layer (LLM) over a mesh of machines, where the agent uses standard Unix tools directly and only adds native tools for genuinely new capabilities (remote execution, fleet queries, vector retrieval, transactional changes).

## Language and Stack

- **Rust** (stable toolchain) for kith-daemon, kith shell PTY wrapper, sync layer
- **Protocol Buffers** for kith-daemon gRPC interface
- **cr-sqlite** for CRDT-synchronized state
- **WireGuard** for mesh transport
- **Nostr** for peer discovery/signaling
- **Model-agnostic**: any LLM with tool calling via OpenAI-compatible API (hosted or self-hosted)

## Key Design Decisions

1. **No tool wrappers for Unix commands.** The model uses cat, grep, sed, git directly. The ingest daemon captures everything. Overlayfs provides rollback. No "Read" or "Edit" tools.
2. **Pact patterns, not pact code.** We borrow acknowledged drift, commit windows, immutable audit, capability reporting, and no-SSH remote access. We don't depend on pact crates.
3. **Nostr for signaling, WireGuard for transport.** No coordination server. Machines discover each other via signed events on public Nostr relays.
4. **Vector space as self-managed memory.** The model retrieves its own context from an embedded vector index over operational state. L1/L2/L3 boundaries emerge from retrieval relevance, not static design.
5. **macOS is agent-only.** Mac dev boxes run kith shell and connect to the mesh. Linux machines run the full kith-daemon with containment (cgroups, overlayfs, namespaces).
6. **Model-agnostic inference.** The LLM backend is behind a trait (`InferenceBackend`). Any model that supports tool calling and streaming works. Bootstrap with hosted APIs (Claude, GPT, Gemini), self-host with open-weight models when ready.

## Repository Layout

```
kith/
├── CLAUDE.md                    # This file (project context, always loaded)
├── README.md
├── Cargo.toml                   # Workspace root
├── .claude/
│   ├── CLAUDE.md                # Workflow, active profile swapped per phase
│   ├── analyst.md
│   ├── architect.md
│   ├── adversary.md
│   ├── implementer.md
│   └── integrator.md
├── specs/
│   ├── domain-model.md
│   ├── ubiquitous-language.md
│   ├── invariants.md
│   ├── assumptions.md
│   ├── failure-modes.md
│   ├── features/
│   ├── architecture/
│   ├── cross-context/
│   ├── escalations/
│   └── integration/
├── crates/
│   ├── kith-common/
│   ├── kith-mesh/
│   ├── kith-daemon/
│   ├── kith-shell/
│   ├── kith-sync/
│   └── kith-state/
├── proto/kith/daemon/v1/
├── config/
├── docs/decisions/
└── examples/
```

## Build and Test

```bash
cargo build --workspace
cargo test --workspace
just check          # clippy + fmt + test
```

## Conventions

- All specs use Gherkin (.feature) for behavioral specifications
- ADRs in docs/decisions/ follow MADR format
- Error types use thiserror, no anyhow in library crates
- All public APIs documented with rustdoc
- gRPC services defined in proto/ and generated via tonic-build
- LLM backend is accessed exclusively through the InferenceBackend trait
