# Mesh Networking

## Overview

Kith machines form a peer-to-peer mesh with no coordination server. Peer discovery uses Nostr relay signaling. Transport uses WireGuard tunnels.

## Nostr Signaling (ADR-003)

Each machine publishes a parameterized replaceable event (kind 30078) to public Nostr relays. The event contains:
- Machine's WireGuard public key
- Endpoint address (IP + port)
- Mesh identifier (for filtering)
- Ed25519 signature

Peers subscribe to events matching the mesh identifier and discover each other's WireGuard endpoints.

## WireGuard Transport

Direct P2P WireGuard tunnels for all mesh traffic. Uses `defguard_wireguard_rs` with boringtun userspace backend for portability.

### IP Allocation

IPv6 ULA (Unique Local Address) is the default mesh addressing:
- Prefix: `fd00:kith::{mesh_id}::/64`
- Each peer gets a deterministic address derived from its public key
- IPv4 fallback (10.x.x.x) for environments without IPv6

## Peer Registry

The `PeerRegistry` maintains the current mesh topology:
- Known peers with their WireGuard endpoints
- Connection state (connected, unreachable, stale)
- Last-seen timestamps

## Partition Tolerance

During mesh partition:
- Local operations continue unaffected
- Remote tools return "unreachable" errors
- Event sync pauses and resumes on reconnection
- cr-sqlite CRDT merge resolves conflicts automatically
