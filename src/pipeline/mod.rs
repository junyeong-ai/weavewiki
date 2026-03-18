//! Pipeline Module - Quality Loop with Adaptive Pipeline
//!
//! - Phase 1: Project Detection
//! - Phase 2: Monorepo Analysis
//! - Phase 3: Deep Analysis + Synthesis
//! - Phase 4: Convention Inference
//! - Phase 5: Constraint Extraction
//! - Phase 6: Output Planning + Enrichment
//! - Phase 7: Draft Generation
//! - Phase 8: Quality Refinement
//! - Phase 9: Final Validation (LLM Judge)

pub mod adaptive;
pub mod analysis;
pub mod checkpoint;
pub mod context;
pub mod deep_review;
pub mod enrichment;
pub mod events;
pub mod evidence;
pub mod feedback;
pub mod file_reference;
pub mod generation;
pub mod insight;
pub mod iteration_state;
pub mod learning;
pub mod phases;
pub mod quality;
pub mod quality_assessment;
pub mod quality_loop;
pub mod refinement;
pub mod session_lock;
pub mod strategy;
pub mod sync;
pub mod validation;

pub use adaptive::{AdaptivePipeline, AdaptivePipelineOutput};
pub use analysis::{DeepAnalysisResult, DeepAnalyzer};
pub use checkpoint::{
    CheckpointManager, CrashRecovery, ExecutionCheckpoint, PipelinePhase, RecoveryResult,
};
pub use context::{
    AnalysisResults, AnalysisSynthesis, ClaudegenContext, ContextStats, KeyAbstraction,
    TrackedConstraint, VerifiedFileRegistry,
};
pub use enrichment::{
    AgentInternalKnowledge, ConstraintCoverage, EnrichedConstraint, EnrichedPlan, EnrichmentEngine,
};
// Canonical SynthesizedAnalysis from analysis module
pub use analysis::SynthesizedAnalysis;
pub use deep_review::{DeepReviewEngine, DeepReviewResult, ReviewArtifacts, TwoPassResult};
pub use feedback::{AggregatedFeedback, FeedbackAggregator};
pub use insight::{
    ArtifactClassification, ExtractedInsight, InsightCategory, TierClassification, ValueScore,
};
pub use iteration_state::{BudgetExtensionTrigger, IterationRecord, IterationState, RevisionMeta};
pub use learning::{LearningHistory, ProgressSummary};
pub use quality_assessment::{
    AssessmentPath, ContinueReason, QualityAssessment, QualityAssessor, TerminationDecision,
    TerminationReason,
};
pub use quality_loop::{QualityLoop, QualityLoopResult};
pub use refinement::{RefinementEngine, RefinementResult};
pub use strategy::{RefinementStrategy, StrategyRotator};

pub use quality::{
    // LLM Judge
    Artifacts,
    IssueSeverity,
    JudgeConfig,
    JudgmentResult,
    LlmJudge,
    QualityIssue,
    Suggestion,
};
