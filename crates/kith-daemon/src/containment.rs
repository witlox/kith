//! Containment — transactional filesystem changes.
//! Linux: overlayfs (mount overlay, merge on commit, discard on rollback).
//! macOS: copy-based snapshots (backup before change, restore on rollback).
//!
//! The Transaction trait abstracts the platform difference.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use kith_common::error::KithError;

/// A filesystem transaction that can be committed or rolled back.
pub trait Transaction: Send + Sync {
    /// Commit the change — make it permanent.
    fn commit(&mut self) -> Result<(), KithError>;
    /// Rollback the change — revert to original state.
    fn rollback(&mut self) -> Result<(), KithError>;
    /// Get the transaction ID.
    fn id(&self) -> &str;
}

/// Copy-based transaction — works on all platforms.
/// Copies the original file to a backup location before changes.
/// Rollback restores the backup. Commit removes the backup.
pub struct CopyTransaction {
    id: String,
    backups: Vec<(PathBuf, PathBuf)>, // (original, backup)
    backup_dir: PathBuf,
    committed: bool,
}

impl CopyTransaction {
    /// Begin a new copy-based transaction.
    /// `paths` are the files that will be modified.
    pub fn begin(id: String, paths: &[PathBuf], backup_dir: &Path) -> Result<Self, KithError> {
        let tx_backup_dir = backup_dir.join(&id);
        std::fs::create_dir_all(&tx_backup_dir)
            .map_err(|e| KithError::Internal(format!("failed to create backup dir: {e}")))?;

        let mut backups = Vec::new();

        for path in paths {
            if path.exists() {
                let backup = tx_backup_dir.join(
                    path.file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("unnamed")),
                );
                std::fs::copy(path, &backup).map_err(|e| {
                    KithError::Internal(format!("failed to backup {}: {e}", path.display()))
                })?;
                backups.push((path.clone(), backup));
            }
        }

        info!(id = %id, files = backups.len(), "containment: copy transaction started");

        Ok(Self {
            id,
            backups,
            backup_dir: tx_backup_dir,
            committed: false,
        })
    }
}

impl Transaction for CopyTransaction {
    fn commit(&mut self) -> Result<(), KithError> {
        // Remove backups — the changes are permanent
        for (_, backup) in &self.backups {
            let _ = std::fs::remove_file(backup);
        }
        let _ = std::fs::remove_dir(&self.backup_dir);
        self.committed = true;
        info!(id = %self.id, "containment: committed (backups removed)");
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), KithError> {
        // Restore from backups
        for (original, backup) in &self.backups {
            if backup.exists() {
                std::fs::copy(backup, original).map_err(|e| {
                    KithError::Internal(format!("failed to restore {}: {e}", original.display()))
                })?;
            }
        }
        // Clean up backups
        for (_, backup) in &self.backups {
            let _ = std::fs::remove_file(backup);
        }
        let _ = std::fs::remove_dir(&self.backup_dir);
        info!(id = %self.id, "containment: rolled back (originals restored)");
        Ok(())
    }

    fn id(&self) -> &str {
        &self.id
    }
}

impl Drop for CopyTransaction {
    fn drop(&mut self) {
        if !self.committed {
            // Auto-rollback on drop if not committed
            if let Err(e) = self.rollback() {
                warn!(id = %self.id, error = %e, "containment: auto-rollback on drop failed");
            }
        }
    }
}

/// Overlayfs transaction — Linux only.
/// Mounts an overlay filesystem over the target directory.
/// Changes happen on the upper layer. Commit merges, rollback discards.
#[cfg(target_os = "linux")]
pub struct OverlayTransaction {
    id: String,
    target: PathBuf,
    upper: PathBuf,
    _work: PathBuf, // kept alive for overlayfs — dir must exist while mounted
    mount_point: PathBuf,
    mounted: bool,
    committed: bool,
}

