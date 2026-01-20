//! Clean Pass Tracker - Consecutive Zero-Issue Verification
//!
//! Guarantees quality by requiring N consecutive validation passes with zero issues.
//! Any issue (Error or Critical severity) resets the streak counter.

use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::layers::{IssueSeverity, ValidationResults};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanPassStatus {
    InProgress { streak: usize, required: usize },
    Converged { passes: usize },
    Failed { reason: FailureReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    MaxAttemptsReached,
    LlmValidationFailed,
    UnrecoverableError,
}

impl CleanPassStatus {
    pub fn is_converged(&self) -> bool {
        matches!(self, Self::Converged { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    pub fn is_in_progress(&self) -> bool {
        matches!(self, Self::InProgress { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanPassAttempt {
    pub attempt_number: usize,
    pub was_clean: bool,
    pub issue_count: usize,
    pub critical_count: usize,
    pub error_count: usize,
    pub warning_count: usize,
    pub score: f32,
    #[serde(skip)]
    pub timestamp: Option<Instant>,
}

impl CleanPassAttempt {
    pub fn from_results(attempt_number: usize, results: &ValidationResults) -> Self {
        let issues = results.all_issues();
        Self {
            attempt_number,
            was_clean: results.is_clean(),
            issue_count: issues.len(),
            critical_count: issues
                .iter()
                .filter(|i| i.severity == IssueSeverity::Critical)
                .count(),
            error_count: issues
                .iter()
                .filter(|i| i.severity == IssueSeverity::Error)
                .count(),
            warning_count: issues
                .iter()
                .filter(|i| i.severity == IssueSeverity::Warning)
                .count(),
            score: results.overall_score,
            timestamp: Some(Instant::now()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CleanPassTracker {
    required_passes: usize,
    current_streak: usize,
    total_attempts: usize,
    max_attempts: usize,
    history: Vec<CleanPassAttempt>,
    require_zero_issues: bool,
    reset_severities: Vec<IssueSeverity>,
}

impl CleanPassTracker {
    pub fn new(required_passes: usize) -> Self {
        Self {
            required_passes,
            current_streak: 0,
            total_attempts: 0,
            max_attempts: 10,
            history: Vec::new(),
            require_zero_issues: true,
            reset_severities: vec![IssueSeverity::Error, IssueSeverity::Critical],
        }
    }

    pub fn with_max_attempts(mut self, max: usize) -> Self {
        self.max_attempts = max;
        self
    }

    pub fn with_reset_severities(mut self, severities: Vec<IssueSeverity>) -> Self {
        self.reset_severities = severities;
        self
    }

    pub fn record_attempt(&mut self, results: &ValidationResults) -> CleanPassStatus {
        self.total_attempts += 1;
        let attempt = CleanPassAttempt::from_results(self.total_attempts, results);

        let has_reset_issue = results.all_issues().iter().any(|issue| {
            self.reset_severities.contains(&issue.severity)
        });

        let is_clean = if self.require_zero_issues {
            results.is_clean()
        } else {
            !has_reset_issue
        };

        if is_clean {
            self.current_streak += 1;
        } else {
            self.current_streak = 0;
        }

        self.history.push(attempt);
        self.trim_history();

        if self.current_streak >= self.required_passes {
            return CleanPassStatus::Converged {
                passes: self.current_streak,
            };
        }

        if self.total_attempts >= self.max_attempts {
            return CleanPassStatus::Failed {
                reason: FailureReason::MaxAttemptsReached,
            };
        }

        CleanPassStatus::InProgress {
            streak: self.current_streak,
            required: self.required_passes,
        }
    }

    pub fn record_llm_failure(&mut self) -> CleanPassStatus {
        self.total_attempts += 1;
        self.current_streak = 0;

        let attempt = CleanPassAttempt {
            attempt_number: self.total_attempts,
            was_clean: false,
            issue_count: 1,
            critical_count: 1,
            error_count: 0,
            warning_count: 0,
            score: 0.0,
            timestamp: Some(Instant::now()),
        };
        self.history.push(attempt);
        self.trim_history();

        CleanPassStatus::Failed {
            reason: FailureReason::LlmValidationFailed,
        }
    }

    /// Trim history to max_attempts to prevent unbounded memory growth
    fn trim_history(&mut self) {
        if self.history.len() > self.max_attempts {
            let excess = self.history.len() - self.max_attempts;
            self.history.drain(0..excess);
        }
    }

    pub fn current_streak(&self) -> usize {
        self.current_streak
    }

    pub fn total_attempts(&self) -> usize {
        self.total_attempts
    }

    pub fn required_passes(&self) -> usize {
        self.required_passes
    }

    pub fn remaining_passes(&self) -> usize {
        self.required_passes.saturating_sub(self.current_streak)
    }

    pub fn history(&self) -> &[CleanPassAttempt] {
        &self.history
    }

    pub fn reset(&mut self) {
        self.current_streak = 0;
        self.total_attempts = 0;
        self.history.clear();
    }

    pub fn trend(&self) -> PassTrend {
        if self.history.len() < 2 {
            return PassTrend::Insufficient;
        }

        let recent: Vec<_> = self.history.iter().rev().take(3).collect();
        let avg_issues: f32 =
            recent.iter().map(|a| a.issue_count as f32).sum::<f32>() / recent.len() as f32;

        let older: Vec<_> = self.history.iter().rev().skip(3).take(3).collect();
        if older.is_empty() {
            return PassTrend::Insufficient;
        }

        let old_avg: f32 =
            older.iter().map(|a| a.issue_count as f32).sum::<f32>() / older.len() as f32;

        if avg_issues < old_avg * 0.8 {
            PassTrend::Improving
        } else if avg_issues > old_avg * 1.2 {
            PassTrend::Degrading
        } else {
            PassTrend::Stable
        }
    }

    pub fn progress_summary(&self) -> ProgressSummary {
        ProgressSummary {
            current_streak: self.current_streak,
            required_passes: self.required_passes,
            total_attempts: self.total_attempts,
            max_attempts: self.max_attempts,
            clean_passes: self.history.iter().filter(|a| a.was_clean).count(),
            trend: self.trend(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassTrend {
    Improving,
    Stable,
    Degrading,
    Insufficient,
}

#[derive(Debug, Clone)]
pub struct ProgressSummary {
    pub current_streak: usize,
    pub required_passes: usize,
    pub total_attempts: usize,
    pub max_attempts: usize,
    pub clean_passes: usize,
    pub trend: PassTrend,
}

impl ProgressSummary {
    pub fn progress_percentage(&self) -> f32 {
        if self.required_passes == 0 {
            return 100.0;
        }
        (self.current_streak as f32 / self.required_passes as f32 * 100.0).min(100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::validation::layers::{LayerResult, ValidationLayer};

    fn clean_results() -> ValidationResults {
        let mut results = ValidationResults::new();
        results.add_layer_result(LayerResult::pass(ValidationLayer::Format));
        results.add_layer_result(LayerResult::pass(ValidationLayer::Evidence));
        results
    }

    fn results_with_error() -> ValidationResults {
        use crate::pipeline::validation::layers::{IssueCode, ValidationIssue};

        let mut results = ValidationResults::new();
        results.add_layer_result(LayerResult::pass(ValidationLayer::Format));

        let issues = vec![ValidationIssue::error(
            ValidationLayer::Evidence,
            "test",
            IssueCode::FileNotFound,
            "File not found",
        )];
        results.add_layer_result(LayerResult::fail(ValidationLayer::Evidence, issues));
        results
    }

    #[test]
    fn test_clean_pass_resets_on_any_issue() {
        let mut tracker = CleanPassTracker::new(2);
        tracker.record_attempt(&clean_results());
        assert_eq!(tracker.current_streak(), 1);

        tracker.record_attempt(&results_with_error());
        assert_eq!(tracker.current_streak(), 0);
    }

    #[test]
    fn test_converges_after_n_consecutive_clean() {
        let mut tracker = CleanPassTracker::new(2);
        tracker.record_attempt(&clean_results());
        assert_eq!(tracker.current_streak(), 1);

        let status = tracker.record_attempt(&clean_results());
        assert!(matches!(status, CleanPassStatus::Converged { passes: 2 }));
    }

    #[test]
    fn test_fails_after_max_attempts() {
        let mut tracker = CleanPassTracker::new(2).with_max_attempts(3);

        for _ in 0..3 {
            tracker.record_attempt(&results_with_error());
        }

        let status = tracker.record_attempt(&results_with_error());
        assert!(matches!(
            status,
            CleanPassStatus::Failed {
                reason: FailureReason::MaxAttemptsReached
            }
        ));
    }

    #[test]
    fn test_llm_failure_handling() {
        let mut tracker = CleanPassTracker::new(2);
        tracker.record_attempt(&clean_results());

        let status = tracker.record_llm_failure();
        assert!(matches!(
            status,
            CleanPassStatus::Failed {
                reason: FailureReason::LlmValidationFailed
            }
        ));
        assert_eq!(tracker.current_streak(), 0);
    }

    #[test]
    fn test_remaining_passes() {
        let mut tracker = CleanPassTracker::new(3);
        assert_eq!(tracker.remaining_passes(), 3);

        tracker.record_attempt(&clean_results());
        assert_eq!(tracker.remaining_passes(), 2);

        tracker.record_attempt(&clean_results());
        assert_eq!(tracker.remaining_passes(), 1);
    }

    #[test]
    fn test_progress_summary() {
        let mut tracker = CleanPassTracker::new(2).with_max_attempts(5);
        tracker.record_attempt(&clean_results());

        let summary = tracker.progress_summary();
        assert_eq!(summary.current_streak, 1);
        assert_eq!(summary.required_passes, 2);
        assert_eq!(summary.progress_percentage(), 50.0);
    }
}
