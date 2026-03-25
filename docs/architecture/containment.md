# Containment

## Overview

Containment limits the blast radius of agent actions. Every change goes through a transaction that can be committed or rolled back.

## Transaction Types

### CopyTransaction (all platforms)

Creates file backups before modification. Works on macOS and Linux.

- Copies specified paths to a temporary backup directory
- On commit: backups are deleted
- On rollback: originals are restored from backups

### OverlayTransaction (Linux only)

Uses overlayfs to isolate changes in a filesystem layer.

- Creates an overlay mount over the target directory
- Changes are written to the upper layer
- On commit: upper layer is merged to lower
- On rollback: upper layer is discarded

## Apply Tool Integration

The `apply` native tool accepts an optional `paths` parameter:

```json
{"host": "prod-1", "command": "apt upgrade nginx", "paths": ["/etc/nginx/", "/usr/sbin/nginx"]}
```

- **With paths:** Files are backed up before the command runs. Rollback restores them.
- **Without paths:** The change is audit-only — recorded in the event log but no file-level protection.

## Commit Windows (ADR-002)

Changes enter a pending state with a configurable timeout (default: 600 seconds). The operator must explicitly commit or rollback. Expired changes are automatically rolled back.

## Policy Scope

Containment is enforced by the daemon's `PolicyEvaluator`:
- **Ops scope:** Full access — exec, apply, commit, rollback
- **Viewer scope:** Read-only — query, events, capabilities
