# ADR-002: Linux Containment with macOS Graceful Degradation

## Status: Accepted

## Context

The agent executes commands — both locally and remotely. Containment limits blast radius when things go wrong. Pact uses cgroups v2 for per-service resource isolation and acts as PID 1 on diskless nodes. Kith is lighter: background service, not init system.

macOS dev boxes need to participate in the mesh but lack Linux containment primitives (cgroups, overlayfs, namespaces).

## Decision

- **Linux machines**: kith-daemon uses cgroups v2 for resource limits on agent-spawned processes, and overlayfs for transactional file changes (commit windows). Feature-gated behind `containment` cargo feature.
- **macOS machines**: kith-daemon runs without containment. Commit windows for file changes use copy-based snapshots instead of overlayfs. No cgroup resource limits. Remote execution still has policy enforcement.
- **Containment is defense-in-depth**, not the primary safety mechanism. Policy enforcement (INV-SEC-2) is the first line. Containment is the second.

## Consequences

**Positive:**
- Linux machines get OS-level blast radius control
- macOS machines work as first-class mesh members for agent use
- Incremental: start without containment, add when needed

**Negative:**
- macOS has weaker isolation — acceptable because macOS is the agent side (issuing commands), not the execution target for sensitive operations
- Two code paths for file transactions (overlayfs vs. copy-based)

## Validation

Test commit/rollback on both Linux (overlayfs) and macOS (copy-based). Verify same semantic behavior. Test cgroup limits under load on Linux.
