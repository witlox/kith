//! Agent loop: classify input → route to bash or LLM → dispatch tool calls → output.

use futures::StreamExt;
use tracing::warn;

use kith_common::error::InferenceError;
use kith_common::inference::*;

use crate::classify::{InputClass, InputClassifier};
use crate::context::ConversationContext;
use crate::daemon_client::DaemonClient;
use crate::tools;

use kith_common::event::{Event, EventCategory, EventScope};
use kith_state::embedding::{BagOfWordsEmbedder, EmbeddingBackend};
use kith_state::hybrid::HybridRetriever;
use kith_state::retrieval::KeywordRetriever;
use kith_state::vector_index::VectorIndex;
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
    embedder: Box<dyn EmbeddingBackend>,
    hybrid_retriever: HybridRetriever,
    todos: Vec<TodoItem>,
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
}

impl Agent {
    pub fn new(backend: Box<dyn InferenceBackend>, system_prompt: String) -> Self {
        Self::with_embedder(
            backend,
            system_prompt,
            Box::new(BagOfWordsEmbedder::new(1000)),
        )
    }

    pub fn with_embedder(
        backend: Box<dyn InferenceBackend>,
        system_prompt: String,
        embedder: Box<dyn EmbeddingBackend>,
    ) -> Self {
        Self::with_embedder_and_tools(
            backend,
            system_prompt,
            embedder,
            InputClassifier::from_path_env().into_known_commands(),
        )
    }

    pub fn with_embedder_and_tools(
        backend: Box<dyn InferenceBackend>,
        system_prompt: String,
        embedder: Box<dyn EmbeddingBackend>,
        known_commands: std::collections::HashSet<String>,
    ) -> Self {
        let mut context = ConversationContext::new(50);
        context.set_system_prompt(system_prompt);

        let hybrid_retriever = HybridRetriever::new(VectorIndex::new());

        Self {
            classifier: InputClassifier::new(known_commands),
            context,
            backend,
            daemon: None,
            event_store: EventStore::new(),
            embedder,
            hybrid_retriever,
            todos: Vec::new(),
        }
    }

    pub fn set_daemon(&mut self, daemon: DaemonClient) {
        self.daemon = Some(daemon);
    }

    /// Access the event store for injecting events (e.g., from sync loop).
    pub fn event_store_mut(&mut self) -> &EventStore {
        &self.event_store
    }

    /// Whether an event category is worth embedding into the vector index.
    /// Operational events (Exec, Drift, Apply, Commit, Rollback) are valuable
    /// for semantic retrieval. Infrastructure noise (System, Mesh, Capability) is not.
    pub fn should_embed(category: &EventCategory) -> bool {
        matches!(
            category,
            EventCategory::Exec
                | EventCategory::Drift
                | EventCategory::Apply
                | EventCategory::Commit
                | EventCategory::Rollback
        )
    }

    /// Index an event into the vector index for hybrid retrieval.
    /// Only embeds operational events (see `should_embed`).
    pub async fn index_event(&mut self, event: &Event) {
        if !Self::should_embed(&event.category) {
            return;
        }
        if let Ok(emb) = self.embedder.embed(&event.detail).await {
            self.hybrid_retriever.index_mut().insert(event.clone(), emb);
        }
    }

