//! Evidence Metrics and Profiling
//!
//! Provides evidence metrics for generation context.
//! LLM decides how to use evidence - we just track counts for informational context.

use super::context::GenerationContext;

/// Label-based evidence profile from content scanning
#[derive(Debug, Clone, Default)]
pub struct EvidenceProfile {
    pub verified_count: usize,
    pub inferred_count: usize,
    pub convention_count: usize,
}

impl EvidenceProfile {
    pub fn total(&self) -> usize {
        self.verified_count + self.inferred_count + self.convention_count
    }

    pub fn merge(&mut self, other: &EvidenceProfile) {
        self.verified_count += other.verified_count;
        self.inferred_count += other.inferred_count;
        self.convention_count += other.convention_count;
    }

    pub fn verification_ratio(&self) -> f32 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        self.verified_count as f32 / total as f32
    }
}

/// Evidence metrics from generation context
#[derive(Debug, Clone)]
pub struct EvidenceMetrics {
    pub verified_refs: usize,
    pub patterns: usize,
    pub constraints: usize,
    pub confidence: f32,
}

impl EvidenceMetrics {
    pub fn from_context(ctx: &GenerationContext<'_>) -> Self {
        Self {
            verified_refs: ctx
                .reference_pool
                .as_ref()
                .map(|p| p.total_count())
                .unwrap_or(0),
            patterns: ctx.pattern_count(),
            constraints: ctx.constraint_count(),
            confidence: ctx.overall_confidence(),
        }
    }

    pub fn has_evidence(&self) -> bool {
        self.verified_refs > 0 || self.patterns > 0
    }

    pub fn total_evidence(&self) -> usize {
        self.verified_refs + self.patterns + self.constraints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::context::VerifiedFileRegistry;
    use crate::pipeline::phases::{
        constraint_extraction::ExtractedConstraints, convention_inference::InferredConventions,
        project_detection::ProjectDetection,
    };
    use crate::types::module_map::TechStack;

    fn empty_context<'a>(
        detection: &'a ProjectDetection,
        tech_stack: &'a TechStack,
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        registry: &'a VerifiedFileRegistry,
    ) -> GenerationContext<'a> {
        GenerationContext::new(
            detection,
            tech_stack,
            "test-project",
            &[],
            &[],
            &[],
            conventions,
            constraints,
            registry,
        )
    }

    #[test]
    fn test_empty_context_has_no_evidence() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = empty_context(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );

        let metrics = EvidenceMetrics::from_context(&ctx);
        assert!(!metrics.has_evidence());
        assert_eq!(metrics.total_evidence(), 0);
    }

    #[test]
    fn test_evidence_totals() {
        let metrics = EvidenceMetrics {
            verified_refs: 25,
            patterns: 10,
            constraints: 5,
            confidence: 0.9,
        };
        assert_eq!(metrics.total_evidence(), 40);
        assert!(metrics.has_evidence());
    }

    #[test]
    fn test_empty_profile() {
        let p = EvidenceProfile::default();
        assert_eq!(p.total(), 0);
        assert_eq!(p.verification_ratio(), 0.0);
    }

    #[test]
    fn test_profile_ratio() {
        let p = EvidenceProfile {
            verified_count: 6,
            inferred_count: 3,
            convention_count: 1,
        };
        assert_eq!(p.total(), 10);
        assert!((p.verification_ratio() - 0.6).abs() < 0.001);
    }
}
