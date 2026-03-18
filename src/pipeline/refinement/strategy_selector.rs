//! Feedback-Aware Strategy Selector
//!
//! Selects refinement strategies based on:
//! 1. Previous failure history
//! 2. LLM feedback suggestions
//! 3. Issue type classification

use std::collections::HashMap;

use super::failure_tracker::FailureTracker;
use super::types::DetectedIssue;

const MAX_TRACKED_ARTIFACTS: usize = 200;

/// Strategy recommendation based on feedback analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecommendedStrategy {
    /// Re-generate with enriched evidence
    EvidenceInjection,
    /// Semantic restructuring and enhancement
    Semantic,
}

impl RecommendedStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EvidenceInjection => "evidence",
            Self::Semantic => "semantic",
        }
    }
}

/// Context for failed strategy attempts
#[derive(Debug, Clone)]
pub struct FailureContext {
    pub failed_strategies: Vec<String>,
    pub failure_count: usize,
    pub last_suggestion: Option<String>,
}

/// Feedback-aware strategy selector
pub struct FeedbackAwareSelector {
    failure_history: HashMap<String, FailureContext>,
    strategy_order: Vec<RecommendedStrategy>,
}

impl Default for FeedbackAwareSelector {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedbackAwareSelector {
    pub fn new() -> Self {
        Self {
            failure_history: HashMap::new(),
            strategy_order: vec![
                RecommendedStrategy::EvidenceInjection,
                RecommendedStrategy::Semantic,
            ],
        }
    }

    /// Select strategy based on issue, feedback, and failure history
    pub fn select_strategy(
        &self,
        artifact_name: &str,
        issue: &DetectedIssue,
        suggestion: Option<&str>,
        tracker: &FailureTracker,
    ) -> RecommendedStrategy {
        // 1. Check if we have failure context
        if let Some(ctx) = self.failure_history.get(artifact_name) {
            for strategy in &self.strategy_order {
                if !ctx
                    .failed_strategies
                    .contains(&strategy.as_str().to_string())
                    && !tracker.should_skip(artifact_name, strategy.as_str())
                {
                    return *strategy;
                }
            }
            // All strategies failed, use semantic as last resort
            return RecommendedStrategy::Semantic;
        }

        // 2. Use feedback to guide strategy selection
        if let Some(s) = suggestion {
            let suggestion_lower = s.to_lowercase();
            if suggestion_lower.contains("evidence")
                || suggestion_lower.contains("reference")
                || suggestion_lower.contains("cite")
            {
                return RecommendedStrategy::EvidenceInjection;
            }
            if suggestion_lower.contains("restructure")
                || suggestion_lower.contains("reorganize")
                || suggestion_lower.contains("rewrite")
                || suggestion_lower.contains("remove")
                || suggestion_lower.contains("improve")
            {
                return RecommendedStrategy::Semantic;
            }
        }

        // 3. Use issue type to guide strategy
        self.strategy_for_issue(issue)
    }

    /// Record failure for an artifact/strategy pair
    pub fn record_failure(
        &mut self,
        artifact_name: &str,
        strategy: &str,
        suggestion: Option<String>,
    ) {
        // Prune if at capacity (keep most recently failed)
        if self.failure_history.len() >= MAX_TRACKED_ARTIFACTS
            && !self.failure_history.contains_key(artifact_name)
        {
            // Remove entry with lowest failure_count
            if let Some(key_to_remove) = self
                .failure_history
                .iter()
                .min_by_key(|(_, ctx)| ctx.failure_count)
                .map(|(k, _)| k.clone())
            {
                self.failure_history.remove(&key_to_remove);
            }
        }

        let ctx = self
            .failure_history
            .entry(artifact_name.to_string())
            .or_insert_with(|| FailureContext {
                failed_strategies: Vec::new(),
                failure_count: 0,
                last_suggestion: None,
            });

        if !ctx.failed_strategies.contains(&strategy.to_string()) {
            ctx.failed_strategies.push(strategy.to_string());
        }
        ctx.failure_count += 1;
        ctx.last_suggestion = suggestion;
    }

