# Mesh Interfaces

WireGuard tunnel management and Nostr signaling interfaces.

---

## MeshManager

```rust
/// Manages the WireGuard mesh and Nostr signaling.
pub trait MeshManager: Send + Sync {
    /// Start mesh networking: publish to Nostr, discover peers, establish tunnels.
    async fn start(&self) -> Result<(), KithError>;

    /// Stop mesh networking.
    async fn stop(&self) -> Result<(), KithError>;

    /// Get list of currently connected peers.
    async fn peers(&self) -> Vec<PeerInfo>;

    /// Check if a specific peer is reachable.
    async fn is_reachable(&self, peer_id: &str) -> bool;

    /// Subscribe to peer events (join, leave, endpoint change).
    fn subscribe(&self) -> broadcast::Receiver<MeshEvent>;
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub id: String,                 // machine hostname or ID
    pub wireguard_pubkey: String,
    pub endpoint: Option<SocketAddr>,
    pub mesh_ip: IpAddr,            // WireGuard tunnel IP
    pub last_handshake: Option<chrono::DateTime<chrono::Utc>>,
    pub connected: bool,
}

#[derive(Debug, Clone)]
pub enum MeshEvent {
    PeerJoined(PeerInfo),
    PeerLeft { id: String },
    PeerEndpointChanged { id: String, new_endpoint: SocketAddr },
}
```

## Nostr Event Schema

Peer discovery events published to Nostr relays:

```json
{
  "kind": 30078,
  "tags": [
    ["d", "<mesh_identifier>"],
    ["machine", "<hostname>"],
    ["wg_pubkey", "<wireguard_public_key>"],
    ["endpoint", "<ip:port>"],
    ["mesh_ip", "<wireguard_tunnel_ip>"]
  ],
  "content": "",
  "created_at": 1711234567
}
```

- **kind 30078**: parameterized replaceable event (NIP-33). Each machine publishes one, replaces on endpoint change.
- **mesh_identifier**: pre-shared secret identifying the mesh. Only machines with the same identifier discover each other.
- **content**: empty. All data in tags. Signed with the machine's Nostr keypair.

## Configuration

```toml
[mesh]
identifier = "my-mesh-2026"              # shared mesh identifier
wireguard_interface = "kith0"            # WireGuard interface name
listen_port = 51820                      # WireGuard listen port
mesh_cidr = "10.47.0.0/24"              # IP range for mesh members

[mesh.nostr]
relays = [
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.nostr.band",
]

[mesh.derp]
# Optional DERP relay for NAT-hostile environments
# url = "https://derp.example.com"
```
