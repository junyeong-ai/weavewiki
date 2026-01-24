//! Checkpoint Manager for Durable Execution
//!
//! Enables long-running tasks (days to weeks) to survive crashes
//! and resume from the last saved state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::fs;
use tracing::{debug, info, warn};

use crate::types::Result;

/// Pipeline phases for checkpoint tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelinePhase {
    Initialization,
    ProjectDetection,
    Analysis,
    ConventionInference,
    ConstraintExtraction,
    Planning,
    Generation,
    Refinement,
    DeepReview,
    Finalization,
}

impl PipelinePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Initialization => "initialization",
            Self::ProjectDetection => "project_detection",
            Self::Analysis => "analysis",
            Self::ConventionInference => "convention_inference",
            Self::ConstraintExtraction => "constraint_extraction",
            Self::Planning => "planning",
            Self::Generation => "generation",
            Self::Refinement => "refinement",
            Self::DeepReview => "deep_review",
            Self::Finalization => "finalization",
        }
    }
}

/// Completed phase with timing info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedPhase {
    pub phase: PipelinePhase,
    pub completed_at: DateTime<Utc>,
    pub duration_secs: f64,
    pub quality_score: Option<f32>,
}

/// Quality snapshot at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualitySnapshot {
    pub timestamp: DateTime<Utc>,
    pub iteration: usize,
    pub semantic_score: f32,
    pub evidence_score: f32,
    pub overall_score: f32,
}

/// Cached analysis data (to avoid re-running expensive analysis)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisCache {
    pub project_type: String,
    pub conventions: Vec<String>,
    pub constraints: Vec<String>,
    pub file_count: usize,
    pub analyzed_at: DateTime<Utc>,
}

/// Generated artifact state (partial progress)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeneratedArtifacts {
    pub claude_md: Option<String>,
    pub skills: HashMap<String, String>,
    pub agents: HashMap<String, String>,
    pub rules: HashMap<String, String>,
}

/// Lock file content for crash detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFileContent {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub hostname: String,
}

/// Execution checkpoint - captures entire pipeline state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionCheckpoint {
    /// Checkpoint format version (for compatibility)
    pub version: u32,
    /// When this checkpoint was created
    pub created_at: DateTime<Utc>,
    /// Current pipeline phase
    pub current_phase: PipelinePhase,
    /// Phase-specific progress (0.0-1.0)
    pub phase_progress: f32,
    /// Completed phases with timing
    pub completed_phases: Vec<CompletedPhase>,
    /// Cached analysis results
    pub analysis_cache: Option<AnalysisCache>,
    /// Generated artifacts (partial)
    pub generated_artifacts: GeneratedArtifacts,
    /// Quality history
    pub quality_history: Vec<QualitySnapshot>,
    /// Tokens used so far
    pub tokens_used: u64,
    /// Budget remaining
    pub budget_remaining: u64,
    /// Iteration counts
    pub refinement_iteration: usize,
    pub deep_review_pass: u32,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl ExecutionCheckpoint {
    pub fn new() -> Self {
        Self {
            version: 1,
            created_at: Utc::now(),
            current_phase: PipelinePhase::Initialization,
            phase_progress: 0.0,
            completed_phases: Vec::new(),
            analysis_cache: None,
            generated_artifacts: GeneratedArtifacts::default(),
            quality_history: Vec::new(),
            tokens_used: 0,
            budget_remaining: 0,
            refinement_iteration: 0,
            deep_review_pass: 0,
            metadata: HashMap::new(),
        }
    }

    /// Calculate overall progress percentage
    pub fn progress_percentage(&self) -> f32 {
        let total_phases = 10.0; // Total number of phases
        let completed = self.completed_phases.len() as f32;
        let current_progress = self.phase_progress / total_phases;
        ((completed + current_progress) / total_phases) * 100.0
    }

    /// Check if compatible with current version
    pub fn is_compatible(&self) -> bool {
        self.version == 1
    }
}

impl Default for ExecutionCheckpoint {
    fn default() -> Self {
        Self::new()
    }
}

