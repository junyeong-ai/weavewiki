//! Feedback Aggregator Module
//!
//! Aggregates feedback from validation sources into unified feedback.
//! Uses real metrics only: LlmJudge, Structural, and Evidence validation.

use std::collections::HashMap;

/// Maximum number of suggestions to return (0 = unlimited).
///
/// Rationale: More than 10 suggestions overwhelms users and dilutes focus.
/// LLM should prioritize most impactful issues; this truncation ensures
/// output remains actionable rather than exhaustive.
const MAX_SUGGESTIONS: usize = 10;

use serde::{Deserialize, Serialize};

use super::analysis::architectural_analyzer::StructuralValidationResult;
use super::quality::{JudgmentResult, QualityIssue};
use super::validation::CrossValidationResult;
use crate::types::Severity;

#[derive(Debug, Clone)]
pub struct AggregatedFeedback {
    pub converged: bool,
    pub overall_score: f32,
    pub dimension_scores: DimensionScores,
    pub prioritized_issues: Vec<PrioritizedIssue>,
    pub suggestions: Vec<String>,
    pub analysis_feedback: AnalysisFeedback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionScores {
    pub quality: f32,
    pub structural: f32,
    pub evidence: f32,
}

impl DimensionScores {
    pub fn all_pass(&self, threshold: f32) -> bool {
        self.quality >= threshold && self.structural >= threshold && self.evidence >= threshold
    }
}

#[derive(Debug, Clone)]
pub struct PrioritizedIssue {
    pub priority: Severity,
    pub source: ValidationSource,
    pub artifact: String,
    pub description: String,
    pub suggestion: String,
    pub impact_score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSource {
    Quality,
    Structural,
    Evidence,
}

#[derive(Debug, Clone)]
pub struct AnalysisFeedback {
    pub missing_modules: Vec<String>,
    pub weak_coverage_areas: Vec<String>,
    pub suggested_skills: Vec<String>,
    pub suggested_agents: Vec<String>,
    pub refinement_hints: HashMap<String, Vec<String>>,
}

pub struct FeedbackAggregator {
    target_quality: f32,
    dimension_pass_threshold: f32,
    quality_weight: f32,
    structural_weight: f32,
    evidence_weight: f32,
}

impl Default for FeedbackAggregator {
    /// Default feedback aggregation settings.
    ///
    /// Threshold rationale:
    /// - `target_quality: 0.85` (85%) - High bar for production readiness; below this
    ///   indicates significant gaps. Based on manual review of generated artifacts.
    /// - `dimension_pass_threshold: 0.6` (60%) - Minimum for any dimension. Below 60%
    ///   means critical failures in that area (e.g., no evidence, no structure).
    ///
    /// Weight rationale (40/30/30):
    /// - Quality (40%): Semantic correctness is most important - useless if inaccurate.
    /// - Structural (30%): Module coverage ensures completeness.
    /// - Evidence (30%): File references ground claims in reality.
    fn default() -> Self {
        Self {
            target_quality: 0.85,
            dimension_pass_threshold: 0.6,
            quality_weight: 0.40,
            structural_weight: 0.30,
            evidence_weight: 0.30,
        }
    }
}

impl FeedbackAggregator {
    pub fn new(target_quality: f32) -> Self {
        Self {
            target_quality,
            ..Default::default()
        }
    }

    pub fn with_weights(mut self, quality: f32, structural: f32, evidence: f32) -> Self {
        let total = quality + structural + evidence;
        self.quality_weight = quality / total;
        self.structural_weight = structural / total;
        self.evidence_weight = evidence / total;
        self
    }

    pub fn with_dimension_threshold(mut self, threshold: f32) -> Self {
        self.dimension_pass_threshold = threshold;
        self
    }

    pub fn aggregate(
        &self,
        judgment: &JudgmentResult,
        structural: Option<&StructuralValidationResult>,
        cross_validation: Option<&CrossValidationResult>,
    ) -> AggregatedFeedback {
        let dimension_scores =
            self.calculate_dimension_scores(judgment, structural, cross_validation);
        let overall_score = self.calculate_overall_score(&dimension_scores);
        let converged = overall_score >= self.target_quality
            && dimension_scores.all_pass(self.dimension_pass_threshold);

        let mut prioritized_issues = self.collect_issues(judgment, structural);
        prioritized_issues.sort_by(|a, b| b.priority.cmp(&a.priority));

        let suggestions = self.generate_suggestions(&prioritized_issues, &dimension_scores);
        let analysis_feedback = self.generate_analysis_feedback(structural);

        AggregatedFeedback {
            converged,
            overall_score,
            dimension_scores,
            prioritized_issues,
            suggestions,
            analysis_feedback,
        }
    }

    fn calculate_dimension_scores(
        &self,
        judgment: &JudgmentResult,
        structural: Option<&StructuralValidationResult>,
        cross_validation: Option<&CrossValidationResult>,
    ) -> DimensionScores {
        DimensionScores {
            quality: judgment.overall_score,
            structural: structural
                .map(|s| s.coverage_report.coverage)
                .unwrap_or(1.0),
            evidence: cross_validation
                .map(|c| c.evidence_traceability.coverage_score)
                .unwrap_or(1.0),
        }
    }

