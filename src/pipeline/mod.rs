//! Pipeline Module - Multi-Agent Orchestration Pipeline
//!
//! Durable execution pipeline with hierarchical analysis and LLM-based validation:
//! - Phase 1: AST Analysis (ground-truth extraction)
//! - Phase 2: File-level LLM Analysis (parallel deep analysis)
//! - Phase 3: Hierarchical Summary (Map-Reduce synthesis)
//! - Phase 4: Cross-Analysis (matrix validation)
//! - Phase 5: Generation (search-based artifact creation)
//! - Phase 6: Validation (LLM-as-Judge quality assurance)

pub mod accumulative_context;
pub mod adaptive;
pub mod analysis;
pub mod checkpoint;
pub mod context;
pub mod convergence;
pub mod cross;
pub mod deep_review;
pub mod enrichment;
pub mod execution;
pub mod feedback;
pub mod file_reference;
pub mod generation;
pub mod insight;
pub mod learning;
pub mod patterns;
pub mod phase_provider;
pub mod phases;
pub mod quality;
pub mod quality_loop;
pub mod reference_extractor;
pub mod refinement;
pub mod search;
pub mod storage;
pub mod strategy;
pub mod synthesis;
pub mod thinking;
pub mod validation;

pub use adaptive::{AdaptivePipeline, AdaptivePipelineOutput};
pub use analysis::{DeepAnalysisResult, DeepAnalyzer};
pub use enrichment::{
    AgentInternalKnowledge, ConstraintCoverage, EnrichedConstraint, EnrichedPlan,
    EnrichmentEngine,
};
pub use checkpoint::{CheckpointManager, CrashRecovery, ExecutionCheckpoint, PipelinePhase, RecoveryResult};
pub use context::{ProjectContext, VerifiedFileRegistry};
pub use convergence::{
    ConvergencePath, ConvergenceReport, TerminationDecision, TerminationReason, ContinueReason,
};
pub use deep_review::{DeepReviewEngine, DeepReviewResult, ReviewArtifacts, TwoPassResult};
pub use feedback::{AggregatedFeedback, FeedbackAggregator};
pub use learning::{LearningHistory, ProgressSummary};
pub use quality_loop::{QualityLoop, QualityLoopResult};
pub use refinement::{RefinementEngine, RefinementResult};
pub use strategy::{RefinementStrategy, StrategyRotator};
pub use thinking::{ThinkingState, ThinkingRecord, ExtensionTrigger};
pub use insight::{
    ArtifactClassification, ArtifactInsights, ExtractedInsight, ExtractionStats,
    HybridClassifier, InsightContext, InsightEngine, InsightExtractionResult, MistakeFinder,
    PotentialMistake, TierClassification, ValueScore, ValueScorer,
};
pub use accumulative_context::{
    AccumulativeContext, AbstractionSummary, ContextStats, ContextSummary,
    FileGotcha, Tier, Tier3Category, Tier3Item,
};
pub use analysis::language_analyzer::{
    LanguageAnalyzerRegistry, LanguageAnalysisResult, LanguageConstraint, LanguageGotcha,
    LanguagePattern, LanguageSpecificAnalyzer, PatternCategory, RustAnalyzer,
};

pub use search::{
    EntryType, IndexStats, Keyword, KeywordExtractor, SearchApi, SearchEntry, SearchHit,
    SearchIndex, SearchQuery, SearchResult,
};

pub use cross::{
    AnalysisIntersection, ArchitectureStyle, BoundedContext, ConstraintCrossAnalysis,
    CrossAnalysisConflict, CrossAnalysisGap, CrossAnalysisMatrix, CrossValidationAgent,
    CrossValidationConfig, CriticalPath, DesignPrinciple, DomainConcept, HiddenConstraint,
    LayerCrossAnalysis, ModuleCrossAnalysis, PatternCrossAnalysis, ProjectCharacteristics,
    ProjectGotcha, RelationshipType,
};

pub use execution::{
    DurableExecutor, ExecutionPhase, ExecutionState, PendingTask, RecoveryStrategy,
    TaskResult, TaskTracker, TaskType,
};
pub use storage::{DurableStore, StoreConfig};

pub use synthesis::{
    DomainSummary, FileGrouper, GroupingStrategy, HierarchicalSummarizer,
    ModuleGroup, ModuleSummary, ProjectSynthesis, QualityChecker, SummaryQuality,
    SynthesisConfig,
};


pub use quality::{
    Artifacts, ConvergenceChecker, ConvergenceReason, ConvergenceResult,
    IssueSeverity, JudgeConfig, JudgmentResult, LlmJudge, QualityIssue, Suggestion,
    // LLM Judge Agent types
    AgentJudgmentResult, AgentQualityIssue, Artifact, ArtifactType, BatchJudgmentResult,
    EvidenceValidity, ImprovementPriority, ImprovementSuggestion, JudgeContext, LlmJudgeAgent,
    LlmJudgeAgentConfig,
};