    /// Record success and clear failure history for artifact
    pub fn record_success(&mut self, artifact_name: &str) {
        self.failure_history.remove(artifact_name);
    }

    fn strategy_for_issue(&self, issue: &DetectedIssue) -> RecommendedStrategy {
        match issue {
            // Evidence-related issues → inject more evidence
            DetectedIssue::MissingReferences { .. }
            | DetectedIssue::WeakEvidence { .. }
            | DetectedIssue::LowVerificationRatio { .. }
            | DetectedIssue::PartialModuleCoverage { .. } => RecommendedStrategy::EvidenceInjection,

            // All other issues → semantic restructuring
            DetectedIssue::Redundant { .. }
            | DetectedIssue::TooGeneric { .. }
            | DetectedIssue::MissingSections { .. }
            | DetectedIssue::TooShort { .. }
            | DetectedIssue::Shallow { .. }
            | DetectedIssue::LowActionability { .. }
            | DetectedIssue::PlanMismatch
            | DetectedIssue::MissingModule { .. }
            | DetectedIssue::Tier1Content { .. }
            | DetectedIssue::Other { .. } => RecommendedStrategy::Semantic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feedback_guides_strategy() {
        let selector = FeedbackAwareSelector::new();
        let tracker = FailureTracker::new(2);
        let issue = DetectedIssue::Other {
            kind: "test".into(),
            description: "test".into(),
        };

        // Evidence suggestion
        let strategy = selector.select_strategy(
            "skill-a",
            &issue,
            Some("Add more evidence and references"),
            &tracker,
        );
        assert_eq!(strategy, RecommendedStrategy::EvidenceInjection);

        // Remove suggestion → now maps to Semantic
        let strategy = selector.select_strategy(
            "skill-a",
            &issue,
            Some("Remove the generic content"),
            &tracker,
        );
        assert_eq!(strategy, RecommendedStrategy::Semantic);
    }

    #[test]
    fn test_issue_type_guides_strategy() {
        let selector = FeedbackAwareSelector::new();
        let tracker = FailureTracker::new(2);

        let issue = DetectedIssue::WeakEvidence {
            description: "test".into(),
        };
        let strategy = selector.select_strategy("skill-a", &issue, None, &tracker);
        assert_eq!(strategy, RecommendedStrategy::EvidenceInjection);

        let issue = DetectedIssue::Redundant {
            description: "test".into(),
        };
        let strategy = selector.select_strategy("skill-a", &issue, None, &tracker);
        assert_eq!(strategy, RecommendedStrategy::Semantic);
    }

    #[test]
    fn test_failure_history_rotates_strategy() {
        let mut selector = FeedbackAwareSelector::new();
        let tracker = FailureTracker::new(2);
        let issue = DetectedIssue::Other {
            kind: "test".into(),
            description: "test".into(),
        };

        // First attempt
        let strategy1 = selector.select_strategy("skill-a", &issue, None, &tracker);

        // Record failure
        selector.record_failure("skill-a", strategy1.as_str(), None);

        // Second attempt should rotate
        let strategy2 = selector.select_strategy("skill-a", &issue, None, &tracker);
        assert_ne!(strategy1, strategy2);
    }

    #[test]
    fn test_success_clears_history() {
        let mut selector = FeedbackAwareSelector::new();
        let tracker = FailureTracker::new(2);
        let issue = DetectedIssue::WeakEvidence {
            description: "test".into(),
        };

        // Record some failures
        selector.record_failure("skill-a", "evidence", None);
        selector.record_failure("skill-a", "semantic", None);

        // Success should clear
        selector.record_success("skill-a");

        // Should start fresh - WeakEvidence maps to EvidenceInjection
        let strategy = selector.select_strategy("skill-a", &issue, None, &tracker);
        assert_eq!(strategy, RecommendedStrategy::EvidenceInjection);
    }
}
