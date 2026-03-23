Feature: Policy enforcement
  Per-machine, per-user access rules enforced by kith-daemon in Rust code.
  Policy is never enforced in the LLM prompt. The daemon rejects
  unauthorized requests regardless of what the model asks for.

  Background:
    Given kith-daemon is running on "staging-1"
    And "staging-1" has a policy configuration

  Scenario: Ops user can execute commands
    Given user "pim" has "ops" scope on "staging-1"
    When the agent sends an exec request for "docker ps" as "pim"
    Then kith-daemon allows the execution
    And an audit entry records the allowed exec

  Scenario: Viewer user can query state but not execute
    Given user "intern" has "viewer" scope on "staging-1"
    When the agent sends a query request as "intern"
    Then kith-daemon returns the machine state
    When the agent sends an exec request for "docker restart api" as "intern"
    Then kith-daemon rejects with "policy denied: viewer scope cannot execute commands"
    And an audit entry records the denial

  Scenario: Unauthenticated request is rejected
    When a request arrives without valid credentials
    Then kith-daemon rejects with "authentication required"
    And an audit entry records the rejection

  Scenario: Expired credentials are rejected
    Given user "pim" has expired credentials
    When the agent sends an exec request as "pim"
    Then kith-daemon rejects with "credentials expired"

  Scenario: Policy is enforced for state-changing operations
    Given user "pim" has "ops" scope on "staging-1"
    When the agent calls apply("staging-1", "systemctl restart nginx")
    Then kith-daemon checks "pim" has "ops" scope
    And the apply proceeds with a commit window

  Scenario: Policy is enforced independently of the model
    Given user "intern" has "viewer" scope on "staging-1"
    When the InferenceBackend produces a tool call for exec("staging-1", "rm -rf /data")
    Then kith-daemon rejects based on policy
    And the model's request is irrelevant to the policy decision

  Scenario: Different users have different scopes on the same machine
    Given user "pim" has "ops" scope on "staging-1"
    And user "intern" has "viewer" scope on "staging-1"
    When "pim" sends an exec request for "docker ps"
    Then it succeeds
    When "intern" sends an exec request for "docker ps"
    Then it is denied

  Scenario: Policy denial produces an audit entry
    Given any policy denial occurs
    Then the audit entry includes who requested, what was requested, which machine, and the denial reason

  Scenario: Policy configuration is per-machine
    Given user "pim" has "ops" scope on "staging-1"
    And user "pim" has "viewer" scope on "prod-1"
    When "pim" sends an exec request to "staging-1"
    Then it succeeds
    When "pim" sends an exec request to "prod-1"
    Then it is denied with "viewer scope cannot execute commands"

  Scenario: Scope determines what fleet_query reveals
    Given user "intern" has "viewer" scope on "staging-1"
    And "staging-1" has events tagged with "ops" scope
    When "intern" calls fleet_query about "staging-1"
    Then metadata is returned but ops-scoped content is withheld
    And the response indicates restricted entries exist
