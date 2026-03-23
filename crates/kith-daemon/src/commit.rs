//! Commit window manager — pending changes with auto-rollback on expiry.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use kith_common::error::KithError;
use kith_common::types::PendingChange;

pub struct CommitWindowManager {
    pending: HashMap<String, PendingChange>,
    default_duration: Duration,
}

impl CommitWindowManager {
    pub fn new(default_duration: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            default_duration,
        }
    }

    /// Open a new pending change. Returns pending_id.
    pub fn open(&mut self, command: &str, duration: Option<Duration>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let dur = duration.unwrap_or(self.default_duration);
        let expires_at = now + dur;

        self.pending.insert(
            id.clone(),
            PendingChange {
                id: id.clone(),
                command: command.into(),
                created_at: now,
                expires_at,
            },
        );

        id
    }

    /// Commit a single pending change.
    pub fn commit(&mut self, pending_id: &str) -> Result<PendingChange, KithError> {
        self.pending
            .remove(pending_id)
            .ok_or_else(|| KithError::PendingNotFound(pending_id.into()))
    }

    /// Commit all pending changes atomically (F-06).
    pub fn commit_all(&mut self) -> Vec<PendingChange> {
        let committed: Vec<PendingChange> = self.pending.drain().map(|(_, v)| v).collect();
        committed
    }

    /// Rollback a single pending change.
    pub fn rollback(&mut self, pending_id: &str) -> Result<PendingChange, KithError> {
        self.pending
            .remove(pending_id)
            .ok_or_else(|| KithError::PendingNotFound(pending_id.into()))
    }

    /// Rollback all pending changes.
    pub fn rollback_all(&mut self) -> Vec<PendingChange> {
        self.pending.drain().map(|(_, v)| v).collect()
    }

    /// Check for expired windows and auto-rollback. Returns expired IDs.
    pub fn tick(&mut self) -> Vec<PendingChange> {
        let now = Utc::now();
        let expired_ids: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, p)| now >= p.expires_at)
            .map(|(id, _)| id.clone())
            .collect();

        expired_ids
            .iter()
            .filter_map(|id| self.pending.remove(id))
            .collect()
    }

    /// List pending changes.
    pub fn pending(&self) -> Vec<&PendingChange> {
        self.pending.values().collect()
    }

    /// Get a specific pending change.
    pub fn get(&self, pending_id: &str) -> Option<&PendingChange> {
        self.pending.get(pending_id)
    }

    /// Check if any changes are pending.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_pending_change() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(600));
        let id = mgr.open("docker compose up -d", None);
        assert!(!id.is_empty());
        assert!(mgr.has_pending());
        assert_eq!(mgr.pending().len(), 1);
        assert_eq!(mgr.get(&id).unwrap().command, "docker compose up -d");
    }

    #[test]
    fn commit_removes_pending() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(600));
        let id = mgr.open("cmd", None);
        let change = mgr.commit(&id).unwrap();
        assert_eq!(change.command, "cmd");
        assert!(!mgr.has_pending());
    }

    #[test]
    fn commit_unknown_id_errors() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(600));
        let result = mgr.commit("nonexistent");
        assert!(matches!(result, Err(KithError::PendingNotFound(_))));
    }

    #[test]
    fn rollback_removes_pending() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(600));
        let id = mgr.open("cmd", None);
        let change = mgr.rollback(&id).unwrap();
        assert_eq!(change.command, "cmd");
        assert!(!mgr.has_pending());
    }

    #[test]
    fn commit_all_removes_all() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(600));
        mgr.open("cmd-a", None);
        mgr.open("cmd-b", None);
        mgr.open("cmd-c", None);
        let committed = mgr.commit_all();
        assert_eq!(committed.len(), 3);
        assert!(!mgr.has_pending());
    }

    #[test]
    fn rollback_all_removes_all() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(600));
        mgr.open("cmd-a", None);
        mgr.open("cmd-b", None);
        let rolled = mgr.rollback_all();
        assert_eq!(rolled.len(), 2);
        assert!(!mgr.has_pending());
    }

    #[test]
    fn tick_expires_old_changes() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(0)); // instant expiry
        mgr.open("cmd", None);
        // The change was created with duration 0, so it's already expired
        std::thread::sleep(std::time::Duration::from_millis(10));
        let expired = mgr.tick();
        assert_eq!(expired.len(), 1);
        assert!(!mgr.has_pending());
    }

    #[test]
    fn tick_does_not_expire_fresh_changes() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(600));
        mgr.open("cmd", None);
        let expired = mgr.tick();
        assert!(expired.is_empty());
        assert!(mgr.has_pending());
    }

    #[test]
    fn custom_duration_per_change() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(600));
        let id = mgr.open("cmd", Some(Duration::from_secs(0)));
        std::thread::sleep(std::time::Duration::from_millis(10));
        let expired = mgr.tick();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, id);
    }

    #[test]
    fn multiple_changes_independent_expiry() {
        let mut mgr = CommitWindowManager::new(Duration::from_secs(600));
        let _id_long = mgr.open("long", Some(Duration::from_secs(600)));
        let _id_short = mgr.open("short", Some(Duration::from_secs(0)));
        std::thread::sleep(std::time::Duration::from_millis(10));
        let expired = mgr.tick();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].command, "short");
        assert_eq!(mgr.pending().len(), 1);
        assert_eq!(mgr.pending()[0].command, "long");
    }
}