/// Checkpoint manager for saving and restoring pipeline state
pub struct CheckpointManager {
    checkpoint_dir: PathBuf,
    lock_file: PathBuf,
    interval: Duration,
    last_checkpoint: Instant,
    max_checkpoints: usize,
    /// Counter to ensure unique filenames even within same millisecond
    save_counter: std::sync::atomic::AtomicU32,
}

impl CheckpointManager {
    /// Create a new checkpoint manager
    ///
    /// Interval is dynamically calculated as 1/4 of quality_loop_timeout (min 60s)
    pub fn new(project_root: &Path, timeout_config: &crate::config::TimeoutConfig) -> Self {
        let checkpoint_dir = project_root.join(".claudegen").join("checkpoints");
        let lock_file = project_root.join(".claudegen").join(".lock");
        let interval_secs = timeout_config.effective_checkpoint_interval_secs();

        Self {
            checkpoint_dir,
            lock_file,
            interval: Duration::from_secs(interval_secs),
            last_checkpoint: Instant::now(),
            max_checkpoints: 5,
            save_counter: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// Initialize checkpoint directory
    pub async fn initialize(&self) -> Result<()> {
        fs::create_dir_all(&self.checkpoint_dir).await?;
        Ok(())
    }

    /// Check if it's time to save a checkpoint
    pub fn should_checkpoint(&self) -> bool {
        self.last_checkpoint.elapsed() >= self.interval
    }

    /// Save checkpoint if interval has elapsed
    pub async fn maybe_checkpoint(&mut self, checkpoint: &ExecutionCheckpoint) -> Result<bool> {
        if !self.should_checkpoint() {
            return Ok(false);
        }

        self.save_checkpoint(checkpoint).await?;
        Ok(true)
    }

    /// Force save a checkpoint immediately
    pub async fn save_checkpoint(&mut self, checkpoint: &ExecutionCheckpoint) -> Result<()> {
        use std::sync::atomic::Ordering;

        // Use timestamp + counter + iteration for guaranteed unique filenames
        // Counter prevents overwrites even in async bursts within same millisecond
        let now = chrono::Utc::now();
        let counter = self.save_counter.fetch_add(1, Ordering::Relaxed);
        let filename = format!(
            "checkpoint_{}_{}_{}.json",
            now.format("%Y%m%d_%H%M%S_%3f"),
            counter,
            checkpoint.refinement_iteration
        );
        let path = self.checkpoint_dir.join(&filename);
        let temp_path = path.with_extension("tmp");

        // Atomic write: write to temp file, then rename
        let json = serde_json::to_vec_pretty(checkpoint)?;

        fs::write(&temp_path, &json).await?;
        fs::rename(&temp_path, &path).await?;

        // Cleanup old checkpoints
        self.cleanup_old_checkpoints().await?;

        self.last_checkpoint = Instant::now();
        info!(
            "Checkpoint saved: {} (progress: {:.1}%)",
            filename,
            checkpoint.progress_percentage()
        );

        Ok(())
    }

    /// List all checkpoints sorted by creation time (newest first)
    pub async fn list_checkpoints(&self) -> Result<Vec<ExecutionCheckpoint>> {
        let mut checkpoints = Vec::new();

        if !self.checkpoint_dir.exists() {
            return Ok(checkpoints);
        }

        let mut entries = fs::read_dir(&self.checkpoint_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                match fs::read_to_string(&path).await {
                    Ok(content) => match serde_json::from_str::<ExecutionCheckpoint>(&content) {
                        Ok(cp) => checkpoints.push(cp),
                        Err(e) => {
                            warn!("Failed to parse checkpoint {:?}: {}", path, e);
                        }
                    },
                    Err(e) => {
                        warn!("Failed to read checkpoint {:?}: {}", path, e);
                    }
                }
            }
        }

        // Sort by creation time, newest first
        checkpoints.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(checkpoints)
    }

    /// Restore the most recent compatible checkpoint
    pub async fn restore_latest(&self) -> Result<Option<ExecutionCheckpoint>> {
        let checkpoints = self.list_checkpoints().await?;

        for checkpoint in checkpoints {
            if checkpoint.is_compatible() {
                info!(
                    "Restoring checkpoint from {} (phase: {:?}, progress: {:.1}%)",
                    checkpoint.created_at.format("%Y-%m-%d %H:%M:%S"),
                    checkpoint.current_phase,
                    checkpoint.progress_percentage()
                );
                return Ok(Some(checkpoint));
            }
        }

        Ok(None)
    }

    /// Restore to a specific phase
    pub async fn restore_to_phase(
        &self,
        target_phase: PipelinePhase,
    ) -> Result<Option<ExecutionCheckpoint>> {
        let checkpoints = self.list_checkpoints().await?;

        for checkpoint in checkpoints {
            if checkpoint.is_compatible()
                && checkpoint
                    .completed_phases
                    .iter()
                    .any(|p| p.phase == target_phase)
            {
                return Ok(Some(checkpoint));
            }
        }

        Ok(None)
    }

    /// Cleanup old checkpoints, keeping only the most recent ones
    async fn cleanup_old_checkpoints(&self) -> Result<()> {
        let mut entries: Vec<_> = Vec::new();

        if !self.checkpoint_dir.exists() {
            return Ok(());
        }

        let mut dir = fs::read_dir(&self.checkpoint_dir).await?;

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(metadata) = entry.metadata().await
                && let Ok(modified) = metadata.modified()
            {
                entries.push((path, modified));
            }
        }

        // Sort by modification time, oldest first
        entries.sort_by(|a, b| a.1.cmp(&b.1));

        // Remove old checkpoints
        while entries.len() > self.max_checkpoints {
            if let Some((path, _)) = entries.first() {
                debug!("Removing old checkpoint: {:?}", path);
                let _ = fs::remove_file(path).await;
            }
            entries.remove(0);
        }

        Ok(())
    }

