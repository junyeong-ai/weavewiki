//! Incremental Compaction
//!
//! Manages state compaction to prevent unbounded memory growth during
//! long-running refinement sessions.

use super::state::RefinementResumeState;

use crate::constants::refinement::{MAX_LEVEL_HISTORY, MAX_QUALITY_HISTORY};
const QUALITY_SUMMARY_THRESHOLD: usize = 30;

/// Incremental state compactor
pub struct IncrementalCompactor {
    retain_iterations: usize,
}

impl Default for IncrementalCompactor {
    fn default() -> Self {
        Self::new()
    }
}

impl IncrementalCompactor {
    pub fn new() -> Self {
        Self {
            retain_iterations: 5,
        }
    }

    /// Compact refinement state to reduce memory usage
    pub fn compact(&self, state: &mut RefinementResumeState) -> CompactionResult {
        let mut result = CompactionResult::default();

        // 1. Prune old iteration progress
        let before_iterations = state.iteration_progress.len();
        self.prune_old_iterations(state);
        result.iterations_removed =
            before_iterations.saturating_sub(state.iteration_progress.len());

        // 2. Compact quality history if too large
        if state.quality_history.len() > QUALITY_SUMMARY_THRESHOLD {
            let before_quality = state.quality_history.len();
            result.quality_summary = Some(self.compact_quality_history(state));
            result.quality_entries_compacted = before_quality.saturating_sub(state.quality_history.len());
        }

        // 3. Compact level history (keep only recent)
        let before_levels = state.level_history.len();
        self.compact_level_history(state);
        result.level_entries_removed = before_levels.saturating_sub(state.level_history.len());

        // 4. Prune strategy outcomes (keep only recent failures)
        let before_outcomes = state.strategy_outcomes.len();
        self.prune_strategy_outcomes(state);
        result.outcomes_removed = before_outcomes.saturating_sub(state.strategy_outcomes.len());

        result
    }

    fn prune_old_iterations(&self, state: &mut RefinementResumeState) {
        if state.iteration_progress.len() <= self.retain_iterations {
            return;
        }

        let current = state.last_completed_iteration.unwrap_or(0);
        let min_keep = current.saturating_sub(self.retain_iterations - 1);
        state.iteration_progress.retain(|&iter, _| iter >= min_keep);
    }

    fn compact_quality_history(&self, state: &mut RefinementResumeState) -> CompactionSummary {
        let removed = if state.quality_history.len() > MAX_QUALITY_HISTORY {
            let to_remove = state.quality_history.len() - MAX_QUALITY_HISTORY;
            state.quality_history.drain(0..to_remove).collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        // Build summary from compacted entries
        let all_entries: Vec<f32> = removed
            .iter()
            .chain(state.quality_history.iter())
            .copied()
            .collect();
        let total = all_entries.len();

        if total == 0 {
            return CompactionSummary::default();
        }

        let min_quality = all_entries.iter().copied().fold(f32::INFINITY, f32::min);
        let max_quality = all_entries.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let avg_quality = all_entries.iter().sum::<f32>() / total as f32;

        CompactionSummary {
            min_quality,
            max_quality,
            avg_quality,
            total_iterations: total,
        }
    }

    fn compact_level_history(&self, state: &mut RefinementResumeState) {
        if state.level_history.len() <= MAX_LEVEL_HISTORY {
            return;
        }
        let to_remove = state.level_history.len() - MAX_LEVEL_HISTORY;
        state.level_history.drain(0..to_remove);
    }

    fn prune_strategy_outcomes(&self, state: &mut RefinementResumeState) {
        const MAX_OUTCOMES: usize = 50;
        if state.strategy_outcomes.len() <= MAX_OUTCOMES {
            return;
        }

        // Keep only outcomes with failures (they guide future strategy selection)
        // failures = attempts - successes
        state
            .strategy_outcomes
            .retain(|_, outcome| outcome.attempts > outcome.successes);

        // If still too many, remove least recently used
        if state.strategy_outcomes.len() > MAX_OUTCOMES {
            let mut entries: Vec<_> = state
                .strategy_outcomes
                .iter()
                .map(|(k, v)| (k.clone(), v.last_used_iteration))
                .collect();
            entries.sort_by_key(|(_, iter)| *iter);
            let to_remove = entries.len() - MAX_OUTCOMES;
            for (key, _) in entries.into_iter().take(to_remove) {
                state.strategy_outcomes.remove(&key);
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct CompactionResult {
    pub iterations_removed: usize,
    pub quality_entries_compacted: usize,
    pub level_entries_removed: usize,
    pub outcomes_removed: usize,
    /// Summary of compacted quality history (preserved for resume)
    pub quality_summary: Option<CompactionSummary>,
}

/// Summary statistics of compacted quality data, preserved in resume state
/// so that compacted entries are not lost on resume.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CompactionSummary {
    pub min_quality: f32,
    pub max_quality: f32,
    pub avg_quality: f32,
    pub total_iterations: usize,
}

impl CompactionResult {
    pub fn was_compacted(&self) -> bool {
        self.iterations_removed > 0
            || self.quality_entries_compacted > 0
            || self.level_entries_removed > 0
            || self.outcomes_removed > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create_test_state(iterations: usize, quality_entries: usize) -> RefinementResumeState {
        let mut state = RefinementResumeState {
            last_completed_iteration: Some(iterations.saturating_sub(1)),
            ..Default::default()
        };

        for i in 0..iterations {
            state
                .iteration_progress
                .insert(i, crate::pipeline::events::state::IterationProgress::default());
        }

        for i in 0..quality_entries {
            state.quality_history.push(0.5 + (i as f32 * 0.01));
        }

        state
    }

    #[test]
    fn test_prune_old_iterations() {
        let mut state = create_test_state(20, 0);
        let compactor = IncrementalCompactor::new();
        let result = compactor.compact(&mut state);

        assert!(state.iteration_progress.len() <= 5);
        assert!(result.iterations_removed > 0);
    }

    #[test]
    fn test_compact_quality_history() {
        // Use more entries than MAX_QUALITY_HISTORY (100) to trigger actual compaction
        let mut state = create_test_state(0, 150);
        let compactor = IncrementalCompactor::new();
        let result = compactor.compact(&mut state);

        assert!(state.quality_history.len() <= MAX_QUALITY_HISTORY);
        assert!(result.quality_entries_compacted > 0);
    }

    #[test]
    fn test_no_compaction_needed() {
        let mut state = create_test_state(3, 5);
        let compactor = IncrementalCompactor::new();
        let result = compactor.compact(&mut state);

        assert!(!result.was_compacted());
    }
}
