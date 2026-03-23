# ADR-003: Nostr for Signaling, WireGuard for Transport

## Status: Accepted

## Context

Machines in the mesh need to discover each other and establish encrypted connections. They may be behind NAT, on different networks, or on the move (laptop at home vs. office).

Options considered:
- **Headscale/Tailscale**: excellent UX, requires running a coordination server
- **libp2p**: fully decentralized, complex, heavy dependency
- **Matrix**: rich protocol, high latency for signaling
- **Nostr**: simple signed events on public relays, minimal protocol
- **Plain WireGuard**: works but requires manual peer configuration

## Decision

**Nostr for peer discovery and signaling. WireGuard for encrypted transport.**

- Each kith-daemon publishes a signed Nostr event (kind 30078, parameterized replaceable) containing its WireGuard public key and current endpoint
- Events are tagged with a shared mesh identifier (pre-shared secret)
- Other daemons subscribe to the mesh tag and establish WireGuard tunnels
- When a machine's endpoint changes, it publishes an updated event
- Multiple Nostr relays for redundancy (default: 3+)
- Optional DERP relay for NAT-hostile environments

## Consequences

**Positive:**
- No coordination server to run (sovereignty)
- Nostr relays are public, free, redundant — dozens available
- WireGuard is proven, fast, kernel-level encryption
- Signaling overhead is minimal (a few hundred bytes per peer per update)
- Mesh identifier limits discoverability to authorized machines

**Negative:**
- Public Nostr relays can see that your machines exist (but not traffic content — that's WireGuard)
- NAT hole-punching depends on WireGuard; if it fails, need a DERP relay (which requires hosting something)
- Nostr relays can go down — mitigated by using multiple relays

**Why not Headscale:**
- Requires running and maintaining a coordination server
- Adds an external dependency that can become a single point of failure
- Overkill for a 3-10 machine personal mesh

## Validation

Test peer discovery via Nostr from 3+ network configurations (same LAN, across NAT, mobile hotspot). Measure signaling latency (target: <5s per ASM-NET-3). Test relay fallback when Nostr relays are unavailable (ASM-NET-1).
