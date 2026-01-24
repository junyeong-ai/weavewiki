//! Generation Context Builder
//!
//! Builds rich context for artifact generation from extracted insights,
//! analysis results, and project information.

use std::path::PathBuf;
use std::sync::Arc;

use super::types::{GenerationContext, SynthesisSlice};
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::insight::{ExtractedInsight, InsightCategory};
use crate::types::generation::{
    ArtifactRef, ConfidenceMetrics, GenerationQualityThresholds, GenerationSynthesis,
    InferredConventions, ModuleAnalysis, ProjectDetection,
};
use crate::types::insight::{DomainContext, ModuleContext};

/// Builder for constructing GenerationContext with all necessary information
pub struct GenerationContextBuilder {
    project_root: PathBuf,
    source_insights: Vec<ExtractedInsight>,
    conventions: Option<Arc<InferredConventions>>,
    detection: Option<Arc<ProjectDetection>>,
    file_registry: Option<Arc<VerifiedFileRegistry>>,
    synthesis: Option<Arc<GenerationSynthesis>>,
    module_context: Option<ModuleContext>,
    domain_context: Option<DomainContext>,
    related_artifacts: Vec<ArtifactRef>,
    quality_config: Option<GenerationQualityThresholds>,
}

impl GenerationContextBuilder {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            source_insights: Vec::new(),
            conventions: None,
            detection: None,
            file_registry: None,
            synthesis: None,
            module_context: None,
            domain_context: None,
            related_artifacts: Vec::new(),
            quality_config: None,
        }
    }

    pub fn with_insights(mut self, insights: Vec<ExtractedInsight>) -> Self {
        self.source_insights = insights;
        self
    }

    pub fn add_insight(mut self, insight: ExtractedInsight) -> Self {
        self.source_insights.push(insight);
        self
    }

    pub fn with_conventions(mut self, conventions: Arc<InferredConventions>) -> Self {
        self.conventions = Some(conventions);
        self
    }

    pub fn with_detection(mut self, detection: Arc<ProjectDetection>) -> Self {
        self.detection = Some(detection);
        self
    }

    pub fn with_file_registry(mut self, registry: Arc<VerifiedFileRegistry>) -> Self {
        self.file_registry = Some(registry);
        self
    }

    pub fn with_synthesis(mut self, synthesis: Arc<GenerationSynthesis>) -> Self {
        self.synthesis = Some(synthesis);
        self
    }

    pub fn with_module_context(mut self, ctx: ModuleContext) -> Self {
        self.module_context = Some(ctx);
        self
    }

    pub fn with_domain_context(mut self, ctx: DomainContext) -> Self {
        self.domain_context = Some(ctx);
        self
    }

    pub fn with_related_artifacts(mut self, artifacts: Vec<ArtifactRef>) -> Self {
        self.related_artifacts = artifacts;
        self
    }

    pub fn add_related_artifact(mut self, artifact: ArtifactRef) -> Self {
        self.related_artifacts.push(artifact);
        self
    }

    pub fn with_quality_config(mut self, config: GenerationQualityThresholds) -> Self {
        self.quality_config = Some(config);
        self
    }

    /// Build the context, extracting a synthesis slice relevant to the insights
    pub fn build(self) -> GenerationContext {
        let synthesis_slice = self.extract_synthesis_slice();

        GenerationContext {
            source_insights: self.source_insights,
            synthesis: synthesis_slice,
            conventions: self
                .conventions
                .unwrap_or_else(|| Arc::new(InferredConventions::default())),
            detection: self
                .detection
                .unwrap_or_else(|| Arc::new(ProjectDetection::default())),
            file_registry: self
                .file_registry
                .unwrap_or_else(|| Arc::new(VerifiedFileRegistry::empty())),
            project_root: self.project_root,
            module_context: self.module_context,
            domain_context: self.domain_context,
            related_artifacts: self.related_artifacts,
            quality_config: self.quality_config.unwrap_or_default(),
        }
    }

    /// Extract a slice of synthesis relevant to the source insights
    fn extract_synthesis_slice(&self) -> SynthesisSlice {
        let synthesis = match &self.synthesis {
            Some(s) => s,
            None => return SynthesisSlice::default(),
        };

        // Collect file paths from insights
        let insight_files: std::collections::HashSet<&str> = self
            .source_insights
            .iter()
            .flat_map(|i| i.insight.evidence.iter())
            .filter_map(|e| e.split(':').next())
            .collect();

        // Find relevant modules
        let relevant_modules: Vec<ModuleAnalysis> = synthesis
            .modules
            .iter()
            .filter(|m| {
                insight_files
                    .iter()
                    .any(|f| m.files.iter().any(|mf| mf.starts_with(*f)))
                    || self
                        .module_context
                        .as_ref()
                        .is_some_and(|mc| mc.path == m.path)
            })
            .cloned()
            .collect();

        // Find relevant architectural decisions
        let relevant_decisions = synthesis
            .architectural_decisions
            .iter()
            .filter(|d| {
                d.affected_modules
                    .iter()
                    .any(|m| relevant_modules.iter().any(|rm| rm.name == *m))
            })
            .cloned()
            .collect();

        // Find relevant cross-cutting concerns
        let relevant_concerns = synthesis
            .cross_cutting_concerns
            .iter()
            .filter(|c| {
                c.affected_modules
                    .iter()
                    .any(|m| relevant_modules.iter().any(|rm| rm.name == *m))
            })
            .cloned()
            .collect();

        // Calculate confidence
        let confidence = Self::calculate_slice_confidence(&relevant_modules);

        SynthesisSlice {
            modules: relevant_modules,
            architectural_decisions: relevant_decisions,
            cross_cutting_concerns: relevant_concerns,
            confidence,
        }
    }

    fn calculate_slice_confidence(modules: &[ModuleAnalysis]) -> ConfidenceMetrics {
        if modules.is_empty() {
            return ConfidenceMetrics::default();
        }

        let total_files: usize = modules.iter().map(|m| m.files_analyzed).sum();
        let weighted_confidence: f32 = modules
            .iter()
            .map(|m| m.confidence * m.files_analyzed as f32)
            .sum::<f32>()
            / total_files.max(1) as f32;

        ConfidenceMetrics {
            overall: weighted_confidence,
            coverage: 1.0,
            evidence_strength: weighted_confidence,
        }
    }
}

