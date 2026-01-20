//! Convergence Reporting System
//!
//! Provides clear, explainable convergence criteria and reporting.
//! Answers: "WHY did the refinement loop converge?"

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvergenceReport {
    pub achieved: bool,
    pub path: ConvergencePath,
    pub iterations: usize,
    pub quality_trajectory: Vec<f32>,
    pub dimensions_status: DimensionsStatus,
    pub critical_improvements: Vec<Improvement>,
    pub remaining_issues: Vec<RemainingIssue>,
    pub explanation: String,
}

impl ConvergenceReport {
    pub fn success(
        path: ConvergencePath,
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
            path: ConvergencePath::MaxIterations,
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
pub enum ConvergencePath {
    AllDimensionsPassed,
    QualityTargetMet,
    AggregatedFeedback,
    NoIssuesRemaining,
    MaxIterations,
    /// Quality oscillated around threshold but stabilized
    OscillationSettled,
    /// Tier3 content generation plateaued with acceptable quality
    ValuePlateau,
}

impl ConvergencePath {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AllDimensionsPassed => "all_dimensions_passed",
            Self::QualityTargetMet => "quality_target_met",
            Self::AggregatedFeedback => "aggregated_feedback",
            Self::NoIssuesRemaining => "no_issues_remaining",
            Self::MaxIterations => "max_iterations",
            Self::OscillationSettled => "oscillation_settled",
            Self::ValuePlateau => "value_plateau",
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
            // Strict mode: optional dimensions must be present AND pass
            self.structural_coverage.as_ref().map(|d| d.passed).unwrap_or(false)
                && self.cross_artifact.as_ref().map(|d| d.passed).unwrap_or(false)
                && self.usability.as_ref().map(|d| d.passed).unwrap_or(false)
        } else {
            // Non-strict: optional dimensions pass if present OR not run
            self.structural_coverage.as_ref().map(|d| d.passed).unwrap_or(true)
                && self.cross_artifact.as_ref().map(|d| d.passed).unwrap_or(true)
                && self.usability.as_ref().map(|d| d.passed).unwrap_or(true)
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "A:{} S:{} E:{} D:{} R:{}",
            if self.actionability.passed { "✓" } else { "✗" },
            if self.specificity.passed { "✓" } else { "✗" },
            if self.evidence_quality.passed { "✓" } else { "✗" },
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

pub struct ConvergenceChecker {
    target_quality: f32,
    require_all_dimensions: bool,
}

impl ConvergenceChecker {
    pub fn new(target_quality: f32, require_all_dimensions: bool) -> Self {
        Self {
            target_quality,
            require_all_dimensions,
        }
    }

    pub fn check(
        &self,
        combined_quality: f32,
        dimensions: &DimensionsStatus,
        aggregated_converged: bool,
        issues_remaining: usize,
    ) -> Option<ConvergencePath> {
        // Core dimensions (actionability, evidence) are ALWAYS required
        if !dimensions.core_passed() {
            return None;
        }

        // NoIssuesRemaining requires meeting quality target
        if issues_remaining == 0 && combined_quality >= self.target_quality {
            return Some(ConvergencePath::NoIssuesRemaining);
        }

        if self.require_all_dimensions {
            // Strict mode: all dimensions must pass
            if combined_quality >= self.target_quality && dimensions.all_passed(true) {
                return Some(ConvergencePath::AllDimensionsPassed);
            }
        } else {
            // Quality target met with core dimensions
            if combined_quality >= self.target_quality {
                return Some(ConvergencePath::QualityTargetMet);
            }
            // AggregatedFeedback now requires quality floor
            if aggregated_converged && combined_quality >= self.target_quality {
                return Some(ConvergencePath::AggregatedFeedback);
            }
        }

        None
    }
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
    fn test_all_dimensions_convergence() {
        let checker = ConvergenceChecker::new(0.85, true);
        let dims = make_dimensions(true, true);
        let result = checker.check(0.90, &dims, false, 1);
        assert_eq!(result, Some(ConvergencePath::AllDimensionsPassed));
    }

    #[test]
    fn test_quality_target_convergence() {
        let checker = ConvergenceChecker::new(0.85, false);
        let dims = make_dimensions(true, false);
        let result = checker.check(0.90, &dims, false, 1);
        assert_eq!(result, Some(ConvergencePath::QualityTargetMet));
    }

    #[test]
    fn test_no_issues_convergence_requires_quality() {
        let checker = ConvergenceChecker::new(0.85, true);
        let dims = make_dimensions(true, false);
        // Below quality target: should NOT converge even with 0 issues
        let result = checker.check(0.60, &dims, false, 0);
        assert_eq!(result, None);
        // At quality target: should converge
        let result = checker.check(0.85, &dims, false, 0);
        assert_eq!(result, Some(ConvergencePath::NoIssuesRemaining));
    }

    #[test]
    fn test_core_dimensions_required() {
        let checker = ConvergenceChecker::new(0.85, false);
        let dims = make_dimensions(false, true); // core fails, others pass
        // Even with high quality, core dimensions must pass
        let result = checker.check(0.95, &dims, true, 1);
        assert_eq!(result, None);
    }

    #[test]
    fn test_aggregated_feedback_requires_quality() {
        let checker = ConvergenceChecker::new(0.85, false);
        let dims = make_dimensions(true, false);
        // Below quality: aggregated_converged=true but should NOT converge
        let result = checker.check(0.70, &dims, true, 1);
        assert_eq!(result, None);
        // At quality: should converge via AggregatedFeedback
        let result = checker.check(0.85, &dims, true, 1);
        assert_eq!(result, Some(ConvergencePath::QualityTargetMet));
    }

    #[test]
    fn test_strict_mode_requires_all_optional_dimensions() {
        let checker = ConvergenceChecker::new(0.85, true);
        let mut dims = make_dimensions(true, true);
        // Missing optional dimension in strict mode
        dims.usability = None;
        let result = checker.check(0.90, &dims, false, 1);
        assert_eq!(result, None); // Strict mode requires ALL dimensions present
    }
}
