Feature: Tool Discovery
  The system discovers available tools on the local machine and remote
  mesh members, categorizes them, tracks versions, and makes this
  information available to the LLM via the system prompt and to
  peers via the Capabilities RPC.

  Background:
    Given a kith shell running on a machine with PATH set

  # ── Shell-side tool scanning ──

  Scenario: Shell scans PATH on startup and builds tool registry
    Given PATH contains "/usr/bin" with "git", "curl", "python3"
    And PATH contains "/usr/local/bin" with "docker", "cargo"
    When the shell starts
    Then the tool registry contains "git", "curl", "python3", "docker", "cargo"

  Scenario: Tool registry categorizes tools by function
    Given PATH contains "git", "docker", "python3", "nginx", "cargo", "kubectl"
    When the shell starts
    Then the tool registry categorizes "git" as "vcs"
    And the tool registry categorizes "docker" as "container"
    And the tool registry categorizes "python3" as "language"
    And the tool registry categorizes "nginx" as "server"
    And the tool registry categorizes "cargo" as "build"
    And the tool registry categorizes "kubectl" as "container"

  Scenario: Tool registry detects versions for key tools
    Given PATH contains "git" which reports version "git version 2.45.0"
    And PATH contains "docker" which reports version "Docker version 27.1.1"
    When the shell starts
    Then the tool registry records "git" version "2.45.0"
    And the tool registry records "docker" version "27.1.1"

  Scenario: Unknown tools are categorized as "other"
    Given PATH contains "my-custom-script"
    When the shell starts
    Then the tool registry categorizes "my-custom-script" as "other"

  # ── System prompt enrichment ──

  Scenario: System prompt includes categorized tool summary
    Given the tool registry contains: git (vcs, 2.45.0), docker (container, 27.1.1), python3 (language, 3.12.0), cargo (build)
    When the system prompt is assembled
    Then the prompt includes a "Available tools" section
    And the section groups tools by category
    And the section includes version numbers where known

  Scenario: System prompt stays under size budget with many tools
    Given the tool registry contains 500 tools
    When the system prompt is assembled
    Then the prompt is under 4000 characters
    And only categorized tools with versions are listed individually
    And remaining tools are summarized as a count per category

  # ── Daemon-side capability scanning ──

  Scenario: Daemon scans PATH for real capability report
    Given a kith-daemon running on a machine with PATH set
    When a Capabilities request is received
    Then the response includes all executables found in PATH
    And the response categorizes tools by function
    And the response includes versions for key tools

  Scenario: Daemon capability report includes system resources
    Given a kith-daemon running on a Linux machine
    When a Capabilities request is received
    Then the response includes CPU count, memory total, memory available
    And the response includes disk total, disk available

  # ── Re-scan ──

  Scenario: Shell re-scans PATH on explicit command
    Given the tool registry was built at startup
    And a new tool "helm" has been installed into PATH
    When the user types "run: hash -r" or the agent triggers a rescan
    Then the tool registry is rebuilt
    And "helm" appears in the tool registry

  Scenario: Daemon re-scans on Capabilities request
    Given a kith-daemon with a cached capability report
    And 5 minutes have elapsed since the last scan
    When a Capabilities request is received
    Then the daemon performs a fresh PATH scan
    And the response reflects current tools

  # ── Cross-machine visibility ──

  Scenario: Shell sees remote machine tools via daemon
    Given the shell is connected to a daemon on "staging-1"
    And "staging-1" has "nvidia-smi" in PATH
    When the user asks "what GPU tools are available on staging-1?"
    Then the agent can retrieve the capability report for "staging-1"
    And the response includes "nvidia-smi"

  # ── Platform differences ──

  Scenario: macOS tool scan finds Homebrew tools
    Given the shell is running on macOS
    And PATH contains "/opt/homebrew/bin" with "brew", "python3"
    When the shell starts
    Then the tool registry includes "brew" and "python3"

  Scenario: Linux tool scan finds system and snap tools
    Given the shell is running on Linux
    And PATH contains "/usr/bin", "/snap/bin"
    When the shell starts
    Then the tool registry includes tools from both directories
