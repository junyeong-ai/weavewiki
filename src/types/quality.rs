//! Unified Quality Measurement Types
//!
//! Programmatic gating uses only reference_validity (hallucination detection).
//! Semantic quality (actionability, specificity) is LLM-evaluated.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::utils::patterns::extract_file_refs;

/// Minimum overall value assessment score to be considered "high value".
pub const HIGH_VALUE_THRESHOLD: f32 = 0.6;

/// Minimum reference validity ratio for artifact acceptance.
/// 90%+ of file references must point to real files.
const REFERENCE_VALIDITY_GATE: f32 = 0.90;

const DOMAIN_SPECIFICITY_WEIGHT: f32 = 0.30;
const ACTIONABILITY_WEIGHT: f32 = 0.25;
const INFORMATION_DENSITY_WEIGHT: f32 = 0.25;
const COMPLETENESS_WEIGHT: f32 = 0.20;

const VALIDITY_DIAGNOSTIC_WEIGHT: f32 = 0.60;
const DENSITY_DIAGNOSTIC_WEIGHT: f32 = 0.40;

const DENSITY_SCALING_FACTOR: f32 = 500.0;
const DENSITY_LOG_BASE: f32 = 10.0;

/// Value assessment for generated artifacts (LLM-evaluated).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ValueAssessment {
    pub domain_specificity: f32,
    pub actionability: f32,
    pub information_density: f32,
    pub completeness: f32,
    #[serde(default)]
    pub generic_content_ratio: f32,
    pub value_evidence: Vec<String>,
}

impl ValueAssessment {
    pub fn overall(&self) -> f32 {
        DOMAIN_SPECIFICITY_WEIGHT * self.domain_specificity
            + ACTIONABILITY_WEIGHT * self.actionability
            + INFORMATION_DENSITY_WEIGHT * self.information_density
            + COMPLETENESS_WEIGHT * self.completeness
    }

    pub fn is_high_value(&self) -> bool {
        self.overall() >= HIGH_VALUE_THRESHOLD
    }
}

/// Artifact quality measurement based on objective metrics.
///
/// Gating uses only `reference_validity` (hallucination detection).
/// `overall()` is diagnostic-only for logging.
///
/// Semantic quality (actionability, specificity) is assessed separately
/// by `ValueAssessment` from the LLM Judge.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArtifactQuality {
    pub reference_validity: f32,
    pub reference_density: f32,
}

impl ArtifactQuality {
    /// Diagnostic-only overall quality score (0.0-1.0).
    /// NOT used for gating. Use `is_acceptable()` for pass/fail decisions.
    pub fn overall(&self) -> f32 {
        VALIDITY_DIAGNOSTIC_WEIGHT * self.reference_validity + DENSITY_DIAGNOSTIC_WEIGHT * self.reference_density.min(1.0)
    }

    /// Acceptance gate: reference validity must meet minimum threshold.
    pub fn is_acceptable(&self) -> bool {
        self.reference_validity >= REFERENCE_VALIDITY_GATE
    }

    pub fn from_judgment(
        content: &str,
        valid_paths: &impl Fn(&str) -> bool,
    ) -> Self {
        let (reference_validity, reference_density) =
            Self::compute_objective(content, valid_paths);

        Self {
            reference_validity,
            reference_density,
        }
    }

    pub fn compute_objective(
        content: &str,
        valid_paths: &impl Fn(&str) -> bool,
    ) -> (f32, f32) {
        let refs = extract_file_refs(content);
        let total_refs = refs.len();

        let valid_refs = refs
            .iter()
            .filter(|r| !r.path.ends_with('/') && valid_paths(&r.path))
            .count();

        let char_count = content.chars().count().max(1);

        let raw_density = (total_refs as f32 / char_count as f32) * DENSITY_SCALING_FACTOR;
        let reference_density = (1.0 + raw_density).ln() / (1.0 + DENSITY_LOG_BASE).ln();

        let reference_validity = if total_refs > 0 {
            valid_refs as f32 / total_refs as f32
        } else {
            1.0
        };

        (reference_validity, reference_density)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_validity_gates_acceptance() {
        let high_ref = ArtifactQuality {
            reference_validity: 0.95,
            reference_density: 0.1,
        };
        assert!(high_ref.is_acceptable());

        let low_ref = ArtifactQuality {
            reference_validity: 0.3,
            reference_density: 1.0,
        };
        assert!(!low_ref.is_acceptable());
    }

    #[test]
    fn test_no_references_is_valid() {
        let always_valid = |_: &str| true;
        let content = "Just some text without any references";
        let (validity, _) = ArtifactQuality::compute_objective(content, &always_valid);
        assert_eq!(validity, 1.0);
    }

    #[test]
    fn test_reference_density_logarithmic() {
        let always_valid = |_: &str| true;

        let content_5refs = "@a:1 @b:2 @c:3 @d:4 @e:5 some text content here";
        let content_10refs = "@a:1 @b:2 @c:3 @d:4 @e:5 @f:6 @g:7 @h:8 @i:9 @j:10";

        let (_, density_5) = ArtifactQuality::compute_objective(content_5refs, &always_valid);
        let (_, density_10) = ArtifactQuality::compute_objective(content_10refs, &always_valid);

        assert!(density_10 > density_5);
        assert!(density_10 < density_5 * 1.5);
    }

    #[test]
    fn test_from_judgment() {
        let always_valid = |_: &str| true;
        let content = "## Overview\n\nSee @src/main.rs:42 for details.\n\n```rust\ncode\n```";

        let quality = ArtifactQuality::from_judgment(content, &always_valid);

        assert!(quality.overall() > 0.5);
        assert!(quality.reference_validity > 0.0);
    }

    #[test]
    fn test_overall_is_diagnostic_only() {
        let low_overall_high_ref = ArtifactQuality {
            reference_validity: 0.95,
            reference_density: 0.0,
        };
        assert!(low_overall_high_ref.is_acceptable(), "High ref validity should pass regardless of overall score");
    }

    #[test]
    fn test_quality_boundary_at_gate() {
        let q_pass = ArtifactQuality {
            reference_validity: 0.90,
            ..Default::default()
        };
        assert!(q_pass.is_acceptable(), "Exactly at gate should pass");
        let q_fail = ArtifactQuality {
            reference_validity: 0.899,
            ..Default::default()
        };
        assert!(!q_fail.is_acceptable(), "Below gate should fail");
    }
}