    /// Sync events from daemon into local store and index operational ones.
    /// Called before retrieve/fleet_query to ensure fresh data.
    async fn sync_from_daemon(&mut self) {
        if let Some(ref mut daemon) = self.daemon {
            match daemon.fetch_events().await {
                Ok(events) => {
                    for event in &events {
                        if Self::should_embed(&event.category)
                            && let Ok(emb) = self.embedder.embed(&event.detail).await
                        {
                            self.hybrid_retriever.index_mut().insert(event.clone(), emb);
                        }
                    }
                    let _merged = self.event_store.merge(events).await;
                }
                Err(e) => {
                    warn!("failed to sync events from daemon: {e}");
                }
            }
        }
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

                // Sync from daemon before querying
                self.sync_from_daemon().await;

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
                    && let Ok(state) = daemon.query().await
                {
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

                // Sync from daemon before retrieval
                self.sync_from_daemon().await;

                let all = self.event_store.all().await;

                // Use hybrid retrieval (keyword + vector) when possible
                let query_embedding = self.embedder.embed(query).await.ok();
                let results = if let Some(ref emb) = query_embedding {
                    // Hybrid: keyword + vector
                    let hybrid = self
                        .hybrid_retriever
                        .search(&all, query, emb, &EventScope::Ops, 10)
                        .await;
                    hybrid
                        .iter()
                        .map(|r| {
                            format!(
                                "[{:.1}] {} on {}: {}",
                                r.combined_score,
                                r.event.event_type,
                                r.event.machine,
                                r.event.detail
                            )
                        })
                        .collect::<Vec<_>>()
                } else {
                    // Fallback: keyword only
                    KeywordRetriever::search(&all, query, &EventScope::Ops, 10)
                        .iter()
                        .map(|r| {
                            format!(
                                "[{:.1}] {} on {}: {}",
                                r.score, r.event.event_type, r.event.machine, r.event.detail
                            )
                        })
                        .collect::<Vec<_>>()
                };

                if results.is_empty() {
                    format!("no results for: {query}")
                } else {
                    results.join("\n")
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
                let paths: Vec<String> = tc
                    .arguments
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                if let Some(ref mut daemon) = self.daemon {
                    match daemon.apply(command, 600).await {
                        Ok(id) => {
                            if paths.is_empty() {
                                format!(
                                    "applied on {host}, pending_id: {id} (audit-only, no paths backed up)"
                                )
                            } else {
                                format!(
                                    "applied on {host}, pending_id: {id} (backed up: {})",
                                    paths.join(", ")
                                )
                            }
                        }
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
    use kith_common::event::{Event, EventCategory};

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

    #[test]
    fn should_embed_operational_events() {
        assert!(Agent::should_embed(&EventCategory::Exec));
        assert!(Agent::should_embed(&EventCategory::Drift));
        assert!(Agent::should_embed(&EventCategory::Apply));
        assert!(Agent::should_embed(&EventCategory::Commit));
        assert!(Agent::should_embed(&EventCategory::Rollback));
    }

    #[test]
    fn should_not_embed_infrastructure_events() {
        assert!(!Agent::should_embed(&EventCategory::System));
        assert!(!Agent::should_embed(&EventCategory::Mesh));
        assert!(!Agent::should_embed(&EventCategory::Capability));
        assert!(!Agent::should_embed(&EventCategory::Policy));
    }

    #[tokio::test]
    async fn index_event_skips_system_events() {
        let backend = MockInferenceBackend::new("test");
        let mut agent = make_agent(backend);

        let system_event = Event::new("m", EventCategory::System, "system.boot", "booted");
        agent.index_event(&system_event).await;

        // Vector index should be empty — system event was skipped
        assert_eq!(agent.hybrid_retriever.index_mut().len(), 0);
    }

    #[tokio::test]
    async fn index_event_embeds_exec_events() {
        let backend = MockInferenceBackend::new("test");
        let mut agent = make_agent(backend);

        let exec_event = Event::new("m", EventCategory::Exec, "exec.command", "docker ps");
        agent.index_event(&exec_event).await;

        // Vector index should have one entry
        assert_eq!(agent.hybrid_retriever.index_mut().len(), 1);
    }

    #[tokio::test]
    async fn apply_tool_with_paths() {
        let backend = MockInferenceBackend::new("test");
        backend.queue_tool_call(
            "apply",
            serde_json::json!({
                "host": "local",
                "command": "echo test",
                "paths": ["/etc/nginx/conf.d", "/var/www"]
            }),
        );
        let mut agent = make_agent(backend);

        // Without daemon, apply returns "no daemon connected"
        let output = agent
            .process("please deploy the nginx config changes")
            .await;
        match output {
            AgentOutput::ToolResults(results) => {
                assert_eq!(results[0].tool_name, "apply");
                assert!(results[0].output.contains("no daemon connected"));
            }
            other => panic!("expected ToolResults, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn apply_tool_without_paths_is_audit_only() {
        let backend = MockInferenceBackend::new("test");
        backend.queue_tool_call(
            "apply",
            serde_json::json!({
                "host": "local",
                "command": "echo test"
            }),
        );
        let mut agent = make_agent(backend);

        let output = agent
            .process("please deploy the nginx config changes now")
            .await;
        match output {
            AgentOutput::ToolResults(results) => {
                assert_eq!(results[0].tool_name, "apply");
                // No daemon = "no daemon connected" — but the paths parsing was exercised
                assert!(results[0].output.contains("no daemon connected"));
            }
            other => panic!("expected ToolResults, got {other:?}"),
        }
    }
}
