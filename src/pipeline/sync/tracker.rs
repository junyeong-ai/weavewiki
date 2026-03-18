//! File Tracker
//!
//! blake3-based file change detection for incremental sync.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::pin::Pin;

use modmap::{ProjectManifest, TrackedFile};
use tokio::fs;

use super::ChangeSet;
use crate::constants::scanner::SOURCE_EXTENSIONS;
use crate::types::Result;

const EXCLUDE_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "vendor",
    ".git",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    "coverage",
];

pub struct FileTracker {
    project_root: PathBuf,
    tracked: Vec<TrackedFile>,
}

impl FileTracker {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            tracked: Vec::new(),
        }
    }

    pub fn from_manifest(manifest: &ProjectManifest) -> Self {
        let root = manifest
            .project
            .project
            .workspace
            .root
            .clone()
            .unwrap_or_else(|| ".".into());

        Self {
            project_root: PathBuf::from(root),
            tracked: manifest.tracked.clone(),
        }
    }

    pub async fn detect_changes(&self) -> Result<ChangeSet> {
        let current = self.scan_current_files().await?;
        let tracked_map: HashMap<&str, &TrackedFile> =
            self.tracked.iter().map(|f| (f.path.as_str(), f)).collect();

        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut deleted = Vec::new();

        for (path, hash) in &current {
            match tracked_map.get(path.as_str()) {
                None => added.push(path.clone()),
                Some(tracked) if tracked.hash != *hash => modified.push(path.clone()),
                _ => {}
            }
        }

        let current_paths: HashSet<&str> = current.keys().map(String::as_str).collect();
        for tracked in &self.tracked {
            if !current_paths.contains(tracked.path.as_str()) {
                deleted.push(tracked.path.clone());
            }
        }

        Ok(ChangeSet {
            added,
            modified,
            deleted,
        })
    }

    pub async fn scan_and_track(&mut self) -> Result<Vec<TrackedFile>> {
        let files = self.scan_current_files().await?;
        let now = chrono::Utc::now().timestamp();

        self.tracked = files
            .into_iter()
            .map(|(path, hash)| TrackedFile::new(path, hash, now))
            .collect();

        Ok(self.tracked.clone())
    }

    async fn scan_current_files(&self) -> Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        self.scan_dir(&self.project_root, &mut files).await?;
        Ok(files)
    }

    fn scan_dir<'a>(
        &'a self,
        dir: &'a Path,
        files: &'a mut HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = match fs::read_dir(dir).await {
                Ok(e) => e,
                Err(_) => return Ok(()),
            };

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if EXCLUDE_DIRS.contains(&file_name) {
                    continue;
                }

                if path.is_dir() {
                    self.scan_dir(&path, files).await?;
                } else if Self::is_source_file(&path)
                    && let Ok(content) = fs::read(&path).await
                {
                    let hash = crate::utils::hash::content_hash(&String::from_utf8_lossy(&content));
                    let relative = crate::utils::path::relative_path(&self.project_root, &path);
                    files.insert(relative, hash);
                }
            }

            Ok(())
        })
    }

    fn is_source_file(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
    }

    pub fn tracked_files(&self) -> &[TrackedFile] {
        &self.tracked
    }

    pub fn update_tracked(&mut self, new_tracked: Vec<TrackedFile>) {
        self.tracked = new_tracked;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    async fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).await.unwrap();

        let mut file = File::create(src.join("main.rs")).await.unwrap();
        file.write_all(b"fn main() {}").await.unwrap();

        let mut file2 = File::create(src.join("lib.rs")).await.unwrap();
        file2.write_all(b"pub fn hello() {}").await.unwrap();

        dir
    }

    #[tokio::test]
    async fn test_scan_finds_source_files() {
        let dir = setup_test_dir().await;
        let tracker = FileTracker::new(dir.path());

        let files = tracker.scan_current_files().await.unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.contains_key("src/main.rs"));
        assert!(files.contains_key("src/lib.rs"));
    }

    #[tokio::test]
    async fn test_detects_added_files() {
        let dir = setup_test_dir().await;
        let tracker = FileTracker::new(dir.path());

        let changes = tracker.detect_changes().await.unwrap();

        assert_eq!(changes.added.len(), 2);
        assert!(changes.modified.is_empty());
        assert!(changes.deleted.is_empty());
    }

    #[tokio::test]
    async fn test_detects_modified_files() {
        let dir = setup_test_dir().await;
        let mut tracker = FileTracker::new(dir.path());

        tracker.scan_and_track().await.unwrap();

        let mut file = File::create(dir.path().join("src/main.rs")).await.unwrap();
        file.write_all(b"fn main() { println!(\"changed\"); }")
            .await
            .unwrap();

        let changes = tracker.detect_changes().await.unwrap();

        assert!(changes.added.is_empty());
        assert_eq!(changes.modified.len(), 1);
        assert!(changes.modified.contains(&"src/main.rs".to_string()));
    }

    #[tokio::test]
    async fn test_detects_deleted_files() {
        let dir = setup_test_dir().await;
        let mut tracker = FileTracker::new(dir.path());

        tracker.scan_and_track().await.unwrap();

        fs::remove_file(dir.path().join("src/lib.rs"))
            .await
            .unwrap();

        let changes = tracker.detect_changes().await.unwrap();

        assert!(changes.added.is_empty());
        assert!(changes.modified.is_empty());
        assert_eq!(changes.deleted.len(), 1);
        assert!(changes.deleted.contains(&"src/lib.rs".to_string()));
    }

    #[tokio::test]
    async fn test_excludes_target_dir() {
        let dir = setup_test_dir().await;

        let target = dir.path().join("target");
        fs::create_dir_all(&target).await.unwrap();
        let mut file = File::create(target.join("debug.rs")).await.unwrap();
        file.write_all(b"should be excluded").await.unwrap();

        let tracker = FileTracker::new(dir.path());
        let files = tracker.scan_current_files().await.unwrap();

        assert!(!files.contains_key("target/debug.rs"));
    }
}
