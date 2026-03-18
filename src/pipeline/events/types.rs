use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::compaction::CompactionSummary;

/// Importance level for event durability.
///
/// Critical events trigger `sync_all()` after write to ensure they survive
/// hard crashes. Normal events rely on OS buffering and periodic flushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventImportance {
    /// Standard event -- flush only, no fsync. Acceptable to lose on hard crash.
    Normal,
    /// Must survive hard crash -- calls `sync_all()` after write.
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineEvent {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub schema_version: u32,
    pub event_type: EventType,
    pub payload: EventPayload,
}

impl PipelineEvent {
    pub const SCHEMA_VERSION: u32 = 1;

    #[must_use]
    pub fn new(event_type: EventType, payload: EventPayload) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            schema_version: Self::SCHEMA_VERSION,
            event_type,
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventType {
    SessionStarted,
    SessionCompleted,

    ChunkAnalysisCached,

    ConventionInferenceCompleted,
    ConstraintExtractionCompleted,
    DomainAnalysisCompleted,
    CrossSynthesisCompleted,

    ProjectDetectionCompleted,
    MonorepoAnalysisCompleted,
    ModuleDetectionCompleted,

    IterationStarted,
    QualityAssessed,
    IssueRefined,
    BestStateUpdated,
    IterationCompleted,

    PatternDeduped,
    ArtifactJudged,

    DeepReviewStarted,
    DeepReviewPassCompleted,
    DeepReviewCompleted,

    RefinementCheckpoint,
    AnalysisCheckpoint,

    PhaseSnapshotSaved,

    BudgetSnapshot,

    /// Custom event for extensions
    Custom { name: String },
}

impl EventType {
    /// Classify this event's importance for durability decisions.
    ///
    /// Critical events are fsynced to disk immediately because losing them
    /// on a hard crash would make the session unresumable or lose convergence
    /// state. Normal events are flushed but not fsynced -- they may be lost
    /// on power failure but are either reconstructible or purely diagnostic.
    pub fn importance(&self) -> EventImportance {
        match self {
            // Session lifecycle
            Self::SessionStarted | Self::SessionCompleted => EventImportance::Critical,

            // Phase completions (each represents significant analysis work)
            Self::ProjectDetectionCompleted
            | Self::MonorepoAnalysisCompleted
            | Self::ModuleDetectionCompleted
            | Self::ConventionInferenceCompleted
            | Self::ConstraintExtractionCompleted
            | Self::DomainAnalysisCompleted
            | Self::CrossSynthesisCompleted
            | Self::PhaseSnapshotSaved => EventImportance::Critical,

            // Checkpoints (resume anchors)
            Self::RefinementCheckpoint | Self::AnalysisCheckpoint => EventImportance::Critical,

            // Quality convergence / failure results
            Self::IterationCompleted => EventImportance::Critical,

            // Best-state snapshot path (needed for final output on resume)
            Self::BestStateUpdated => EventImportance::Critical,

            // Deep review final result
            Self::DeepReviewCompleted => EventImportance::Critical,

            // Diagnostic / high-volume telemetry -- tolerable to lose
            Self::ChunkAnalysisCached
            | Self::IterationStarted
            | Self::QualityAssessed
            | Self::IssueRefined
            | Self::PatternDeduped
            | Self::ArtifactJudged
            | Self::DeepReviewStarted
            | Self::DeepReviewPassCompleted
            | Self::BudgetSnapshot
            | Self::Custom { .. } => EventImportance::Normal,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventPayload {
    Session {
        config_hash: String,
    },

    ChunkAnalysisCached {
        chunk_id: String,
        content_hash: String,
        hit: bool,
    },

    PhaseCompleted {
        phase_name: String,
        snapshot_path: String,
        item_count: usize,
        #[serde(default)]
        input_hash: Option<String>,
    },

    ProjectDetectionCompleted {
        tech_stack: Vec<String>,
        frameworks: Vec<String>,
        workspace_type: String,
    },

    MonorepoAnalysisCompleted {
        packages_count: usize,
        services_count: usize,
    },

    ModuleDetectionCompleted {
        module_count: usize,
        high_value_count: usize,
    },

    AnalysisCheckpoint {
        checkpoint: AnalysisCheckpoint,
    },

    IterationStarted {
        iteration: usize,
    },

    QualityAssessed {
        iteration: usize,
        surface: f32,
        judgment: f32,
        combined: f32,
        issues_count: usize,
    },

    IssueRefined {
        iteration: usize,
        issue_index: usize,
        item_type: String,
        item_name: String,
        strategy: String,
        success: bool,
        quality_delta: f32,
    },

    BestStateUpdated {
        iteration: usize,
        quality: f32,
        snapshot_path: String,
    },

    IterationCompleted {
        iteration: usize,
        quality: f32,
        converged: bool,
    },

    RefinementCheckpoint {
        checkpoint: RefinementCheckpoint,
    },

    PatternDeduped {
        original_count: usize,
        deduped_count: usize,
        removed_count: usize,
    },

    ArtifactJudged {
        artifact_type: String,
        artifact_name: String,
        score: f32,
        pass: bool,
    },

    DeepReviewStarted {
        artifact_count: usize,
    },

    DeepReviewPassCompleted {
        pass_type: String,
        findings_count: usize,
    },

    DeepReviewCompleted {
        improved_count: usize,
    },

    BudgetSnapshot {
        phase: String,
        total_budget: u64,
        consumed: u64,
        remaining: u64,
        utilization: f64,
    },

    /// Custom event payload for extensions
    Custom { data: String },
}

/// Analysis checkpoint for distributed chunk analysis resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisCheckpoint {
    pub version: u32,
    pub current_phase: u8,
    pub total_chunks: usize,
    pub completed_chunk_ids: Vec<String>,
    pub failed_chunk_ids: Vec<String>,
    #[serde(default)]
    pub file_hashes: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
}

/// Refinement checkpoint for resume support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinementCheckpoint {
    pub version: u32,
    pub iteration: usize,
    pub quality_history: Vec<f32>,
    pub level_history: Vec<QualityLevelSnapshot>,
    pub stagnation_count: usize,
    pub consecutive_clean_passes: usize,
    pub strategy_outcomes: HashMap<String, StrategyOutcome>,
    pub best_quality: f32,
    pub best_snapshot_path: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Preserved compaction summary from prior compaction cycles
    #[serde(default)]
    pub compaction_summary: Option<CompactionSummary>,
    /// Path to the most recent periodic artifact checkpoint.
    /// Contains the full artifact state (skills, agents, rules) at that iteration,
    /// enabling crash recovery without losing intermediate refinement work.
    #[serde(default)]
    pub latest_checkpoint_path: Option<String>,
}

impl RefinementCheckpoint {
    pub const CURRENT_VERSION: u32 = 1;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        iteration: usize,
        quality_history: Vec<f32>,
        level_history: Vec<QualityLevelSnapshot>,
        stagnation_count: usize,
        consecutive_clean_passes: usize,
        strategy_outcomes: HashMap<String, StrategyOutcome>,
        best_quality: f32,
        best_snapshot_path: Option<String>,
    ) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            iteration,
            quality_history,
            level_history,
            stagnation_count,
            consecutive_clean_passes,
            strategy_outcomes,
            best_quality,
            best_snapshot_path,
            created_at: Utc::now(),
            compaction_summary: None,
            latest_checkpoint_path: None,
        }
    }

    pub fn compaction_summary(mut self, summary: CompactionSummary) -> Self {
        self.compaction_summary = Some(summary);
        self
    }

    pub fn latest_checkpoint_path(mut self, path: String) -> Self {
        self.latest_checkpoint_path = Some(path);
        self
    }
}

