//! Quality Assessment System
//!
//! Provides clear, explainable quality assessment criteria and reporting.
//! Answers: "WHY did the refinement loop converge?" and "What is the quality status?"

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityAssessment {
    pub achieved: bool,
    pub path: AssessmentPath,
    pub iterations: usize,
    pub quality_trajectory: Vec<f32>,
    pub dimensions_status: DimensionsStatus,
    pub critical_improvements: Vec<Improvement>,
    pub remaining_issues: Vec<RemainingIssue>,
    pub explanation: String,
}

impl QualityAssessment {
    pub fn success(
        path: AssessmentPath,
        iterations: usize,
        trajectory: Vec<f32>,
        dimensions: DimensionsStatus,
        improvements: Vec<Improvement>,
    ) -> Self {
        let explanation = format!(
            "Converged via {} after {} iterations. Final quality: {:.1}%",
            path.as_str(),
            iterations,
            trajectory.last().copied().unwrap_or(0.0) * 100.0
        );

        Self {
            achieved: true,
            path,
            iterations,
            quality_trajectory: trajectory,
            dimensions_status: dimensions,
            critical_improvements: improvements,
            remaining_issues: Vec::new(),
            explanation,
        }
    }

    pub fn failure(
        iterations: usize,
        trajectory: Vec<f32>,
        dimensions: DimensionsStatus,
        remaining: Vec<RemainingIssue>,
    ) -> Self {
        let explanation = format!(
            "Did not converge after {} iterations. Final quality: {:.1}%. {} issues remain.",
            iterations,
            trajectory.last().copied().unwrap_or(0.0) * 100.0,
            remaining.len()
        );

        Self {
            achieved: false,
            path: AssessmentPath::MaxIterations,
            iterations,
            quality_trajectory: trajectory,
            dimensions_status: dimensions,
            critical_improvements: Vec::new(),
            remaining_issues: remaining,
            explanation,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssessmentPath {
    AllDimensionsPassed,
    QualityTargetMet,
    AggregatedFeedback,
    NoIssuesRemaining,
    MaxIterations,
    OscillationSettled,
    /// Early exit triggered (highest quality, bypasses dimensions)
    EarlyExit,
    /// Quality floor met with minimum viable dimensions
    QualityFloorMet,
}

impl AssessmentPath {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AllDimensionsPassed => "all_dimensions_passed",
            Self::QualityTargetMet => "quality_target_met",
            Self::AggregatedFeedback => "aggregated_feedback",
            Self::NoIssuesRemaining => "no_issues_remaining",
            Self::MaxIterations => "max_iterations",
            Self::OscillationSettled => "oscillation_settled",
            Self::EarlyExit => "early_exit",
            Self::QualityFloorMet => "quality_floor_met",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionsStatus {
    pub actionability: DimensionScore,
    pub specificity: DimensionScore,
    pub evidence_quality: DimensionScore,
    pub depth: DimensionScore,
    pub redundancy: DimensionScore,
    pub structural_coverage: Option<DimensionScore>,
    pub cross_artifact: Option<DimensionScore>,
    pub usability: Option<DimensionScore>,
}

impl DimensionsStatus {
    /// Core dimensions that are always required (actionability, evidence_quality)
    pub fn core_passed(&self) -> bool {
        self.actionability.passed && self.evidence_quality.passed
    }

    /// Minimum viable: actionability OR evidence_quality passes (relaxed requirement)
    pub fn minimum_viable(&self) -> bool {
        self.actionability.passed || self.evidence_quality.passed
    }

    /// Most relaxed: ANY dimension passes
    /// Used for quality floor convergence when strict dimensions are too hard to meet
    pub fn any_passed(&self) -> bool {
        self.actionability.passed
            || self.specificity.passed
            || self.evidence_quality.passed
            || self.depth.passed
            || self.redundancy.passed
            || self
                .cross_artifact
                .as_ref()
                .map(|d| d.passed)
                .unwrap_or(false)
            || self.usability.as_ref().map(|d| d.passed).unwrap_or(false)
    }

    /// Count how many dimensions passed
    pub fn passed_count(&self) -> usize {
        let mut count = 0;
        if self.actionability.passed {
            count += 1;
        }
        if self.specificity.passed {
            count += 1;
        }
        if self.evidence_quality.passed {
            count += 1;
        }
        if self.depth.passed {
            count += 1;
        }
        if self.redundancy.passed {
            count += 1;
        }
        if self
            .cross_artifact
            .as_ref()
            .map(|d| d.passed)
            .unwrap_or(false)
        {
            count += 1;
        }
        if self.usability.as_ref().map(|d| d.passed).unwrap_or(false) {
            count += 1;
        }
        count
    }

    /// All dimensions passed (strict mode)
    /// Optional dimensions that weren't run are treated as NOT passed
    pub fn all_passed(&self, strict: bool) -> bool {
        let core = self.actionability.passed
            && self.specificity.passed
            && self.evidence_quality.passed
            && self.depth.passed
            && self.redundancy.passed;

        if !core {
            return false;
        }

        if strict {
            self.structural_coverage
                .as_ref()
                .map(|d| d.passed)
                .unwrap_or(false)
                && self
                    .cross_artifact
                    .as_ref()
                    .map(|d| d.passed)
                    .unwrap_or(false)
                && self.usability.as_ref().map(|d| d.passed).unwrap_or(false)
        } else {
            self.structural_coverage
                .as_ref()
                .map(|d| d.passed)
                .unwrap_or(true)
                && self
                    .cross_artifact
                    .as_ref()
                    .map(|d| d.passed)
                    .unwrap_or(true)
                && self.usability.as_ref().map(|d| d.passed).unwrap_or(true)
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "A:{} S:{} E:{} D:{} R:{}",
            if self.actionability.passed {
                "✓"
            } else {
                "✗"
            },
            if self.specificity.passed {
                "✓"
            } else {
                "✗"
            },
            if self.evidence_quality.passed {
                "✓"
            } else {
                "✗"
            },
            if self.depth.passed { "✓" } else { "✗" },
            if self.redundancy.passed { "✓" } else { "✗" },
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionScore {
    pub name: String,
    pub score: f32,
    pub threshold: f32,
    pub passed: bool,
}

impl DimensionScore {
    pub fn new(name: &str, score: f32, threshold: f32) -> Self {
        Self {
            name: name.to_string(),
            score,
            threshold,
            passed: score >= threshold,
        }
    }

    pub fn new_inverted(name: &str, score: f32, max_threshold: f32) -> Self {
        Self {
            name: name.to_string(),
            score,
            threshold: max_threshold,
            passed: score <= max_threshold,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Improvement {
    pub iteration: usize,
    pub target: String,
    pub strategy: String,
    pub quality_delta: f32,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemainingIssue {
    pub target: String,
    pub category: String,
    pub severity: String,
    pub description: String,
    pub attempts: usize,
}

#[derive(Debug, Clone)]
pub struct QualityAssessor {
    target_quality: f32,
    quality_floor: f32,
    early_exit_threshold: f32,
    early_exit_bypasses_dimensions: bool,
    require_all_dimensions: bool,
}

impl QualityAssessor {
    pub fn new(target_quality: f32, require_all_dimensions: bool) -> Self {
        Self {
            target_quality,
            quality_floor: 0.75,
            early_exit_threshold: 0.90,
            early_exit_bypasses_dimensions: false,
            require_all_dimensions,
        }
    }

    pub fn with_quality_floor(mut self, floor: f32) -> Self {
        self.quality_floor = floor;
        self
    }

    pub fn with_early_exit(mut self, threshold: f32, bypasses_dimensions: bool) -> Self {
        self.early_exit_threshold = threshold;
        self.early_exit_bypasses_dimensions = bypasses_dimensions;
        self
    }

    pub fn check(
        &self,
        combined_quality: f32,
        dimensions: &DimensionsStatus,
        aggregated_converged: bool,
        issues_remaining: usize,
    ) -> Option<AssessmentPath> {
        // PATH 1: Early exit - UNCONDITIONAL (highest priority)
        if self.early_exit_bypasses_dimensions && combined_quality >= self.early_exit_threshold {
            return Some(AssessmentPath::EarlyExit);
        }

        // PATH 2: Quality floor with minimum viable dimensions
        if combined_quality >= self.quality_floor && dimensions.minimum_viable() {
            if combined_quality >= self.target_quality {
                return Some(AssessmentPath::QualityTargetMet);
            }
            return Some(AssessmentPath::QualityFloorMet);
        }

        // PATH 2.5: Relaxed quality floor - ANY dimension passes with high quality
        // Use quality_floor for consistency, require 3+ dimensions for safety
        if combined_quality >= self.quality_floor
            && dimensions.any_passed()
            && dimensions.passed_count() >= 3
        {
            return Some(AssessmentPath::QualityFloorMet);
        }

        // PATH 3: Full quality with core dimensions (original behavior)
        if dimensions.core_passed() {
            if issues_remaining == 0 && combined_quality >= self.target_quality {
                return Some(AssessmentPath::NoIssuesRemaining);
            }

            if self.require_all_dimensions {
                if combined_quality >= self.target_quality && dimensions.all_passed(true) {
                    return Some(AssessmentPath::AllDimensionsPassed);
                }
            } else {
                if combined_quality >= self.target_quality {
                    return Some(AssessmentPath::QualityTargetMet);
                }
                if aggregated_converged && combined_quality >= self.quality_floor {
                    return Some(AssessmentPath::AggregatedFeedback);
                }
            }
        }

        None
    }

    pub fn check_with_thinking(
        &self,
        combined_quality: f32,
        dimensions: &DimensionsStatus,
        uncertainty: f32,
        iteration: usize,
        estimated_total: usize,
        is_improving: bool,
    ) -> TerminationDecision {
        // 1. Early exit: very high quality with low uncertainty
        if self.early_exit_bypasses_dimensions
            && combined_quality >= self.early_exit_threshold
            && uncertainty < 0.15
        {
            return TerminationDecision::Terminate(TerminationReason::EarlyExit {
                quality: combined_quality,
                uncertainty,
            });
        }

        // 1.5. Stagnation convergence: quality stable at acceptable level
        // Require minimum_viable or 3+ dimensions to prevent premature convergence
        if !is_improving
            && combined_quality >= self.quality_floor
            && (dimensions.minimum_viable()
                || (dimensions.any_passed() && dimensions.passed_count() >= 3))
        {
            if combined_quality >= self.target_quality {
                return TerminationDecision::Terminate(TerminationReason::Converged(
                    AssessmentPath::QualityTargetMet,
                ));
            }
            return TerminationDecision::Terminate(TerminationReason::Converged(
                AssessmentPath::QualityFloorMet,
            ));
        }

        // 2. Continue if high uncertainty (even with good quality) - but only if quality is still improving
        if uncertainty > 0.3 && iteration < estimated_total && is_improving {
            return TerminationDecision::Continue(ContinueReason::HighUncertainty {
                uncertainty,
                threshold: 0.3,
            });
        }

        // 3. Quality floor + minimum viable + low uncertainty
        if combined_quality >= self.quality_floor
            && dimensions.minimum_viable()
            && uncertainty <= 0.25
        {
            if combined_quality >= self.target_quality {
                return TerminationDecision::Terminate(TerminationReason::Converged(
                    AssessmentPath::QualityTargetMet,
                ));
            }
            return TerminationDecision::Terminate(TerminationReason::Converged(
                AssessmentPath::QualityFloorMet,
            ));
        }

        // 4. Continue if quality is improving
        if is_improving {
            return TerminationDecision::Continue(ContinueReason::Improving);
        }

        // 5. Fallback to basic check logic
        if let Some(path) = self.check(combined_quality, dimensions, false, 0) {
            return TerminationDecision::Terminate(TerminationReason::Converged(path));
        }

        TerminationDecision::Continue(ContinueReason::NotConverged)
    }
}

#[derive(Debug, Clone)]
pub enum TerminationDecision {
    Terminate(TerminationReason),
    Continue(ContinueReason),
}

impl TerminationDecision {
    pub fn is_terminate(&self) -> bool {
        matches!(self, Self::Terminate(_))
    }

    pub fn is_continue(&self) -> bool {
        matches!(self, Self::Continue(_))
    }
}

#[derive(Debug, Clone)]
pub enum TerminationReason {
    Satisfied,
    EarlyExit { quality: f32, uncertainty: f32 },
    Converged(AssessmentPath),
    MaxIterations,
}

impl TerminationReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Satisfied => "satisfied",
            Self::EarlyExit { .. } => "early_exit",
            Self::Converged(path) => path.as_str(),
            Self::MaxIterations => "max_iterations",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ContinueReason {
    HighUncertainty { uncertainty: f32, threshold: f32 },
    Improving,
    NotConverged,
    NeedsMoreThinking,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dimensions(core_pass: bool, all_pass: bool) -> DimensionsStatus {
        let core_score = if core_pass { 0.8 } else { 0.4 };
        let other_score = if all_pass { 0.8 } else { 0.4 };
        DimensionsStatus {
            actionability: DimensionScore::new("actionability", core_score, 0.6),
            specificity: DimensionScore::new("specificity", other_score, 0.5),
            evidence_quality: DimensionScore::new("evidence_quality", core_score, 0.7),
            depth: DimensionScore::new("depth", other_score, 0.5),
            redundancy: DimensionScore::new_inverted("redundancy", 0.2, 0.3),
            structural_coverage: Some(DimensionScore::new("structural", other_score, 0.6)),
            cross_artifact: Some(DimensionScore::new("cross_artifact", other_score, 0.6)),
            usability: Some(DimensionScore::new("usability", other_score, 0.6)),
        }
    }

    #[test]
    fn test_early_exit_convergence() {
        // Explicit early_exit_bypasses_dimensions=true to test early exit path
        let checker = QualityAssessor::new(0.85, true).with_early_exit(0.90, true);
        let dims = make_dimensions(true, true);
        let result = checker.check(0.90, &dims, false, 1);
        assert_eq!(result, Some(AssessmentPath::EarlyExit));
    }

    #[test]
    fn test_all_dimensions_convergence() {
        let checker = QualityAssessor::new(0.85, true).with_early_exit(1.0, false);
        let dims = make_dimensions(true, true);
        let result = checker.check(0.88, &dims, false, 1);
        assert_eq!(result, Some(AssessmentPath::QualityTargetMet));
    }

    #[test]
    fn test_quality_target_convergence() {
        let checker = QualityAssessor::new(0.85, false).with_early_exit(1.0, false);
        let dims = make_dimensions(true, false);
        let result = checker.check(0.88, &dims, false, 1);
        assert_eq!(result, Some(AssessmentPath::QualityTargetMet));
    }

    #[test]
    fn test_quality_floor_convergence() {
        let checker = QualityAssessor::new(0.85, false)
            .with_quality_floor(0.65)
            .with_early_exit(1.0, false);
        let dims = make_dimensions(true, false);
        let result = checker.check(0.70, &dims, false, 1);
        assert_eq!(result, Some(AssessmentPath::QualityFloorMet));
    }

    #[test]
    fn test_no_issues_convergence_requires_quality() {
        let checker = QualityAssessor::new(0.85, true)
            .with_quality_floor(0.65)
            .with_early_exit(1.0, false);
        let dims = make_dimensions(true, false);
        let result = checker.check(0.60, &dims, false, 0);
        assert_eq!(result, None);
        let result = checker.check(0.85, &dims, false, 0);
        assert_eq!(result, Some(AssessmentPath::QualityTargetMet));
    }

    #[test]
    fn test_minimum_viable_path() {
        let checker = QualityAssessor::new(0.85, false)
            .with_quality_floor(0.65)
            .with_early_exit(1.0, false);
        let mut dims = make_dimensions(true, false);
        dims.evidence_quality.passed = false;
        let result = checker.check(0.70, &dims, false, 1);
        assert_eq!(result, Some(AssessmentPath::QualityFloorMet));
    }

    #[test]
    fn test_aggregated_feedback_convergence() {
        let checker = QualityAssessor::new(0.85, false)
            .with_quality_floor(0.65)
            .with_early_exit(1.0, false);
        let dims = make_dimensions(true, false);
        let result = checker.check(0.70, &dims, true, 1);
        assert_eq!(result, Some(AssessmentPath::QualityFloorMet));
    }
}
