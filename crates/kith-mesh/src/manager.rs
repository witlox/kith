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
    our_mesh_ip: String,
    signaling: S,
    wireguard: W,
    registry: PeerRegistry,
}

impl<S: SignalingBackend, W: WireguardBackend> DefaultMeshManager<S, W> {
    pub fn new(config: MeshConfig, our_machine_id: String, signaling: S, wireguard: W) -> Self {
        let mesh_ip = allocate_mesh_ip(&config.mesh_cidr, &our_machine_id);
        Self {
            config,
            our_machine_id,
            our_mesh_ip: mesh_ip,
            signaling,
            wireguard,
            registry: PeerRegistry::new(3600),
        }
    }

    /// Publish our presence to the signaling network.
    pub async fn announce(&self, endpoint: Option<SocketAddr>) -> Result<(), KithError> {
        let own_pubkey = self.wireguard.own_pubkey().await?;
        let event = PeerDiscoveryEvent {
            machine_id: self.our_machine_id.clone(),
            wireguard_pubkey: own_pubkey,
            endpoint: endpoint.map_or_else(String::new, |e| e.to_string()),
            mesh_ip: self.our_mesh_ip.clone(),
            timestamp: chrono::Utc::now(),
        };

        self.signaling.publish(&event).await?;
        info!(machine = %self.our_machine_id, "announced to mesh");
        Ok(())
    }

    /// Discover peers from signaling and configure WireGuard tunnels.
    pub async fn discover_and_connect(&mut self) -> Result<Vec<MeshEvent>, KithError> {
        let peer_events = self.signaling.fetch_peers(&self.config.identifier).await?;

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

    pub fn our_mesh_ip(&self) -> &str {
        &self.our_mesh_ip
    }
}

/// Allocate a deterministic mesh IP. Prefers IPv6 ULA, falls back to IPv4 CIDR.
///
/// IPv6 ULA (default): derives a /64 prefix from mesh_identifier, then a /128
/// host address from machine_id. Format: `fd<mesh_hash>::<machine_hash>`
///
/// IPv4 fallback: if mesh_cidr contains dots (e.g. "10.47.0.0/24"), allocates
/// a host address from the CIDR based on machine_id hash.
fn allocate_mesh_ip(mesh_cidr: &str, machine_id: &str) -> String {
    if mesh_cidr.contains('.') {
        allocate_ipv4(mesh_cidr, machine_id)
    } else {
        allocate_ipv6_ula(mesh_cidr, machine_id)
    }
}

/// Generate a deterministic IPv6 ULA address from mesh identifier + machine id.
/// mesh_cidr can be a ULA prefix like "fd00::/64" or just the mesh identifier string.
/// Result: fd<40-bit global ID from mesh>::<16-bit subnet>:<48-bit interface from machine>
fn allocate_ipv6_ula(mesh_cidr: &str, machine_id: &str) -> String {
    // If it's already an IPv6 prefix, extract the base
    if mesh_cidr.contains(':') {
        let base = mesh_cidr.split('/').next().unwrap_or("fd00::");
        let machine_hash = hash_to_u64(machine_id);
        let h1 = ((machine_hash >> 16) & 0xFFFF) as u16;
        let h2 = (machine_hash & 0xFFFF) as u16;
        return format!("{base}{h1:x}:{h2:x}");
    }

    // Otherwise, derive the full ULA from the mesh identifier string
    let mesh_hash = hash_to_u64(mesh_cidr);
    let machine_hash = hash_to_u64(machine_id);

    // fd + 40-bit global ID (from mesh) + 16-bit subnet (0001) + 64-bit interface (from machine)
    let global_id = mesh_hash & 0xFF_FFFF_FFFF; // 40 bits
    let g1 = ((global_id >> 32) & 0xFF) as u8;
    let g2 = ((global_id >> 16) & 0xFFFF) as u16;
    let g3 = (global_id & 0xFFFF) as u16;

    let iface_hi = ((machine_hash >> 48) & 0xFFFF) as u16;
    let iface_mid1 = ((machine_hash >> 32) & 0xFFFF) as u16;
    let iface_mid2 = ((machine_hash >> 16) & 0xFFFF) as u16;
    let iface_lo = (machine_hash & 0xFFFF) as u16;

    format!(
        "fd{g1:02x}:{g2:04x}:{g3:04x}:1:{iface_hi:x}:{iface_mid1:x}:{iface_mid2:x}:{iface_lo:x}"
    )
}

/// Allocate IPv4 from CIDR (fallback).
fn allocate_ipv4(cidr: &str, machine_id: &str) -> String {
    let parts: Vec<&str> = cidr.split('/').collect();
    let base = parts.first().copied().unwrap_or("10.47.0.0");
    let octets: Vec<u8> = base.split('.').filter_map(|s| s.parse().ok()).collect();

    if octets.len() != 4 {
        return base.to_string();
    }

    let hash: u32 = hash_to_u64(machine_id) as u32;
    let host_part = (hash % 253 + 1) as u8;

    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], host_part)
}

