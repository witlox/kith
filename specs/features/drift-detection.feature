Feature: Drift detection
  Kith-daemon detects differences between expected and actual state
  on a machine. Drift is surfaced as events, never silently corrected.
  The user or agent decides how to handle drift.

  Background:
    Given kith-daemon is running on "staging-1"
    And "staging-1" has a declared state from its last commit

  Scenario: File change detected as drift
    When the file "/etc/nginx/conf.d/api.conf" is modified outside kith
    Then kith-daemon detects drift in the "files" category
    And a drift event is written to the local cr-sqlite store
    And the drift event includes the path "/etc/nginx/conf.d/api.conf"
    And the drift event includes a timestamp

  Scenario: Service state change detected as drift
    Given the declared state expects "postgres" to be running
    When "postgres" stops unexpectedly
    Then kith-daemon detects drift in the "services" category
    And the drift magnitude increases

  Scenario: Network change detected as drift
    Given the declared state expects port 8080 to be listening
    When port 8080 is no longer listening
    Then kith-daemon detects drift in the "network" category

  Scenario: Package change detected as drift
    When a package is installed or removed outside kith
    Then kith-daemon detects drift in the "packages" category

  Scenario: Blacklisted paths are excluded from drift
    Given the blacklist includes "/tmp/**" and "/var/log/**"
    When a file is modified at "/tmp/scratch/output.txt"
    Then no drift event is generated

  Scenario: Drift magnitude is computed from weighted categories
    Given drift weights are configured as files=1.0, services=2.0, network=1.5, packages=1.0
    And 2 file changes and 1 service change have been detected
    Then the squared drift magnitude is 8.0
    And the drift vector shows files=2.0, services=1.0, network=0.0, packages=0.0

  Scenario: Drift resets after commit
    Given drift has been detected on "staging-1"
    When the user commits the current state
    Then the drift vector resets to zero
    And an audit entry records the commit

  Scenario: Drift is surfaced to the agent via fleet query
    Given "staging-1" has drift magnitude 3.5
    When the agent on "dev-mac" calls fleet_query("what's the state of things?")
    Then the response includes "staging-1" with drift magnitude 3.5
    And the response includes which categories have drifted

  Scenario: Drift during mesh partition
    Given "staging-1" is partitioned from the mesh
    And drift accumulates on "staging-1"
    When connectivity is restored
    Then all drift events sync to peers via cr-sqlite
    And peers see the full drift history with timestamps

  Scenario: Drift event carries enough context for the agent to reason
    When kith-daemon detects a file change at "/etc/nginx/conf.d/api.conf"
    Then the drift event metadata includes the category "files"
    And the drift event metadata includes the path
    And the drift event metadata includes the timestamp
    And the drift event metadata includes the machine hostname "staging-1"
    And the agent can retrieve this event via retrieve("nginx config change on staging")
