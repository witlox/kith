//! Configuration types.

use std::net::SocketAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::policy::MachinePolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KithConfig {
    pub daemon: Option<DaemonConfig>,
    pub shell: Option<ShellConfig>,
    pub mesh: MeshConfig,
    pub inference: Option<InferenceProviderConfig>,
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
    pub cgroups: bool,
    pub overlayfs: bool,
}

impl Default for ContainmentConfig {
    fn default() -> Self {
        Self {
            cgroups: cfg!(target_os = "linux"),
            overlayfs: cfg!(target_os = "linux"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellConfig {
    pub context_file: Option<PathBuf>,
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
    pub backend: String,
    pub endpoint: Option<String>,
    pub model: String,
    pub api_key_env: Option<String>,
}

impl KithConfig {
    /// Load config from ~/.config/kith/config.toml (or the given path).
    /// Returns None if the file doesn't exist (not an error).
    pub fn load(path: Option<&std::path::Path>) -> Result<Option<Self>, String> {
        let config_path = path
            .map(std::path::PathBuf::from)
            .or_else(|| {
                dirs_next::config_dir().map(|d| d.join("kith").join("config.toml"))
            })
            .unwrap_or_else(|| std::path::PathBuf::from(".config/kith/config.toml"));

        if !config_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read {}: {e}", config_path.display()))?;

        let config: KithConfig = toml::from_str(&content)
            .map_err(|e| format!("failed to parse {}: {e}", config_path.display()))?;

        Ok(Some(config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containment_defaults_by_platform() {
        let c = ContainmentConfig::default();
        if cfg!(target_os = "linux") {
            assert!(c.cgroups);
            assert!(c.overlayfs);
        } else {
            assert!(!c.cgroups);
            assert!(!c.overlayfs);
        }
    }

    #[test]
    fn inference_provider_config_serialization() {
        let c = InferenceProviderConfig {
            backend: "openai-compatible".into(),
            endpoint: Some("http://gpu-server:8000/v1".into()),
            model: "minimax-m2.5".into(),
            api_key_env: None,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("openai-compatible"));
        assert!(json.contains("minimax-m2.5"));
    }

    #[test]
    fn config_load_nonexistent_returns_none() {
        let result = KithConfig::load(Some(std::path::Path::new("/nonexistent/config.toml")));
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn config_load_from_toml_string() {
        let toml_str = r#"
[mesh]
identifier = "test-mesh"
wireguard_interface = "kith0"
listen_port = 51820
mesh_cidr = "10.47.0.0/24"
nostr_relays = ["wss://relay.example.com"]

[inference]
backend = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"
"#;
        let config: KithConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mesh.identifier, "test-mesh");
        assert_eq!(config.mesh.listen_port, 51820);
        assert!(config.inference.is_some());
        let inf = config.inference.unwrap();
        assert_eq!(inf.backend, "anthropic");
        assert_eq!(inf.model, "claude-sonnet-4-20250514");
    }
}