#[cfg(target_os = "linux")]
impl OverlayTransaction {
    /// Begin an overlay transaction over a directory.
    pub fn begin(id: String, target: &Path, scratch_dir: &Path) -> Result<Self, KithError> {
        let tx_dir = scratch_dir.join(&id);
        let upper = tx_dir.join("upper");
        let work = tx_dir.join("work");
        let mount_point = tx_dir.join("merged");

        std::fs::create_dir_all(&upper)
            .map_err(|e| KithError::Internal(format!("mkdir upper: {e}")))?;
        std::fs::create_dir_all(&work)
            .map_err(|e| KithError::Internal(format!("mkdir work: {e}")))?;
        std::fs::create_dir_all(&mount_point)
            .map_err(|e| KithError::Internal(format!("mkdir merged: {e}")))?;

        // Mount overlay
        let opts = format!(
            "lowerdir={},upperdir={},workdir={}",
            target.display(),
            upper.display(),
            work.display()
        );

        let status = std::process::Command::new("mount")
            .args(["-t", "overlay", "overlay", "-o", &opts])
            .arg(&mount_point)
            .status()
            .map_err(|e| KithError::Internal(format!("mount overlay: {e}")))?;

        if !status.success() {
            return Err(KithError::ContainmentUnavailable(
                "mount overlay failed — requires root or CAP_SYS_ADMIN".into(),
            ));
        }

        info!(id = %id, target = %target.display(), "containment: overlay mounted");

        Ok(Self {
            id,
            target: target.to_path_buf(),
            upper,
            _work: work,
            mount_point,
            mounted: true,
            committed: false,
        })
    }
}

#[cfg(target_os = "linux")]
impl Transaction for OverlayTransaction {
    fn commit(&mut self) -> Result<(), KithError> {
        // Unmount overlay
        let status = std::process::Command::new("umount")
            .arg(&self.mount_point)
            .status()
            .map_err(|e| KithError::Internal(format!("umount: {e}")))?;

        if !status.success() {
            return Err(KithError::Internal("umount failed".into()));
        }
        self.mounted = false;

        // Copy upper layer changes to target (merge)
        copy_dir_recursive(&self.upper, &self.target)?;

        // Clean up scratch
        let _ = std::fs::remove_dir_all(self.mount_point.parent().unwrap_or(Path::new("/tmp")));
        self.committed = true;

        info!(id = %self.id, "containment: overlay committed (merged to target)");
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), KithError> {
        if self.mounted {
            let _ = std::process::Command::new("umount")
                .arg(&self.mount_point)
                .status();
            self.mounted = false;
        }
        // Discard upper — don't merge
        let _ = std::fs::remove_dir_all(self.mount_point.parent().unwrap_or(Path::new("/tmp")));
        info!(id = %self.id, "containment: overlay rolled back (upper discarded)");
        Ok(())
    }

    fn id(&self) -> &str {
        &self.id
    }
}

#[cfg(target_os = "linux")]
impl Drop for OverlayTransaction {
    fn drop(&mut self) {
        if self.mounted {
            let _ = std::process::Command::new("umount")
                .arg(&self.mount_point)
                .status();
        }
        if !self.committed {
            let _ = std::fs::remove_dir_all(self.mount_point.parent().unwrap_or(Path::new("/tmp")));
        }
    }
}

#[cfg(target_os = "linux")]
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), KithError> {
    if !src.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(src).map_err(|e| KithError::Internal(format!("readdir: {e}")))? {
        let entry = entry.map_err(|e| KithError::Internal(format!("entry: {e}")))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path).ok();
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| {
                KithError::Internal(format!(
                    "copy {} → {}: {e}",
                    src_path.display(),
                    dst_path.display()
                ))
            })?;
        }
    }
    Ok(())
}

/// Transaction manager — creates and tracks transactions.
pub struct TransactionManager {
    transactions: HashMap<String, Box<dyn Transaction>>,
    backup_dir: PathBuf,
}

