//! WireGuard backend trait + in-memory mock for testing.
//! Real WireGuard implementation (wireguard-control crate or wg CLI)
//! will be added behind a feature flag.

use std::net::SocketAddr;

use async_trait::async_trait;

use kith_common::error::KithError;

/// Trait abstracting WireGuard tunnel management.
#[async_trait]
pub trait WireguardBackend: Send + Sync {
    /// Add a peer to the WireGuard interface.
    async fn add_peer(
        &self,
        pubkey: &str,
        endpoint: Option<SocketAddr>,
        allowed_ip: &str,
    ) -> Result<(), KithError>;

    /// Remove a peer from the WireGuard interface.
    async fn remove_peer(&self, pubkey: &str) -> Result<(), KithError>;

    /// Check if a peer has completed a handshake recently.
    async fn is_peer_connected(&self, pubkey: &str) -> Result<bool, KithError>;

    /// Get our own WireGuard public key.
    async fn own_pubkey(&self) -> Result<String, KithError>;
}

/// In-memory WireGuard mock for testing.
pub struct InMemoryWireguard {
    own_pubkey: String,
    peers: std::sync::Mutex<Vec<MockPeer>>,
}

#[derive(Debug, Clone)]
struct MockPeer {
    pubkey: String,
    endpoint: Option<SocketAddr>,
    allowed_ip: String,
    connected: bool,
}

impl InMemoryWireguard {
    pub fn new(own_pubkey: impl Into<String>) -> Self {
        Self {
            own_pubkey: own_pubkey.into(),
            peers: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Simulate a peer completing handshake.
    pub fn simulate_handshake(&self, pubkey: &str) {
        let mut peers = self.peers.lock().unwrap();
        if let Some(peer) = peers.iter_mut().find(|p| p.pubkey == pubkey) {
            peer.connected = true;
        }
    }

    /// Get count of configured peers.
    pub fn peer_count(&self) -> usize {
        self.peers.lock().unwrap().len()
    }
}

#[async_trait]
impl WireguardBackend for InMemoryWireguard {
    async fn add_peer(
        &self,
        pubkey: &str,
        endpoint: Option<SocketAddr>,
        allowed_ip: &str,
    ) -> Result<(), KithError> {
        let mut peers = self
            .peers
            .lock()
            .map_err(|e| KithError::MeshError(e.to_string()))?;
        // Update if already exists
        if let Some(existing) = peers.iter_mut().find(|p| p.pubkey == pubkey) {
            existing.endpoint = endpoint;
            existing.allowed_ip = allowed_ip.into();
            return Ok(());
        }
        peers.push(MockPeer {
            pubkey: pubkey.into(),
            endpoint,
            allowed_ip: allowed_ip.into(),
            connected: false,
        });
        Ok(())
    }

    async fn remove_peer(&self, pubkey: &str) -> Result<(), KithError> {
        let mut peers = self
            .peers
            .lock()
            .map_err(|e| KithError::MeshError(e.to_string()))?;
        peers.retain(|p| p.pubkey != pubkey);
        Ok(())
    }

    async fn is_peer_connected(&self, pubkey: &str) -> Result<bool, KithError> {
        let peers = self
            .peers
            .lock()
            .map_err(|e| KithError::MeshError(e.to_string()))?;
        Ok(peers.iter().any(|p| p.pubkey == pubkey && p.connected))
    }

    async fn own_pubkey(&self) -> Result<String, KithError> {
        Ok(self.own_pubkey.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn add_and_check_peer() {
        let wg = InMemoryWireguard::new("my-pub-key");
        wg.add_peer(
            "peer-key",
            Some("10.0.0.1:51820".parse().unwrap()),
            "10.47.0.2/32",
        )
        .await
        .unwrap();
        assert_eq!(wg.peer_count(), 1);
        assert!(!wg.is_peer_connected("peer-key").await.unwrap());
    }

    #[tokio::test]
    async fn simulate_handshake() {
        let wg = InMemoryWireguard::new("my-pub-key");
        wg.add_peer("peer-key", None, "10.47.0.2/32").await.unwrap();
        wg.simulate_handshake("peer-key");
        assert!(wg.is_peer_connected("peer-key").await.unwrap());
    }

    #[tokio::test]
    async fn remove_peer() {
        let wg = InMemoryWireguard::new("my-pub-key");
        wg.add_peer("peer-key", None, "10.47.0.2/32").await.unwrap();
        wg.remove_peer("peer-key").await.unwrap();
        assert_eq!(wg.peer_count(), 0);
    }

    #[tokio::test]
    async fn add_peer_updates_existing() {
        let wg = InMemoryWireguard::new("my-pub-key");
        wg.add_peer(
            "peer-key",
            Some("10.0.0.1:51820".parse().unwrap()),
            "10.47.0.2/32",
        )
        .await
        .unwrap();
        wg.add_peer(
            "peer-key",
            Some("10.0.0.2:51820".parse().unwrap()),
            "10.47.0.2/32",
        )
        .await
        .unwrap();
        assert_eq!(wg.peer_count(), 1); // Updated, not duplicated
    }

    #[tokio::test]
    async fn own_pubkey() {
        let wg = InMemoryWireguard::new("my-pub-key");
        assert_eq!(wg.own_pubkey().await.unwrap(), "my-pub-key");
    }
}