/// Simple deterministic hash — not cryptographic, just for address derivation.
fn hash_to_u64(input: &str) -> u64 {
    // FNV-1a 64-bit
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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

        let manager = DefaultMeshManager::new(test_config(), "dev-mac".into(), signaling, wg);

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

        let mut manager = DefaultMeshManager::new(test_config(), "dev-mac".into(), signaling, wg);

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

        let mut manager = DefaultMeshManager::new(test_config(), "dev-mac".into(), signaling, wg);

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

        let mut manager_a = DefaultMeshManager::new(test_config(), "dev-mac".into(), sig_a, wg_a);
        let mut manager_b = DefaultMeshManager::new(test_config(), "staging-1".into(), sig_b, wg_b);

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

        let mut manager = DefaultMeshManager::new(test_config(), "dev-mac".into(), signaling, wg);

        manager.discover_and_connect().await.unwrap();
        assert!(!manager.registry().is_reachable("staging-1"));

        // Simulate handshake
        manager.wireguard.simulate_handshake("staging-wg-pubkey");
        manager.refresh_connectivity().await.unwrap();

        assert!(manager.registry().is_reachable("staging-1"));
    }

    // --- IP allocation tests ---

    #[test]
    fn ipv6_ula_from_mesh_identifier() {
        let ip = allocate_mesh_ip("my-mesh-2026", "dev-mac");
        assert!(ip.starts_with("fd"), "should be ULA: {ip}");
        assert!(ip.contains(':'), "should be IPv6: {ip}");
    }

    #[test]
    fn ipv6_ula_deterministic() {
        let ip1 = allocate_mesh_ip("my-mesh", "dev-mac");
        let ip2 = allocate_mesh_ip("my-mesh", "dev-mac");
        assert_eq!(ip1, ip2, "same input should produce same IP");
    }

    #[test]
    fn ipv6_ula_different_machines_different_ips() {
        let ip1 = allocate_mesh_ip("my-mesh", "dev-mac");
        let ip2 = allocate_mesh_ip("my-mesh", "staging-1");
        assert_ne!(ip1, ip2, "different machines should get different IPs");
    }

    #[test]
    fn ipv6_ula_different_meshes_different_prefixes() {
        let ip1 = allocate_mesh_ip("mesh-a", "dev-mac");
        let ip2 = allocate_mesh_ip("mesh-b", "dev-mac");
        // Same machine in different meshes should get different prefixes
        assert_ne!(ip1, ip2);
    }

    #[test]
    fn ipv6_explicit_prefix() {
        let ip = allocate_mesh_ip("fd12:3456:7890::/64", "dev-mac");
        assert!(
            ip.starts_with("fd12:3456:7890::"),
            "should use given prefix: {ip}"
        );
    }

    #[test]
    fn ipv4_fallback() {
        let ip = allocate_mesh_ip("10.47.0.0/24", "dev-mac");
        assert!(ip.starts_with("10.47.0."), "should be IPv4: {ip}");
        let host: u8 = ip.split('.').last().unwrap().parse().unwrap();
        assert!(
            host >= 1 && host <= 253,
            "host part should be 1-253: {host}"
        );
    }

    #[test]
    fn ipv4_deterministic() {
        let ip1 = allocate_mesh_ip("10.47.0.0/24", "dev-mac");
        let ip2 = allocate_mesh_ip("10.47.0.0/24", "dev-mac");
        assert_eq!(ip1, ip2);
    }

    #[test]
    fn ipv4_different_machines() {
        let ip1 = allocate_mesh_ip("10.47.0.0/24", "dev-mac");
        let ip2 = allocate_mesh_ip("10.47.0.0/24", "staging-1");
        assert_ne!(ip1, ip2);
    }

    #[test]
    fn manager_has_mesh_ip() {
        let wg = InMemoryWireguard::new("wg-key");
        let signaling = InMemorySignaling::new();
        let manager = DefaultMeshManager::new(test_config(), "dev-mac".into(), signaling, wg);
        let ip = manager.our_mesh_ip();
        assert!(!ip.is_empty(), "should have an allocated IP");
        // Default test config uses "10.47.0.0/24" so should be IPv4
        assert!(
            ip.starts_with("10.47.0."),
            "should be IPv4 from test config: {ip}"
        );
    }
}
