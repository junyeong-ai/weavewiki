use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::constants::event_store::{DEFAULT_SHARD_SIZE, INDEX_SAVE_INTERVAL};
use crate::types::Result;

use super::types::{EventImportance, EventPayload, EventType, PipelineEvent};

/// Shard index persisted as `index.json` in the session directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShardIndex {
    shard_size: usize,
    shards: Vec<ShardEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShardEntry {
    shard_id: usize,
    event_count: u64,
}

impl ShardIndex {
    fn new(shard_size: usize) -> Self {
        Self {
            shard_size,
            shards: Vec::new(),
        }
    }

    fn total_events(&self) -> u64 {
        self.shards.iter().map(|s| s.event_count).sum()
    }

    fn active_shard_id(&self) -> usize {
        self.shards.last().map_or(0, |s| {
            if s.event_count >= self.shard_size as u64 {
                s.shard_id + 1
            } else {
                s.shard_id
            }
        })
    }

    fn record_event(&mut self, shard_id: usize) {
        if let Some(entry) = self.shards.iter_mut().find(|s| s.shard_id == shard_id) {
            entry.event_count += 1;
        } else {
            self.shards.push(ShardEntry {
                shard_id,
                event_count: 1,
            });
        }
    }

    fn needs_rotation(&self) -> bool {
        self.shards
            .last()
            .is_some_and(|s| s.event_count >= self.shard_size as u64)
    }
}

fn shard_filename(shard_id: usize) -> String {
    format!("shard_{:03}.jsonl", shard_id)
}

/// Wrapper around phase snapshot content with an input hash for invalidation.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PhaseSnapshotWrapper {
    input_hash: String,
    content: String,
}

pub struct EventStore {
    session_dir: PathBuf,
    writer: Mutex<ShardWriter>,
    event_count: AtomicU64,
}

struct ShardWriter {
    writer: BufWriter<File>,
    index: ShardIndex,
    session_dir: PathBuf,
    current_shard_id: usize,
    events_since_index_save: usize,
}

impl ShardWriter {
    async fn rotate(&mut self) -> Result<()> {
        self.writer.flush().await?;
        let new_shard_id = self.current_shard_id + 1;
        let shard_path = self.session_dir.join(shard_filename(new_shard_id));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&shard_path)
            .await?;
        self.writer = BufWriter::new(file);
        self.current_shard_id = new_shard_id;
        Ok(())
    }

    async fn save_index(&self) -> Result<()> {
        let index_path = self.session_dir.join("index.json");
        let json = serde_json::to_string(&self.index)?;
        atomic_write(&index_path, json.as_bytes()).await
    }
}

/// Atomic write: temp file → sync → rename. Prevents partial writes on crash.
async fn atomic_write(target: &Path, data: &[u8]) -> Result<()> {
    let parent = target.parent().unwrap_or(target);
    let temp_path = parent.join(format!(".tmp_{}", Uuid::new_v4()));
    let mut file = File::create(&temp_path).await?;
    file.write_all(data).await?;
    file.sync_all().await?;
    drop(file);
    if let Err(e) = fs::rename(&temp_path, target).await {
        let _ = fs::remove_file(&temp_path).await;
        return Err(e.into());
    }
    Ok(())
}

impl EventStore {
    pub async fn create(project_root: &Path) -> Result<Self> {
        let session_id = Uuid::new_v4();
        let session_dir = project_root
            .join(".claudegen")
            .join("sessions")
            .join(session_id.to_string());

        fs::create_dir_all(&session_dir).await?;

        let shard_path = session_dir.join(shard_filename(0));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&shard_path)
            .await?;

        let index = ShardIndex::new(DEFAULT_SHARD_SIZE);
        let shard_writer = ShardWriter {
            writer: BufWriter::new(file),
            index,
            session_dir: session_dir.clone(),
            current_shard_id: 0,
            events_since_index_save: 0,
        };

        tracing::debug!(session_id = %session_id, "Created new event store session");