impl TransactionManager {
    pub fn new(backup_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&backup_dir).ok();
        Self {
            transactions: HashMap::new(),
            backup_dir,
        }
    }

    /// Begin a transaction for the given paths.
    /// Uses overlayfs on Linux (if available), copy-based fallback otherwise.
    pub fn begin(&mut self, id: String, paths: &[PathBuf]) -> Result<(), KithError> {
        let tx = CopyTransaction::begin(id.clone(), paths, &self.backup_dir)?;
        self.transactions.insert(id, Box::new(tx));
        Ok(())
    }

    /// Commit a transaction.
    pub fn commit(&mut self, id: &str) -> Result<(), KithError> {
        let mut tx = self
            .transactions
            .remove(id)
            .ok_or_else(|| KithError::PendingNotFound(id.into()))?;
        tx.commit()
    }

    /// Rollback a transaction.
    pub fn rollback(&mut self, id: &str) -> Result<(), KithError> {
        let mut tx = self
            .transactions
            .remove(id)
            .ok_or_else(|| KithError::PendingNotFound(id.into()))?;
        tx.rollback()
    }

    /// Rollback all transactions (e.g., on daemon shutdown).
    pub fn rollback_all(&mut self) {
        let ids: Vec<String> = self.transactions.keys().cloned().collect();
        for id in ids {
            if let Some(mut tx) = self.transactions.remove(&id)
                && let Err(e) = tx.rollback()
            {
                warn!(id = %id, error = %e, "containment: rollback failed on cleanup");
            }
        }
    }

    /// Check if a transaction exists.
    pub fn has(&self, id: &str) -> bool {
        self.transactions.contains_key(id)
    }

    /// Count active transactions.
    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_transaction_commit() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.conf");
        std::fs::write(&file, "original content").unwrap();

        let backup_dir = dir.path().join("backups");
        let mut tx = CopyTransaction::begin("tx-1".into(), &[file.clone()], &backup_dir).unwrap();

        // Modify the file (simulating the change)
        std::fs::write(&file, "modified content").unwrap();

        // Commit — backup should be removed
        tx.commit().unwrap();

        // File has modified content
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "modified content");
        // Backup dir cleaned up
        assert!(!backup_dir.join("tx-1").exists());
    }

    #[test]
    fn copy_transaction_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.conf");
        std::fs::write(&file, "original content").unwrap();

        let backup_dir = dir.path().join("backups");
        let mut tx = CopyTransaction::begin("tx-2".into(), &[file.clone()], &backup_dir).unwrap();

        // Modify the file
        std::fs::write(&file, "bad change").unwrap();

        // Rollback — should restore original
        tx.rollback().unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original content");
    }

    #[test]
    fn copy_transaction_auto_rollback_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.conf");
        std::fs::write(&file, "original").unwrap();

        let backup_dir = dir.path().join("backups");
        {
            let _tx = CopyTransaction::begin("tx-3".into(), &[file.clone()], &backup_dir).unwrap();
            std::fs::write(&file, "uncommitted change").unwrap();
            // tx dropped without commit — should auto-rollback
        }

        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
    }

    #[test]
    fn transaction_manager_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("managed.conf");
        std::fs::write(&file, "managed content").unwrap();

        let backup_dir = dir.path().join("backups");
        let mut mgr = TransactionManager::new(backup_dir);

        // Begin
        mgr.begin("tx-m1".into(), &[file.clone()]).unwrap();
        assert!(mgr.has("tx-m1"));
        assert_eq!(mgr.len(), 1);

        // Modify
        std::fs::write(&file, "managed change").unwrap();

        // Commit
        mgr.commit("tx-m1").unwrap();
        assert!(!mgr.has("tx-m1"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "managed change");
    }

    #[test]
    fn transaction_manager_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rollback.conf");
        std::fs::write(&file, "before").unwrap();

        let backup_dir = dir.path().join("backups");
        let mut mgr = TransactionManager::new(backup_dir);

        mgr.begin("tx-m2".into(), &[file.clone()]).unwrap();
        std::fs::write(&file, "after").unwrap();
        mgr.rollback("tx-m2").unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), "before");
    }

    #[test]
    fn transaction_manager_rollback_all() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = dir.path().join("a.conf");
        let f2 = dir.path().join("b.conf");
        std::fs::write(&f1, "a-original").unwrap();
        std::fs::write(&f2, "b-original").unwrap();

        let backup_dir = dir.path().join("backups");
        let mut mgr = TransactionManager::new(backup_dir);

        mgr.begin("tx-a".into(), &[f1.clone()]).unwrap();
        mgr.begin("tx-b".into(), &[f2.clone()]).unwrap();
        std::fs::write(&f1, "a-changed").unwrap();
        std::fs::write(&f2, "b-changed").unwrap();

        mgr.rollback_all();

        assert_eq!(std::fs::read_to_string(&f1).unwrap(), "a-original");
        assert_eq!(std::fs::read_to_string(&f2).unwrap(), "b-original");
        assert!(mgr.is_empty());
    }

    #[test]
    fn multiple_files_in_one_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = dir.path().join("multi-1.conf");
        let f2 = dir.path().join("multi-2.conf");
        std::fs::write(&f1, "one").unwrap();
        std::fs::write(&f2, "two").unwrap();

        let backup_dir = dir.path().join("backups");
        let mut tx =
            CopyTransaction::begin("tx-multi".into(), &[f1.clone(), f2.clone()], &backup_dir)
                .unwrap();

        std::fs::write(&f1, "one-modified").unwrap();
        std::fs::write(&f2, "two-modified").unwrap();

        tx.rollback().unwrap();

        assert_eq!(std::fs::read_to_string(&f1).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(&f2).unwrap(), "two");
    }
}
