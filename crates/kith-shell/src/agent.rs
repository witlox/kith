//! Agent loop: classify input → route to bash or LLM → dispatch tool calls → output.

use futures::StreamExt;
use tracing::warn;

use kith_common::error::InferenceError;
use kith_common::inference::*;

use crate::classify::{InputClass, InputClassifier};
use crate::context::ConversationContext;
use crate::daemon_client::DaemonClient;
use crate::tools;

use kith_common::event::EventScope;
use kith_state::retrieval::KeywordRetriever;
use kith_sync::store::{EventFilter, EventStore};

/// Result of processing one user input.
#[derive(Debug)]
pub enum AgentOutput {
    /// Command was pass-through — executed directly, here's the output.
    PassThrough {
        command: String,
        stdout: String,
        stderr: String,
        exit_code: i32,
    },
    /// LLM produced text response.
    Text(String),
    /// LLM produced tool calls that were executed.
    ToolResults(Vec<ToolResult>),
    /// Inference backend unavailable — degraded to pass-through.
    Degraded { input: String },
    /// Error during processing.
    Error(String),
}

#[derive(Debug)]
pub struct ToolResult {
    pub tool_name: String,
    pub output: String,
}

/// The agent — holds state across turns.
pub struct Agent {
    classifier: InputClassifier,
    context: ConversationContext,
    backend: Box<dyn InferenceBackend>,
    daemon: Option<DaemonClient>,
    event_store: EventStore,
    todos: Vec<TodoItem>,
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
}

impl Agent {
    pub fn new(backend: Box<dyn InferenceBackend>, system_prompt: String) -> Self {
        let mut context = ConversationContext::new(50);
        context.set_system_prompt(system_prompt);

        Self {
            classifier: InputClassifier::from_path_env(),
            context,
            backend,
            daemon: None,
            event_store: EventStore::new(),
            todos: Vec::new(),
        }
    }

    pub fn set_daemon(&mut self, daemon: DaemonClient) {
        self.daemon = Some(daemon);
    }

    pub fn backend_name(&self) -> &str {
        self.backend.name()
    }

    pub fn classifier(&self) -> &InputClassifier {
        &self.classifier
    }

    /// Process one line of user input. Returns what happened.
    pub async fn process(&mut self, input: &str) -> AgentOutput {
        match self.classifier.classify(input) {
            InputClass::PassThrough(cmd) => {
                if cmd.is_empty() {
                    return AgentOutput::PassThrough {
                        command: String::new(),
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 0,
                    };
                }
                self.exec_local(&cmd).await
            }
            InputClass::Intent(text) => self.process_intent(&text).await,
        }
    }

    async fn exec_local(&self, command: &str) -> AgentOutput {
        match kith_daemon::exec::exec_command(command).await {
            Ok(result) => AgentOutput::PassThrough {
                command: command.into(),
                stdout: result.stdout,
                stderr: result.stderr,
                exit_code: result.exit_code,
            },
            Err(e) => AgentOutput::Error(e.to_string()),
        }
    }

