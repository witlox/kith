pub mod peer;
pub mod signaling;
pub mod wireguard;

mod manager;

pub use manager::DefaultMeshManager;
pub use peer::{MeshEvent, PeerInfo, PeerRegistry};
pub use signaling::SignalingBackend;
pub use wireguard::WireguardBackend;
