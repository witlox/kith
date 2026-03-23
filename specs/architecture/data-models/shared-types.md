# Shared Types (kith-common)

All domain types owned by kith-common.

---

## Events

```rust
/// A system event. The fundamental unit of state in kith.
/// Written to cr-sqlite, synced across mesh, embedded for retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,                              // UUID
    pub machine: String,                         // origin hostname
    pub category: EventCategory,
    pub event_type: String,                      // specific type within category
    pub path: Option<String>,                    // affected path
    pub detail: String,                          // human-readable description
    pub metadata: serde_json::Value,             // arbitrary structured metadata
    pub scope: EventScope,                       // access scope
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventCategory {
    Drift,          // state change detected
    Exec,           // command executed
    Apply,          // change applied
    Commit,         // change committed
    Rollback,       // change rolled back
    Policy,         // policy decision (allow/deny)
    Mesh,           // peer join/leave/endpoint change
    Capability,     // capability report updated
    System,         // daemon start/stop, errors
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventScope {
    /// Visible to anyone with any scope on this machine.
    Public,
    /// Visible only to users with ops scope.
    Ops,
}
```

## Drift

```rust
/// Drift across 4 categories. Simpler than pact's 7 — appropriate for
/// personal/small-team infrastructure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriftVector {
    pub files: f64,
    pub services: f64,
    pub network: f64,
    pub packages: f64,
}

impl DriftVector {
    /// Weighted squared magnitude. Returns sum of squared weighted dimensions.
    /// Not Euclidean norm (no sqrt) — consistent with pact, cheaper to compute,
    /// fine for comparison (ordering is preserved). See adversary F-05.
    pub fn magnitude_sq(&self, weights: &DriftWeights) -> f64 {
        (self.files * weights.files).powi(2)
            + (self.services * weights.services).powi(2)
            + (self.network * weights.network).powi(2)
            + (self.packages * weights.packages).powi(2)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftWeights {
    pub files: f64,
    pub services: f64,
    pub network: f64,
    pub packages: f64,
}

impl Default for DriftWeights {
    fn default() -> Self {
        Self { files: 1.0, services: 2.0, network: 1.5, packages: 1.0 }
    }
}
```

## Capability Report

```rust
/// What a machine can do. Published by kith-daemon, synced via CRDT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub machine: String,
    pub os: OsInfo,
    pub resources: ResourceInfo,
    pub software: Vec<SoftwareInfo>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: String,       // "Linux", "Darwin"
    pub version: String,    // kernel version
    pub arch: String,       // "x86_64", "aarch64"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub cpu_cores: u32,
    pub memory_bytes: u64,
    pub disk_free_bytes: u64,
    pub disk_total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftwareInfo {
    pub name: String,       // "docker", "python3", "nginx"
    pub version: String,
    pub path: String,       // binary path
}
```

## Policy

```rust
/// Per-machine, per-user scope. Intentionally simple.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Scope {
    /// Can execute commands, apply changes, commit/rollback.
    Ops,
    /// Can query state and read events. No execution or changes.
    Viewer,
}

/// Policy configuration for a machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachinePolicy {
    /// Map of user identity -> scope.
    pub users: HashMap<String, Scope>,
    /// Default scope for unknown users. None = deny.
    pub default_scope: Option<Scope>,
    /// Default commit window duration in seconds.
    pub commit_window_seconds: u32,
    /// Drift detection blacklist patterns.
    pub drift_blacklist: Vec<String>,
    /// Drift weights.
    pub drift_weights: DriftWeights,
}
```

## Configuration

```rust
/// Top-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KithConfig {
    pub daemon: Option<DaemonConfig>,
    pub shell: Option<ShellConfig>,
    pub mesh: MeshConfig,
    pub inference: Option<InferenceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub listen_addr: SocketAddr,
    pub policy: MachinePolicy,
    pub data_dir: PathBuf,
    pub containment: ContainmentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainmentConfig {
    /// Enable cgroups v2 containment (Linux only).
    pub cgroups: bool,
    /// Enable overlayfs transactions (Linux only).
    pub overlayfs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellConfig {
    pub context_file: Option<PathBuf>,  // equivalent of CLAUDE.md
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfig {
    pub identifier: String,
    pub wireguard_interface: String,
    pub listen_port: u16,
    pub mesh_cidr: String,
    pub nostr_relays: Vec<String>,
    pub derp_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceProviderConfig {
    pub backend: String,            // "openai-compatible" or "anthropic"
    pub endpoint: Option<String>,
    pub model: String,
    pub api_key_env: Option<String>,
}
```
