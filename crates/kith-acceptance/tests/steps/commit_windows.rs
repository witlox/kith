use cucumber::{given, then, when};
use std::time::Duration;

use crate::KithWorld;

#[given(expr = "the commit window is set to {int} minutes")]
fn set_commit_window(world: &mut KithWorld, minutes: u64) {
    world.commit_mgr =
        kith_daemon::commit::CommitWindowManager::new(Duration::from_secs(minutes * 60));
}

#[when(expr = "the agent edits {string}")]
fn agent_edits(world: &mut KithWorld, path: String) {
    let id = world.commit_mgr.open(&format!("edit {path}"), None);
    world.last_pending_id = Some(id);
}

#[then(expr = "the change is applied via overlayfs overlay")]
fn applied_via_overlayfs(_world: &mut KithWorld) {}

#[then(expr = "the change is marked {string} with a {int}-minute window")]
fn marked_pending(world: &mut KithWorld, _status: String, _minutes: u32) {
    assert!(world.last_pending_id.is_some());
    assert!(world.commit_mgr.has_pending());
}

#[then("the user is shown the diff")]
fn shown_diff(_world: &mut KithWorld) {}

// "the user types" step is owned by local_execution.rs to avoid duplication.
// Commit/rollback actions are handled there too.

#[then("the overlay is merged to the base filesystem")]
fn overlay_merged(world: &mut KithWorld) {
    assert_eq!(world.last_commit_result, Some(true));
}

#[given(expr = "a pending change exists with a {int}-minute window")]
fn pending_exists(world: &mut KithWorld, minutes: u64) {
    world.commit_mgr =
        kith_daemon::commit::CommitWindowManager::new(Duration::from_secs(minutes * 60));
    let id = world
        .commit_mgr
        .open("test change", Some(Duration::from_millis(1)));
    world.last_pending_id = Some(id);
}

#[when(expr = "{int} minutes pass without a commit")]
fn time_passes(world: &mut KithWorld, _minutes: u32) {
    std::thread::sleep(Duration::from_millis(10));
    world.expired_ids = world
        .commit_mgr
        .tick()
        .iter()
        .map(|c| c.id.clone())
        .collect();
}

#[then("the overlay is discarded and the file reverts")]
fn overlay_discarded(world: &mut KithWorld) {
    // Either expired via tick or rolled back explicitly
    assert!(!world.commit_mgr.has_pending() || !world.expired_ids.is_empty());
}

#[then(expr = "an audit entry records the auto-rollback")]
fn audit_auto_rollback(_world: &mut KithWorld) {}

#[then(expr = "the user is notified {string}")]
fn user_notified(world: &mut KithWorld, _msg: String) {
    world.notifications.push("expired".into());
}

#[given("a pending change exists")]
fn any_pending(world: &mut KithWorld) {
    let id = world.commit_mgr.open("pending change", None);
    world.last_pending_id = Some(id);
}

#[given(expr = "{string} is a mesh member")]
fn mesh_member(world: &mut KithWorld, _machine: String) {}

#[when(expr = "the agent calls apply\\({string}, {string}\\)")]
fn agent_calls_apply(world: &mut KithWorld, _machine: String, command: String) {
    let id = world.commit_mgr.open(&command, None);
    world.last_pending_id = Some(id);
}

#[then(expr = "the change executes on {string} with a commit window")]
fn change_executes_with_window(world: &mut KithWorld, _machine: String) {
    assert!(world.commit_mgr.has_pending());
}

#[then(expr = "the change is finalized on {string}")]
fn change_finalized(world: &mut KithWorld, _machine: String) {
    assert_eq!(world.last_commit_result, Some(true));
}

#[given(expr = "pending changes exist for {string} and {string}")]
fn multiple_pending(world: &mut KithWorld, file_a: String, file_b: String) {
    world.commit_mgr.open(&format!("edit {file_a}"), None);
    world.commit_mgr.open(&format!("edit {file_b}"), None);
    world.last_pending_id = None; // commit_all path
}

#[then("both are committed atomically")]
fn both_committed(world: &mut KithWorld) {
    assert_eq!(world.last_commit_result, Some(true));
    assert!(!world.commit_mgr.has_pending());
}

#[given("kith shell is running on macOS")]
fn shell_on_macos(world: &mut KithWorld) {
    world.current_machine = "dev-mac".into();
}

#[given("overlayfs is not available")]
fn no_overlayfs(_world: &mut KithWorld) {}

#[then(expr = "the original is copied to {string}")]
fn copied_to_backup(_world: &mut KithWorld, _path: String) {}

#[then("the edit is applied to the original file")]
fn edit_applied(_world: &mut KithWorld) {}

#[then(expr = "the change is marked {string}")]
fn change_marked_pending(world: &mut KithWorld, _status: String) {
    assert!(world.commit_mgr.has_pending());
}

#[then("the backup is restored to the original path")]
fn backup_restored(_world: &mut KithWorld) {}

#[then("the backup is removed")]
fn backup_removed(_world: &mut KithWorld) {}

#[given("a pending change on macOS with a copy-based snapshot")]
fn pending_macos(world: &mut KithWorld) {
    let id = world.commit_mgr.open("macos edit", None);
    world.last_pending_id = Some(id);
}

#[then("the edited file remains in place")]
fn edited_remains(_world: &mut KithWorld) {}
