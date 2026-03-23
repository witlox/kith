Feature: Remote command execution
  The agent executes commands on remote machines via kith-daemon,
  with authentication, policy enforcement, and audit logging.

  Scenario: Agent executes a command on a remote machine
    Given kith shell is running on "dev-mac"
    And "staging-1" is a mesh member with a running kith-daemon
    And the user has "ops" scope on "staging-1"
    When the user types "check what's running on staging-1 port 8000"
    Then the agent calls remote("staging-1", "lsof -i :8000")
    And kith-daemon on "staging-1" authenticates the request
    And kith-daemon verifies "ops" scope permits "lsof"
    And the command output streams back to kith shell
    And an audit entry is written on "staging-1"

  Scenario: Policy denies a remote command
    Given the user has "viewer" scope on "prod-1"
    When the agent calls remote("prod-1", "systemctl restart nginx")
    Then kith-daemon on "prod-1" rejects with "policy denied: viewer scope cannot execute state-changing commands"
    And an audit entry records the denial

  Scenario: Remote machine is unreachable
    Given "staging-1" is not reachable via the mesh
    When the agent calls remote("staging-1", "docker ps")
    Then the tool returns "staging-1 unreachable"

  Scenario: Streaming output from long-running remote command
    Given "staging-1" is reachable
    When the agent calls remote("staging-1", "docker build -t myapp .")
    Then output streams back incrementally via gRPC streaming
    And the user sees real-time build progress
