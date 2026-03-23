//! Audit log — append-only local event store.
//! Every state-changing action produces an entry (INV-SEC-4).

use kith_common::event::{Event, EventCategory, EventScope};

/// Append-only audit log. In production this writes to cr-sqlite.
/// For testing, this is an in-memory vec.
pub struct AuditLog {
    entries: Vec<Event>,
    machine: String,
}

impl AuditLog {
    pub fn new(machine: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            machine: machine.into(),
        }
    }

    /// Record an exec event (command executed or denied).
    pub fn record_exec(
        &mut self,
        user: &str,
        command: &str,
        exit_code: Option<i32>,
        denied_reason: Option<&str>,
    ) {
        let (event_type, category) = if denied_reason.is_some() {
            ("exec.denied", EventCategory::Policy)
        } else {
            ("exec.command", EventCategory::Exec)
        };

        let mut metadata = serde_json::json!({
            "command": command,
            "user": user,
        });
        if let Some(code) = exit_code {
            metadata["exit_code"] = serde_json::json!(code);
        }
        if let Some(reason) = denied_reason {
            metadata["reason"] = serde_json::json!(reason);
        }

        let event = Event::new(&self.machine, category, event_type, command)
            .with_metadata(metadata)
            .with_scope(EventScope::Ops);

        self.entries.push(event);
    }

    /// Record a change event (applied, committed, rolled back, expired).
    pub fn record_change(&mut self, event_type: &str, pending_id: &str, user: &str) {
        let category = match event_type {
            "change.applied" => EventCategory::Apply,
            "change.committed" => EventCategory::Commit,
            "change.rolled_back" | "change.expired" => EventCategory::Rollback,
            _ => EventCategory::System,
        };

        let event = Event::new(&self.machine, category, event_type, pending_id)
            .with_metadata(serde_json::json!({
                "pending_id": pending_id,
                "user": user,
            }))
            .with_scope(EventScope::Ops);

        self.entries.push(event);
    }

    /// Record a system event.
    pub fn record_system(&mut self, event_type: &str, detail: &str) {
        let event = Event::new(&self.machine, EventCategory::System, event_type, detail)
            .with_scope(EventScope::Ops);
        self.entries.push(event);
    }

    /// Get all entries (for testing and querying).
    pub fn entries(&self) -> &[Event] {
        &self.entries
    }

    /// Get entries filtered by scope.
    pub fn entries_for_scope(&self, scope: &EventScope) -> Vec<&Event> {
        self.entries
            .iter()
            .filter(|e| match scope {
                EventScope::Ops => true, // ops sees everything
                EventScope::Public => e.scope == EventScope::Public,
            })
            .collect()
    }

    /// Count entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_exec_success() {
        let mut log = AuditLog::new("staging-1");
        log.record_exec("pim", "docker ps", Some(0), None);

        assert_eq!(log.len(), 1);
        let entry = &log.entries()[0];
        assert_eq!(entry.event_type, "exec.command");
        assert_eq!(entry.category, EventCategory::Exec);
        assert!(entry.metadata["command"].as_str().unwrap().contains("docker ps"));
        assert_eq!(entry.metadata["exit_code"], 0);
    }

    #[test]
    fn record_exec_denied() {
        let mut log = AuditLog::new("staging-1");
        log.record_exec("intern", "rm -rf /", None, Some("viewer scope cannot execute"));

        let entry = &log.entries()[0];
        assert_eq!(entry.event_type, "exec.denied");
        assert_eq!(entry.category, EventCategory::Policy);
        assert!(entry.metadata["reason"].as_str().unwrap().contains("viewer"));
    }

    #[test]
    fn record_change_lifecycle() {
        let mut log = AuditLog::new("staging-1");
        log.record_change("change.applied", "abc-123", "pim");
        log.record_change("change.committed", "abc-123", "pim");

        assert_eq!(log.len(), 2);
        assert_eq!(log.entries()[0].category, EventCategory::Apply);
        assert_eq!(log.entries()[1].category, EventCategory::Commit);
    }

    #[test]
    fn record_change_expired() {
        let mut log = AuditLog::new("staging-1");
        log.record_change("change.expired", "abc-123", "system");

        assert_eq!(log.entries()[0].event_type, "change.expired");
        assert_eq!(log.entries()[0].category, EventCategory::Rollback);
    }

    #[test]
    fn scope_filtering() {
        let mut log = AuditLog::new("staging-1");
        log.record_exec("pim", "docker ps", Some(0), None); // Ops scope
        log.record_system("system.daemon_started", "v0.1.0"); // Ops scope

        // Ops sees everything
        assert_eq!(log.entries_for_scope(&EventScope::Ops).len(), 2);
        // Public sees nothing (all entries are Ops-scoped)
        assert_eq!(log.entries_for_scope(&EventScope::Public).len(), 0);
    }

    #[test]
    fn audit_entries_have_unique_ids() {
        let mut log = AuditLog::new("staging-1");
        log.record_exec("pim", "cmd1", Some(0), None);
        log.record_exec("pim", "cmd2", Some(0), None);
        assert_ne!(log.entries()[0].id, log.entries()[1].id);
    }

    #[test]
    fn audit_entries_have_machine_name() {
        let mut log = AuditLog::new("prod-1");
        log.record_exec("pim", "cmd", Some(0), None);
        assert_eq!(log.entries()[0].machine, "prod-1");
    }
}
