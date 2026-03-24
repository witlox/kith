//! Real Nostr signaling backend using nostr-sdk.
//! Publishes and subscribes to peer discovery events on Nostr relays.
//! Enabled with: cargo build -p kith-mesh --features nostr

use async_trait::async_trait;
use nostr::prelude::*;
use nostr_sdk::prelude::*;
use tracing::{debug, info, warn};

use kith_common::error::KithError;

use crate::signaling::{PeerDiscoveryEvent, SignalingBackend};

/// Nostr-based signaling backend. Publishes kind 30078 parameterized
/// replaceable events tagged with the mesh identifier (ADR-003).
pub struct NostrSignaling {
    client: nostr_sdk::Client,
    mesh_identifier: String,
}

impl NostrSignaling {
    /// Create and connect to relays.
    pub async fn new(mesh_identifier: String, relay_urls: &[String]) -> Result<Self, KithError> {
        let keys = Keys::generate();
        let client = nostr_sdk::Client::new(keys);

        for url in relay_urls {
            if let Err(e) = client.add_relay(url.as_str()).await {
                warn!(relay = %url, error = %e, "failed to add relay");
            }
        }

        client.connect().await;
        info!(relays = relay_urls.len(), "connected to Nostr relays");

        Ok(Self {
            client,
            mesh_identifier,
        })
    }
}

#[async_trait]
impl SignalingBackend for NostrSignaling {
    async fn publish(&self, event: &PeerDiscoveryEvent) -> Result<(), KithError> {
        // Kind 30078: parameterized replaceable (NIP-33)
        // "d" tag = mesh_identifier (one event per machine per mesh)
        let tags = vec![
            Tag::identifier(self.mesh_identifier.clone()),
            Tag::custom(
                nostr::TagKind::Custom(std::borrow::Cow::Borrowed("machine")),
                [event.machine_id.clone()],
            ),
            Tag::custom(
                nostr::TagKind::Custom(std::borrow::Cow::Borrowed("wg_pubkey")),
                [event.wireguard_pubkey.clone()],
            ),
            Tag::custom(
                nostr::TagKind::Custom(std::borrow::Cow::Borrowed("endpoint")),
                [event.endpoint.clone()],
            ),
            Tag::custom(
                nostr::TagKind::Custom(std::borrow::Cow::Borrowed("mesh_ip")),
                [event.mesh_ip.clone()],
            ),
        ];

        let builder = EventBuilder::new(Kind::Custom(30078), "").tags(tags);

        self.client
            .send_event_builder(builder)
            .await
            .map_err(|e| KithError::MeshError(format!("failed to publish: {e}")))?;

        info!(
            machine = %event.machine_id,
            endpoint = %event.endpoint,
            "published peer discovery to Nostr"
        );
        Ok(())
    }

    async fn fetch_peers(
        &self,
        mesh_identifier: &str,
    ) -> Result<Vec<PeerDiscoveryEvent>, KithError> {
        let filter = Filter::new()
            .kind(Kind::Custom(30078))
            .identifier(mesh_identifier);

        let events = self
            .client
            .fetch_events(filter, std::time::Duration::from_secs(5))
            .await
            .map_err(|e| KithError::MeshError(format!("failed to fetch: {e}")))?;

        let peers: Vec<PeerDiscoveryEvent> = events
            .iter()
            .filter_map(|event| {
                let get_tag = |key: &str| -> Option<String> {
                    event.tags.iter().find_map(|tag| {
                        let values = tag.as_slice();
                        if values.first() == Some(&key.to_string()) {
                            values.get(1).cloned()
                        } else {
                            None
                        }
                    })
                };

                Some(PeerDiscoveryEvent {
                    machine_id: get_tag("machine")?,
                    wireguard_pubkey: get_tag("wg_pubkey")?,
                    endpoint: get_tag("endpoint").unwrap_or_default(),
                    mesh_ip: get_tag("mesh_ip").unwrap_or_default(),
                    timestamp: chrono::DateTime::from_timestamp(
                        event.created_at.as_u64() as i64,
                        0,
                    )
                    .unwrap_or_else(chrono::Utc::now),
                })
            })
            .collect();

        debug!(count = peers.len(), "fetched peers from Nostr");
        Ok(peers)
    }
}
