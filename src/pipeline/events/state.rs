use std::collections::{HashMap, HashSet};

use super::compaction::CompactionSummary;
use super::types::{
    EventPayload, EventType, PipelineEvent, QualityLevelSnapshot, StrategyOutcome,
};


const MAX_ITERATION_PROGRESS: usize = 5;

#[derive(Debug, Default)]
pub struct ResumeState {
    pub analysis: AnalysisResumeState,
    pub refinement: RefinementResumeState,
    pub project_detected: bool,
    pub monorepo_analyzed: bool,
    pub modules_detected: bool,
    pub deep_review_completed: bool,
    pub file_hashes: HashMap<String, String>,
    /// Phase snapshot paths keyed by phase name, for per-phase resumption.
    pub phase_snapshots: HashMap<String, PhaseSnapshotInfo>,
}

/// Info about a cached phase snapshot for resumption.
#[derive(Debug, Clone)]
pub struct PhaseSnapshotInfo {
    pub snapshot_path: String,
    pub input_hash: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct AnalysisResumeState {
    pub current_phase: u8,
    pub completed_chunks: HashSet<String>,
    pub failed_chunks: HashSet<String>,
    pub total_chunks: usize,
    pub cached_chunks: HashSet<String>,
}

#[derive(Debug, Default, Clone)]
pub struct RefinementResumeState {
    pub last_completed_iteration: Option<usize>,
    pub iteration_progress: std::collections::HashMap<usize, IterationProgress>,
    pub best_state_path: Option<String>,
    pub best_quality: f32,

    // State preserved via RefinementCheckpoint
    pub quality_history: Vec<f32>,
    pub level_history: Vec<QualityLevelSnapshot>,
    pub stagnation_count: usize,
    pub consecutive_clean_passes: usize,
    pub strategy_outcomes: HashMap<String, StrategyOutcome>,

    /// Summary of compacted quality data, preserved so that
    /// compacted entries are not lost across resume cycles.
    pub compaction_summary: Option<CompactionSummary>,