    /// Acquire execution lock
    pub async fn acquire_lock(&self) -> Result<()> {
        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());

        let content = LockFileContent {
            pid: std::process::id(),
            started_at: Utc::now(),
            hostname,
        };

        let json = serde_json::to_vec_pretty(&content)?;

        if let Some(parent) = self.lock_file.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&self.lock_file, json).await?;

        debug!("Lock acquired: {:?}", self.lock_file);
        Ok(())
    }

    /// Release execution lock
    pub async fn release_lock(&self) -> Result<()> {
        if self.lock_file.exists() {
            fs::remove_file(&self.lock_file).await?;
            debug!("Lock released: {:?}", self.lock_file);
        }
        Ok(())
    }

    /// Check if a stale lock exists (previous crash)
    pub async fn check_stale_lock(&self) -> Result<Option<LockFileContent>> {
        if !self.lock_file.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.lock_file).await?;
        let lock: LockFileContent = serde_json::from_str(&content)?;

        let current_pid = std::process::id();
        if lock.pid == current_pid {
            return Ok(None);
        }

        // Check if the process is actually running
        if Self::is_process_alive(lock.pid) {
            info!(
                "Lock held by running process (PID {}, started {})",
                lock.pid, lock.started_at
            );
            return Ok(Some(lock));
        }

        // Process is not running - lock is stale
        info!(
            "Stale lock detected (PID {} is no longer running, started {})",
            lock.pid, lock.started_at
        );
        Ok(Some(lock))
    }

    /// Check if process is still running
    #[cfg(target_os = "linux")]
    fn is_process_alive(pid: u32) -> bool {
        // On Linux, /proc/{pid} exists if process is running
        std::path::Path::new(&format!("/proc/{}", pid)).exists()
    }

    #[cfg(target_os = "macos")]
    fn is_process_alive(pid: u32) -> bool {
        // On macOS, use ps to check if process exists
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(target_os = "windows")]
    fn is_process_alive(pid: u32) -> bool {
        // On Windows, use tasklist to check if process exists
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid)])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    fn is_process_alive(_pid: u32) -> bool {
        // Fallback: assume process is dead (conservative for recovery)
        false
    }

    /// Check if the lock is stale (process dead) vs held by running process
    pub async fn is_lock_stale(&self) -> Result<bool> {
        match self.check_stale_lock().await? {
            None => Ok(false), // No lock file or same PID
            Some(lock) => Ok(!Self::is_process_alive(lock.pid)),
        }
    }

    /// Force release a stale lock (use after confirming staleness)
    pub async fn force_release_stale_lock(&self) -> Result<()> {
        if self.lock_file.exists() {
            warn!("Force releasing stale lock: {:?}", self.lock_file);
            fs::remove_file(&self.lock_file).await?;
        }
        Ok(())
    }
}

