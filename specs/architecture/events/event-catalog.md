# Event Catalog

All event types in the system. Events are the fundamental unit of state — written to cr-sqlite, synced across mesh, embedded for retrieval.

---

## Drift Events

| Event Type | Category | When | Metadata |
|-----------|----------|------|----------|
| `drift.file_changed` | Drift | File modified outside kith | `{ "path": "/etc/...", "change": "modified" }` |
| `drift.file_created` | Drift | File created outside kith | `{ "path": "/etc/..." }` |
| `drift.file_deleted` | Drift | File deleted outside kith | `{ "path": "/etc/..." }` |
| `drift.service_stopped` | Drift | Expected service stopped | `{ "service": "nginx", "reason": "exited" }` |
| `drift.service_started` | Drift | Unexpected service started | `{ "service": "...", "pid": 1234 }` |
| `drift.port_closed` | Drift | Expected port no longer listening | `{ "port": 8080, "protocol": "tcp" }` |
| `drift.port_opened` | Drift | Unexpected port opened | `{ "port": 9090, "pid": 5678 }` |
| `drift.package_installed` | Drift | Package installed outside kith | `{ "package": "...", "version": "..." }` |
| `drift.package_removed` | Drift | Package removed outside kith | `{ "package": "..." }` |

## Execution Events

| Event Type | Category | When | Metadata |
|-----------|----------|------|----------|
| `exec.command` | Exec | Command executed via daemon | `{ "command": "...", "exit_code": 0, "user": "..." }` |
| `exec.denied` | Policy | Exec request denied by policy | `{ "command": "...", "user": "...", "reason": "..." }` |

## Change Events

| Event Type | Category | When | Metadata |
|-----------|----------|------|----------|
| `change.applied` | Apply | Change applied with commit window | `{ "pending_id": "...", "command": "...", "expires_at": "..." }` |
| `change.committed` | Commit | Pending change committed | `{ "pending_id": "...", "user": "..." }` |
| `change.rolled_back` | Rollback | Change explicitly rolled back | `{ "pending_id": "...", "user": "..." }` |
| `change.expired` | Rollback | Commit window expired, auto-rollback | `{ "pending_id": "..." }` |

## Mesh Events

| Event Type | Category | When | Metadata |
|-----------|----------|------|----------|
| `mesh.peer_joined` | Mesh | New peer discovered via Nostr | `{ "peer": "...", "endpoint": "..." }` |
| `mesh.peer_left` | Mesh | Peer unreachable (heartbeat timeout) | `{ "peer": "..." }` |
| `mesh.endpoint_changed` | Mesh | Peer's network endpoint changed | `{ "peer": "...", "old": "...", "new": "..." }` |
| `mesh.tunnel_established` | Mesh | WireGuard tunnel handshake complete | `{ "peer": "..." }` |

## Capability Events

| Event Type | Category | When | Metadata |
|-----------|----------|------|----------|
| `capability.updated` | Capability | Capability report refreshed | `{ "report_hash": "..." }` |

## System Events

| Event Type | Category | When | Metadata |
|-----------|----------|------|----------|
| `system.daemon_started` | System | kith-daemon started | `{ "version": "...", "config_hash": "..." }` |
| `system.daemon_stopped` | System | kith-daemon stopping | `{ "reason": "..." }` |
| `system.sync_completed` | System | cr-sqlite sync with peer completed | `{ "peer": "...", "events_received": 42 }` |
| `system.sync_failed` | System | cr-sqlite sync failed | `{ "peer": "...", "error": "..." }` |
| `system.error` | System | Unexpected error | `{ "error": "...", "context": "..." }` |

---

## Event Schema

All events share the `Event` struct defined in [shared-types.md](../data-models/shared-types.md). The `event_type` field holds the dot-notation type from this catalog. The `metadata` field holds the type-specific JSON payload.

## Scope Rules

| Event Category | Default Scope | Rationale |
|---------------|---------------|-----------|
| Drift | Public | Drift affects everyone who uses the machine |
| Exec | Ops | Command execution details are sensitive |
| Apply/Commit/Rollback | Ops | Change details are sensitive |
| Policy (denials) | Ops | Security-relevant |
| Mesh | Public | Connectivity is operational info |
| Capability | Public | Machine capabilities are not sensitive |
| System | Ops | Internal daemon state |
