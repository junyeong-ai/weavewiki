//! Iteration State Module
//!
//! Quality loop state management with dynamic iteration budget,
//! uncertainty tracking, and self-determined termination.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

const MAX_TRAJECTORY_SIZE: usize = 100;
const UNCERTAINTY_WINDOW: usize = 5;

#[derive(Debug, Clone)]
pub struct IterationState {
    pub iteration: usize,
    pub estimated_total: usize,
    pub max_allowed: usize,
    pub needs_more_thinking: bool,
    pub uncertainty: f32,
    pub satisfied: bool,
    pub revision: Option<RevisionMeta>,
    pub quality_trajectory: VecDeque<f32>,
    pub history: Vec<IterationRecord>,
}

#[derive(Debug, Clone)]
pub struct RevisionMeta {
    pub revises_iteration: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationRecord {
    pub iteration: usize,
    pub quality_before: f32,
    pub quality_after: f32,
    pub uncertainty: f32,

    pub is_revision: bool,
    pub revises_iteration: Option<usize>,
    pub revision_reason: Option<String>,

    pub decision_rationale: String,
    pub strategies_used: Vec<String>,
    pub issues_addressed: Vec<String>,
    pub changes_made: Vec<String>,

    pub needs_more_thinking: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum BudgetExtensionTrigger {
    QualityImproving { min_delta: f32 },
    HighUncertainty { threshold: f32 },
}

impl IterationState {
    pub fn new(base_iterations: usize, max_extension: usize) -> Self {
        Self {
            iteration: 0,
            estimated_total: base_iterations,
            max_allowed: base_iterations + max_extension,
            needs_more_thinking: true,
            uncertainty: 1.0,
            satisfied: false,
            revision: None,
            quality_trajectory: VecDeque::with_capacity(MAX_TRAJECTORY_SIZE),
            history: Vec::new(),
        }
    }

    pub fn current_quality(&self) -> f32 {
        self.quality_trajectory.back().copied().unwrap_or(0.0)
    }

    pub fn quality_trajectory_vec(&self) -> Vec<f32> {
        self.quality_trajectory.iter().copied().collect()
    }

    pub fn maybe_extend(&mut self, trigger: BudgetExtensionTrigger) -> bool {
        if self.iteration < self.estimated_total || self.estimated_total >= self.max_allowed {
            return false;
        }

        let should_extend = match trigger {
            BudgetExtensionTrigger::QualityImproving { min_delta } => {
                self.is_quality_improving(min_delta)
            }
            BudgetExtensionTrigger::HighUncertainty { threshold } => self.uncertainty > threshold,
        };

        if should_extend {
            self.estimated_total = (self.estimated_total + 1).min(self.max_allowed);
            true
        } else {
            false
        }
    }

    pub fn is_quality_improving(&self, min_delta: f32) -> bool {
        if self.quality_trajectory.len() < 2 {
            return false;
        }
        let len = self.quality_trajectory.len();
        self.quality_trajectory[len - 1] - self.quality_trajectory[len - 2] >= min_delta
    }

    pub fn calculate_uncertainty(&mut self) {
        if self.quality_trajectory.len() < 3 {
            self.uncertainty = 0.5;
            return;
        }

        let window_size = self.quality_trajectory.len().min(UNCERTAINTY_WINDOW);
        let start = self.quality_trajectory.len() - window_size;
        let window: Vec<f32> = self
            .quality_trajectory
            .iter()
            .skip(start)
            .copied()
            .collect();

        let mean = window.iter().sum::<f32>() / window.len() as f32;
        let variance =
            window.iter().map(|&x| (x - mean).powi(2)).sum::<f32>() / window.len() as f32;

        self.uncertainty = (variance.sqrt() * 5.0).clamp(0.0, 1.0);
    }

    pub fn is_satisfied(&self, target_quality: f32, max_uncertainty: f32) -> bool {
        self.current_quality() >= target_quality
            && self.uncertainty <= max_uncertainty
            && !self.needs_more_thinking
    }

    pub fn start_revision(&mut self, target_iteration: usize, reason: &str) {
        self.revision = Some(RevisionMeta {
            revises_iteration: target_iteration,
            reason: reason.to_string(),
        });
    }

    pub fn end_revision(&mut self) {
        self.revision = None;
    }

    pub fn record_quality(&mut self, quality: f32) {
        if self.quality_trajectory.len() >= MAX_TRAJECTORY_SIZE {
            self.quality_trajectory.pop_front();
        }
        self.quality_trajectory.push_back(quality);
        self.calculate_uncertainty();
    }

    pub fn record(&mut self, record: IterationRecord) {
        self.needs_more_thinking = record.needs_more_thinking;
        self.history.push(record);
        self.iteration += 1;
    }

    pub fn should_continue(&self) -> bool {
        !self.satisfied && self.iteration < self.max_allowed
    }

    pub fn mark_satisfied(&mut self) {
        self.satisfied = true;
        self.needs_more_thinking = false;
    }
}

impl IterationRecord {
    pub fn new(iteration: usize, quality_before: f32) -> Self {
        Self {
            iteration,
            quality_before,
            quality_after: quality_before,
            uncertainty: 0.5,
            is_revision: false,
            revises_iteration: None,
            revision_reason: None,
            decision_rationale: String::new(),
            strategies_used: Vec::new(),
            issues_addressed: Vec::new(),
            changes_made: Vec::new(),
            needs_more_thinking: true,
        }
    }

    pub fn with_revision(mut self, iteration: usize, reason: &str) -> Self {
        self.is_revision = true;
        self.revises_iteration = Some(iteration);
        self.revision_reason = Some(reason.to_string());
        self
    }

    pub fn with_rationale(mut self, rationale: &str) -> Self {
        self.decision_rationale = rationale.to_string();
        self
    }

    pub fn with_quality_after(mut self, quality: f32) -> Self {
        self.quality_after = quality;
        self
    }

    pub fn with_uncertainty(mut self, uncertainty: f32) -> Self {
        self.uncertainty = uncertainty;
        self
    }

    pub fn with_strategies(mut self, strategies: Vec<String>) -> Self {
        self.strategies_used = strategies;
        self
    }

    pub fn with_changes(mut self, changes: Vec<String>) -> Self {
        self.changes_made = changes;
        self
    }

    pub fn with_issues(mut self, issues: Vec<String>) -> Self {
        self.issues_addressed = issues;
        self
    }

    pub fn needs_continuation(mut self, needs: bool) -> Self {
        self.needs_more_thinking = needs;
        self
    }

    pub fn improvement(&self) -> f32 {
        self.quality_after - self.quality_before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iteration_state_creation() {
        let state = IterationState::new(10, 5);
        assert_eq!(state.iteration, 0);
        assert_eq!(state.estimated_total, 10);
        assert_eq!(state.max_allowed, 15);
        assert!(state.needs_more_thinking);
        assert!(!state.satisfied);
    }

    #[test]
    fn test_quality_tracking() {
        let mut state = IterationState::new(10, 5);
        state.record_quality(0.5);
        state.record_quality(0.6);
        state.record_quality(0.7);

        assert_eq!(state.current_quality(), 0.7);
        assert!(state.is_quality_improving(0.05));
        assert!(!state.is_quality_improving(0.2));
    }

    #[test]
    fn test_uncertainty_calculation() {
        let mut state = IterationState::new(10, 5);

        for q in [0.7, 0.71, 0.70, 0.72, 0.71] {
            state.record_quality(q);
        }
        assert!(
            state.uncertainty < 0.2,
            "Expected low uncertainty, got {}",
            state.uncertainty
        );

        let mut state2 = IterationState::new(10, 5);
        for q in [0.5, 0.8, 0.5, 0.8, 0.5] {
            state2.record_quality(q);
        }
        assert!(
            state2.uncertainty > 0.3,
            "Expected high uncertainty, got {}",
            state2.uncertainty
        );
    }

    #[test]
    fn test_extension_trigger() {
        let mut state = IterationState::new(5, 3);

        for i in 0..5 {
            state.record_quality(0.5 + i as f32 * 0.05);
            state.iteration = i + 1;
        }

        assert!(state.maybe_extend(BudgetExtensionTrigger::QualityImproving { min_delta: 0.01 }));
        assert_eq!(state.estimated_total, 6);

        state.estimated_total = 8;
        assert!(!state.maybe_extend(BudgetExtensionTrigger::QualityImproving { min_delta: 0.01 }));
    }

    #[test]
    fn test_satisfaction() {
        let mut state = IterationState::new(10, 5);
        state.record_quality(0.9);
        state.uncertainty = 0.1;
        state.needs_more_thinking = false;

        assert!(state.is_satisfied(0.85, 0.2));
        assert!(!state.is_satisfied(0.95, 0.2));
        assert!(!state.is_satisfied(0.85, 0.05));
    }

    #[test]
    fn test_iteration_record() {
        let record = IterationRecord::new(1, 0.5)
            .with_quality_after(0.7)
            .with_rationale("Improve actionability")
            .with_strategies(vec!["SemanticStrategy".into()])
            .needs_continuation(true);

        assert!((record.improvement() - 0.2).abs() < 0.001);
        assert!(record.needs_more_thinking);
        assert!(!record.is_revision);
    }

    #[test]
    fn test_revision_tracking() {
        let record = IterationRecord::new(3, 0.6)
            .with_revision(1, "Previous strategy failed")
            .with_quality_after(0.75);

        assert!(record.is_revision);
        assert_eq!(record.revises_iteration, Some(1));
    }
}
