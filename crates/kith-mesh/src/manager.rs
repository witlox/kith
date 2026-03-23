//! DefaultMeshManager — orchestrates signaling + WireGuard + peer registry.

use std::net::SocketAddr;

use kith_common::config::MeshConfig;
use kith_common::error::KithError;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::peer::{MeshEvent, PeerInfo, PeerRegistry};
use crate::signaling::{PeerDiscoveryEvent, SignalingBackend};
use crate::wireguard::WireguardBackend;

pub struct DefaultMeshManager<S: SignalingBackend, W: WireguardBackend> {
    config: MeshConfig,
    our_machine_id: String,
    signaling: S,
    wireguard: W,
    registry: PeerRegistry,
}

impl<S: SignalingBackend, W: WireguardBackend> DefaultMeshManager<S, W> {
    pub fn new(
        config: MeshConfig,
        our_machine_id: String,
        signaling: S,
        wireguard: W,
    ) -> Self {
        Self {
            config,
            our_machine_id,
            signaling,
            wireguard,
            registry: PeerRegistry::new(3600), // 1 hour max event age
        }
    }

    /// Publish our presence to the signaling network.
    pub async fn announce(&self, endpoint: Option<SocketAddr>) -> Result<(), KithError> {
        let own_pubkey = self.wireguard.own_pubkey().await?;
        let event = PeerDiscoveryEvent {
            machine_id: self.our_machine_id.clone(),
            wireguard_pubkey: own_pubkey,
            endpoint: endpoint.map_or_else(String::new, |e| e.to_string()),
            mesh_ip: String::new(), // TODO: allocate from mesh_cidr
            timestamp: chrono::Utc::now(),
        };

        self.signaling.publish(&event).await?;
        info!(machine = %self.our_machine_id, "announced to mesh");
        Ok(())
    }

    /// Discover peers from signaling and configure WireGuard tunnels.
    pub async fn discover_and_connect(&mut self) -> Result<Vec<MeshEvent>, KithError> {
        let peer_events = self
            .signaling
            .fetch_peers(&self.config.identifier)
            .await?;

        let mut mesh_events = Vec::new();

        for disc_event in peer_events {
            // Skip ourselves
            if disc_event.machine_id == self.our_machine_id {
                continue;
            }

            let endpoint: Option<SocketAddr> = disc_event.endpoint.parse().ok();

            let peer_info = PeerInfo {
                id: disc_event.machine_id.clone(),
                wireguard_pubkey: disc_event.wireguard_pubkey.clone(),
                endpoint,
                mesh_ip: disc_event
                    .mesh_ip
                    .parse()
                    .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
                last_handshake: None,
                last_seen: disc_event.timestamp,
                connected: false,
            };

            if let Some(event) = self.registry.upsert(peer_info) {
                // Configure WireGuard for new/changed peer
                let allowed_ip = format!("{}/32", disc_event.mesh_ip);
                if let Err(e) = self
                    .wireguard
                    .add_peer(&disc_event.wireguard_pubkey, endpoint, &allowed_ip)
                    .await
                {
                    warn!(
                        peer = %disc_event.machine_id,
                        error = %e,
                        "failed to configure WireGuard peer"
                    );
                } else {
                    info!(peer = %disc_event.machine_id, "configured WireGuard peer");
                }
                mesh_events.push(event);
            }
        }

        Ok(mesh_events)
    }

    /// Update peer connectivity status from WireGuard handshake state.
    pub async fn refresh_connectivity(&mut self) -> Result<(), KithError> {
        let peer_ids: Vec<(String, String)> = self
            .registry
            .peers()
            .iter()
            .map(|p| (p.id.clone(), p.wireguard_pubkey.clone()))
            .collect();

        for (id, pubkey) in peer_ids {
            let connected = self.wireguard.is_peer_connected(&pubkey).await?;
            self.registry.set_connected(&id, connected);
        }

        Ok(())
    }

    /// Get the peer registry (for querying state).
    pub fn registry(&self) -> &PeerRegistry {
        &self.registry
    }

    /// Subscribe to mesh events.
    pub fn subscribe(&self) -> broadcast::Receiver<MeshEvent> {
        self.registry.subscribe()
    }