    async fn process_intent(&mut self, input: &str) -> AgentOutput {
        self.context.add_user(input.into());

        let tool_defs = tools::native_tools();
        let config = InferenceConfig::default();

        let stream_result = self
            .backend
            .complete(self.context.messages(), &tool_defs, &config)
            .await;

        let mut stream = match stream_result {
            Ok(s) => s,
            Err(InferenceError::Unreachable(_) | InferenceError::Timeout(_)) => {
                warn!("inference unavailable — pass-through mode");
                return AgentOutput::Degraded {
                    input: input.into(),
                };
            }
            Err(e) => return AgentOutput::Error(e.to_string()),
        };

        let mut text_output = String::new();
        let mut tool_calls = Vec::new();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::TextDelta(t)) => {
                    text_output.push_str(&t);
                }
                Ok(StreamChunk::ThinkingDelta(_)) => {
                    // Thinking is visible but not part of the response
                }
                Ok(StreamChunk::ToolCall(tc)) => {
                    tool_calls.push(tc);
                }
                Ok(StreamChunk::Done { .. }) => break,
                Err(e) => return AgentOutput::Error(e.to_string()),
            }
        }

        if tool_calls.is_empty() {
            self.context
                .add_assistant(MessageContent::Text(text_output.clone()));
            return AgentOutput::Text(text_output);
        }

        // Dispatch tool calls
        self.context
            .add_assistant(MessageContent::ToolCalls(tool_calls.clone()));
        let mut results = Vec::new();

        for tc in &tool_calls {
            let output = self.dispatch_tool(tc).await;
            self.context.add_tool_result(tc.id.clone(), output.clone());
            results.push(ToolResult {
                tool_name: tc.name.clone(),
                output,
            });
        }

        AgentOutput::ToolResults(results)
    }

    async fn dispatch_tool(&mut self, tc: &ToolCall) -> String {
        match tc.name.as_str() {
            "remote" => {
                let _host = tc
                    .arguments
                    .get("host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let command = tc
                    .arguments
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if let Some(ref mut daemon) = self.daemon {
                    match daemon.exec(command).await {
                        Ok(result) => {
                            if result.exit_code == 0 {
                                result.stdout
                            } else {
                                format!(
                                    "exit code {}: {}{}",
                                    result.exit_code, result.stdout, result.stderr
                                )
                            }
                        }
                        Err(e) => format!("error: {e}"),
                    }
                } else {
                    // No daemon connected — exec locally as fallback
                    match kith_daemon::exec::exec_command(command).await {
                        Ok(r) => r.stdout,
                        Err(e) => format!("error: {e}"),
                    }
                }
            }
            "fleet_query" => {
                let query = tc
                    .arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Query local event store with keyword matching (FS-06)
                let all = self.event_store.all().await;
                let results = if query.is_empty() {
                    // No query — return recent events
                    let events = self
                        .event_store
                        .query(&EventFilter {
                            limit: Some(20),
                            ..Default::default()
                        })
                        .await;
                    events
                        .iter()
                        .map(|e| {
                            format!(
                                "[{}] {} on {}: {}",
                                e.event_type, e.category, e.machine, e.detail
                            )
                        })
                        .collect::<Vec<_>>()
                } else {
                    // Keyword search over all events
                    let search = KeywordRetriever::search(&all, query, &EventScope::Ops, 20);
                    search
                        .iter()
                        .map(|r| {
                            format!(
                                "[{:.1}] {} on {}: {}",
                                r.score, r.event.event_type, r.event.machine, r.event.detail
                            )
                        })
                        .collect::<Vec<_>>()
                };

                // Also query daemon if connected
                if let Some(ref mut daemon) = self.daemon
                    && let Ok(state) = daemon.query().await {
                        let mut output = results.join("\n");
                        if !output.is_empty() {
                            output.push('\n');
                        }
                        output.push_str(&format!("[daemon] {}", state));
                        return output;
                    }

                if results.is_empty() {
                    format!("no events for query: {query}")
                } else {
                    results.join("\n")
                }
            }
            "retrieve" => {
                let query = tc
                    .arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let all = self.event_store.all().await;
                let results = KeywordRetriever::search(&all, query, &EventScope::Ops, 10);
                if results.is_empty() {
                    format!("no results for: {query}")
                } else {
                    let summaries: Vec<String> = results
                        .iter()
                        .map(|r| {
                            format!(
                                "[{:.1}] {} on {}: {}",
                                r.score, r.event.event_type, r.event.machine, r.event.detail
                            )
                        })
                        .collect();
                    summaries.join("\n")
                }
            }
            "apply" => {
                let host = tc
                    .arguments
                    .get("host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("local");
                let command = tc
                    .arguments
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if let Some(ref mut daemon) = self.daemon {
                    match daemon.apply(command, 600).await {
                        Ok(id) => format!("applied on {host}, pending_id: {id}"),
                        Err(e) => format!("error: {e}"),
                    }
                } else {
                    format!("no daemon connected for apply on {host}")
                }
            }
            "commit" => {
                let pending_id = tc.arguments.get("pending_id").and_then(|v| v.as_str());
                if let (Some(daemon), Some(id)) = (&mut self.daemon, pending_id) {
                    match daemon.commit(id).await {
                        Ok(true) => "committed".into(),
                        Ok(false) => "commit failed".into(),
                        Err(e) => format!("error: {e}"),
                    }
                } else {
                    "commit: no daemon or pending_id".into()
                }
            }
            "rollback" => {
                let pending_id = tc.arguments.get("pending_id").and_then(|v| v.as_str());
                if let (Some(daemon), Some(id)) = (&mut self.daemon, pending_id) {
                    match daemon.rollback(id).await {
                        Ok(true) => "rolled back".into(),
                        Ok(false) => "rollback failed".into(),
                        Err(e) => format!("error: {e}"),
                    }
                } else {
                    "rollback: no daemon or pending_id".into()
                }
            }
            "todo" => {
                let action = tc
                    .arguments
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("list");
                let text = tc
                    .arguments
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match action {
                    "add" => {
                        self.todos.push(TodoItem {
                            text: text.into(),
                            done: false,
                        });
                        format!("added: {text}")
                    }
                    "list" => {
                        if self.todos.is_empty() {
                            "no todos".into()
                        } else {
                            self.todos
                                .iter()
                                .enumerate()
                                .map(|(i, t)| {
                                    let mark = if t.done { "x" } else { " " };
                                    format!("[{mark}] {}: {}", i + 1, t.text)
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        }
                    }
                    "done" => {
                        if let Some(item) =
                            self.todos.iter_mut().find(|t| t.text == text && !t.done)
                        {
                            item.done = true;
                            format!("done: {text}")
                        } else {
                            format!("not found: {text}")
                        }
                    }
                    "clear" => {
                        let count = self.todos.len();
                        self.todos.clear();
                        format!("cleared {count} todos")
                    }
                    _ => format!("unknown todo action: {action}"),
                }
            }
            other => format!("unknown tool: {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_backend::MockInferenceBackend;

    fn make_agent(backend: MockInferenceBackend) -> Agent {
        Agent::new(Box::new(backend), "you are a test agent".into())
    }

    #[tokio::test]
    async fn passthrough_command() {
        let backend = MockInferenceBackend::new("test");
        let mut agent = make_agent(backend);

        let output = agent.process("echo hello").await;
        match output {
            AgentOutput::PassThrough {
                stdout, exit_code, ..
            } => {
                assert_eq!(stdout.trim(), "hello");
                assert_eq!(exit_code, 0);
            }
            other => panic!("expected PassThrough, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn escape_hatch() {
        let backend = MockInferenceBackend::new("test");
        let mut agent = make_agent(backend);

        let output = agent.process("run: echo escaped").await;
        match output {
            AgentOutput::PassThrough { stdout, .. } => {
                assert_eq!(stdout.trim(), "escaped");
            }
            other => panic!("expected PassThrough, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn intent_produces_text() {
        let backend = MockInferenceBackend::new("test");
        backend.queue_text("I can help with that.");
        let mut agent = make_agent(backend);

        let output = agent.process("what's the meaning of life?").await;
        match output {
            AgentOutput::Text(t) => assert!(t.contains("help")),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn intent_produces_tool_call() {
        let backend = MockInferenceBackend::new("test");
        backend.queue_tool_call(
            "remote",
            serde_json::json!({"host": "local", "command": "echo tool-test"}),
        );
        let mut agent = make_agent(backend);

        let output = agent.process("check what's running").await;
        match output {
            AgentOutput::ToolResults(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].tool_name, "remote");
                assert!(results[0].output.contains("tool-test"));
            }
            other => panic!("expected ToolResults, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backend_unavailable_degrades() {
        let backend = MockInferenceBackend::new("test");
        backend.set_healthy(false);
        let mut agent = make_agent(backend);

        let output = agent.process("what's using port 3000?").await;
        assert!(matches!(output, AgentOutput::Degraded { .. }));
    }

    #[tokio::test]
    async fn context_accumulates() {
        let backend = MockInferenceBackend::new("test");
        backend.queue_text("first response");
        backend.queue_text("second response");
        let mut agent = make_agent(backend);

        agent.process("first question").await;
        agent.process("second question").await;

        // System + 2 user + 2 assistant = 5
        assert_eq!(agent.context.len(), 5);
    }

    #[tokio::test]
    async fn empty_input() {
        let backend = MockInferenceBackend::new("test");
        let mut agent = make_agent(backend);

        let output = agent.process("").await;
        assert!(matches!(
            output,
            AgentOutput::PassThrough { exit_code: 0, .. }
        ));
    }
}