    /// Path to the most recent periodic artifact checkpoint.
    /// On resume, artifacts are restored from this path instead of starting
    /// from scratch, preventing loss of intermediate refinement work.
    pub latest_checkpoint_path: Option<String>,
}

impl RefinementResumeState {
    /// Prune old iteration progress to prevent unbounded growth.
    /// Keeps only the most recent MAX_ITERATION_PROGRESS iterations.
    fn prune_old_progress(&mut self) {
        if self.iteration_progress.len() <= MAX_ITERATION_PROGRESS {
            return;
        }

        let current = self.last_completed_iteration.unwrap_or(0);
        // Keep iterations from (current - MAX + 1) to current inclusive
        let min_keep = current.saturating_sub(MAX_ITERATION_PROGRESS - 1);

        self.iteration_progress.retain(|&iter, _| iter >= min_keep);
    }
}

#[derive(Debug, Default, Clone)]
pub struct IterationProgress {
    pub completed_items: HashSet<String>,
    pub quality_assessed: bool,
}

impl IterationProgress {
    pub fn item_key(item_type: &str, item_name: &str) -> String {
        format!("{}:{}", item_type, item_name)
    }
}


impl ResumeState {
    pub fn from_events(events: &[PipelineEvent]) -> Self {
        let mut state = Self::default();

        for event in events {
            match (&event.event_type, &event.payload) {
                (
                    EventType::ChunkAnalysisCached,
                    EventPayload::ChunkAnalysisCached { chunk_id, hit, .. },
                ) => {
                    if *hit {
                        state.analysis.cached_chunks.insert(chunk_id.clone());
                    }
                }

                (
                    EventType::PhaseSnapshotSaved,
                    EventPayload::PhaseCompleted {
                        phase_name,
                        snapshot_path,
                        input_hash,
                        ..
                    },
                ) => {
                    state.phase_snapshots.insert(
                        phase_name.clone(),
                        PhaseSnapshotInfo {
                            snapshot_path: snapshot_path.clone(),
                            input_hash: input_hash.clone(),
                        },
                    );
                }

                (EventType::ProjectDetectionCompleted, _) => {
                    state.project_detected = true;
                }

                (EventType::MonorepoAnalysisCompleted, _) => {
                    state.monorepo_analyzed = true;
                }

                (EventType::ModuleDetectionCompleted, _) => {
                    state.modules_detected = true;
                }

                (EventType::DeepReviewCompleted, _) => {
                    state.deep_review_completed = true;
                }

                (
                    EventType::AnalysisCheckpoint,
                    EventPayload::AnalysisCheckpoint { checkpoint },
                ) => {
                    state.analysis.current_phase = checkpoint.current_phase;
                    state.analysis.total_chunks = checkpoint.total_chunks;
                    state.analysis.completed_chunks =
                        checkpoint.completed_chunk_ids.iter().cloned().collect();
                    state.analysis.failed_chunks =
                        checkpoint.failed_chunk_ids.iter().cloned().collect();
                    state.file_hashes.clone_from(&checkpoint.file_hashes);
                }

                (
                    EventType::IterationCompleted,
                    EventPayload::IterationCompleted { iteration, .. },
                ) => {
                    state.refinement.last_completed_iteration = Some(*iteration);
                }

                (
                    EventType::IssueRefined,
                    EventPayload::IssueRefined {
                        iteration,
                        item_type,
                        item_name,
                        ..
                    },
                ) => {
                    let item_key = IterationProgress::item_key(item_type, item_name);
                    state
                        .refinement
                        .iteration_progress
                        .entry(*iteration)
                        .or_default()
                        .completed_items
                        .insert(item_key);
                }

                (EventType::QualityAssessed, EventPayload::QualityAssessed { iteration, .. }) => {
                    state
                        .refinement
                        .iteration_progress
                        .entry(*iteration)
                        .or_default()
                        .quality_assessed = true;
                }

                (
                    EventType::BestStateUpdated,
                    EventPayload::BestStateUpdated {
                        quality,
                        snapshot_path,
                        ..
                    },
                ) => {
                    state.refinement.best_quality = *quality;
                    state.refinement.best_state_path = Some(snapshot_path.clone());
                }

                (
                    EventType::RefinementCheckpoint,
                    EventPayload::RefinementCheckpoint { checkpoint },
                ) => {
                    state.refinement.quality_history = checkpoint.quality_history.clone();
                    state.refinement.level_history = checkpoint.level_history.clone();
                    state.refinement.stagnation_count = checkpoint.stagnation_count;
                    state.refinement.consecutive_clean_passes = checkpoint.consecutive_clean_passes;
                    state.refinement.strategy_outcomes = checkpoint.strategy_outcomes.clone();
                    state.refinement.best_quality = checkpoint.best_quality;
                    state.refinement.best_state_path = checkpoint.best_snapshot_path.clone();
                    state.refinement.compaction_summary =
                        checkpoint.compaction_summary.clone();
                    state.refinement.latest_checkpoint_path =
                        checkpoint.latest_checkpoint_path.clone();
                }

                _ => {}
            }
        }

        state.refinement.prune_old_progress();

        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(event_type: EventType, payload: EventPayload) -> PipelineEvent {
        PipelineEvent::new(event_type, payload)
    }

    #[test]
    fn test_empty_events() {
        let state = ResumeState::from_events(&[]);
        assert_eq!(state.refinement.last_completed_iteration, None);
    }

    #[test]
    fn test_refinement_progress_reconstruction() {
        let events = vec![
            make_event(
                EventType::QualityAssessed,
                EventPayload::QualityAssessed {
                    iteration: 0,
                    surface: 0.8,
                    judgment: 0.7,
                    combined: 0.75,
                    issues_count: 5,
                },
            ),
            make_event(
                EventType::IssueRefined,
                EventPayload::IssueRefined {
                    iteration: 0,
                    issue_index: 0,
                    item_type: "skill".to_string(),
                    item_name: "test".to_string(),
                    strategy: "evidence".to_string(),
                    success: true,
                    quality_delta: 0.05,
                },
            ),
            make_event(
                EventType::IssueRefined,
                EventPayload::IssueRefined {
                    iteration: 0,
                    issue_index: 1,
                    item_type: "agent".to_string(),
                    item_name: "reviewer".to_string(),
                    strategy: "semantic".to_string(),
                    success: false,
                    quality_delta: 0.0,
                },
            ),
            make_event(
                EventType::IterationCompleted,
                EventPayload::IterationCompleted {
                    iteration: 0,
                    quality: 0.78,
                    converged: false,
                },
            ),
        ];

        let state = ResumeState::from_events(&events);
        assert_eq!(state.refinement.last_completed_iteration, Some(0));

        let progress = state.refinement.iteration_progress.get(&0).unwrap();
        assert!(progress.quality_assessed);
        assert_eq!(progress.completed_items.len(), 2);
        assert!(progress.completed_items.contains("skill:test"));
        assert!(progress.completed_items.contains("agent:reviewer"));
    }

    #[test]
    fn test_best_state_reconstruction() {
        let events = vec![
            make_event(
                EventType::BestStateUpdated,
                EventPayload::BestStateUpdated {
                    iteration: 2,
                    quality: 0.82,
                    snapshot_path: "/path/to/snapshot.json".to_string(),
                },
            ),
            make_event(
                EventType::BestStateUpdated,
                EventPayload::BestStateUpdated {
                    iteration: 5,
                    quality: 0.88,
                    snapshot_path: "/path/to/snapshot2.json".to_string(),
                },
            ),
        ];

        let state = ResumeState::from_events(&events);
        assert_eq!(state.refinement.best_quality, 0.88);
        assert_eq!(
            state.refinement.best_state_path,
            Some("/path/to/snapshot2.json".to_string())
        );
    }

    #[test]
    fn test_item_key_stability() {
        let key1 = IterationProgress::item_key("skill", "code-review");
        let key2 = IterationProgress::item_key("skill", "code-review");
        let key3 = IterationProgress::item_key("agent", "code-review");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert_eq!(key1, "skill:code-review");
    }

    #[test]
    fn test_iteration_progress_pruning() {
        // Simulate many iterations to test pruning
        let mut events = vec![make_event(
            EventType::SessionStarted,
            EventPayload::Session {
                config_hash: "test".to_string(),
            },
        )];

        // Add 20 iterations worth of events
        for i in 0..20 {
            events.push(make_event(
                EventType::IterationStarted,
                EventPayload::IterationStarted { iteration: i },
            ));
            events.push(make_event(
                EventType::QualityAssessed,
                EventPayload::QualityAssessed {
                    iteration: i,
                    surface: 0.8,
                    judgment: 0.7,
                    combined: 0.75,
                    issues_count: 1,
                },
            ));
            events.push(make_event(
                EventType::IterationCompleted,
                EventPayload::IterationCompleted {
                    iteration: i,
                    quality: 0.78,
                    converged: false,
                },
            ));
        }

        let state = ResumeState::from_events(&events);

        // Should have pruned old iterations, keeping only MAX_ITERATION_PROGRESS
        assert!(
            state.refinement.iteration_progress.len() <= super::MAX_ITERATION_PROGRESS,
            "Expected at most {} iterations, got {}",
            super::MAX_ITERATION_PROGRESS,
            state.refinement.iteration_progress.len()
        );

        // Should still track the most recent iteration
        assert_eq!(state.refinement.last_completed_iteration, Some(19));
    }

    #[test]
    fn test_project_detection_event() {
        let events = vec![make_event(
            EventType::ProjectDetectionCompleted,
            EventPayload::ProjectDetectionCompleted {
                tech_stack: vec!["Rust".to_string()],
                frameworks: vec!["tokio".to_string()],
                workspace_type: "Library".to_string(),
            },
        )];

        let state = ResumeState::from_events(&events);
        assert!(state.project_detected);
    }

    #[test]
    fn test_monorepo_analysis_event() {
        let events = vec![make_event(
            EventType::MonorepoAnalysisCompleted,
            EventPayload::MonorepoAnalysisCompleted {
                packages_count: 5,
                services_count: 3,
            },
        )];

        let state = ResumeState::from_events(&events);
        assert!(state.monorepo_analyzed);
    }

    #[test]
    fn test_module_detection_event() {
        let events = vec![make_event(
            EventType::ModuleDetectionCompleted,
            EventPayload::ModuleDetectionCompleted {
                module_count: 8,
                high_value_count: 3,
            },
        )];

        let state = ResumeState::from_events(&events);
        assert!(state.modules_detected);
    }

    #[test]
    fn test_deep_review_event() {
        let events = vec![
            make_event(
                EventType::DeepReviewStarted,
                EventPayload::DeepReviewStarted { artifact_count: 10 },
            ),
            make_event(
                EventType::DeepReviewPassCompleted,
                EventPayload::DeepReviewPassCompleted {
                    pass_type: "passed".to_string(),
                    findings_count: 2,
                },
            ),
            make_event(
                EventType::DeepReviewCompleted,
                EventPayload::DeepReviewCompleted { improved_count: 10 },
            ),
        ];

        let state = ResumeState::from_events(&events);
        assert!(state.deep_review_completed);
    }

    #[test]
    fn test_analysis_checkpoint_with_file_hashes() {
        use crate::pipeline::events::types::AnalysisCheckpoint;

        let mut file_hashes = std::collections::HashMap::new();
        file_hashes.insert("src/main.rs".to_string(), "abc123".to_string());
        file_hashes.insert("src/lib.rs".to_string(), "def456".to_string());

        let events = vec![make_event(
            EventType::AnalysisCheckpoint,
            EventPayload::AnalysisCheckpoint {
                checkpoint: AnalysisCheckpoint {
                    version: 1,
                    current_phase: 1,
                    total_chunks: 5,
                    completed_chunk_ids: vec!["chunk-1".into(), "chunk-2".into()],
                    failed_chunk_ids: vec![],
                    file_hashes,
                    created_at: chrono::Utc::now(),
                },
            },
        )];

        let state = ResumeState::from_events(&events);
        assert_eq!(state.file_hashes.len(), 2);
        assert_eq!(state.file_hashes.get("src/main.rs"), Some(&"abc123".to_string()));
        assert_eq!(state.file_hashes.get("src/lib.rs"), Some(&"def456".to_string()));
        assert_eq!(state.analysis.completed_chunks.len(), 2);
    }

    #[test]
    fn test_phase_snapshot_reconstruction() {
        let events = vec![
            make_event(
                EventType::PhaseSnapshotSaved,
                EventPayload::PhaseCompleted {
                    phase_name: "project_detection".to_string(),
                    snapshot_path: "/tmp/snapshots/project_detection.json".to_string(),
                    item_count: 1,
                    input_hash: Some("hash_abc".to_string()),
                },
            ),
            make_event(
                EventType::PhaseSnapshotSaved,
                EventPayload::PhaseCompleted {
                    phase_name: "convention_inference".to_string(),
                    snapshot_path: "/tmp/snapshots/convention_inference.json".to_string(),
                    item_count: 5,
                    input_hash: Some("hash_def".to_string()),
                },
            ),
        ];

        let state = ResumeState::from_events(&events);
        assert_eq!(state.phase_snapshots.len(), 2);

        let pd = state.phase_snapshots.get("project_detection").unwrap();
        assert_eq!(pd.snapshot_path, "/tmp/snapshots/project_detection.json");
        assert_eq!(pd.input_hash, Some("hash_abc".to_string()));

        let ci = state.phase_snapshots.get("convention_inference").unwrap();
        assert_eq!(ci.snapshot_path, "/tmp/snapshots/convention_inference.json");
        assert_eq!(ci.input_hash, Some("hash_def".to_string()));
    }

    #[test]
    fn test_compaction_summary_preservation() {
        use crate::pipeline::events::CompactionSummary;

        let checkpoint = crate::pipeline::events::types::RefinementCheckpoint::new(
            10,
            vec![0.5, 0.6, 0.7],
            vec![QualityLevelSnapshot::BelowFloor, QualityLevelSnapshot::AtFloor],
            2,
            1,
            HashMap::new(),
            0.7,
            None,
        )
        .compaction_summary(CompactionSummary {
            min_quality: 0.3,
            max_quality: 0.8,
            avg_quality: 0.55,
            total_iterations: 50,
        });

        let events = vec![make_event(
            EventType::RefinementCheckpoint,
            EventPayload::RefinementCheckpoint { checkpoint },
        )];

        let state = ResumeState::from_events(&events);
        let summary = state.refinement.compaction_summary.as_ref().unwrap();
        assert_eq!(summary.min_quality, 0.3);
        assert_eq!(summary.max_quality, 0.8);
        assert_eq!(summary.avg_quality, 0.55);
        assert_eq!(summary.total_iterations, 50);
    }

    #[test]
    fn test_latest_checkpoint_path_preservation() {
        let checkpoint = crate::pipeline::events::types::RefinementCheckpoint::new(
            9,
            vec![0.5, 0.6, 0.7],
            vec![QualityLevelSnapshot::AtFloor],
            1,
            1,
            HashMap::new(),
            0.7,
            Some("/snapshots/best.json".to_string()),
        )
        .latest_checkpoint_path("/snapshots/iter_9.json".to_string());

        let events = vec![make_event(
            EventType::RefinementCheckpoint,
            EventPayload::RefinementCheckpoint { checkpoint },
        )];

        let state = ResumeState::from_events(&events);
        assert_eq!(
            state.refinement.latest_checkpoint_path,
            Some("/snapshots/iter_9.json".to_string()),
        );
        assert_eq!(
            state.refinement.best_state_path,
            Some("/snapshots/best.json".to_string()),
        );
        assert_eq!(state.refinement.best_quality, 0.7);
    }

    #[test]
    fn test_latest_checkpoint_path_updates_across_events() {
        // Simulate multiple checkpoint events: latest should win
        let checkpoint1 = crate::pipeline::events::types::RefinementCheckpoint::new(
            4,
            vec![0.5],
            vec![],
            0,
            0,
            HashMap::new(),
            0.5,
            None,
        )
        .latest_checkpoint_path("/snapshots/iter_4.json".to_string());

        let checkpoint2 = crate::pipeline::events::types::RefinementCheckpoint::new(
            9,
            vec![0.5, 0.7],
            vec![],
            0,
            1,
            HashMap::new(),
            0.7,
            None,
        )
        .latest_checkpoint_path("/snapshots/iter_9.json".to_string());

        let events = vec![
            make_event(
                EventType::RefinementCheckpoint,
                EventPayload::RefinementCheckpoint {
                    checkpoint: checkpoint1,
                },
            ),
            make_event(
                EventType::RefinementCheckpoint,
                EventPayload::RefinementCheckpoint {
                    checkpoint: checkpoint2,
                },
            ),
        ];

        let state = ResumeState::from_events(&events);
        // Second checkpoint should overwrite the first
        assert_eq!(
            state.refinement.latest_checkpoint_path,
            Some("/snapshots/iter_9.json".to_string()),
        );
    }

    #[test]
    fn test_latest_checkpoint_path_none_when_no_checkpoints() {
        let state = ResumeState::from_events(&[]);
        assert!(state.refinement.latest_checkpoint_path.is_none());
    }
}
