//! Checkpoint Management
//!
//! Unified checkpoint management for pipeline phases.
//! Eliminates duplicate checkpoint saving patterns across phases.

use crate::storage::SharedDatabase;
use crate::types::Result;

/// Simple context for checkpoint operations within phase analyzers
///
/// Eliminates the repeated `db: Option<SharedDatabase>` + `session_id: Option<String>` pattern.
#[derive(Clone)]
pub struct CheckpointContext {
    pub db: SharedDatabase,
    pub session_id: String,
}

impl CheckpointContext {
    pub fn new(db: SharedDatabase, session_id: String) -> Self {
        Self { db, session_id }
    }
}

/// Phase identifier for checkpoint tracking
///
/// Phases are numbered 1-6 to match the pipeline execution order:
/// - 1: Characterization - Project profiling
/// - 2: FileDiscovery - Source file scanning
/// - 3: BottomUp - File-level analysis
/// - 4: TopDown - Architecture insights
/// - 5: Consolidation - Domain grouping
/// - 6: Refinement - Quality iteration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelinePhase {
    Characterization = 1,
    FileDiscovery = 2,
    BottomUp = 3,
    TopDown = 4,
    Consolidation = 5,
    Refinement = 6,
}

impl PipelinePhase {
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Characterization => "Characterization",
            Self::FileDiscovery => "File Discovery",
            Self::BottomUp => "Bottom-Up Analysis",
            Self::TopDown => "Top-Down Analysis",
            Self::Consolidation => "Consolidation",
            Self::Refinement => "Refinement",
        }
    }

    /// Total number of phases
    pub const COUNT: usize = 6;

    /// Create from u8 phase number
    pub fn from_u8(phase: u8) -> Option<Self> {
        match phase {
            1 => Some(Self::Characterization),
            2 => Some(Self::FileDiscovery),
            3 => Some(Self::BottomUp),
            4 => Some(Self::TopDown),
            5 => Some(Self::Consolidation),
            6 => Some(Self::Refinement),
            _ => None,
        }
    }
}

/// Checkpoint manager for consistent checkpoint operations
pub struct CheckpointManager {
    db: SharedDatabase,
    session_id: String,
}

impl CheckpointManager {
    pub fn new(db: SharedDatabase, session_id: String) -> Self {
        Self { db, session_id }
    }

    /// Save checkpoint data for the pipeline
    pub fn save_checkpoint(&self, checkpoint: &super::PipelineCheckpoint) -> Result<()> {
        let checkpoint_json = serde_json::to_string(checkpoint)?;
        let now = chrono::Utc::now().to_rfc3339();

        self.db.execute(
            "UPDATE doc_sessions SET checkpoint_data = ?2, last_checkpoint_at = ?3 WHERE id = ?1",
            &[&self.session_id, &checkpoint_json, &now],
        )?;

        tracing::debug!(
            "Checkpoint saved: phase={}, files={}",
            checkpoint.last_completed_phase,
            checkpoint.files.len()
        );
        Ok(())
    }

    /// Mark phase as completed and save checkpoint
    pub fn complete_phase(
        &self,
        phase: PipelinePhase,
        checkpoint: &mut super::PipelineCheckpoint,
    ) -> Result<()> {
        self.complete_phase_with_counts(phase, checkpoint, None, None)
    }

    /// Mark phase as completed with optional file counts
    pub fn complete_phase_with_counts(
        &self,
        phase: PipelinePhase,
        checkpoint: &mut super::PipelineCheckpoint,
        total_files: Option<usize>,
        files_analyzed: Option<usize>,
    ) -> Result<()> {
        checkpoint.last_completed_phase = phase.as_u8();
        checkpoint.touch();
        self.save_checkpoint(checkpoint)?;
        self.update_progress(total_files, files_analyzed, Some(phase.as_u8()))?;

        tracing::info!("Phase {} completed", phase.name());
        Ok(())
    }

    /// Update session progress (delegates to Database)
    pub fn update_progress(
        &self,
        total_files: Option<usize>,
        files_analyzed: Option<usize>,
        current_phase: Option<u8>,
    ) -> Result<()> {
        self.db.update_session_progress(
            &self.session_id,
            total_files,
            files_analyzed,
            current_phase,
        )
    }

    /// Get session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get database reference
    pub fn db(&self) -> &SharedDatabase {
        &self.db
    }
}

// Note: Checkpointable trait removed - it was defined but never implemented.
// Each analyzer implements with_checkpoint() directly with its own logic.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_as_u8() {
        assert_eq!(PipelinePhase::Characterization.as_u8(), 1);
        assert_eq!(PipelinePhase::FileDiscovery.as_u8(), 2);
        assert_eq!(PipelinePhase::BottomUp.as_u8(), 3);
        assert_eq!(PipelinePhase::TopDown.as_u8(), 4);
        assert_eq!(PipelinePhase::Consolidation.as_u8(), 5);
        assert_eq!(PipelinePhase::Refinement.as_u8(), 6);
    }

    #[test]
    fn test_phase_name() {
        assert_eq!(PipelinePhase::BottomUp.name(), "Bottom-Up Analysis");
        assert_eq!(PipelinePhase::FileDiscovery.name(), "File Discovery");
    }

    #[test]
    fn test_phase_count() {
        assert_eq!(PipelinePhase::COUNT, 6);
    }

    #[test]
    fn test_phase_from_u8() {
        assert_eq!(
            PipelinePhase::from_u8(1),
            Some(PipelinePhase::Characterization)
        );
        assert_eq!(PipelinePhase::from_u8(3), Some(PipelinePhase::BottomUp));
        assert_eq!(PipelinePhase::from_u8(0), None);
        assert_eq!(PipelinePhase::from_u8(7), None);
    }
}
