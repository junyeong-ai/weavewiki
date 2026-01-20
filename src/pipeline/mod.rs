//! Pipeline Module - Adaptive Generation Pipeline
//!
//! Project-Type Agnostic generation pipeline with quality loop:
//! - Outer Loop: Quality verification with analysis re-run on gaps
//! - Phase 1: Project Detection (auto-detect CLI/Library/Backend/Frontend/Monorepo)
//! - Phase 2: Monorepo Analysis (workspace structure)
//! - Phase 3: Multi-Agent Deep Analysis (parallel specialist agents)
//! - Phase 4: Convention Inference (few-shot based)
//! - Phase 5: Constraint Extraction (Tier 3 value)
//! - Phase 6: Output Planning (strategy selection)
//! - Phase 7: Insight-Driven Generation (LLM-based content decisions)
//! - Phase 8: Quality-Based Refinement (semantic quality loop with strategy rotation)
//! - Phase 9: Final Validation (tier filtering, evidence validation)

pub mod adaptive;
pub mod analysis;
pub mod checkpoint;
pub mod context;
pub mod convergence;
pub mod deep_review;
pub mod feedback;
pub mod feedback_loop;
pub mod generation;
pub mod insight;
pub mod learning;
pub mod patterns;
pub mod phase_provider;
pub mod phases;
pub mod quality_loop;
pub mod reference_extractor;
pub mod refinement;
pub mod strategy;
pub mod validation;

pub use adaptive::{AdaptivePipeline, AdaptivePipelineOutput};
pub use analysis::{DeepAnalysisResult, DeepAnalyzer};
pub use checkpoint::{CheckpointManager, CrashRecovery, ExecutionCheckpoint, PipelinePhase, RecoveryResult};
pub use context::{ProjectContext, VerifiedFileRegistry};
pub use convergence::{ConvergencePath, ConvergenceReport};
pub use deep_review::{DeepReviewEngine, DeepReviewResult, ReviewArtifacts, TwoPassResult};
pub use feedback::{AggregatedFeedback, FeedbackAggregator};
pub use learning::{LearningHistory, ProgressSummary};
pub use phase_provider::{Phase, PhaseProviderFactory};
pub use quality_loop::{QualityLoop, QualityLoopResult};
pub use refinement::{RefinementEngine, RefinementResult};
pub use strategy::{RefinementStrategy, StrategyRotator};
pub use feedback_loop::{FeedbackLoop, FeedbackLoopConfig, VerifiedAnalysis};
pub use insight::{
    ArtifactClassification, ArtifactInsights, ExtractedInsight, ExtractionStats,
    InsightContext, InsightEngine, InsightExtractionResult, KnowledgeClassifier,
    MistakeFinder, PotentialMistake, TierClassification, ValueScore, ValueScorer,
};
