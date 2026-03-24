//! State observer — watches machine state for drift-producing changes.
//! Cross-platform polling approach (no inotify dependency).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tokio::sync::mpsc;
use tracing::{debug, warn};

use kith_common::drift::DriftCategory;

use crate::drift::ObserverEvent;

/// Polls files for mtime changes. Produces ObserverEvents when files change.
pub struct FileObserver {
    watch_paths: Vec<PathBuf>,
    state: HashMap<PathBuf, SystemTime>,
    interval: Duration,
}

impl FileObserver {
    pub fn new(watch_paths: Vec<PathBuf>, interval: Duration) -> Self {
        Self {
            watch_paths,
            state: HashMap::new(),
            interval,
        }
    }

    /// Run the observer loop, sending events on changes.
    pub async fn run(mut self, tx: mpsc::Sender<ObserverEvent>) {
        // Initial scan — record current state
        self.scan_initial();

        loop {
            tokio::time::sleep(self.interval).await;

            let paths: Vec<PathBuf> = self.watch_paths.clone();
            for path in &paths {
                self.check_path(path, &tx).await;
            }
        }
    }

    fn scan_initial(&mut self) {
        for path in &self.watch_paths {
            if let Ok(meta) = std::fs::metadata(path) {
                if let Ok(mtime) = meta.modified() {
                    self.state.insert(path.clone(), mtime);
                }
            }
            // Also scan directory contents if it's a dir
            if path.is_dir() {
                if let Ok(entries) = std::fs::read_dir(path) {
                    for entry in entries.flatten() {
                        if let Ok(meta) = entry.metadata() {
                            if let Ok(mtime) = meta.modified() {
                                self.state.insert(entry.path(), mtime);
                            }
                        }
                    }
                }
            }
        }
    }

    async fn check_path(&mut self, path: &Path, tx: &mpsc::Sender<ObserverEvent>) {
        if path.is_dir() {
            self.check_dir(path, tx).await;
        } else {
            self.check_file(path, tx).await;
        }
    }

    async fn check_file(&mut self, path: &Path, tx: &mpsc::Sender<ObserverEvent>) {
        match std::fs::metadata(path) {
            Ok(meta) => {
                if let Ok(mtime) = meta.modified() {
                    let changed = self
                        .state
                        .get(path)
                        .map_or(true, |prev| *prev != mtime);

                    if changed {
                        let event_detail = if self.state.contains_key(path) {
                            "modified"
                        } else {
                            "created"
                        };
                        debug!(path = %path.display(), "file {event_detail}");
                        let _ = tx
                            .send(ObserverEvent {
                                category: DriftCategory::Files,
                                path: path.to_string_lossy().into(),
                                detail: format!("file {event_detail}: {}", path.display()),
                                timestamp: chrono::Utc::now(),
                            })
                            .await;
                        self.state.insert(path.to_path_buf(), mtime);
                    }
                }
            }
            Err(_) => {
                // File deleted
                if self.state.remove(path).is_some() {
                    let _ = tx
                        .send(ObserverEvent {
                            category: DriftCategory::Files,
                            path: path.to_string_lossy().into(),
                            detail: format!("file deleted: {}", path.display()),
                            timestamp: chrono::Utc::now(),
                        })
                        .await;
                }
            }
        }
    }

    async fn check_dir(&mut self, dir: &Path, tx: &mpsc::Sender<ObserverEvent>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                self.check_file(&entry.path(), tx).await;
            }
        }
    }
}

/// Polls running processes. Detects service start/stop.
pub struct ProcessObserver {
    expected_services: Vec<String>,
    running: HashMap<String, bool>,
    interval: Duration,
}

impl ProcessObserver {
    pub fn new(expected_services: Vec<String>, interval: Duration) -> Self {
        Self {
            expected_services,
            running: HashMap::new(),
            interval,
        }
    }

    pub async fn run(mut self, tx: mpsc::Sender<ObserverEvent>) {
        loop {
            tokio::time::sleep(self.interval).await;

            let ps_output = tokio::process::Command::new("ps")
                .args(["aux"])
                .output()
                .await;

            let output = match ps_output {
                Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
                Err(e) => {
                    warn!(error = %e, "ps command failed");
                    continue;
                }
            };

            for service in &self.expected_services {
                let is_running = output.lines().any(|line| line.contains(service.as_str()));
                let was_running = self.running.get(service).copied().unwrap_or(is_running);

                if was_running && !is_running {
                    let _ = tx
                        .send(ObserverEvent {
                            category: DriftCategory::Services,
                            path: service.clone(),
                            detail: format!("{service} stopped"),
                            timestamp: chrono::Utc::now(),
                        })
                        .await;
                } else if !was_running && is_running {
                    let _ = tx
                        .send(ObserverEvent {
                            category: DriftCategory::Services,
                            path: service.clone(),
                            detail: format!("{service} started"),
                            timestamp: chrono::Utc::now(),
                        })
                        .await;
                }

                self.running.insert(service.clone(), is_running);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn file_observer_detects_change() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.conf");
        std::fs::write(&file_path, "original").unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let observer = FileObserver::new(vec![file_path.clone()], Duration::from_millis(50));

        let handle = tokio::spawn(async move { observer.run(tx).await });

        // Wait for initial scan
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Modify the file
        std::fs::write(&file_path, "modified").unwrap();

        // Wait for detection
        tokio::time::sleep(Duration::from_millis(200)).await;

        handle.abort();

        let event = rx.try_recv().expect("should detect file change");
        assert_eq!(event.category, DriftCategory::Files);
        assert!(event.detail.contains("modified"));
    }

    #[tokio::test]
    async fn file_observer_detects_new_file() {
        let dir = tempfile::tempdir().unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let observer = FileObserver::new(vec![dir.path().to_path_buf()], Duration::from_millis(50));

        let handle = tokio::spawn(async move { observer.run(tx).await });

        // Wait for initial scan
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create a new file
        let new_file = dir.path().join("new.conf");
        std::fs::write(&new_file, "new content").unwrap();

        // Wait for detection
        tokio::time::sleep(Duration::from_millis(200)).await;

        handle.abort();

        let event = rx.try_recv().expect("should detect new file");
        assert_eq!(event.category, DriftCategory::Files);
        assert!(event.detail.contains("created"));
    }

    #[tokio::test]
    async fn file_observer_detects_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("delete-me.conf");
        std::fs::write(&file_path, "will be deleted").unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let observer = FileObserver::new(vec![file_path.clone()], Duration::from_millis(50));

        let handle = tokio::spawn(async move { observer.run(tx).await });

        // Wait for initial scan
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Delete the file
        std::fs::remove_file(&file_path).unwrap();

        // Wait for detection
        tokio::time::sleep(Duration::from_millis(200)).await;

        handle.abort();

        let event = rx.try_recv().expect("should detect deletion");
        assert_eq!(event.category, DriftCategory::Files);
        assert!(event.detail.contains("deleted"));
    }
}
