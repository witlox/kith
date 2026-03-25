# Ubiquitous Language

Each term has exactly one meaning. No synonyms.

| Term | Definition |
|------|-----------|
| **kith shell** | The user's terminal interface. PTY wrapper + LLM inference + tool dispatch. |
| **kith-daemon** | Background service on a mesh member. Provides remote exec, state observation, audit. |
| **InferenceBackend** | Trait abstracting LLM access. Implementations exist for different providers. The model is never accessed except through this trait. |
| **mesh** | The set of machines running kith-daemons, connected via WireGuard tunnels. |
| **mesh member** | A single machine participating in the mesh. |
| **peer** | Another mesh member, from the perspective of a given machine. |
| **pass-through** | Input sent directly to bash without LLM involvement. |
| **intent** | Input routed to the LLM for reasoning and execution. |
| **escape hatch** | The `run:` prefix forcing pass-through. |
| **native tool** | A tool in kith shell's own API (remote, fleet_query, retrieve, apply, commit, rollback, todo). |
| **commit window** | Time-bounded period for committing or rolling back a pending change. |
| **pending change** | A state change applied but not yet committed. |
| **drift** | Measured difference between expected and actual state. |
| **capability report** | Structured data describing what a machine can do. |
| **tool registry** | Local index of available tools, discovered by scanning PATH. Contains name, path, category, and optional version. |
| **tool category** | Functional grouping for a discovered tool: vcs, container, language, build, server, database, editor, network, monitoring, other. |
| **tool scan** | The process of walking PATH directories to discover available executables and their versions. |
| **manifest** | Merged view of all mesh members' capabilities and state summaries. |
| **ingest** | Capturing operational events for indexing. |
| **retrieval** | Querying the vector index for relevant context. |
| **signaling** | Exchanging connection metadata via Nostr. |
| **audit trail** | Immutable append-only log of all actions. |
| **policy** | Per-machine, per-user access rules enforced by kith-daemon. |
| **scope** | The set of permissions for a user on a machine. |
| **compaction** | Summarizing conversation history to fit the context window. |
