# Shell Design

## Overview

The kith shell is a PTY wrapper with LLM inference. It replaces the traditional terminal workflow with an agent that can understand intent and operate across the mesh.

## Input Classification

Every line of input is classified before processing:

| Classification | Trigger | Latency |
|---------------|---------|---------|
| **PassThrough** | Command found in `$PATH` | ~0ms (direct exec) |
| **Intent** | Not a known command | LLM round-trip |
| **Escape** | `run:` prefix | ~0ms (forced bash) |

The classifier scans `$PATH` at startup and builds a set of known commands. This means `ls`, `git`, `docker`, etc. execute instantly without touching the LLM.

## Agent Loop

```
input → classify → [PassThrough] → exec via PTY → output
                 → [Intent] → LLM complete → [Text] → display
                                            → [ToolCall] → dispatch → output
```

The agent maintains:
- **ConversationContext** — message history with automatic compaction
- **EventStore** — local operational state (synced from daemon)
- **HybridRetriever** — keyword + vector search over events
- **EmbeddingBackend** — bag-of-words (local) or API-based

## PTY Integration

The shell uses `nix::pty::openpty` for a proper PTY on both macOS and Linux. Interactive line editing is provided by `rustyline` with persistent history.

## Daemon Sync

Before `retrieve` or `fleet_query` tool calls, the agent syncs events from connected daemons:

1. Call `fetch_events()` via Events gRPC stream
2. Index operational events (Exec, Drift, Apply, Commit, Rollback) into vector index
3. Merge all events into local EventStore (deduplicated by ID)
4. Search the enriched local state
