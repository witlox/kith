# ADR-001: cr-sqlite CRDT for State Synchronization

## Status: Accepted

## Context

Kith needs to synchronize operational state (events, drift, capabilities) across a mesh of 3-10 machines. The sync must handle network partitions gracefully and not require a central coordinator.

Pact uses Raft consensus for its journal — appropriate for 10,000+ HPC nodes needing strong consistency. Kith's personal/small-team fleet has different requirements: eventual consistency is acceptable, partition tolerance is essential, and no machine should be special (no leader election).

## Decision

Use **cr-sqlite** (CRDTs on SQLite) for state synchronization.

- Each kith-daemon writes events to a local SQLite database
- cr-sqlite adds CRDT merge semantics to selected tables
- Peers exchange deltas over the WireGuard mesh on a configurable interval (default: 5s)
- Convergence is guaranteed by CRDT add-wins semantics: given connectivity, all peers converge to the union of all events

## Consequences

**Positive:**
- No coordinator, no leader election, no quorum requirements
- Each machine operates fully independently during partition (INV-OPS-3)
- SQLite is battle-tested, embedded, zero-ops
- SQL queryability for fleet_query
- Delta sync is bandwidth-efficient for small event payloads

**Negative:**
- Eventual consistency means queries may return slightly stale data (acceptable for operational state)
- cr-sqlite is newer than SQLite itself — less battle-tested at the CRDT layer
- No strong ordering guarantees across machines (events are ordered by timestamp, not causal order)

**Trade-offs vs. alternatives:**
- Raft: too heavy for 3-10 machines, requires leader, adds complexity without proportional benefit
- Custom CRDT over libp2p: more control, much more work, no SQL queryability
- Plain SQLite with manual merge: fragile, conflict-prone

## Validation

cr-sqlite CRDT merge tested with: simultaneous writes on partitioned peers, delta sync after 24h partition, 100K events/day load test (ASM-STO-1).
