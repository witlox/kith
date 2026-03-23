Feature: Mesh networking
  Machines discover each other via Nostr signaling and establish
  WireGuard tunnels for encrypted peer-to-peer communication.

  Scenario: Two machines form a mesh
    Given "dev-mac" and "staging-1" run kith-daemons with the same mesh identifier
    When both publish WireGuard keys and endpoints to Nostr
    Then each discovers the other via Nostr subscription
    And a WireGuard tunnel is established
    And gRPC connectivity is verified

  Scenario: Machine changes network — re-signaling
    Given "dev-mac" and "staging-1" are connected
    When "dev-mac" moves to a new network
    Then "dev-mac" publishes an updated endpoint to Nostr
    And the tunnel re-establishes to the new endpoint

  Scenario: Nostr relays unavailable — cached endpoints
    Given all Nostr relays are unreachable
    And neither machine has changed network
    Then the existing WireGuard tunnel remains active

  Scenario: New machine joins
    Given "dev-mac" and "staging-1" are connected
    When "prod-1" starts with the same mesh identifier
    Then all three establish pairwise WireGuard tunnels
    And cr-sqlite sync begins between all three

  Scenario: Direct connection fails — relay fallback
    Given NAT hole-punching fails between two machines
    And a DERP relay is configured
    Then traffic routes through the relay
    And the connection remains end-to-end encrypted
