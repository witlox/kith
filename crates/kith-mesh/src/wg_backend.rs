//! Real WireGuard backend using defguard_wireguard_rs.
//! Userspace (boringtun) on macOS, kernel WireGuard on Linux.
//! Enabled with: cargo build -p kith-mesh --features wireguard

use std::net::SocketAddr;

use async_trait::async_trait;
use defguard_wireguard_rs::{
    peer::Peer, key::Key, net::IpAddrMask,
    InterfaceConfiguration, WGApi, WireguardInterfaceApi, Userspace,
};
use tracing::info;

use kith_common::error::KithError;

use crate::wireguard::WireguardBackend;

/// Native WireGuard backend using defguard_wireguard_rs.
pub struct NativeWireguard {
    api: WGApi<Userspace>,
    interface: String,
}

impl NativeWireguard {
    /// Create and configure a WireGuard interface.
    pub fn new(
        interface: &str,
        private_key: &str,
        listen_port: u16,
    ) -> Result<Self, KithError> {
        let ifname = if cfg!(target_os = "macos") && !interface.starts_with("utun") {
            format!("utun{}", interface.len() % 10 + 3) // macOS needs utunN
        } else {
            interface.to_string()
        };

        let mut api = WGApi::<Userspace>::new(ifname.clone())
            .map_err(|e| KithError::MeshError(format!("failed to create WG API: {e}")))?;

        api.create_interface()
            .map_err(|e| KithError::MeshError(format!("failed to create interface: {e}")))?;

        let config = InterfaceConfiguration {
            name: ifname.clone(),
            prvkey: private_key.to_string(),
            addresses: vec![],
            port: listen_port,
            peers: vec![],
            mtu: None,
            fwmark: None,
        };

        api.configure_interface(&config)
            .map_err(|e| KithError::MeshError(format!("failed to configure interface: {e}")))?;

        info!(interface = %ifname, listen_port, "WireGuard interface configured");

        Ok(Self {
            api,
            interface: ifname,
        })
    }

    /// Generate a new WireGuard keypair. Returns (private_key_b64, public_key_b64).
    pub fn generate_keypair() -> (String, String) {
        let key = Key::generate();
        let pubkey = key.public_key();
        (key.to_string(), pubkey.to_string())
    }

    /// Clean up the interface.
    pub fn remove_interface(&self) -> Result<(), KithError> {
        self.api
            .remove_interface()
            .map_err(|e| KithError::MeshError(format!("failed to remove interface: {e}")))?;
        info!(interface = %self.interface, "WireGuard interface removed");
        Ok(())
    }
}

impl Drop for NativeWireguard {
    fn drop(&mut self) {
        let _ = self.api.remove_interface();
    }
}

#[async_trait]
impl WireguardBackend for NativeWireguard {
    async fn add_peer(
        &self,
        pubkey: &str,
        endpoint: Option<SocketAddr>,
        allowed_ip: &str,
    ) -> Result<(), KithError> {
        let key: Key = pubkey.parse()
            .map_err(|e| KithError::MeshError(format!("invalid peer pubkey: {e}")))?;

        let allowed = allowed_ip.parse::<IpAddrMask>()
            .map_err(|e| KithError::MeshError(format!("invalid allowed IP {allowed_ip}: {e}")))?;

        let mut peer = Peer::new(key);
        peer.endpoint = endpoint;
        peer.persistent_keepalive_interval = Some(25);
        peer.allowed_ips.push(allowed);

        self.api
            .configure_peer(&peer)
            .map_err(|e| KithError::MeshError(format!("failed to add peer: {e}")))?;

        info!(peer = %pubkey, ?endpoint, "added WireGuard peer");
        Ok(())
    }

    async fn remove_peer(&self, pubkey: &str) -> Result<(), KithError> {
        let key: Key = pubkey.parse()
            .map_err(|e| KithError::MeshError(format!("invalid peer pubkey: {e}")))?;

        self.api
            .remove_peer(&key)
            .map_err(|e| KithError::MeshError(format!("failed to remove peer: {e}")))?;

        info!(peer = %pubkey, "removed WireGuard peer");
        Ok(())
    }

    async fn is_peer_connected(&self, pubkey: &str) -> Result<bool, KithError> {
        let host = self
            .api
            .read_interface_data()
            .map_err(|e| KithError::MeshError(format!("failed to read interface: {e}")))?;

        let key: Key = pubkey.parse()
            .map_err(|e| KithError::MeshError(format!("invalid pubkey: {e}")))?;

        // host.peers is a HashMap<Key, Peer>
        if let Some(peer) = host.peers.get(&key) {
            if let Some(handshake) = peer.last_handshake {
                let age = std::time::SystemTime::now()
                    .duration_since(handshake)
                    .unwrap_or_default();
                return Ok(age.as_secs() < 180);
            }
            return Ok(false);
        }

        Ok(false)
    }

    async fn own_pubkey(&self) -> Result<String, KithError> {
        let host = self
            .api
            .read_interface_data()
            .map_err(|e| KithError::MeshError(format!("failed to read interface: {e}")))?;

        // private_key may be Option<Key> or Key depending on version
        match host.private_key {
            Some(ref k) => Ok(k.public_key().to_string()),
            None => Err(KithError::MeshError("no private key on interface".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_keypair_produces_valid_keys() {
        let (private, public) = NativeWireguard::generate_keypair();
        assert!(!private.is_empty());
        assert!(!public.is_empty());
        assert_ne!(private, public);
    }
}