/// Context builder specialized for different artifact types
impl GenerationContextBuilder {
    /// Build context optimized for skill generation
    pub fn for_skill(mut self, name: &str) -> Self {
        // Filter insights to those most relevant for skills
        self.source_insights.retain(|i| {
            i.insight
                .title
                .to_lowercase()
                .contains(&name.to_lowercase())
                || matches!(
                    i.insight.category,
                    InsightCategory::TechnicalConstraint
                        | InsightCategory::Gotcha
                        | InsightCategory::Workflow
                )
        });
        self
    }

    /// Build context optimized for agent generation
    pub fn for_agent(mut self, domain: &str) -> Self {
        // Filter insights to those most relevant for agents (domain knowledge)
        self.source_insights.retain(|i| {
            i.insight
                .title
                .to_lowercase()
                .contains(&domain.to_lowercase())
                || i.insight
                    .description
                    .to_lowercase()
                    .contains(&domain.to_lowercase())
        });
        self
    }

    /// Build context optimized for rule generation
    pub fn for_rule(mut self, path_pattern: &str) -> Self {
        // Filter insights to those affecting specific paths
        self.source_insights
            .retain(|i| i.insight.evidence.iter().any(|e| e.contains(path_pattern)));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::insight::{
        ArtifactClassification, Insight, TierClassification, ValueScore,
    };

    fn create_test_insight(title: &str, evidence: Vec<String>) -> ExtractedInsight {
        ExtractedInsight::new(
            Insight::new(title, "Test description")
                .with_evidence(evidence)
                .with_category(crate::pipeline::insight::InsightCategory::General),
            TierClassification::Tier3Constraint,
            ArtifactClassification::Skill,
        )
        .with_value(ValueScore::new(0.8, 0.9, 0.85))
    }

    #[test]
    fn test_builder_basic() {
        let ctx = GenerationContextBuilder::new(PathBuf::from("/test/project"))
            .with_insights(vec![create_test_insight(
                "Test Insight",
                vec!["src/main.rs:10".to_string()],
            )])
            .build();

        assert_eq!(ctx.source_insights.len(), 1);
        assert_eq!(ctx.project_root, PathBuf::from("/test/project"));
    }

    #[test]
    fn test_builder_for_skill() {
        let insights = vec![
            create_test_insight("API Validation", vec!["src/api/mod.rs:10".to_string()]),
            create_test_insight("Database Connection", vec!["src/db/pool.rs:20".to_string()]),
        ];

        let ctx = GenerationContextBuilder::new(PathBuf::from("/test"))
            .with_insights(insights)
            .for_skill("api")
            .build();

        // Should filter to API-related insight
        assert_eq!(ctx.source_insights.len(), 1);
        assert!(ctx.source_insights[0].insight.title.contains("API"));
    }

    #[test]
    fn test_empty_synthesis_slice() {
        let ctx = GenerationContextBuilder::new(PathBuf::from("/test")).build();

        assert!(ctx.synthesis.modules.is_empty());
        assert!(ctx.synthesis.architectural_decisions.is_empty());
    }
}
