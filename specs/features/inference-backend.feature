Feature: InferenceBackend abstraction
  The InferenceBackend trait makes kith model-agnostic. Any LLM with
  tool calling and streaming works. The rest of the system never
  touches the LLM directly. Swapping models is a config change.

  Scenario: Agent uses a hosted API backend
    Given kith shell is configured with backend "anthropic/claude-sonnet"
    And the Anthropic API is reachable
    When the user types an intent
    Then kith shell calls InferenceBackend with the input and available tools
    And the backend streams a response with tool calls
    And tool calls execute via the normal dispatch path

  Scenario: Agent uses a self-hosted backend
    Given kith shell is configured with backend "openai-compatible/self-hosted"
    And the endpoint is "http://gpu-server:8000/v1"
    When the user types an intent
    Then kith shell calls the same InferenceBackend trait
    And the backend streams a response from the self-hosted model
    And the rest of the system behaves identically

  Scenario: Model swap is a config change
    Given kith shell is running with backend "anthropic/claude-sonnet"
    When the config is changed to "openai-compatible/self-hosted-model"
    And kith shell is restarted
    Then the new backend is used for all inference
    And no other component (daemon, mesh, sync, state) is affected

  Scenario: InferenceBackend returns tool calls in a standard format
    Given any backend is configured
    When the model produces a tool call for remote("staging-1", "docker ps")
    Then the tool call is returned as a structured object with tool name and arguments
    And the dispatch layer handles it without knowing which model produced it

  Scenario: Streaming output reaches the user incrementally
    Given any backend is configured
    When the model generates a long response
    Then tokens stream to the terminal as they are produced
    And tool call boundaries are detected in the stream

  Scenario: Backend unavailable degrades to bash
    Given any backend is configured
    And the backend becomes unreachable (network failure, GPU busy)
    When the user types an intent
    Then kith shell shows "inference unavailable - pass-through mode"
    And the raw input is passed to bash
    And local operations continue normally

  Scenario: Backend returns malformed response
    Given any backend is configured
    When the backend returns an unparseable response
    Then kith shell logs the error
    And retries once
    And if retry fails, surfaces the error to the user
    And does not pass malformed data to tool dispatch

  Scenario: No model-specific logic outside InferenceBackend implementations
    Given the kith codebase
    Then no code in kith-daemon references any specific model or provider
    And no code in kith-mesh references any specific model or provider
    And no code in kith-sync references any specific model or provider
    And no code in kith-state references any specific model or provider
    And only kith-shell contains InferenceBackend implementations

  Scenario: System prompt is backend-appropriate
    Given backend "anthropic/claude-sonnet" is configured
    Then the system prompt may use backend-specific formatting hints
    When the backend is changed to "openai-compatible/self-hosted"
    Then the system prompt adjusts formatting for the new backend
    And the behavioral instructions remain identical

  Scenario: Thinking/reasoning content is handled gracefully
    Given a backend that produces reasoning traces (thinking tokens)
    When the model reasons before a tool call
    Then the reasoning is rendered in the terminal (collapsible)
    Given a backend that does not produce reasoning traces
    When the model makes a tool call
    Then the absence of reasoning is handled gracefully with no errors
