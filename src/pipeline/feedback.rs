//! Feedback Aggregator Module
//!
//! Aggregates feedback from multiple validation sources into unified feedback.
//! Enables bidirectional communication between analysis and generation phases.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::validation::{
    cross_artifact::{CrossArtifactResult, Severity as CASeverity},
    cross_validation::CrossValidationResult,
    semantic_validator::SemanticQualityResult,
    usability::{UsabilityResult, Severity as USeverity},
};
use super::analysis::architectural_analyzer::StructuralValidationResult;

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
    pub semantic: f32,
    pub structural: f32,
    pub cross_artifact: f32,
    pub usability: f32,
    pub evidence: f32,
}

impl DimensionScores {
    pub fn all_pass(&self, threshold: f32) -> bool {
        self.semantic >= threshold
            && self.structural >= threshold
            && self.cross_artifact >= threshold
            && self.usability >= threshold
            && self.evidence >= threshold
    }
}

#[derive(Debug, Clone)]
pub struct PrioritizedIssue {
    pub priority: IssuePriority,
    pub source: ValidationSource,
    pub artifact: String,
    pub description: String,
    pub suggestion: String,
    pub impact_score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IssuePriority {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSource {
    Semantic,
    Structural,
    CrossArtifact,
    Usability,
    CrossValidation,
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
    semantic_weight: f32,
    structural_weight: f32,
    cross_artifact_weight: f32,
    usability_weight: f32,
    evidence_weight: f32,
}

impl Default for FeedbackAggregator {
    fn default() -> Self {
        Self {
            target_quality: 0.85,
            dimension_pass_threshold: 0.6,
            // Weights include evidence for complete 5-dimension quality
            semantic_weight: 0.25,
            structural_weight: 0.20,
            cross_artifact_weight: 0.15,
            usability_weight: 0.20,
            evidence_weight: 0.20,
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

    pub fn from_config(config: &crate::config::FeedbackConfig, target_quality: f32) -> Self {
        Self {
            target_quality,
            dimension_pass_threshold: config.dimension_pass_threshold,
            semantic_weight: config.semantic_weight,
            structural_weight: config.structural_weight,
            cross_artifact_weight: config.cross_artifact_weight,
            usability_weight: config.usability_weight,
            evidence_weight: config.evidence_weight,
        }
    }

    pub fn with_weights(
        mut self,
        semantic: f32,
        structural: f32,
        cross_artifact: f32,
        usability: f32,
        evidence: f32,
    ) -> Self {
        let total = semantic + structural + cross_artifact + usability + evidence;
        self.semantic_weight = semantic / total;
        self.structural_weight = structural / total;
        self.cross_artifact_weight = cross_artifact / total;
        self.usability_weight = usability / total;
        self.evidence_weight = evidence / total;
        self
    }

    pub fn with_dimension_threshold(mut self, threshold: f32) -> Self {
        self.dimension_pass_threshold = threshold;
        self
    }

    pub fn aggregate(
        &self,
        semantic: &SemanticQualityResult,
        structural: Option<&StructuralValidationResult>,
        cross_artifact: Option<&CrossArtifactResult>,
        usability: Option<&UsabilityResult>,
        cross_validation: Option<&CrossValidationResult>,
    ) -> AggregatedFeedback {
        let dimension_scores = self.calculate_dimension_scores(
            semantic,
            structural,
            cross_artifact,
            usability,
            cross_validation,
        );

        let overall_score = self.calculate_overall_score(&dimension_scores);
        let converged = overall_score >= self.target_quality
            && dimension_scores.all_pass(self.dimension_pass_threshold);

        let mut prioritized_issues = self.collect_and_prioritize_issues(
            semantic,
            structural,
            cross_artifact,
            usability,
        );
        prioritized_issues.sort_by(|a, b| a.priority.cmp(&b.priority));

        let suggestions = self.generate_suggestions(&prioritized_issues, &dimension_scores);
        let analysis_feedback = self.generate_analysis_feedback(
            structural,
            cross_artifact,
            usability,
        );

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
        semantic: &SemanticQualityResult,
        structural: Option<&StructuralValidationResult>,
        cross_artifact: Option<&CrossArtifactResult>,
        usability: Option<&UsabilityResult>,
        cross_validation: Option<&CrossValidationResult>,
    ) -> DimensionScores {
        DimensionScores {
            semantic: semantic.overall_score,
            structural: structural.map(|s| s.coverage_report.coverage).unwrap_or(1.0),
            cross_artifact: cross_artifact.map(|c| c.score).unwrap_or(1.0),
            usability: usability.map(|u| u.score).unwrap_or(1.0),
            evidence: cross_validation.map(|c| c.evidence_traceability.coverage_score).unwrap_or(1.0),
        }
    }

    fn calculate_overall_score(&self, scores: &DimensionScores) -> f32 {
        // CRITICAL FIX: Now includes evidence dimension in quality calculation
        // All 5 dimensions contribute to overall quality score
        (scores.semantic * self.semantic_weight
            + scores.structural * self.structural_weight
            + scores.cross_artifact * self.cross_artifact_weight
            + scores.usability * self.usability_weight
            + scores.evidence * self.evidence_weight)
            .clamp(0.0, 1.0)
    }

    fn collect_and_prioritize_issues(
        &self,
        semantic: &SemanticQualityResult,
        structural: Option<&StructuralValidationResult>,
        cross_artifact: Option<&CrossArtifactResult>,
        usability: Option<&UsabilityResult>,
    ) -> Vec<PrioritizedIssue> {
        let mut issues = Vec::new();

        for issue in &semantic.issues {
            let priority = match issue.severity {
                super::validation::semantic_validator::IssueSeverity::Critical => IssuePriority::Critical,
                super::validation::semantic_validator::IssueSeverity::High => IssuePriority::High,
                super::validation::semantic_validator::IssueSeverity::Medium => IssuePriority::Medium,
                super::validation::semantic_validator::IssueSeverity::Low => IssuePriority::Low,
            };

            issues.push(PrioritizedIssue {
                priority,
                source: ValidationSource::Semantic,
                artifact: issue.target.clone(),
                description: issue.description.clone(),
                suggestion: self.get_semantic_suggestion(&issue.category),
                impact_score: self.calculate_impact(priority),
            });
        }

        if let Some(structural) = structural {
            for missing in &structural.coverage_report.missing_modules {
                issues.push(PrioritizedIssue {
                    priority: IssuePriority::High,
                    source: ValidationSource::Structural,
                    artifact: format!("module:{}", missing.module.name),
                    description: format!("Module '{}' is not documented", missing.module.name),
                    suggestion: format!("Add documentation covering @{}", missing.module.key_files.first().unwrap_or(&"src/".into())),
                    impact_score: 0.8,
                });
            }
        }

        if let Some(cross_artifact) = cross_artifact {
            for issue in &cross_artifact.issues {
                let priority = match issue.severity {
                    CASeverity::Critical => IssuePriority::Critical,
                    CASeverity::High => IssuePriority::High,
                    CASeverity::Medium => IssuePriority::Medium,
                    CASeverity::Low => IssuePriority::Low,
                };

                issues.push(PrioritizedIssue {
                    priority,
                    source: ValidationSource::CrossArtifact,
                    artifact: issue.affected_artifacts.join(", "),
                    description: issue.description.clone(),
                    suggestion: issue.suggestion.clone(),
                    impact_score: self.calculate_impact(priority),
                });
            }
        }

        if let Some(usability) = usability {
            for issue in &usability.issues {
                let priority = match issue.severity {
                    USeverity::Error => IssuePriority::High,
                    USeverity::Warning => IssuePriority::Medium,
                    USeverity::Info => IssuePriority::Low,
                };

                issues.push(PrioritizedIssue {
                    priority,
                    source: ValidationSource::Usability,
                    artifact: issue.artifact.clone(),
                    description: issue.description.clone(),
                    suggestion: issue.fix_suggestion.clone(),
                    impact_score: self.calculate_impact(priority),
                });
            }
        }

        issues
    }

    fn get_semantic_suggestion(&self, category: &super::validation::semantic_validator::IssueCategory) -> String {
        use super::validation::semantic_validator::IssueCategory;
        match category {
            IssueCategory::LowActionability => "Add directive language (must, should, avoid) with specific actions".into(),
            IssueCategory::TooGeneric => "Replace with project-specific details and @file:line references".into(),
            IssueCategory::WeakEvidence => "Add @file:line references pointing to actual code".into(),
            IssueCategory::Redundant => "Remove duplicated content or consolidate".into(),
            IssueCategory::Shallow => "Add rationale, examples, and code references".into(),
            IssueCategory::MissingReference => "Add at least 3 @file:line references".into(),
        }
    }

    fn calculate_impact(&self, priority: IssuePriority) -> f32 {
        match priority {
            IssuePriority::Critical => 1.0,
            IssuePriority::High => 0.8,
            IssuePriority::Medium => 0.5,
            IssuePriority::Low => 0.2,
        }
    }

    fn generate_suggestions(&self, issues: &[PrioritizedIssue], scores: &DimensionScores) -> Vec<String> {
        let mut suggestions = Vec::new();

        if scores.semantic < 0.7 {
            suggestions.push("Improve content quality: add actionable directives and specific references".into());
        }

        if scores.structural < 0.7 {
            suggestions.push("Increase module coverage: document missing core modules".into());
        }

        if scores.cross_artifact < 0.7 {
            suggestions.push("Improve artifact coherence: reduce overlaps and ensure consistent references".into());
        }

        if scores.usability < 0.7 {
            suggestions.push("Improve usability: ensure progressive disclosure from CLAUDE.md to rules/skills".into());
        }

        if scores.evidence < 0.7 {
            suggestions.push("Strengthen evidence: add @file:line references to validate all claims".into());
        }

        let critical_count = issues.iter().filter(|i| i.priority == IssuePriority::Critical).count();
        if critical_count > 0 {
            suggestions.insert(0, format!("Address {} critical issues first", critical_count));
        }

        suggestions.truncate(5);
        suggestions
    }

    fn generate_analysis_feedback(
        &self,
        structural: Option<&StructuralValidationResult>,
        cross_artifact: Option<&CrossArtifactResult>,
        usability: Option<&UsabilityResult>,
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
                .map(|m| m.module.name.clone())
                .collect();
        }

        if let Some(cross_artifact) = cross_artifact {
            feedback.weak_coverage_areas = cross_artifact
                .coverage_balance
                .uncovered_modules
                .clone();
        }

        if let Some(usability) = usability {
            feedback.suggested_skills = usability
                .task_relevance
                .missing_common_tasks
                .iter()
                .map(|t| format!("{}-workflow", t.replace(' ', "-")))
                .collect();
        }

        feedback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feedback_aggregator_creation() {
        let aggregator = FeedbackAggregator::new(0.85);
        assert_eq!(aggregator.target_quality, 0.85);
    }

    #[test]
    fn test_dimension_scores_all_pass() {
        let scores = DimensionScores {
            semantic: 0.8,
            structural: 0.8,
            cross_artifact: 0.8,
            usability: 0.8,
            evidence: 0.8,
        };
        assert!(scores.all_pass(0.7));
        assert!(!scores.all_pass(0.85));
    }

    #[test]
    fn test_issue_priority_ordering() {
        assert!(IssuePriority::Critical < IssuePriority::High);
        assert!(IssuePriority::High < IssuePriority::Medium);
        assert!(IssuePriority::Medium < IssuePriority::Low);
    }
}
