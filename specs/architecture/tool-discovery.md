# Tool Discovery Architecture

## Overview

Tool discovery provides the LLM and operators with accurate knowledge of what executables are available, both locally and across the mesh. It eliminates guessing.

## Components

### ToolEntry (kith-common)

```rust
pub struct ToolEntry {
    pub name: String,
    pub path: PathBuf,
    pub category: ToolCategory,
    pub version: Option<String>,
}

pub enum ToolCategory {
    Vcs,        // git, svn, hg
    Container,  // docker, podman, kubectl, helm, crictl
    Language,   // python3, node, ruby, go, java, rustc
    Build,      // cargo, make, cmake, npm, yarn, pip, gradle, maven
    Server,     // nginx, apache2, caddy, haproxy
    Database,   // psql, mysql, mongosh, redis-cli, sqlite3
    Editor,     // vim, nvim, nano, emacs, code
    Network,    // curl, wget, ssh, scp, rsync, nc, dig, nslookup
    Monitoring, // htop, top, iotop, nethogs, nvidia-smi, rocm-smi
    Other,
}
```

### ToolRegistry (kith-common)

```rust
pub struct ToolRegistry {
    tools: Vec<ToolEntry>,
    scanned_at: DateTime<Utc>,
}

impl ToolRegistry {
    /// Scan PATH and build registry. Runs at startup and on rescan.
    pub fn scan() -> Self;

    /// Get version for a tool by running `tool --version`.
    /// Only run for categorized tools (not "other") to limit subprocess spawns.
    fn detect_version(name: &str, path: &Path) -> Option<String>;

    /// Categorize a tool name using the known-tools table.
    fn categorize(name: &str) -> ToolCategory;

    /// Format as categorized summary for system prompt injection.
    /// Budget: max ~1500 chars. Lists versioned tools individually,
    /// summarizes "other" as count.
    pub fn prompt_summary(&self) -> String;

    /// Full tool list for daemon Capabilities RPC.
    pub fn to_capability_tools(&self) -> Vec<(String, String, Option<String>)>;

    /// Rescan PATH (call when tools may have changed).
    pub fn rescan(&mut self);
}
```

### Known-Tools Table (kith-common)

Static mapping: tool name → category. ~100 entries covering common Unix/macOS/Linux tools. Unknown names get `Other`. The table is a `phf::Map` or simple match block — no dynamic loading.

### Version Detection Strategy

Only detect versions for **categorized** tools (not `Other`). Run `tool --version` with a 2-second timeout, capture first line, parse version number with regex `\d+\.\d+(\.\d+)?`. Failures silently produce `None` — version is best-effort.

## Integration Points

### Shell Startup (kith-shell/bin/kith.rs)

1. `ToolRegistry::scan()` at startup (replaces `InputClassifier::from_path_env()`)
2. Pass registry to `InputClassifier::new()` (uses `tools.iter().map(|t| t.name)`)
3. Pass `registry.prompt_summary()` to `build_system_prompt()`
4. Store registry in `Agent` for rescan capability

### System Prompt (kith-shell/prompt.rs)

Add `available_tools: &str` parameter to `build_system_prompt()`. Inject after the OS info line. Budget-aware: if summary exceeds 1500 chars, truncate `Other` category.

### Daemon Capabilities (kith-daemon/service.rs)

Replace hardcoded `sysinfo().tools` with `ToolRegistry::scan().to_capability_tools()`. Cache for 5 minutes, rescan on cache expiry when Capabilities is called.

### Rescan Trigger

- Shell: explicit `rescan` intent or after a `tool not found` error
- Daemon: time-based cache expiry (5 min default)

## What This Does NOT Do

- No automatic package manager integration (apt, brew, snap)
- No tool installation — just discovery
- No tool recommendation — the LLM decides what to use
- No persistent storage of registry — rebuilt from PATH each time
