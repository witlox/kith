Feature: Local command execution
  Kith shell processes user input and executes commands locally,
  either as pass-through (literal) or intent (LLM-reasoned).
  The LLM backend is accessed via InferenceBackend and is model-agnostic.

  Scenario: Pass-through command executes with no LLM involvement
    Given kith shell is running
    When the user types "ls -la"
    Then the command executes directly via bash
    And the output appears within 5ms of a raw terminal
    And the ingest daemon captures the command and output

  Scenario: Escape hatch forces pass-through
    Given kith shell is running
    When the user types "run: rm -rf /tmp/test"
    Then the command "rm -rf /tmp/test" executes directly via bash
    And no InferenceBackend call is made

  Scenario: Intent is routed to the LLM
    Given kith shell is running
    And the InferenceBackend is reachable
    When the user types "find all Python files that import requests"
    Then kith shell calls InferenceBackend with the user's input and available tools
    And the model produces a tool call for bash execution
    And the command executes via PTY
    And the output is returned to the user

  Scenario: InferenceBackend unavailable degrades to bash
    Given kith shell is running
    And the InferenceBackend is unreachable
    When the user types "find all Python files that import requests"
    Then kith shell shows "inference unavailable — pass-through mode"
    And the raw input is passed to bash

  Scenario: Model swap does not affect local execution
    Given kith shell is running with backend "anthropic/claude-sonnet"
    And the user successfully executes an intent-based command
    When the backend is changed to "openai-compatible/self-hosted-model"
    And the user executes the same intent-based command
    Then the command succeeds with the new backend
    And no other component is aware of the backend change