        Ok(Self {
            session_dir,
            writer: Mutex::new(shard_writer),
            event_count: AtomicU64::new(0),
        })
    }

    pub async fn resume(session_dir: &Path) -> Result<Self> {
        let (index, existing_count) = Self::load_or_migrate_index(session_dir).await?;
        let active_shard_id = index.active_shard_id();

        let shard_path = session_dir.join(shard_filename(active_shard_id));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&shard_path)
            .await?;

        let shard_writer = ShardWriter {
            writer: BufWriter::new(file),
            index,
            session_dir: session_dir.to_path_buf(),
            current_shard_id: active_shard_id,
            events_since_index_save: 0,
        };

        tracing::debug!(
            session_dir = %session_dir.display(),
            existing_events = existing_count,
            active_shard = active_shard_id,
            "Resuming event store session"
        );

        Ok(Self {
            session_dir: session_dir.to_path_buf(),
            writer: Mutex::new(shard_writer),
            event_count: AtomicU64::new(existing_count),
        })
    }

    /// Load existing index or migrate from legacy single-file format.
    ///
    /// Reconciles the active shard's event count with the actual line count
    /// to handle cases where events were written but the index was not saved.
    async fn load_or_migrate_index(session_dir: &Path) -> Result<(ShardIndex, u64)> {
        let index_path = session_dir.join("index.json");

        // Try loading existing index
        if index_path.exists()
            && let Ok(content) = fs::read_to_string(&index_path).await
            && let Ok(mut index) = serde_json::from_str::<ShardIndex>(&content)
        {
            // Reconcile: count actual lines in the active shard and adjust if needed
            if let Some(active) = index.shards.last_mut() {
                let shard_path = session_dir.join(shard_filename(active.shard_id));
                if let Ok(actual_count) = Self::count_lines(&shard_path).await
                    && actual_count != active.event_count
                {
                    tracing::debug!(
                        shard = active.shard_id,
                        indexed = active.event_count,
                        actual = actual_count,
                        "Reconciling active shard event count"
                    );
                    active.event_count = actual_count;
                }
            }
            let total = index.total_events();
            return Ok((index, total));
        }

        // Rebuild index from shard files (e.g. index save was deferred)
        let shard_0 = session_dir.join(shard_filename(0));
        if shard_0.exists() {
            let mut index = ShardIndex::new(DEFAULT_SHARD_SIZE);
            let mut shard_id = 0;
            loop {
                let shard_path = session_dir.join(shard_filename(shard_id));
                if !shard_path.exists() {
                    break;
                }
                let count = Self::count_lines(&shard_path).await.unwrap_or(0);
                index.shards.push(ShardEntry {
                    shard_id,
                    event_count: count,
                });
                shard_id += 1;
            }
            let total = index.total_events();
            return Ok((index, total));
        }

        // Migrate from legacy events.jsonl
        let legacy_path = session_dir.join("events.jsonl");
        if legacy_path.exists() {
            let count = Self::count_lines(&legacy_path).await.unwrap_or(0);
            if count > 0 {
                let shard_path = session_dir.join(shard_filename(0));
                fs::rename(&legacy_path, &shard_path).await?;

                let mut index = ShardIndex::new(DEFAULT_SHARD_SIZE);
                index.shards.push(ShardEntry {
                    shard_id: 0,
                    event_count: count,
                });

                let json = serde_json::to_string(&index)?;
                atomic_write(&index_path, json.as_bytes()).await?;

                return Ok((index, count));
            }
        }

        // Fresh start
        Ok((ShardIndex::new(DEFAULT_SHARD_SIZE), 0))
    }

    pub async fn append(&self, event_type: EventType, payload: EventPayload) -> Result<()> {
        let importance = event_type.importance();
        let event = PipelineEvent::new(event_type, payload);
        let json = serde_json::to_string(&event)?;

        let mut sw = self.writer.lock().await;

        // Rotate shard if current one is full
        if sw.index.needs_rotation() {
            sw.save_index().await?;
            sw.events_since_index_save = 0;
            sw.rotate().await?;
        }

        sw.writer.write_all(json.as_bytes()).await?;
        sw.writer.write_all(b"\n").await?;
        sw.writer.flush().await?;

        // Critical events must survive hard crashes -- fsync to disk.
        if importance == EventImportance::Critical {
            sw.writer.get_ref().sync_all().await?;
        }

        let shard_id = sw.current_shard_id;
        sw.index.record_event(shard_id);
        sw.events_since_index_save += 1;

        if sw.events_since_index_save >= INDEX_SAVE_INTERVAL {
            sw.save_index().await?;
            sw.events_since_index_save = 0;
        }

        self.event_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    pub fn event_count(&self) -> u64 {
        self.event_count.load(Ordering::SeqCst)
    }

    pub async fn find_resumable(project_root: &Path) -> Option<PathBuf> {
        Self::find_resumable_with_validation(project_root, None)
            .await
            .map(|(dir, _)| dir)
    }

    pub async fn find_resumable_with_validation(
        project_root: &Path,
        expected_hash: Option<&str>,
    ) -> Option<(PathBuf, bool)> {
        let sessions_dir = project_root.join(".claudegen").join("sessions");
        if !sessions_dir.exists() {
            return None;
        }

        let mut read_dir = fs::read_dir(&sessions_dir).await.ok()?;
        let mut sessions = Vec::new();

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            if entry.path().is_dir() {
                sessions.push(entry.path());
            }
        }

        sessions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

        for session_dir in sessions {
            if !Self::has_events(&session_dir).await {
                continue;
            }

            if let Ok(events) = Self::load_events(&session_dir).await {
                let is_complete = events
                    .iter()
                    .any(|e| e.event_type == EventType::SessionCompleted);

                if is_complete {
                    continue;
                }

                let config_matches = match expected_hash {
                    None => true,
                    Some(expected) => events
                        .iter()
                        .find(|e| e.event_type == EventType::SessionStarted)
                        .and_then(|e| match &e.payload {
                            EventPayload::Session { config_hash } => Some(config_hash.as_str()),
                            _ => None,
                        })
                        .is_some_and(|h| h == expected),
                };

                return Some((session_dir, config_matches));
            }
        }

        None
    }

    /// Check if a session directory has any event files.
    async fn has_events(session_dir: &Path) -> bool {
        // Check for sharded format
        if session_dir.join(shard_filename(0)).exists() {
            return true;
        }
        // Check for legacy format
        session_dir.join("events.jsonl").exists()
    }

    /// Load all events from a session directory, reading shards sequentially.
    pub async fn load_events(session_dir: &Path) -> Result<Vec<PipelineEvent>> {
        let mut events = Vec::new();

        // Try sharded format: scan all shard files sequentially
        let shard_0 = session_dir.join(shard_filename(0));
        if shard_0.exists() {
            let mut shard_id = 0;
            loop {
                let shard_path = session_dir.join(shard_filename(shard_id));
                if !shard_path.exists() {
                    break;
                }
                Self::read_jsonl_into(&shard_path, &mut events).await?;
                shard_id += 1;
            }
            return Ok(events);
        }

        // Fallback to legacy single-file format
        let legacy_path = session_dir.join("events.jsonl");
        if legacy_path.exists() {
            Self::read_jsonl_into(&legacy_path, &mut events).await?;
        }

        Ok(events)
    }

    async fn read_jsonl_into(path: &Path, events: &mut Vec<PipelineEvent>) -> Result<()> {
        let file = File::open(path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let mut idx = 0usize;
        while let Some(line) = lines.next_line().await? {
            if line.is_empty() {
                idx += 1;
                continue;
            }
            match serde_json::from_str::<PipelineEvent>(&line) {
                Ok(event) => events.push(event),
                Err(e) => tracing::warn!(line_num = idx + 1, error = %e, "Skipping unrecognized event"),
            }
            idx += 1;
        }

        Ok(())
    }

    async fn count_lines(path: &Path) -> Result<u64> {
        let file = File::open(path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let mut count = 0u64;
        while let Some(line) = lines.next_line().await? {
            if !line.is_empty() {
                count += 1;
            }
        }

        Ok(count)
    }

    pub async fn save_drafts(&self, content: &str) -> Result<String> {
        let snapshots_dir = self.session_dir.join("snapshots");
        fs::create_dir_all(&snapshots_dir).await?;

        let snapshot_path = snapshots_dir.join("drafts.json");
        atomic_write(&snapshot_path, content.as_bytes()).await?;

        Ok(snapshot_path.to_string_lossy().to_string())
    }

    pub async fn save_iteration(&self, iteration: usize, content: &str) -> Result<String> {
        let snapshots_dir = self.session_dir.join("snapshots");
        fs::create_dir_all(&snapshots_dir).await?;

        let snapshot_path = snapshots_dir.join(format!("iter_{iteration}.json"));
        atomic_write(&snapshot_path, content.as_bytes()).await?;

        Ok(snapshot_path.to_string_lossy().to_string())
    }

    pub async fn save_phase_snapshot(&self, phase_name: &str, content: &str) -> Result<String> {
        let snapshots_dir = self.session_dir.join("snapshots");
        fs::create_dir_all(&snapshots_dir).await?;

        let snapshot_path = snapshots_dir.join(format!("{phase_name}.json"));
        atomic_write(&snapshot_path, content.as_bytes()).await?;

        Ok(snapshot_path.to_string_lossy().to_string())
    }

    /// Save a phase snapshot with an input hash for invalidation detection.
    /// On resume, if the input hash differs, the cached output is stale.
    pub async fn save_phase_snapshot_with_hash(
        &self,
        phase_name: &str,
        content: &str,
        input_hash: &str,
    ) -> Result<String> {
        let snapshots_dir = self.session_dir.join("snapshots");
        fs::create_dir_all(&snapshots_dir).await?;

        let wrapper = PhaseSnapshotWrapper {
            input_hash: input_hash.to_string(),
            content: content.to_string(),
        };
        let json = serde_json::to_string(&wrapper)?;

        let snapshot_path = snapshots_dir.join(format!("{phase_name}.json"));
        atomic_write(&snapshot_path, json.as_bytes()).await?;

        Ok(snapshot_path.to_string_lossy().to_string())
    }

    /// Load a phase snapshot, validating its input hash.
    /// Returns `Some(content)` if hash matches, `None` if stale.
    pub async fn load_phase_snapshot_if_valid(
        path: &str,
        expected_hash: &str,
    ) -> Result<Option<String>> {
        let raw = fs::read_to_string(path).await?;
        match serde_json::from_str::<PhaseSnapshotWrapper>(&raw) {
            Ok(wrapper) if wrapper.input_hash == expected_hash => Ok(Some(wrapper.content)),
            Ok(_) => {
                tracing::debug!(path, "Phase snapshot stale (input hash mismatch)");
                Ok(None)
            }
            Err(_) => {
                // Legacy format without wrapper — treat as raw content (no hash validation)
                Ok(Some(raw))
            }
        }
    }

    pub async fn load_snapshot(path: &str) -> Result<String> {
        Ok(fs::read_to_string(path).await?)
    }

    /// Resume from last successful state
    ///
    /// Loads all events and reconstructs ResumeState for crash recovery.
    /// Returns None if no events exist (fresh session).
    pub async fn resume_from_events(&self) -> Result<Option<super::state::ResumeState>> {
        let events = Self::load_events(&self.session_dir).await?;

        if events.is_empty() {
            return Ok(None);
        }

        let resume_state = super::state::ResumeState::from_events(&events);

        tracing::info!(
            events = events.len(),
            phase_snapshots = resume_state.phase_snapshots.len(),
            "Resumed session from {} events",
            events.len()
        );

        Ok(Some(resume_state))
    }

    /// Compact old shards
    ///
    /// Deletes shards that are no longer needed (before second-to-last snapshot).
    /// Keeps at least 2 shards for safety. Updates the persisted index after deletion.
    pub async fn compact(&self) -> Result<()> {
        let mut sw = self.writer.lock().await;

        if sw.index.shards.len() < 3 {
            return Ok(());
        }

        let keep_from = sw.index.shards.len().saturating_sub(2);
        let mut deleted_count = 0;

        for entry in &sw.index.shards[..keep_from] {
            let shard_path = self.session_dir.join(shard_filename(entry.shard_id));
            if shard_path.exists() {
                fs::remove_file(&shard_path).await?;
                deleted_count += 1;
                tracing::debug!(
                    shard_id = entry.shard_id,
                    "Compacted old shard"
                );
            }
        }

        if deleted_count > 0 {
            let remaining = sw.index.shards.len() - deleted_count;

            // Remove compacted entries from the in-memory index and persist
            sw.index.shards = sw.index.shards.split_off(keep_from);
            sw.save_index().await?;

            tracing::info!(
                deleted = deleted_count,
                remaining = remaining,
                "Compacted event store"
            );

            // Drop the lock before appending (append re-acquires it)
            drop(sw);

            self.append(
                EventType::Custom {
                    name: "session_compacted".to_string(),
                },
                EventPayload::Custom {
                    data: serde_json::json!({
                        "shards_deleted": deleted_count,
                        "shards_remaining": remaining,
                    })
                    .to_string(),
                },
            )
            .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_event_store_create_and_append() {
        let temp_dir = TempDir::new().unwrap();
        let store = EventStore::create(temp_dir.path()).await.unwrap();

        store
            .append(
                EventType::SessionStarted,
                EventPayload::Session {
                    config_hash: "test".to_string(),
                },
            )
            .await
            .unwrap();

        assert_eq!(store.event_count(), 1);

        let events = EventStore::load_events(store.session_dir()).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::SessionStarted);
    }

    #[tokio::test]
    async fn test_event_store_resume() {
        let temp_dir = TempDir::new().unwrap();

        let store = EventStore::create(temp_dir.path()).await.unwrap();
        let session_dir = store.session_dir().to_path_buf();

        store
            .append(
                EventType::SessionStarted,
                EventPayload::Session {
                    config_hash: "test".to_string(),
                },
            )
            .await
            .unwrap();
        store
            .append(
                EventType::IterationStarted,
                EventPayload::IterationStarted { iteration: 0 },
            )
            .await
            .unwrap();

        drop(store);

        let resumed = EventStore::resume(&session_dir).await.unwrap();
        assert_eq!(resumed.event_count(), 2);

        resumed
            .append(
                EventType::IterationCompleted,
                EventPayload::IterationCompleted {
                    iteration: 0,
                    quality: 0.75,
                    converged: false,
                },
            )
            .await
            .unwrap();

        assert_eq!(resumed.event_count(), 3);
    }

    #[tokio::test]
    async fn test_find_resumable_session() {
        let temp_dir = TempDir::new().unwrap();

        let store = EventStore::create(temp_dir.path()).await.unwrap();
        store
            .append(
                EventType::SessionStarted,
                EventPayload::Session {
                    config_hash: "test".to_string(),
                },
            )
            .await
            .unwrap();

        let resumable = EventStore::find_resumable(temp_dir.path()).await;
        assert!(resumable.is_some());

        store
            .append(
                EventType::SessionCompleted,
                EventPayload::Session {
                    config_hash: "test".to_string(),
                },
            )
            .await
            .unwrap();

        let resumable = EventStore::find_resumable(temp_dir.path()).await;
        assert!(resumable.is_none());
    }

    #[tokio::test]
    async fn test_iteration_snapshot_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let store = EventStore::create(temp_dir.path()).await.unwrap();

        let content = r#"{"skills": [], "agents": [], "rules": []}"#;
        let path = store.save_iteration(5, content).await.unwrap();

        assert!(path.contains("iter_5.json"));
        let loaded = EventStore::load_snapshot(&path).await.unwrap();
        assert_eq!(loaded, content);
    }

    #[tokio::test]
    async fn test_drafts_snapshot_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let store = EventStore::create(temp_dir.path()).await.unwrap();

        let content = r#"{"skills": [], "agents": [], "rules": [], "claude_md": {}}"#;
        let path = store.save_drafts(content).await.unwrap();

        assert!(path.contains("drafts.json"));
        let loaded = EventStore::load_snapshot(&path).await.unwrap();
        assert_eq!(loaded, content);
    }

    #[tokio::test]
    async fn test_shard_rotation() {
        let temp_dir = TempDir::new().unwrap();

        // Create store with small shard size for testing
        let session_dir = temp_dir.path().join(".claudegen").join("sessions").join("test-shard");
        fs::create_dir_all(&session_dir).await.unwrap();

        let shard_path = session_dir.join(shard_filename(0));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&shard_path)
            .await
            .unwrap();

        let index = ShardIndex::new(3); // 3 events per shard
        let shard_writer = ShardWriter {
            writer: BufWriter::new(file),
            index,
            session_dir: session_dir.clone(),
            current_shard_id: 0,
            events_since_index_save: 0,
        };

        let store = EventStore {
            session_dir: session_dir.clone(),
            writer: Mutex::new(shard_writer),
            event_count: AtomicU64::new(0),
        };

        // Write 7 events (should create 3 shards: 3 + 3 + 1)
        for i in 0..7 {
            store
                .append(
                    EventType::IterationStarted,
                    EventPayload::IterationStarted { iteration: i },
                )
                .await
                .unwrap();
        }

        assert_eq!(store.event_count(), 7);

        // Verify shards exist
        assert!(session_dir.join(shard_filename(0)).exists());
        assert!(session_dir.join(shard_filename(1)).exists());
        assert!(session_dir.join(shard_filename(2)).exists());

        // Verify all events load correctly
        let events = EventStore::load_events(&session_dir).await.unwrap();
        assert_eq!(events.len(), 7);
    }

    #[tokio::test]
    async fn test_legacy_migration() {
        let temp_dir = TempDir::new().unwrap();
        let session_dir = temp_dir.path().join("session");
        fs::create_dir_all(&session_dir).await.unwrap();

        // Write a legacy events.jsonl
        let event = PipelineEvent::new(
            EventType::SessionStarted,
            EventPayload::Session {
                config_hash: "legacy".to_string(),
            },
        );
        let json = serde_json::to_string(&event).unwrap();
        fs::write(session_dir.join("events.jsonl"), format!("{json}\n"))
            .await
            .unwrap();

        // Resume should migrate
        let store = EventStore::resume(&session_dir).await.unwrap();
        assert_eq!(store.event_count(), 1);

        // Legacy file should be renamed
        assert!(!session_dir.join("events.jsonl").exists());
        assert!(session_dir.join(shard_filename(0)).exists());
        assert!(session_dir.join("index.json").exists());

        // Events should load correctly
        let events = EventStore::load_events(&session_dir).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::SessionStarted);
    }

    #[tokio::test]
    async fn test_phase_snapshot_with_hash() {
        let temp_dir = TempDir::new().unwrap();
        let store = EventStore::create(temp_dir.path()).await.unwrap();

        let content = r#"{"primary_type":"Library","confidence":0.9}"#;
        let input_hash = "abc123hash";

        // Save snapshot with hash
        let path = store
            .save_phase_snapshot_with_hash("project_detection", content, input_hash)
            .await
            .unwrap();
        assert!(path.contains("project_detection.json"));

        // Load with matching hash — should return content
        let loaded =
            EventStore::load_phase_snapshot_if_valid(&path, input_hash)
                .await
                .unwrap();
        assert_eq!(loaded, Some(content.to_string()));

        // Load with different hash — should return None (stale)
        let stale =
            EventStore::load_phase_snapshot_if_valid(&path, "different_hash")
                .await
                .unwrap();
        assert_eq!(stale, None);
    }

    #[tokio::test]
    async fn test_phase_snapshot_legacy_format() {
        let temp_dir = TempDir::new().unwrap();
        let store = EventStore::create(temp_dir.path()).await.unwrap();

        // Save in legacy format (plain JSON, no wrapper)
        let content = r#"{"primary_type":"Library"}"#;
        let path = store
            .save_phase_snapshot("legacy_phase", content)
            .await
            .unwrap();

        // Loading with any hash should return raw content (legacy fallback)
        let loaded =
            EventStore::load_phase_snapshot_if_valid(&path, "any_hash")
                .await
                .unwrap();
        assert_eq!(loaded, Some(content.to_string()));
    }

    /// Integration test: simulate pipeline interruption → resume from correct phase.
    ///
    /// 1. Creates an EventStore session
    /// 2. Saves phase snapshots for phases 1-3 (simulating completion)
    /// 3. Emits PhaseSnapshotSaved events
    /// 4. Drops the store (simulating interruption)
    /// 5. Resumes from the session and reconstructs ResumeState
    /// 6. Verifies phase snapshots are correctly restored and loadable
    #[tokio::test]
    async fn test_pipeline_interruption_and_resume() {
        use crate::pipeline::events::ResumeState;

        let temp_dir = TempDir::new().unwrap();

        // === FIRST RUN: complete phases 1-3, then "crash" ===
        let store = EventStore::create(temp_dir.path()).await.unwrap();
        let session_dir = store.session_dir().to_path_buf();

        // Emit session start
        store
            .append(
                EventType::SessionStarted,
                EventPayload::Session {
                    config_hash: "cfg_v1".to_string(),
                },
            )
            .await
            .unwrap();

        // Phase 1: Save detection snapshot
        let detection_json = r#"{"primary_type":"Library","confidence":0.95}"#;
        let detection_hash = "detect_hash_1";
        let detection_path = store
            .save_phase_snapshot_with_hash("project_detection", detection_json, detection_hash)
            .await
            .unwrap();
        store
            .append(
                EventType::PhaseSnapshotSaved,
                EventPayload::PhaseCompleted {
                    phase_name: "project_detection".to_string(),
                    snapshot_path: detection_path,
                    item_count: 1,
                    input_hash: Some(detection_hash.to_string()),
                },
            )
            .await
            .unwrap();

        // Phase 3: Save conventions snapshot
        let conventions_json = r#"{"architecture":{"pattern_name":"layered"}}"#;
        let conventions_hash = "conv_hash_1";
        let conventions_path = store
            .save_phase_snapshot_with_hash(
                "convention_inference",
                conventions_json,
                conventions_hash,
            )
            .await
            .unwrap();
        store
            .append(
                EventType::PhaseSnapshotSaved,
                EventPayload::PhaseCompleted {
                    phase_name: "convention_inference".to_string(),
                    snapshot_path: conventions_path,
                    item_count: 3,
                    input_hash: Some(conventions_hash.to_string()),
                },
            )
            .await
            .unwrap();

        // "Crash" — drop the store
        drop(store);

        // === RESUME: reload session and verify phase snapshots ===
        let resumed_store = EventStore::resume(&session_dir).await.unwrap();
        assert_eq!(resumed_store.event_count(), 3); // session_started + 2 phase snapshots

        // Reconstruct resume state from events
        let events = EventStore::load_events(&session_dir).await.unwrap();
        let resume_state = ResumeState::from_events(&events);

        // Verify phase snapshots are tracked
        assert_eq!(resume_state.phase_snapshots.len(), 2);
        assert!(resume_state.phase_snapshots.contains_key("project_detection"));
        assert!(resume_state.phase_snapshots.contains_key("convention_inference"));

        // Verify detection snapshot can be loaded with matching hash
        let pd_info = &resume_state.phase_snapshots["project_detection"];
        let loaded = EventStore::load_phase_snapshot_if_valid(
            &pd_info.snapshot_path,
            detection_hash,
        )
        .await
        .unwrap();
        assert_eq!(loaded, Some(detection_json.to_string()));

        // Verify stale hash returns None
        let stale = EventStore::load_phase_snapshot_if_valid(
            &pd_info.snapshot_path,
            "stale_hash",
        )
        .await
        .unwrap();
        assert!(stale.is_none());

        // Verify conventions snapshot
        let ci_info = &resume_state.phase_snapshots["convention_inference"];
        let loaded = EventStore::load_phase_snapshot_if_valid(
            &ci_info.snapshot_path,
            conventions_hash,
        )
        .await
        .unwrap();
        assert_eq!(loaded, Some(conventions_json.to_string()));

        // Session is NOT complete, so it should be resumable
        let resumable = EventStore::find_resumable(temp_dir.path()).await;
        assert!(resumable.is_some());
    }
}