/// Quality level snapshot for persistence
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum QualityLevelSnapshot {
    BelowFloor,
    AtFloor,
    AtTarget,
}

/// Strategy outcome for tracking what worked
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyOutcome {
    pub attempts: usize,
    pub successes: usize,
    pub last_used_iteration: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_event_creation() {
        let event = PipelineEvent::new(
            EventType::SessionStarted,
            EventPayload::Session {
                config_hash: "abc123".to_string(),
            },
        );

        assert_eq!(event.event_type, EventType::SessionStarted);
        assert!(matches!(event.payload, EventPayload::Session { .. }));
    }

    #[test]
    fn test_event_serialization() {
        let event = PipelineEvent::new(
            EventType::QualityAssessed,
            EventPayload::QualityAssessed {
                iteration: 1,
                surface: 0.8,
                judgment: 0.75,
                combined: 0.78,
                issues_count: 3,
            },
        );

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: PipelineEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.event_type, EventType::QualityAssessed);
    }

    #[test]
    fn test_critical_event_importance() {
        let critical_events = [
            EventType::SessionStarted,
            EventType::SessionCompleted,
            EventType::ProjectDetectionCompleted,
            EventType::MonorepoAnalysisCompleted,
            EventType::ModuleDetectionCompleted,
            EventType::ConventionInferenceCompleted,
            EventType::ConstraintExtractionCompleted,
            EventType::DomainAnalysisCompleted,
            EventType::CrossSynthesisCompleted,
            EventType::PhaseSnapshotSaved,
            EventType::RefinementCheckpoint,
            EventType::AnalysisCheckpoint,
            EventType::IterationCompleted,
            EventType::BestStateUpdated,
            EventType::DeepReviewCompleted,
        ];

        for event_type in &critical_events {
            assert_eq!(
                event_type.importance(),
                EventImportance::Critical,
                "{event_type:?} should be Critical"
            );
        }
    }

    #[test]
    fn test_normal_event_importance() {
        let normal_events = [
            EventType::ChunkAnalysisCached,
            EventType::IterationStarted,
            EventType::QualityAssessed,
            EventType::IssueRefined,
            EventType::PatternDeduped,
            EventType::ArtifactJudged,
            EventType::DeepReviewStarted,
            EventType::DeepReviewPassCompleted,
            EventType::BudgetSnapshot,
            EventType::Custom {
                name: "test".to_string(),
            },
        ];

        for event_type in &normal_events {
            assert_eq!(
                event_type.importance(),
                EventImportance::Normal,
                "{event_type:?} should be Normal"
            );
        }
    }
}
