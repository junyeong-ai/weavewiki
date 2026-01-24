//! Pipeline-specific generation types
//!
//! Types that depend on pipeline infrastructure (ExtractedInsight, VerifiedFileRegistry)
//! belong here, not in types/ to avoid circular dependencies.

use std::path::PathBuf;
use std::sync::Arc;

use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::insight::ExtractedInsight;
use crate::types::generation::{
    ArchitecturalDecision, ArtifactRef, ArtifactType, ConfidenceMetrics, CrossCuttingConcern,
    GenerationQualityThresholds, GenerationSynthesis, InferredConventions, ModuleAnalysis,
    ProjectDetection,
};
use crate::types::insight::{DomainContext, ModuleContext};

/// Rich context for artifact generation
///
/// Contains all information needed to generate high-quality artifacts:
/// - Source insights from analysis
/// - Project conventions and detection results
/// - File registry for reference validation
#[derive(Debug, Clone)]
pub struct GenerationContext {
    pub source_insights: Vec<ExtractedInsight>,
    pub synthesis: SynthesisSlice,
    pub conventions: Arc<InferredConventions>,
    pub detection: Arc<ProjectDetection>,
    pub module_context: Option<ModuleContext>,
    pub domain_context: Option<DomainContext>,
    pub file_registry: Arc<VerifiedFileRegistry>,
    pub project_root: PathBuf,
    pub related_artifacts: Vec<ArtifactRef>,
    pub quality_config: GenerationQualityThresholds,
}

impl GenerationContext {
    pub fn new(
        source_insights: Vec<ExtractedInsight>,
        synthesis: SynthesisSlice,
        conventions: Arc<InferredConventions>,
        detection: Arc<ProjectDetection>,
        file_registry: Arc<VerifiedFileRegistry>,
        project_root: PathBuf,
    ) -> Self {
        Self {
            source_insights,
            synthesis,
            conventions,
            detection,
            module_context: None,
            domain_context: None,
            file_registry,
            project_root,
            related_artifacts: Vec::new(),
            quality_config: GenerationQualityThresholds::default(),
        }
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

    pub fn with_quality_config(mut self, config: GenerationQualityThresholds) -> Self {
        self.quality_config = config;
        self
    }

    pub fn file_context(&self, max_files: usize) -> String {
        self.file_registry.to_prompt_context(max_files)
    }
}

/// Slice of synthesis relevant to specific insights
#[derive(Debug, Clone, Default)]
pub struct SynthesisSlice {
    pub modules: Vec<ModuleAnalysis>,
    pub architectural_decisions: Vec<ArchitecturalDecision>,
    pub cross_cutting_concerns: Vec<CrossCuttingConcern>,
    pub confidence: ConfidenceMetrics,
}

impl SynthesisSlice {
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
            && self.architectural_decisions.is_empty()
            && self.cross_cutting_concerns.is_empty()
    }

    pub fn extract_for(full_analysis: &GenerationSynthesis, insight: &ExtractedInsight) -> Self {
        let relevant_modules: Vec<_> = full_analysis
            .modules
            .iter()
            .filter(|m| {
                insight
                    .insight
                    .evidence
                    .iter()
                    .any(|e| m.files.iter().any(|f| e.contains(f)))
            })
            .cloned()
            .collect();

        let relevant_decisions: Vec<_> = full_analysis
            .architectural_decisions
            .iter()
            .filter(|d| {
                d.affected_modules
                    .iter()
                    .any(|m| relevant_modules.iter().any(|rm| rm.name == *m))
            })
            .cloned()
            .collect();

        let relevant_concerns: Vec<_> = full_analysis
            .cross_cutting_concerns
            .iter()
            .filter(|c| {
                c.affected_modules
                    .iter()
                    .any(|m| relevant_modules.iter().any(|rm| rm.name == *m))
            })
            .cloned()
            .collect();

        let confidence = Self::calculate_slice_confidence(&relevant_modules);

        Self {
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

/// Planned artifact for generation
#[derive(Debug, Clone, Default)]
pub struct PlannedArtifact {
    pub artifact_type: ArtifactType,
    pub name: String,
    pub source_insights: Vec<ExtractedInsight>,
    pub synthesis_context: SynthesisSlice,
    pub generation_priority: u8,
}