    /// Expire stale peers.
    pub fn expire_stale(&mut self, timeout_secs: i64) -> Vec<MeshEvent> {
        self.registry.expire_stale(timeout_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signaling::{InMemorySignaling, SharedSignaling};
    use crate::wireguard::InMemoryWireguard;

    fn test_config() -> MeshConfig {
        MeshConfig {
            identifier: "test-mesh".into(),
            wireguard_interface: "kith0".into(),
            listen_port: 51820,
            mesh_cidr: "10.47.0.0/24".into(),
            nostr_relays: vec![],
            derp_url: None,
        }
    }

    #[tokio::test]
    async fn announce_publishes_to_signaling() {
        let signaling = InMemorySignaling::new();
        let wg = InMemoryWireguard::new("our-wg-pubkey");

        let manager = DefaultMeshManager::new(
            test_config(),
            "dev-mac".into(),
            signaling,
            wg,
        );

        manager
            .announce(Some("10.0.0.1:51820".parse().unwrap()))
            .await
            .unwrap();

        let peers = manager.signaling.fetch_peers("test-mesh").await.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].machine_id, "dev-mac");
        assert_eq!(peers[0].wireguard_pubkey, "our-wg-pubkey");
    }

    #[tokio::test]
    async fn discover_skips_self() {
        let signaling = InMemorySignaling::new();
        let wg = InMemoryWireguard::new("our-wg-pubkey");

        let self_event = PeerDiscoveryEvent {
            machine_id: "dev-mac".into(),
            wireguard_pubkey: "our-wg-pubkey".into(),
            endpoint: "10.0.0.1:51820".into(),
            mesh_ip: "10.47.0.1".into(),
            timestamp: chrono::Utc::now(),
        };
        signaling.publish(&self_event).await.unwrap();

        let mut manager = DefaultMeshManager::new(
            test_config(),
            "dev-mac".into(),
            signaling,
            wg,
        );

        let events = manager.discover_and_connect().await.unwrap();
        assert!(events.is_empty());
        assert!(manager.registry().peers().is_empty());
    }

    #[tokio::test]
    async fn discover_new_peer_configures_wireguard() {
        let signaling = InMemorySignaling::new();
        let wg = InMemoryWireguard::new("our-wg-pubkey");

        let peer_event = PeerDiscoveryEvent {
            machine_id: "staging-1".into(),
            wireguard_pubkey: "staging-wg-pubkey".into(),
            endpoint: "10.0.0.2:51820".into(),
            mesh_ip: "10.47.0.2".into(),
            timestamp: chrono::Utc::now(),
        };
        signaling.publish(&peer_event).await.unwrap();

        let mut manager = DefaultMeshManager::new(
            test_config(),
            "dev-mac".into(),
            signaling,
            wg,
        );

        let events = manager.discover_and_connect().await.unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], MeshEvent::PeerJoined(p) if p.id == "staging-1"));
        assert_eq!(manager.wireguard.peer_count(), 1);
    }

    #[tokio::test]
    async fn two_nodes_discover_each_other() {
        let (sig_a, sig_b) = SharedSignaling::new();
        let wg_a = InMemoryWireguard::new("wg-a");
        let wg_b = InMemoryWireguard::new("wg-b");

        let mut manager_a = DefaultMeshManager::new(
            test_config(),
            "dev-mac".into(),
            sig_a,
            wg_a,
        );
        let mut manager_b = DefaultMeshManager::new(
            test_config(),
            "staging-1".into(),
            sig_b,
            wg_b,
        );

        // Both announce
        manager_a
            .announce(Some("10.0.0.1:51820".parse().unwrap()))
            .await
            .unwrap();
        manager_b
            .announce(Some("10.0.0.2:51820".parse().unwrap()))
            .await
            .unwrap();

        // Both discover
        let events_a = manager_a.discover_and_connect().await.unwrap();
        let events_b = manager_b.discover_and_connect().await.unwrap();

        // A discovered B
        assert_eq!(events_a.len(), 1);
        assert!(matches!(&events_a[0], MeshEvent::PeerJoined(p) if p.id == "staging-1"));

        // B discovered A
        assert_eq!(events_b.len(), 1);
        assert!(matches!(&events_b[0], MeshEvent::PeerJoined(p) if p.id == "dev-mac"));
    }

    #[tokio::test]
    async fn refresh_connectivity_updates_registry() {
        let signaling = InMemorySignaling::new();
        let wg = InMemoryWireguard::new("our-wg-pubkey");

        let peer_event = PeerDiscoveryEvent {
            machine_id: "staging-1".into(),
            wireguard_pubkey: "staging-wg-pubkey".into(),
            endpoint: "10.0.0.2:51820".into(),
            mesh_ip: "10.47.0.2".into(),
            timestamp: chrono::Utc::now(),
        };
        signaling.publish(&peer_event).await.unwrap();

        let mut manager = DefaultMeshManager::new(
            test_config(),
            "dev-mac".into(),
            signaling,
            wg,
        );

        manager.discover_and_connect().await.unwrap();
        assert!(!manager.registry().is_reachable("staging-1"));

        // Simulate handshake
        manager.wireguard.simulate_handshake("staging-wg-pubkey");
        manager.refresh_connectivity().await.unwrap();

        assert!(manager.registry().is_reachable("staging-1"));
    }
}
