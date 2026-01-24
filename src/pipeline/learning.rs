//! Learning History Module
//!
//! Tracks refinement outcomes and learns optimal strategies for different issue types.
//! Enables pattern-based strategy selection across iterations.
//! Supports cross-session persistence for continuous learning.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::config::LearningConfig;
use crate::types::Result;

use super::strategy::IssueKind;

#[derive(Debug, Clone)]
pub struct LearningHistory {
    config: LearningConfig,
    iterations: Vec<IterationRecord>,
    strategy_outcomes: HashMap<String, Vec<StrategyOutcome>>,
    issue_patterns: HashMap<IssuePattern, ResolutionPath>,
    /// CRITICAL FIX: Track failing patterns to avoid repeated failures
    failing_patterns: HashMap<IssuePattern, FailingPatternRecord>,
    session_stats: SessionStats,
}

/// Records which strategies have failed for a specific pattern
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailingPatternRecord {
    /// Strategies that have failed for this pattern
    pub failed_strategies: Vec<String>,
    /// Total number of failures recorded
    pub failure_count: usize,
    /// Last iteration where failure occurred
    pub last_failure_iteration: usize,
}

impl Default for LearningHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationRecord {
    pub iteration: usize,
    pub quality_before: f32,
    pub quality_after: f32,
    pub issues_addressed: usize,
    pub issues_resolved: usize,
    pub strategies_used: Vec<String>,
    pub outcomes: Vec<StrategyOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyOutcome {
    pub strategy_name: String,
    pub issue_kind: String,
    pub item_name: String,
    pub quality_before: f32,
    pub quality_after: f32,
    pub success: bool,
    pub iteration: usize,
}

impl StrategyOutcome {
    pub fn improvement(&self) -> f32 {
        self.quality_after - self.quality_before
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuePattern {
    pub issue_type: String,
    pub artifact_type: String,
    pub quality_range: QualityRange,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityRange {
    VeryLow,  // 0.0 - threshold[0]
    Low,      // threshold[0] - threshold[1]
    Medium,   // threshold[1] - threshold[2]
    High,     // threshold[2] - threshold[3]
    VeryHigh, // threshold[3] - 1.0
}

impl QualityRange {
    /// Classify quality score using default thresholds
    pub fn from_score(score: f32) -> Self {
        Self::from_score_with_thresholds(
            score,
            &LearningConfig::default().quality_thresholds.as_array(),
        )
    }

    /// Classify quality score using custom thresholds
    pub fn from_score_with_thresholds(score: f32, thresholds: &[f32; 4]) -> Self {
        match score {
            s if s < thresholds[0] => Self::VeryLow,
            s if s < thresholds[1] => Self::Low,
            s if s < thresholds[2] => Self::Medium,
            s if s < thresholds[3] => Self::High,
            _ => Self::VeryHigh,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionPath {
    pub best_strategy: String,
    pub success_rate: f32,
    pub avg_improvement: f32,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub total_iterations: usize,
    pub total_issues: usize,
    pub resolved_issues: usize,
    pub total_improvement: f32,
    pub strategy_usage: HashMap<String, usize>,
    pub strategy_success: HashMap<String, (usize, usize)>,
}

impl LearningHistory {
    pub fn new() -> Self {
        Self::with_config(LearningConfig::default())
    }

    pub fn with_config(config: LearningConfig) -> Self {
        Self {
            config,
            iterations: Vec::new(),
            strategy_outcomes: HashMap::new(),
            issue_patterns: HashMap::new(),
            failing_patterns: HashMap::new(),
            session_stats: SessionStats::default(),
        }
    }

    pub fn record_iteration(&mut self, record: IterationRecord) {
        self.session_stats.total_iterations += 1;
        self.session_stats.total_issues += record.issues_addressed;
        self.session_stats.resolved_issues += record.issues_resolved;
        self.session_stats.total_improvement += record.quality_after - record.quality_before;

        for outcome in &record.outcomes {
            self.record_outcome(outcome.clone());
        }

        self.iterations.push(record);

        // Bound iterations to prevent unbounded growth
        let max_iterations = self.config.max_stored_iterations;
        if self.iterations.len() > max_iterations {
            self.iterations
                .drain(0..self.iterations.len() - max_iterations);
        }
    }

    pub fn record_outcome(&mut self, outcome: StrategyOutcome) {
        *self
            .session_stats
            .strategy_usage
            .entry(outcome.strategy_name.clone())
            .or_insert(0) += 1;

        let (successes, total) = self
            .session_stats
            .strategy_success
            .entry(outcome.strategy_name.clone())
            .or_insert((0, 0));
        *total += 1;
        if outcome.success {
            *successes += 1;
        }

        let outcomes = self
            .strategy_outcomes
            .entry(outcome.strategy_name.clone())
            .or_default();
        outcomes.push(outcome.clone());

        // Bound outcomes per strategy
        let max_per_strategy = self.config.max_outcomes_per_strategy;
        if outcomes.len() > max_per_strategy {
            outcomes.drain(0..outcomes.len() - max_per_strategy);
        }

        let pattern = IssuePattern {
            issue_type: outcome.issue_kind.clone(),
            artifact_type: self.extract_artifact_type(&outcome.item_name),
            quality_range: QualityRange::from_score_with_thresholds(
                outcome.quality_before,
                &self.config.quality_thresholds.as_array(),
            ),
        };

        if outcome.success && outcome.improvement() > self.config.min_improvement_for_pattern {
            self.update_resolution_path(pattern, &outcome);
        } else if !outcome.success {
            // CRITICAL FIX: Track failures as negative patterns
            self.record_failure(pattern, &outcome);
        }
    }

    /// Record a strategy failure for a pattern
    fn record_failure(&mut self, pattern: IssuePattern, outcome: &StrategyOutcome) {
        // Bound failing_patterns to prevent unbounded growth
        if self.failing_patterns.len() >= self.config.max_patterns
            && !self.failing_patterns.contains_key(&pattern)
        {
            self.prune_oldest_failing_patterns();
        }

        let record = self.failing_patterns.entry(pattern).or_default();

        if !record.failed_strategies.contains(&outcome.strategy_name) {
            record.failed_strategies.push(outcome.strategy_name.clone());
        }
        record.failure_count += 1;
        record.last_failure_iteration = outcome.iteration;

        tracing::debug!(
            strategy = %outcome.strategy_name,
            issue_kind = %outcome.issue_kind,
            failures = record.failure_count,
            "Recorded strategy failure for pattern"
        );
    }

    /// Prune oldest failing patterns to stay within bounds
    fn prune_oldest_failing_patterns(&mut self) {
        // Remove patterns with oldest last_failure_iteration (least recent)
        let mut patterns: Vec<_> = self
            .failing_patterns
            .iter()
            .map(|(k, v)| (k.clone(), v.last_failure_iteration))
            .collect();
        patterns.sort_by_key(|(_, iteration)| *iteration);

        // Remove bottom 10%
        let to_remove = patterns.len() / 10;
        for (pattern, _) in patterns.into_iter().take(to_remove.max(1)) {
            self.failing_patterns.remove(&pattern);
        }
    }

    /// Check if a strategy should be skipped for a given issue pattern
    pub fn should_skip_strategy(
        &self,
        issue_kind: &IssueKind,
        item_name: &str,
        current_quality: f32,
        strategy_name: &str,
    ) -> bool {
        let pattern = IssuePattern {
            issue_type: format!("{:?}", issue_kind),
            artifact_type: self.extract_artifact_type(item_name),
            quality_range: QualityRange::from_score_with_thresholds(
                current_quality,
                &self.config.quality_thresholds.as_array(),
            ),
        };

        if let Some(record) = self.failing_patterns.get(&pattern) {
            // Skip if strategy has failed 3+ times for this pattern
            if record
                .failed_strategies
                .contains(&strategy_name.to_string())
                && record.failure_count >= 3
            {
                tracing::debug!(
                    strategy = %strategy_name,
                    pattern = ?pattern,
                    failures = record.failure_count,
                    "Skipping strategy due to repeated failures"
                );
                return true;
            }
        }

        false
    }

    /// Get successful resolution patterns for analysis
    pub fn get_issue_patterns(&self) -> &HashMap<IssuePattern, ResolutionPath> {
        &self.issue_patterns
    }

    /// Get the count of learned resolution patterns
    pub fn pattern_count(&self) -> usize {
        self.issue_patterns.len()
    }

    /// Get failing patterns for analysis
    pub fn get_failing_patterns(&self) -> &HashMap<IssuePattern, FailingPatternRecord> {
        &self.failing_patterns
    }

    fn extract_artifact_type(&self, item_name: &str) -> String {
        if item_name.starts_with("skill:") {
            "skill".into()
        } else if item_name.starts_with("agent:") {
            "agent".into()
        } else if item_name.starts_with("rule:") {
            "rule".into()
        } else {
            "unknown".into()
        }
    }

    fn update_resolution_path(&mut self, pattern: IssuePattern, outcome: &StrategyOutcome) {
        // Bound patterns to prevent unbounded growth
        if self.issue_patterns.len() >= self.config.max_patterns
            && !self.issue_patterns.contains_key(&pattern)
        {
            self.prune_oldest_patterns();
        }

        let entry = self
            .issue_patterns
            .entry(pattern)
            .or_insert(ResolutionPath {
                best_strategy: outcome.strategy_name.clone(),
                success_rate: 0.0,
                avg_improvement: 0.0,
                sample_count: 0,
            });

        let new_count = entry.sample_count + 1;
        entry.avg_improvement = (entry.avg_improvement * entry.sample_count as f32
            + outcome.improvement())
            / new_count as f32;
        entry.sample_count = new_count;

        let strategy_outcomes = self.strategy_outcomes.get(&outcome.strategy_name);
        if let Some(outcomes) = strategy_outcomes {
            let successes = outcomes.iter().filter(|o| o.success).count();
            entry.success_rate = successes as f32 / outcomes.len() as f32;
        }

        if outcome.improvement() > entry.avg_improvement {
            entry.best_strategy = outcome.strategy_name.clone();
        }
    }

    fn prune_oldest_patterns(&mut self) {
        // Remove patterns with lowest sample counts (least useful)
        let mut patterns: Vec<_> = self
            .issue_patterns
            .iter()
            .map(|(k, v)| (k.clone(), v.sample_count))
            .collect();
        patterns.sort_by_key(|(_, count)| *count);

        // Remove bottom 10%
        let to_remove = patterns.len() / 10;
        for (pattern, _) in patterns.into_iter().take(to_remove) {
            self.issue_patterns.remove(&pattern);
        }
    }

    pub fn recommend_strategy(
        &self,
        issue_kind: &IssueKind,
        item_name: &str,
        current_quality: f32,
    ) -> Option<String> {
        let pattern = IssuePattern {
            issue_type: format!("{:?}", issue_kind),
            artifact_type: self.extract_artifact_type(item_name),
            quality_range: QualityRange::from_score_with_thresholds(
                current_quality,
                &self.config.quality_thresholds.as_array(),
            ),
        };

        if let Some(path) = self.issue_patterns.get(&pattern)
            && path.success_rate >= self.config.recommend_success_threshold
            && path.sample_count >= self.config.recommend_min_samples
        {
            return Some(path.best_strategy.clone());
        }

        self.find_similar_pattern(&pattern)
            .and_then(|p| self.issue_patterns.get(&p))
            .filter(|path| path.success_rate >= self.config.fallback_success_threshold)
            .map(|path| path.best_strategy.clone())
    }

    fn find_similar_pattern(&self, target: &IssuePattern) -> Option<IssuePattern> {
        self.issue_patterns
            .keys()
            .filter(|p| p.issue_type == target.issue_type)
            .min_by_key(|p| {
                let type_match = if p.artifact_type == target.artifact_type {
                    0
                } else {
                    1
                };
                let range_diff =
                    self.quality_range_distance(&p.quality_range, &target.quality_range);
                type_match * 10 + range_diff
            })
            .cloned()
    }

    fn quality_range_distance(&self, a: &QualityRange, b: &QualityRange) -> i32 {
        let to_num = |r: &QualityRange| -> i32 {
            match r {
                QualityRange::VeryLow => 0,
                QualityRange::Low => 1,
                QualityRange::Medium => 2,
                QualityRange::High => 3,
                QualityRange::VeryHigh => 4,
            }
        };
        (to_num(a) - to_num(b)).abs()
    }

    pub fn get_best_strategies(&self, top_n: usize) -> Vec<(String, f32)> {
        let mut strategies: Vec<_> = self
            .session_stats
            .strategy_success
            .iter()
            .filter(|(_, (_, total))| *total >= 2)
            .map(|(name, (successes, total))| (name.clone(), *successes as f32 / *total as f32))
            .collect();

        strategies.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        strategies.into_iter().take(top_n).collect()
    }

    pub fn get_failing_strategies(&self) -> Vec<String> {
        let min_attempts = self.config.failing_strategy_min_attempts;
        let threshold = self.config.failing_strategy_threshold;

        self.session_stats
            .strategy_success
            .iter()
            .filter(|(_, (successes, total))| {
                *total >= min_attempts && (*successes as f32 / *total as f32) < threshold
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn should_escalate(&self) -> bool {
        let window = self.config.escalation_window;
        let threshold = self.config.escalation_threshold;

        let recent_outcomes: Vec<_> = self
            .iterations
            .iter()
            .rev()
            .take(window)
            .flat_map(|r| r.outcomes.iter())
            .collect();

        if recent_outcomes.len() < window {
            return false;
        }

        let recent_success_rate = recent_outcomes.iter().filter(|o| o.success).count() as f32
            / recent_outcomes.len() as f32;
        recent_success_rate < threshold
    }

    pub fn get_progress_summary(&self) -> ProgressSummary {
        let initial_quality = self
            .iterations
            .first()
            .map(|r| r.quality_before)
            .unwrap_or(0.0);
        let current_quality = self
            .iterations
            .last()
            .map(|r| r.quality_after)
            .unwrap_or(0.0);

        ProgressSummary {
            iterations: self.session_stats.total_iterations,
            initial_quality,
            current_quality,
            improvement: current_quality - initial_quality,
            issues_total: self.session_stats.total_issues,
            issues_resolved: self.session_stats.resolved_issues,
            resolution_rate: if self.session_stats.total_issues > 0 {
                self.session_stats.resolved_issues as f32 / self.session_stats.total_issues as f32
            } else {
                1.0
            },
            best_strategies: self.get_best_strategies(3),
        }
    }

    pub fn iterations(&self) -> &[IterationRecord] {
        &self.iterations
    }

    /// Persist learning patterns to disk for cross-session learning
    ///
    /// Saves resolution patterns and failing patterns to `.claudegen/learning/`
    /// directory. This enables the system to learn from previous sessions.
    pub async fn persist(&self, project_root: &Path) -> Result<()> {
        let learning_dir = project_root.join(".claudegen").join("learning");
        fs::create_dir_all(&learning_dir).await?;

        // Convert HashMaps to Vecs for JSON serialization (JSON keys must be strings)
        let patterns_vec: Vec<PatternEntry> = self
            .issue_patterns
            .iter()
            .map(|(k, v)| PatternEntry {
                pattern: k.clone(),
                resolution: v.clone(),
            })
            .collect();

        // Save resolution patterns (successful strategies for patterns)
        let patterns_path = learning_dir.join("patterns.json");
        let patterns_json = serde_json::to_string_pretty(&patterns_vec).map_err(|e| {
            crate::types::ClaudegenError::Config(format!("Failed to serialize patterns: {}", e))
        })?;
        fs::write(&patterns_path, patterns_json).await?;

        // Convert failing patterns HashMap to Vec
        let failing_vec: Vec<FailingPatternEntry> = self
            .failing_patterns
            .iter()
            .map(|(k, v)| FailingPatternEntry {
                pattern: k.clone(),
                record: v.clone(),
            })
            .collect();

        // Save failing patterns (strategies to avoid)
        let failing_path = learning_dir.join("failing_patterns.json");
        let failing_json = serde_json::to_string_pretty(&failing_vec).map_err(|e| {
            crate::types::ClaudegenError::Config(format!(
                "Failed to serialize failing patterns: {}",
                e
            ))
        })?;
        fs::write(&failing_path, failing_json).await?;

        // Save session summary for analytics
        let summary = self.get_progress_summary();
        let summary_path = learning_dir.join("last_session.json");
        let summary_json = serde_json::to_string_pretty(&summary).map_err(|e| {
            crate::types::ClaudegenError::Config(format!("Failed to serialize summary: {}", e))
        })?;
        fs::write(&summary_path, summary_json).await?;

        tracing::info!(
            patterns = self.issue_patterns.len(),
            failing_patterns = self.failing_patterns.len(),
            path = %learning_dir.display(),
            "Learning patterns persisted"
        );

        Ok(())
    }

    /// Load learning patterns from disk
    ///
    /// Restores resolution patterns and failing patterns from previous sessions.
    /// Returns a fresh LearningHistory with loaded patterns but empty session data.
    pub async fn load(project_root: &Path, config: LearningConfig) -> Result<Self> {
        let learning_dir = project_root.join(".claudegen").join("learning");

        // Load resolution patterns (stored as Vec, convert back to HashMap)
        let patterns_path = learning_dir.join("patterns.json");
        let issue_patterns: HashMap<IssuePattern, ResolutionPath> = if patterns_path.exists() {
            let content = fs::read_to_string(&patterns_path).await?;
            let patterns_vec: Vec<PatternEntry> = serde_json::from_str(&content).map_err(|e| {
                crate::types::ClaudegenError::Config(format!("Failed to parse patterns: {}", e))
            })?;
            patterns_vec
                .into_iter()
                .map(|e| (e.pattern, e.resolution))
                .collect()
        } else {
            HashMap::new()
        };

        // Load failing patterns (stored as Vec, convert back to HashMap)
        let failing_path = learning_dir.join("failing_patterns.json");
        let failing_patterns: HashMap<IssuePattern, FailingPatternRecord> = if failing_path.exists()
        {
            let content = fs::read_to_string(&failing_path).await?;
            let failing_vec: Vec<FailingPatternEntry> =
                serde_json::from_str(&content).map_err(|e| {
                    crate::types::ClaudegenError::Config(format!(
                        "Failed to parse failing patterns: {}",
                        e
                    ))
                })?;
            failing_vec
                .into_iter()
                .map(|e| (e.pattern, e.record))
                .collect()
        } else {
            HashMap::new()
        };

        tracing::info!(
            patterns = issue_patterns.len(),
            failing_patterns = failing_patterns.len(),
            "Learning patterns loaded from previous session"
        );

        Ok(Self {
            config,
            iterations: Vec::new(),                 // Fresh for new session
            strategy_outcomes: HashMap::new(),      // Fresh for new session
            issue_patterns,                         // Loaded from disk
            failing_patterns,                       // Loaded from disk
            session_stats: SessionStats::default(), // Fresh for new session
        })
    }

    /// Check if persisted learning data exists
    pub fn has_persisted_data(project_root: &Path) -> bool {
        let learning_dir = project_root.join(".claudegen").join("learning");
        learning_dir.join("patterns.json").exists()
    }

    pub fn clear(&mut self) {
        self.iterations.clear();
        self.strategy_outcomes.clear();
        self.issue_patterns.clear();
        self.failing_patterns.clear();
        self.session_stats = SessionStats::default();
    }

    /// Clear only session-specific data while preserving learned patterns
    pub fn clear_session(&mut self) {
        self.iterations.clear();
        self.strategy_outcomes.clear();
        self.session_stats = SessionStats::default();
        // Preserve: issue_patterns, failing_patterns
    }
}

/// Serialization wrapper for IssuePattern -> ResolutionPath mapping
///
/// JSON requires string keys in objects, so we use a Vec of entries
/// instead of HashMap for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatternEntry {
    pattern: IssuePattern,
    resolution: ResolutionPath,
}

/// Serialization wrapper for IssuePattern -> FailingPatternRecord mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FailingPatternEntry {
    pattern: IssuePattern,
    record: FailingPatternRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressSummary {
    pub iterations: usize,
    pub initial_quality: f32,
    pub current_quality: f32,
    pub improvement: f32,
    pub issues_total: usize,
    pub issues_resolved: usize,
    pub resolution_rate: f32,
    pub best_strategies: Vec<(String, f32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_learning_history_creation() {
        let history = LearningHistory::new();
        assert_eq!(history.iterations.len(), 0);
    }

    #[test]
    fn test_quality_range() {
        assert_eq!(QualityRange::from_score(0.1), QualityRange::VeryLow);
        assert_eq!(QualityRange::from_score(0.4), QualityRange::Low);
        assert_eq!(QualityRange::from_score(0.6), QualityRange::Medium);
        assert_eq!(QualityRange::from_score(0.8), QualityRange::High);
        assert_eq!(QualityRange::from_score(0.9), QualityRange::VeryHigh);
    }

    #[test]
    fn test_record_iteration() {
        let mut history = LearningHistory::new();

        let record = IterationRecord {
            iteration: 1,
            quality_before: 0.5,
            quality_after: 0.6,
            issues_addressed: 5,
            issues_resolved: 3,
            strategies_used: vec!["SemanticStrategy".into()],
            outcomes: vec![],
        };

        history.record_iteration(record);

        assert_eq!(history.session_stats.total_iterations, 1);
        assert_eq!(history.session_stats.total_issues, 5);
        assert_eq!(history.session_stats.resolved_issues, 3);
    }

    #[test]
    fn test_get_progress_summary() {
        let mut history = LearningHistory::new();

        history.record_iteration(IterationRecord {
            iteration: 1,
            quality_before: 0.5,
            quality_after: 0.6,
            issues_addressed: 5,
            issues_resolved: 3,
            strategies_used: vec![],
            outcomes: vec![],
        });

        let summary = history.get_progress_summary();
        assert_eq!(summary.initial_quality, 0.5);
        assert_eq!(summary.current_quality, 0.6);
        assert!((summary.improvement - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_custom_quality_thresholds() {
        // Custom thresholds: [0.2, 0.4, 0.6, 0.8]
        let thresholds = [0.2, 0.4, 0.6, 0.8];

        assert_eq!(
            QualityRange::from_score_with_thresholds(0.1, &thresholds),
            QualityRange::VeryLow
        );
        assert_eq!(
            QualityRange::from_score_with_thresholds(0.3, &thresholds),
            QualityRange::Low
        );
        assert_eq!(
            QualityRange::from_score_with_thresholds(0.5, &thresholds),
            QualityRange::Medium
        );
        assert_eq!(
            QualityRange::from_score_with_thresholds(0.7, &thresholds),
            QualityRange::High
        );
        assert_eq!(
            QualityRange::from_score_with_thresholds(0.9, &thresholds),
            QualityRange::VeryHigh
        );
    }

    #[test]
    fn test_with_custom_config() {
        let config = LearningConfig {
            min_improvement_for_pattern: 0.05,
            recommend_success_threshold: 0.6,
            escalation_threshold: 0.4,
            ..Default::default()
        };

        let history = LearningHistory::with_config(config);
        assert_eq!(history.iterations.len(), 0);
    }

    #[test]
    fn test_failure_pattern_recording() {
        let mut history = LearningHistory::new();

        // Record a failed outcome
        let outcome = StrategyOutcome {
            strategy_name: "SemanticStrategy".into(),
            issue_kind: "LowActionability".into(),
            item_name: "skill:test-skill".into(),
            quality_before: 0.5,
            quality_after: 0.5, // No improvement = failure
            success: false,
            iteration: 1,
        };

        history.record_outcome(outcome);

        // Check that failure was recorded
        let failing_patterns = history.get_failing_patterns();
        assert_eq!(failing_patterns.len(), 1);

        let pattern = IssuePattern {
            issue_type: "LowActionability".into(),
            artifact_type: "skill".into(),
            quality_range: QualityRange::from_score(0.5),
        };

        let record = failing_patterns.get(&pattern).unwrap();
        assert_eq!(record.failure_count, 1);
        assert!(
            record
                .failed_strategies
                .contains(&"SemanticStrategy".to_string())
        );
    }

    #[test]
    fn test_should_skip_strategy_under_threshold() {
        let mut history = LearningHistory::new();

        // Record 2 failures (under threshold of 3)
        for i in 1..=2 {
            let outcome = StrategyOutcome {
                strategy_name: "EvidenceStrategy".into(),
                issue_kind: "WeakEvidence".into(),
                item_name: "skill:weak-skill".into(),
                quality_before: 0.4,
                quality_after: 0.4,
                success: false,
                iteration: i,
            };
            history.record_outcome(outcome);
        }

        // Should NOT skip with only 2 failures
        let should_skip = history.should_skip_strategy(
            &IssueKind::WeakEvidence,
            "skill:weak-skill",
            0.4,
            "EvidenceStrategy",
        );
        assert!(!should_skip, "Should not skip with only 2 failures");
    }

    #[test]
    fn test_should_skip_strategy_at_threshold() {
        let mut history = LearningHistory::new();

        // Record 3 failures (at threshold)
        for i in 1..=3 {
            let outcome = StrategyOutcome {
                strategy_name: "EvidenceStrategy".into(),
                issue_kind: "WeakEvidence".into(),
                item_name: "skill:weak-skill".into(),
                quality_before: 0.4,
                quality_after: 0.4,
                success: false,
                iteration: i,
            };
            history.record_outcome(outcome);
        }

        // Should skip with 3+ failures
        let should_skip = history.should_skip_strategy(
            &IssueKind::WeakEvidence,
            "skill:weak-skill",
            0.4,
            "EvidenceStrategy",
        );
        assert!(should_skip, "Should skip with 3+ failures");
    }

    #[test]
    fn test_should_skip_different_strategy_not_affected() {
        let mut history = LearningHistory::new();

        // Record 3 failures for SemanticStrategy
        for i in 1..=3 {
            let outcome = StrategyOutcome {
                strategy_name: "SemanticStrategy".into(),
                issue_kind: "LowActionability".into(),
                item_name: "skill:test".into(),
                quality_before: 0.5,
                quality_after: 0.5,
                success: false,
                iteration: i,
            };
            history.record_outcome(outcome);
        }

        // Different strategy should NOT be skipped
        let should_skip = history.should_skip_strategy(
            &IssueKind::LowActionability,
            "skill:test",
            0.5,
            "EvidenceStrategy", // Different strategy
        );
        assert!(!should_skip, "Different strategy should not be affected");
    }

    #[test]
    fn test_clear_resets_failing_patterns() {
        let mut history = LearningHistory::new();

        // Record a failure
        let outcome = StrategyOutcome {
            strategy_name: "SemanticStrategy".into(),
            issue_kind: "LowActionability".into(),
            item_name: "skill:test".into(),
            quality_before: 0.5,
            quality_after: 0.5,
            success: false,
            iteration: 1,
        };
        history.record_outcome(outcome);

        assert!(!history.get_failing_patterns().is_empty());

        // Clear should reset failing patterns
        history.clear();
        assert!(history.get_failing_patterns().is_empty());
    }

    #[test]
    fn test_failure_multiple_strategies_same_pattern() {
        let mut history = LearningHistory::new();

        // Record failures for two different strategies on same pattern
        let outcome1 = StrategyOutcome {
            strategy_name: "SemanticStrategy".into(),
            issue_kind: "TooGeneric".into(),
            item_name: "rule:generic-rule".into(),
            quality_before: 0.6,
            quality_after: 0.6,
            success: false,
            iteration: 1,
        };
        let outcome2 = StrategyOutcome {
            strategy_name: "EvidenceStrategy".into(),
            issue_kind: "TooGeneric".into(),
            item_name: "rule:generic-rule".into(),
            quality_before: 0.6,
            quality_after: 0.6,
            success: false,
            iteration: 2,
        };

        history.record_outcome(outcome1);
        history.record_outcome(outcome2);

        let failing_patterns = history.get_failing_patterns();
        assert_eq!(failing_patterns.len(), 1);

        let pattern = IssuePattern {
            issue_type: "TooGeneric".into(),
            artifact_type: "rule".into(),
            quality_range: QualityRange::from_score(0.6),
        };

        let record = failing_patterns.get(&pattern).unwrap();
        assert_eq!(record.failure_count, 2);
        assert!(
            record
                .failed_strategies
                .contains(&"SemanticStrategy".to_string())
        );
        assert!(
            record
                .failed_strategies
                .contains(&"EvidenceStrategy".to_string())
        );
    }

    #[test]
    fn test_clear_session_preserves_patterns() {
        let mut history = LearningHistory::new();

        // Record a success pattern
        let outcome = StrategyOutcome {
            strategy_name: "SemanticStrategy".into(),
            issue_kind: "LowActionability".into(),
            item_name: "skill:test".into(),
            quality_before: 0.5,
            quality_after: 0.7,
            success: true,
            iteration: 1,
        };
        history.record_outcome(outcome);

        // Record a failure pattern
        let failure = StrategyOutcome {
            strategy_name: "EvidenceStrategy".into(),
            issue_kind: "WeakEvidence".into(),
            item_name: "skill:weak".into(),
            quality_before: 0.4,
            quality_after: 0.4,
            success: false,
            iteration: 2,
        };
        history.record_outcome(failure);

        // Ensure patterns are recorded
        assert!(!history.issue_patterns.is_empty() || !history.get_failing_patterns().is_empty());

        // Clear session - should preserve patterns
        history.clear_session();

        // Session data should be cleared
        assert!(history.iterations.is_empty());
        assert!(history.strategy_outcomes.is_empty());
        assert_eq!(history.session_stats.total_iterations, 0);

        // Patterns should be preserved (at least failing patterns should exist)
        assert!(!history.get_failing_patterns().is_empty());
    }

    #[test]
    fn test_issue_pattern_serialization() {
        let pattern = IssuePattern {
            issue_type: "WeakEvidence".into(),
            artifact_type: "skill".into(),
            quality_range: QualityRange::Medium,
        };

        let json = serde_json::to_string(&pattern).unwrap();
        let deserialized: IssuePattern = serde_json::from_str(&json).unwrap();

        assert_eq!(pattern.issue_type, deserialized.issue_type);
        assert_eq!(pattern.artifact_type, deserialized.artifact_type);
        assert_eq!(pattern.quality_range, deserialized.quality_range);
    }

    #[test]
    fn test_resolution_path_serialization() {
        let path = ResolutionPath {
            best_strategy: "SemanticStrategy".into(),
            success_rate: 0.85,
            avg_improvement: 0.12,
            sample_count: 10,
        };

        let json = serde_json::to_string(&path).unwrap();
        let deserialized: ResolutionPath = serde_json::from_str(&json).unwrap();

        assert_eq!(path.best_strategy, deserialized.best_strategy);
        assert!((path.success_rate - deserialized.success_rate).abs() < 0.001);
        assert!((path.avg_improvement - deserialized.avg_improvement).abs() < 0.001);
        assert_eq!(path.sample_count, deserialized.sample_count);
    }

    #[test]
    fn test_failing_pattern_record_serialization() {
        let record = FailingPatternRecord {
            failed_strategies: vec!["Semantic".into(), "Evidence".into()],
            failure_count: 5,
            last_failure_iteration: 12,
        };

        let json = serde_json::to_string(&record).unwrap();
        let deserialized: FailingPatternRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(record.failed_strategies, deserialized.failed_strategies);
        assert_eq!(record.failure_count, deserialized.failure_count);
        assert_eq!(
            record.last_failure_iteration,
            deserialized.last_failure_iteration
        );
    }

    #[test]
    fn test_has_persisted_data_no_dir() {
        let temp_dir = std::env::temp_dir().join("claudegen_test_no_data");
        assert!(!LearningHistory::has_persisted_data(&temp_dir));
    }

    #[tokio::test]
    async fn test_persist_and_load_patterns() {
        // Create a temporary directory
        let temp_dir = std::env::temp_dir().join(format!("claudegen_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut history = LearningHistory::new();

        // Record some patterns
        let outcome = StrategyOutcome {
            strategy_name: "SemanticStrategy".into(),
            issue_kind: "LowActionability".into(),
            item_name: "skill:test".into(),
            quality_before: 0.4,
            quality_after: 0.7,
            success: true,
            iteration: 1,
        };
        history.record_outcome(outcome);

        let failure = StrategyOutcome {
            strategy_name: "EvidenceStrategy".into(),
            issue_kind: "WeakEvidence".into(),
            item_name: "skill:weak".into(),
            quality_before: 0.3,
            quality_after: 0.3,
            success: false,
            iteration: 2,
        };
        history.record_outcome(failure);

        // Persist
        history.persist(&temp_dir).await.unwrap();

        // Verify files exist
        assert!(temp_dir.join(".claudegen/learning/patterns.json").exists());
        assert!(
            temp_dir
                .join(".claudegen/learning/failing_patterns.json")
                .exists()
        );
        assert!(
            temp_dir
                .join(".claudegen/learning/last_session.json")
                .exists()
        );

        // Load and verify
        let loaded = LearningHistory::load(&temp_dir, LearningConfig::default())
            .await
            .unwrap();

        // Session data should be fresh
        assert!(loaded.iterations.is_empty());

        // Patterns should be loaded (failing patterns since success rate might not meet threshold)
        assert!(!loaded.get_failing_patterns().is_empty());

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
