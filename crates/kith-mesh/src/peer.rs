//! Peer registry — tracks known mesh members and their state.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: String,
    pub wireguard_pubkey: String,
    pub endpoint: Option<SocketAddr>,
    pub mesh_ip: IpAddr,
    pub last_handshake: Option<DateTime<Utc>>,
    pub last_seen: DateTime<Utc>,
    pub connected: bool,
}

#[derive(Debug, Clone)]
pub enum MeshEvent {
    PeerJoined(PeerInfo),
    PeerLeft { id: String },
    PeerEndpointChanged {
        id: String,
        new_endpoint: SocketAddr,
    },
}

/// In-memory registry of known peers. The source of truth for mesh membership.
pub struct PeerRegistry {
    peers: HashMap<String, PeerInfo>,
    event_tx: broadcast::Sender<MeshEvent>,
    max_event_age_secs: i64,
}

impl PeerRegistry {
    pub fn new(max_event_age_secs: i64) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            peers: HashMap::new(),
            event_tx,
            max_event_age_secs,
        }
    }

    /// Register or update a peer from a signaling event.
    /// Returns the MeshEvent produced (if any).
    pub fn upsert(&mut self, info: PeerInfo) -> Option<MeshEvent> {
        // Reject events older than max_event_age
        let age = Utc::now()
            .signed_duration_since(info.last_seen)
            .num_seconds();
        if age > self.max_event_age_secs {
            tracing::debug!(
                peer = %info.id,
                age_secs = age,
                "ignoring stale peer event"
            );
            return None;
        }

        if let Some(existing) = self.peers.get(&info.id) {
            // Peer already known — check for endpoint change
            if existing.endpoint != info.endpoint {
                let event = MeshEvent::PeerEndpointChanged {
                    id: info.id.clone(),
                    new_endpoint: info.endpoint.unwrap_or_else(|| {
                        existing.endpoint.unwrap_or(SocketAddr::from(([0, 0, 0, 0], 0)))
                    }),
                };
                self.peers.insert(info.id.clone(), info);
                let _ = self.event_tx.send(event.clone());
                return Some(event);
            }
            // Update last_seen even if endpoint didn't change
            self.peers.insert(info.id.clone(), info);
            None
        } else {
            // New peer
            let event = MeshEvent::PeerJoined(info.clone());
            self.peers.insert(info.id.clone(), info);
            let _ = self.event_tx.send(event.clone());
            Some(event)
        }
    }

    /// Remove a peer (e.g., heartbeat timeout).
    pub fn remove(&mut self, id: &str) -> Option<MeshEvent> {
        if self.peers.remove(id).is_some() {
            let event = MeshEvent::PeerLeft { id: id.into() };
            let _ = self.event_tx.send(event.clone());
            Some(event)
        } else {
            None
        }
    }

    /// Get all known peers.
    pub fn peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Get a specific peer.
    pub fn get(&self, id: &str) -> Option<&PeerInfo> {
        self.peers.get(id)
    }

    /// Check if a peer is known and connected.
    pub fn is_reachable(&self, id: &str) -> bool {
        self.peers.get(id).is_some_and(|p| p.connected)
    }

    /// Subscribe to mesh events.
    pub fn subscribe(&self) -> broadcast::Receiver<MeshEvent> {
        self.event_tx.subscribe()
    }

    /// Mark a peer as connected/disconnected.
    pub fn set_connected(&mut self, id: &str, connected: bool) {
        if let Some(peer) = self.peers.get_mut(id) {
            peer.connected = connected;
            if connected {
                peer.last_handshake = Some(Utc::now());
            }
        }
    }

    /// Remove peers not seen within timeout.
    pub fn expire_stale(&mut self, timeout_secs: i64) -> Vec<MeshEvent> {
        let now = Utc::now();
        let stale_ids: Vec<String> = self
            .peers
            .iter()
            .filter(|(_, p)| {
                now.signed_duration_since(p.last_seen).num_seconds() > timeout_secs
            })
            .map(|(id, _)| id.clone())
            .collect();

        stale_ids.iter().filter_map(|id| self.remove(id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn make_peer(id: &str, endpoint_port: u16) -> PeerInfo {
        PeerInfo {
            id: id.into(),
            wireguard_pubkey: format!("wg-key-{id}"),
            endpoint: Some(SocketAddr::from(([10, 0, 0, 1], endpoint_port))),
            mesh_ip: IpAddr::V4(Ipv4Addr::new(10, 47, 0, 1)),
            last_handshake: None,
            last_seen: Utc::now(),
            connected: false,
        }
    }

    #[test]
    fn new_peer_emits_joined_event() {
        let mut reg = PeerRegistry::new(3600);
        let event = reg.upsert(make_peer("staging-1", 51820));
        assert!(matches!(event, Some(MeshEvent::PeerJoined(_))));
        assert_eq!(reg.peers().len(), 1);
    }

    #[test]
    fn duplicate_peer_same_endpoint_no_event() {
        let mut reg = PeerRegistry::new(3600);
        reg.upsert(make_peer("staging-1", 51820));
        let event = reg.upsert(make_peer("staging-1", 51820));
        assert!(event.is_none());
        assert_eq!(reg.peers().len(), 1);
    }

    #[test]
    fn peer_endpoint_change_emits_event() {
        let mut reg = PeerRegistry::new(3600);
        reg.upsert(make_peer("staging-1", 51820));
        let event = reg.upsert(make_peer("staging-1", 51821));
        assert!(matches!(event, Some(MeshEvent::PeerEndpointChanged { .. })));
    }

    #[test]
    fn remove_peer_emits_left_event() {
        let mut reg = PeerRegistry::new(3600);
        reg.upsert(make_peer("staging-1", 51820));
        let event = reg.remove("staging-1");
        assert!(matches!(event, Some(MeshEvent::PeerLeft { .. })));
        assert!(reg.peers().is_empty());
    }

    #[test]
    fn remove_unknown_peer_returns_none() {
        let mut reg = PeerRegistry::new(3600);
        assert!(reg.remove("nonexistent").is_none());
    }

    #[test]
    fn is_reachable_false_by_default() {
        let mut reg = PeerRegistry::new(3600);
        reg.upsert(make_peer("staging-1", 51820));
        assert!(!reg.is_reachable("staging-1"));
    }

    #[test]
    fn set_connected_makes_reachable() {
        let mut reg = PeerRegistry::new(3600);
        reg.upsert(make_peer("staging-1", 51820));
        reg.set_connected("staging-1", true);
        assert!(reg.is_reachable("staging-1"));
        assert!(reg.get("staging-1").unwrap().last_handshake.is_some());
    }

    #[test]
    fn stale_peer_event_rejected() {
        let mut reg = PeerRegistry::new(3600); // 1 hour max
        let mut peer = make_peer("staging-1", 51820);
        peer.last_seen = Utc::now() - chrono::Duration::hours(2);
        let event = reg.upsert(peer);
        assert!(event.is_none());
        assert!(reg.peers().is_empty());
    }

    #[test]
    fn expire_stale_removes_old_peers() {
        let mut reg = PeerRegistry::new(3600);
        let mut old_peer = make_peer("old-one", 51820);
        old_peer.last_seen = Utc::now() - chrono::Duration::seconds(10);
        // Insert directly to bypass age check on upsert
        reg.peers.insert("old-one".into(), old_peer);

        let fresh_peer = make_peer("fresh-one", 51821);
        reg.upsert(fresh_peer);

        assert_eq!(reg.peers().len(), 2);

        let events = reg.expire_stale(5); // 5 second timeout
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], MeshEvent::PeerLeft { id } if id == "old-one"));
        assert_eq!(reg.peers().len(), 1);
        assert!(reg.get("fresh-one").is_some());
    }

    #[test]
    fn subscribe_receives_events() {
        let mut reg = PeerRegistry::new(3600);
        let mut rx = reg.subscribe();
        reg.upsert(make_peer("staging-1", 51820));
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, MeshEvent::PeerJoined(_)));
    }

    #[test]
    fn multiple_peers_tracked() {
        let mut reg = PeerRegistry::new(3600);
        reg.upsert(make_peer("dev-mac", 51820));
        reg.upsert(make_peer("staging-1", 51821));
        reg.upsert(make_peer("prod-1", 51822));
        assert_eq!(reg.peers().len(), 3);
    }
}
