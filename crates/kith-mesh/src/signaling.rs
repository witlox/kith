//! Signaling backend trait + in-memory mock for testing.
//! Real Nostr implementation will be added when nostr-sdk is integrated.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use kith_common::error::KithError;

/// A peer discovery event received from the signaling layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDiscoveryEvent {
    pub machine_id: String,
    pub wireguard_pubkey: String,
    pub endpoint: String,
    pub mesh_ip: String,
    pub timestamp: DateTime<Utc>,
}

/// Trait abstracting the signaling layer. Nostr is the production implementation.
#[async_trait]
pub trait SignalingBackend: Send + Sync {
    /// Publish our peer info to the signaling network.
    async fn publish(&self, event: &PeerDiscoveryEvent) -> Result<(), KithError>;

    /// Fetch all peer events matching our mesh identifier.
    async fn fetch_peers(&self, mesh_identifier: &str) -> Result<Vec<PeerDiscoveryEvent>, KithError>;
}

/// In-memory signaling backend for testing.
pub struct InMemorySignaling {
    events: std::sync::Mutex<Vec<(String, PeerDiscoveryEvent)>>,
}

impl InMemorySignaling {
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Default for InMemorySignaling {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SignalingBackend for InMemorySignaling {
    async fn publish(&self, event: &PeerDiscoveryEvent) -> Result<(), KithError> {
        let mut events = self.events.lock().map_err(|e| KithError::Internal(e.to_string()))?;
        // Parameterized replaceable: remove existing event from same machine
        events.retain(|(_, e)| e.machine_id != event.machine_id);
        // Use machine_id as the mesh identifier tag for simplicity in mock
        events.push((event.machine_id.clone(), event.clone()));
        Ok(())
    }

    async fn fetch_peers(&self, _mesh_identifier: &str) -> Result<Vec<PeerDiscoveryEvent>, KithError> {
        let events = self.events.lock().map_err(|e| KithError::Internal(e.to_string()))?;
        Ok(events.iter().map(|(_, e)| e.clone()).collect())
    }
}

/// Shared in-memory signaling for multi-node tests (simulates a relay).
pub struct SharedSignaling {
    inner: std::sync::Arc<InMemorySignaling>,
}

impl SharedSignaling {
    pub fn new() -> (Self, Self) {
        let inner = std::sync::Arc::new(InMemorySignaling::new());
        (
            Self { inner: inner.clone() },
            Self { inner },
        )
    }

    pub fn clone_backend(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[async_trait]
impl SignalingBackend for SharedSignaling {
    async fn publish(&self, event: &PeerDiscoveryEvent) -> Result<(), KithError> {
        self.inner.publish(event).await
    }

    async fn fetch_peers(&self, mesh_identifier: &str) -> Result<Vec<PeerDiscoveryEvent>, KithError> {
        self.inner.fetch_peers(mesh_identifier).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_discovery_event(machine: &str) -> PeerDiscoveryEvent {
        PeerDiscoveryEvent {
            machine_id: machine.into(),
            wireguard_pubkey: format!("wg-pub-{machine}"),
            endpoint: "10.0.0.1:51820".into(),
            mesh_ip: "10.47.0.1".into(),
            timestamp: Utc::now(),
        }
    }

    #[tokio::test]
    async fn publish_and_fetch() {
        let backend = InMemorySignaling::new();
        backend.publish(&make_discovery_event("staging-1")).await.unwrap();
        let peers = backend.fetch_peers("mesh-id").await.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].machine_id, "staging-1");
    }

    #[tokio::test]
    async fn publish_replaces_existing() {
        let backend = InMemorySignaling::new();
        let mut event = make_discovery_event("staging-1");
        backend.publish(&event).await.unwrap();

        event.endpoint = "10.0.0.2:51820".into();
        backend.publish(&event).await.unwrap();

        let peers = backend.fetch_peers("mesh-id").await.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].endpoint, "10.0.0.2:51820");
    }

    #[tokio::test]
    async fn multiple_machines() {
        let backend = InMemorySignaling::new();
        backend.publish(&make_discovery_event("dev-mac")).await.unwrap();
        backend.publish(&make_discovery_event("staging-1")).await.unwrap();
        backend.publish(&make_discovery_event("prod-1")).await.unwrap();

        let peers = backend.fetch_peers("mesh-id").await.unwrap();
        assert_eq!(peers.len(), 3);
    }

    #[tokio::test]
    async fn shared_signaling_visible_across_nodes() {
        let (node_a, node_b) = SharedSignaling::new();

        node_a.publish(&make_discovery_event("dev-mac")).await.unwrap();
        node_b.publish(&make_discovery_event("staging-1")).await.unwrap();

        // Both nodes see both peers
        let peers_a = node_a.fetch_peers("mesh").await.unwrap();
        let peers_b = node_b.fetch_peers("mesh").await.unwrap();
        assert_eq!(peers_a.len(), 2);
        assert_eq!(peers_b.len(), 2);
    }
}
