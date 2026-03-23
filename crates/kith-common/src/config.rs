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
}
