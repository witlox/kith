# Native Tools

Kith provides 7 native tools — capabilities that don't exist in standard Unix. Everything else (file ops, git, builds, tests, processes) is standard bash via PTY.

## Tool Reference

### `remote(host, command)`

Execute a command on a remote machine via its kith-daemon.

```json
{"host": "staging-1", "command": "docker ps"}
```

### `fleet_query(query)`

Query synced state across the mesh. Searches the local event store (enriched by daemon sync) using keyword matching.

```json
{"query": "disk usage on prod machines"}
```

### `retrieve(query)`

Semantic search over operational history using hybrid retrieval (keyword + vector similarity).

```json
{"query": "nginx configuration changes last week"}
```

### `apply(host, command, paths?)`

Make a change with commit window semantics. Optionally specify file paths to back up before applying.

```json
{"host": "prod-1", "command": "systemctl restart nginx", "paths": ["/etc/nginx/"]}
```

When `paths` is provided, those files are backed up for rollback. When omitted, the change is audit-only.

### `commit(pending_id)`

Commit a pending change after verifying it's correct.

```json
{"pending_id": "abc123"}
```

### `rollback(pending_id)`

Rollback a pending change to restore the previous state.

```json
{"pending_id": "abc123"}
```

### `todo(action, text?)`

Agent self-managed task tracking.

```json
{"action": "add", "text": "verify nginx config on all nodes"}
{"action": "list"}
{"action": "done", "text": "verify nginx config on all nodes"}
{"action": "clear"}
```
