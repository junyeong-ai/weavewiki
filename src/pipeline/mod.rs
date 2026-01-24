//! Pipeline Module - Multi-Agent Orchestration Pipeline
//!
//! Durable execution pipeline with hierarchical analysis and LLM-based validation:
//! - Phase 1: AST Analysis (ground-truth extraction)
//! - Phase 2: File-level LLM Analysis (parallel deep analysis)
//! - Phase 3: Hierarchical Summary (Map-Reduce synthesis)
//! - Phase 4: Cross-Analysis (matrix validation)
//! - Phase 5: Generation (search-based artifact creation)
//! - Phase 6: Validation (LLM-as-Judge quality assurance)

// accumulative_context module removed - replaced by ClaudegenContext in context.rs
pub mod adaptive;
pub mod analysis;
pub mod checkpoint;
pub mod context;
pub mod deep_review;
pub mod enrichment;
pub mod feedback;
pub mod file_reference;
pub mod generation;
pub mod insight;
pub mod iteration_state;
pub mod learning;
pub mod patterns;
pub mod phases;
pub mod quality;
pub mod quality_assessment;
pub mod quality_loop;
pub mod reference_extractor;
pub mod refinement;
pub mod storage;
pub mod strategy;
pub mod validation;

pub use adaptive::{AdaptivePipeline, AdaptivePipelineOutput};
pub use analysis::{
    // Distributed analysis
    AnalysisChunk,
    AnalysisMerger,
    ChunkAnalyzer,
    // Deep analysis
    DeepAnalysisResult,
    DeepAnalyzer,
    LlmChunkAnalyzer,
    MergedAnalysis,
    ModulePartitioner,
    ParallelAnalyzer,
};
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
    ArtifactClassification, ArtifactInsights, ExtractedInsight, ExtractionStats, HybridClassifier,
    InsightContext, InsightEngine, InsightExtractionResult, MistakeFinder, PotentialMistake,
    TierClassification, ValueScore, ValueScorer,
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

pub use storage::{DurableStore, StoreConfig};

pub use quality::{
    // Quality gate
    ArtifactGateResult,
    ArtifactOverlapResult,
    // LLM Judge
    Artifacts,
    GateIssue,
    GateIssueCategory,
    GateResult,
    GateSummary,
    InconsistencyIssue,
    IssueSeverity,
    JudgeConfig,
    JudgmentResult,
    LlmJudge,
    QualityGate,
    QualityGateConfig,
    QualityIssue,
    RedundancyIssue,
    Suggestion,
};
