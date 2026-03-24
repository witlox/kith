//! Event types — the fundamental unit of state in kith.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub machine: String,
    pub category: EventCategory,
    pub event_type: String,
    pub path: Option<String>,
    pub detail: String,
    pub metadata: serde_json::Value,
    pub scope: EventScope,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventCategory {
    Drift,
    Exec,
    Apply,
    Commit,
    Rollback,
    Policy,
    Mesh,
    Capability,
    System,
}

impl std::fmt::Display for EventCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventScope {
    Public,
    Ops,
}

impl Event {
    pub fn new(
        machine: impl Into<String>,
        category: EventCategory,
        event_type: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            machine: machine.into(),
            category,
            event_type: event_type.into(),
            path: None,
            detail: detail.into(),
            metadata: serde_json::Value::Null,
            scope: EventScope::Ops,
            timestamp: Utc::now(),
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_scope(mut self, scope: EventScope) -> Self {
        self.scope = scope;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_builder() {
        let e = Event::new(
            "staging-1",
            EventCategory::Drift,
            "drift.file_changed",
            "config modified",
        )
        .with_path("/etc/nginx/conf.d/api.conf")
        .with_scope(EventScope::Public)
        .with_metadata(serde_json::json!({"change": "modified"}));

        assert_eq!(e.machine, "staging-1");
        assert_eq!(e.category, EventCategory::Drift);
        assert_eq!(e.event_type, "drift.file_changed");
        assert_eq!(e.path.as_deref(), Some("/etc/nginx/conf.d/api.conf"));
        assert_eq!(e.scope, EventScope::Public);
        assert!(!e.id.is_empty());
    }

    #[test]
    fn event_serialization_roundtrip() {
        let e = Event::new(
            "dev-mac",
            EventCategory::Exec,
            "exec.command",
            "ran docker ps",
        );
        let json = serde_json::to_string(&e).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.machine, "dev-mac");
        assert_eq!(parsed.category, EventCategory::Exec);
    }

    #[test]
    fn event_ids_are_unique() {
        let e1 = Event::new("m", EventCategory::System, "t", "d");
        let e2 = Event::new("m", EventCategory::System, "t", "d");
        assert_ne!(e1.id, e2.id);
    }
}