/// Recovery result after checking for crash
#[derive(Debug)]
pub enum RecoveryResult {
    /// No recovery needed - fresh start
    NoRecoveryNeeded,
    /// Recovered from checkpoint (boxed to reduce enum size)
    Recovered(Box<ExecutionCheckpoint>),
    /// No checkpoint found - starting fresh
    StartFresh,
    /// Another process is still running
    ProcessRunning(LockFileContent),
}

/// Crash recovery helper
pub struct CrashRecovery {
    checkpoint_manager: CheckpointManager,
}

impl CrashRecovery {
    pub fn new(checkpoint_manager: CheckpointManager) -> Self {
        Self { checkpoint_manager }
    }

    /// Attempt to recover from a previous crash
    pub async fn attempt_recovery(&self) -> Result<RecoveryResult> {
        let stale_lock = self.checkpoint_manager.check_stale_lock().await?;

        match stale_lock {
            None => Ok(RecoveryResult::NoRecoveryNeeded),
            Some(lock) => {
                // Check if the process is still running
                if CheckpointManager::is_process_alive(lock.pid) {
                    return Ok(RecoveryResult::ProcessRunning(lock));
                }

                // Process is dead - this is a stale lock from a crashed process
                // Release the stale lock before attempting recovery
                self.checkpoint_manager.force_release_stale_lock().await?;

                // Try to restore from checkpoint
                match self.checkpoint_manager.restore_latest().await? {
                    Some(checkpoint) => {
                        info!(
                            "Recovered from crash: phase={:?}, progress={:.1}%",
                            checkpoint.current_phase,
                            checkpoint.progress_percentage()
                        );
                        Ok(RecoveryResult::Recovered(Box::new(checkpoint)))
                    }
                    None => {
                        warn!(
                            "Stale lock found from PID {} but no checkpoint available",
                            lock.pid
                        );
                        Ok(RecoveryResult::StartFresh)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_checkpoint_save_and_restore() {
        let temp_dir = TempDir::new().unwrap();
        let config = crate::config::TimeoutConfig::default();
        let mut manager = CheckpointManager::new(temp_dir.path(), &config);
        manager.initialize().await.unwrap();

        // Create and save checkpoint
        let mut checkpoint = ExecutionCheckpoint::new();
        checkpoint.current_phase = PipelinePhase::Analysis;
        checkpoint.phase_progress = 0.5;
        checkpoint.tokens_used = 1000;

        manager.save_checkpoint(&checkpoint).await.unwrap();

        // Restore checkpoint
        let restored = manager.restore_latest().await.unwrap();
        assert!(restored.is_some());

        let restored = restored.unwrap();
        assert_eq!(restored.current_phase, PipelinePhase::Analysis);
        assert_eq!(restored.phase_progress, 0.5);
        assert_eq!(restored.tokens_used, 1000);
    }

    #[tokio::test]
    async fn test_lock_acquire_release() {
        let temp_dir = TempDir::new().unwrap();
        let config = crate::config::TimeoutConfig::default();
        let manager = CheckpointManager::new(temp_dir.path(), &config);
        manager.initialize().await.unwrap();

        // Acquire lock
        manager.acquire_lock().await.unwrap();
        assert!(manager.lock_file.exists());

        // Release lock
        manager.release_lock().await.unwrap();
        assert!(!manager.lock_file.exists());
    }

    #[test]
    fn test_checkpoint_progress() {
        let mut checkpoint = ExecutionCheckpoint::new();
        checkpoint.completed_phases.push(CompletedPhase {
            phase: PipelinePhase::Initialization,
            completed_at: Utc::now(),
            duration_secs: 1.0,
            quality_score: None,
        });
        checkpoint.phase_progress = 0.5;

        let progress = checkpoint.progress_percentage();
        assert!(progress > 0.0 && progress < 100.0);
    }
}
