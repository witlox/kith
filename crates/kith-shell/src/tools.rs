//! Native tool definitions — the 7 tools that don't exist in Unix.
//! Everything else is standard bash via PTY (INV-OPS-4).

use kith_common::inference::ToolDefinition;

/// Build the set of native tool definitions passed to the InferenceBackend.
pub fn native_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "remote".into(),
            description: "Execute a command on a remote machine via kith-daemon".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "Target machine hostname" },
                    "command": { "type": "string", "description": "Command to execute" }
                },
                "required": ["host", "command"]
            }),
        },
        ToolDefinition {
            name: "fleet_query".into(),
            description: "Query synced state across the mesh".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to query about the fleet" }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "retrieve".into(),
            description: "Semantic search over operational history".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to search for" }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "apply".into(),
            description: "Make a change with commit window semantics".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "Target machine" },
                    "command": { "type": "string", "description": "Change to apply" }
                },
                "required": ["host", "command"]
            }),
        },
        ToolDefinition {
            name: "commit".into(),
            description: "Commit pending changes".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pending_id": { "type": "string", "description": "Specific pending change ID, or omit for commit_all" }
                }
            }),
        },
        ToolDefinition {
            name: "rollback".into(),
            description: "Rollback pending changes".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pending_id": { "type": "string", "description": "Specific pending change ID, or omit for rollback_all" }
                }
            }),
        },
        ToolDefinition {
            name: "todo".into(),
            description: "Agent self-managed task tracking".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["add", "list", "done", "clear"],
                        "description": "Action to perform"
                    },
                    "text": { "type": "string", "description": "Task text (for add/done)" }
                },
                "required": ["action"]
            }),
        },
    ]
}

/// Check if a tool name is a native tool.
pub fn is_native_tool(name: &str) -> bool {
    matches!(
        name,
        "remote" | "fleet_query" | "retrieve" | "apply" | "commit" | "rollback" | "todo"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_tools_count() {
        assert_eq!(native_tools().len(), 7);
    }

    #[test]
    fn all_tools_have_names_and_descriptions() {
        for tool in native_tools() {
            assert!(!tool.name.is_empty());
            assert!(!tool.description.is_empty());
        }
    }

    #[test]
    fn all_tools_have_valid_json_schema_params() {
        for tool in native_tools() {
            assert!(
                tool.parameters.is_object(),
                "{} params should be object",
                tool.name
            );
            assert_eq!(
                tool.parameters["type"], "object",
                "{} params should have type=object",
                tool.name
            );
        }
    }

    #[test]
    fn is_native_tool_recognizes_all() {
        for tool in native_tools() {
            assert!(is_native_tool(&tool.name), "{} should be native", tool.name);
        }
    }

    #[test]
    fn is_native_tool_rejects_unix_commands() {
        assert!(!is_native_tool("ls"));
        assert!(!is_native_tool("grep"));
        assert!(!is_native_tool("docker"));
        assert!(!is_native_tool("git"));
    }
}
