//! Session Lock Management
//!
//! Prevents concurrent claudegen runs on the same project.
//! Extracted from CheckpointManager for single-responsibility design.

use std::path::{Path, PathBuf};

use tokio::fs;

use crate::types::{ClaudegenError, Result};

/// Session lock prevents concurrent runs
pub struct SessionLock {
    lock_file: PathBuf,
    acquired: bool,
}

impl SessionLock {
    /// Create new session lock for output directory
    pub fn new(output_dir: &Path) -> Self {
        Self {
            lock_file: output_dir.join(".session.lock"),
            acquired: false,
        }
    }

    /// Acquire the session lock atomically
    ///
    /// Uses create_new for atomic lock acquisition (no TOCTOU race).
    /// Returns error if lock already exists (another session running).
    pub async fn acquire(&mut self) -> Result<()> {
        let lock_info = format!(
            "pid: {}\nstarted: {}\n",
            std::process::id(),
            chrono::Utc::now().to_rfc3339()
        );

        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.lock_file)
            .await
        {
            Ok(mut file) => {
                use tokio::io::AsyncWriteExt;
                file.write_all(lock_info.as_bytes()).await?;
                self.acquired = true;
                tracing::debug!(lock_file = ?self.lock_file, "Session lock acquired");
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = fs::read_to_string(&self.lock_file)
                    .await
                    .unwrap_or_default();
                Err(ClaudegenError::Session(format!(
                    "Session already running: {}",
                    existing.trim()
                )))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Release the session lock
    pub async fn release(&mut self) -> Result<()> {
        if !self.acquired {
            return Ok(());
        }

        if self.lock_file.exists() {
            fs::remove_file(&self.lock_file).await?;
            tracing::debug!(lock_file = ?self.lock_file, "Session lock released");
        }

        self.acquired = false;
        Ok(())
    }

    /// Check if lock exists (another session might be running)
    pub fn exists(&self) -> bool {
        self.lock_file.exists()
    }

    /// Get lock file path
    pub fn path(&self) -> &Path {
        &self.lock_file
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        if self.acquired {
            // Best-effort cleanup (async not available in Drop)
            if let Err(e) = std::fs::remove_file(&self.lock_file) {
                tracing::warn!(
                    lock_file = ?self.lock_file,
                    error = %e,
                    "Failed to remove lock file in Drop"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_acquire_and_release() {
        let temp_dir = TempDir::new().unwrap();
        let mut lock = SessionLock::new(temp_dir.path());

        assert!(!lock.exists());

        lock.acquire().await.unwrap();
        assert!(lock.exists());

        lock.release().await.unwrap();
        assert!(!lock.exists());
    }

    #[tokio::test]
    async fn test_double_acquire_fails() {
        let temp_dir = TempDir::new().unwrap();
        let mut lock1 = SessionLock::new(temp_dir.path());
        let mut lock2 = SessionLock::new(temp_dir.path());

        lock1.acquire().await.unwrap();
        assert!(lock2.acquire().await.is_err());

        lock1.release().await.unwrap();
    }

    #[tokio::test]
    async fn test_drop_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join(".session.lock");

        {
            let mut lock = SessionLock::new(temp_dir.path());
            lock.acquire().await.unwrap();
            assert!(lock_path.exists());
        }

        // Lock should be cleaned up after drop
        assert!(!lock_path.exists());
    }
}
