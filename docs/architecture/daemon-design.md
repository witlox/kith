# Daemon Design

## Overview

`kith-daemon` is a lightweight gRPC service running on each machine in the mesh. It provides authenticated remote execution, state observation, drift detection, audit logging, and event sync.

## gRPC Interface

| RPC | Purpose |
|-----|---------|
| `Exec` | Stream command execution output |
| `Query` | Machine state snapshot |
| `Apply` | Start a change with commit window |
| `Commit` | Finalize a pending change |
| `Rollback` | Revert a pending change |
| `Events` | Stream audit log entries |
| `Capabilities` | Report installed tools, resources, services |
| `ExchangeEvents` | Bidirectional event sync for CRDT replication |

## Policy Enforcement

Every RPC is authenticated via Ed25519 credentials (ADR-006). The `PolicyEvaluator` checks:

1. Signature validity (Ed25519 over pubkey + timestamp + request hash)
2. Timestamp freshness (replay protection)
3. Scope authorization (Ops vs Viewer, per public key)
4. Action category allowlisting

## Commit Windows

Changes via `Apply` enter a pending state with a configurable timeout. The agent (or operator) must explicitly `Commit` or `Rollback`. If the window expires, the change is automatically rolled back.

## Drift Detection

Observers (`FileObserver`, `ProcessObserver`) detect changes to monitored paths and processes. Drift is categorized into four types: Added, Removed, Modified, Permissions. Events are written to the audit log and synced across the mesh.

## Audit Log

Immutable, append-only event log. Every action is recorded before execution. Supports write-through to external sinks (SQLite, remote). The audit log is the source of truth for the Events RPC.