    fn calculate_overall_score(&self, scores: &DimensionScores) -> f32 {
        (scores.quality * self.quality_weight
            + scores.structural * self.structural_weight
            + scores.evidence * self.evidence_weight)
            .clamp(0.0, 1.0)
    }

    fn collect_issues(
        &self,
        judgment: &JudgmentResult,
        structural: Option<&StructuralValidationResult>,
    ) -> Vec<PrioritizedIssue> {
        let mut issues = Vec::new();

        for issue in &judgment.issues {
            issues.push(PrioritizedIssue {
                priority: severity_from_issue(issue),
                source: ValidationSource::Quality,
                artifact: issue.code.clone(),
                description: issue.message.clone(),
                suggestion: issue.evidence.first().cloned().unwrap_or_default(),
                impact_score: self.calculate_impact(severity_from_issue(issue)),
            });
        }

        if let Some(structural) = structural {
            for missing in &structural.coverage_report.missing_modules {
                issues.push(PrioritizedIssue {
                    priority: Severity::High,
                    source: ValidationSource::Structural,
                    artifact: format!("module:{}", missing.name),
                    description: format!("Module '{}' is not documented", missing.name),
                    suggestion: format!("Add documentation covering @{}", missing.path),
                    impact_score: 0.8,
                });
            }
        }

        issues
    }

    /// Calculate impact score for issue prioritization.
    ///
    /// Scores reflect relative urgency for refinement:
    /// - Critical (1.0): Blocks release; must fix immediately
    /// - High (0.8): Significant issue; high priority
    /// - Medium (0.5): Notable issue; address in iteration
    /// - Low (0.2): Minor; can defer if time-constrained
    ///
    /// These weights affect issue sorting, not pass/fail decisions.
    fn calculate_impact(&self, priority: Severity) -> f32 {
        match priority {
            Severity::Critical => 1.0,
            Severity::High => 0.8,
            Severity::Medium => 0.5,
            Severity::Low => 0.2,
        }
    }

    fn generate_suggestions(
        &self,
        issues: &[PrioritizedIssue],
        scores: &DimensionScores,
    ) -> Vec<String> {
        let mut suggestions = Vec::new();

        if scores.quality < 0.7 {
            suggestions.push(
                "Improve content quality: add actionable directives and specific references".into(),
            );
        }

        if scores.structural < 0.7 {
            suggestions.push("Increase module coverage: document missing core modules".into());
        }

        if scores.evidence < 0.7 {
            suggestions.push(
                "Strengthen evidence: add @file:line references to validate all claims".into(),
            );
        }

        let critical_count = issues
            .iter()
            .filter(|i| i.priority == Severity::Critical)
            .count();
        if critical_count > 0 {
            suggestions.insert(
                0,
                format!("Address {} critical issues first", critical_count),
            );
        }

        if MAX_SUGGESTIONS > 0 {
            suggestions.truncate(MAX_SUGGESTIONS);
        }
        suggestions
    }

    fn generate_analysis_feedback(
        &self,
        structural: Option<&StructuralValidationResult>,
    ) -> AnalysisFeedback {
        let mut feedback = AnalysisFeedback {
            missing_modules: Vec::new(),
            weak_coverage_areas: Vec::new(),
            suggested_skills: Vec::new(),
            suggested_agents: Vec::new(),
            refinement_hints: HashMap::new(),
        };

        if let Some(structural) = structural {
            feedback.missing_modules = structural
                .coverage_report
                .missing_modules
                .iter()
                .map(|m| m.name.clone())
                .collect();
        }

        feedback
    }
}

fn severity_from_issue(issue: &QualityIssue) -> Severity {
    match issue.severity {
        super::quality::IssueSeverity::Critical => Severity::Critical,
        super::quality::IssueSeverity::Major => Severity::High,
        super::quality::IssueSeverity::Minor => Severity::Medium,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContentTier;

    #[test]
    fn test_feedback_aggregator_creation() {
        let aggregator = FeedbackAggregator::new(0.85);
        assert_eq!(aggregator.target_quality, 0.85);
    }

    #[test]
    fn test_dimension_scores_all_pass() {
        let scores = DimensionScores {
            quality: 0.8,
            structural: 0.8,
            evidence: 0.8,
        };
        assert!(scores.all_pass(0.7));
        assert!(!scores.all_pass(0.85));
    }

    #[test]
    fn test_aggregate_basic() {
        let aggregator = FeedbackAggregator::new(0.80);
        let judgment = JudgmentResult {
            overall_score: 0.85,
            tier: ContentTier::Tier3Constraint,
            issues: Vec::new(),
            suggestions: Vec::new(),
        };

        let feedback = aggregator.aggregate(&judgment, None, None);
        assert!(feedback.overall_score > 0.8);
        assert!(feedback.dimension_scores.quality == 0.85);
    }
}
