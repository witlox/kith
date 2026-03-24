pub mod peer;
pub mod signaling;
pub mod wireguard;

mod manager;

#[cfg(feature = "nostr")]
pub mod nostr_signaling;
#[cfg(feature = "wireguard")]
pub mod wg_backend;

pub use manager::DefaultMeshManager;
pub use peer::{MeshEvent, PeerInfo, PeerRegistry};
pub use signaling::SignalingBackend;
pub use wireguard::WireguardBackend;
