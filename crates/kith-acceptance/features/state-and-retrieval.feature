Feature: Distributed state sync and context retrieval
  Operational events sync across the mesh via CRDTs.
  The agent retrieves context from a vector index over synced state.

  Scenario: Local event syncs to peer
    Given "dev-mac" and "staging-1" have active sync
    When a command executes on "dev-mac" and is ingested
    Then within 5 seconds the event appears in cr-sqlite on "staging-1"

  Scenario: Agent retrieves cross-machine context
    Given "staging-1" had a deployment failure logged 2 hours ago
    And the event is synced and embedded on "dev-mac"
    When the user on "dev-mac" types "why is staging broken?"
    Then retrieve() returns the failure event from "staging-1"

  Scenario: Fleet query returns structured state
    Given three machines are in the mesh
    When the agent calls fleet_query("what machines are in the mesh?")
    Then it receives each machine's hostname, capabilities, and last-sync timestamp

  Scenario: Partition and recovery
    Given "dev-mac" and "staging-1" lose connectivity
    And events accumulate independently on both
    When connectivity is restored
    Then cr-sqlite merges all events on both machines with no data loss

  Scenario: Retrieval respects permission scope
    Given "prod-1" has events tagged with "security" scope
    And the user has "engineering" scope
    When the agent calls retrieve("recent security events on prod")
    Then metadata is returned but content is withheld
    And the agent reports "3 entries exist but require security scope"
