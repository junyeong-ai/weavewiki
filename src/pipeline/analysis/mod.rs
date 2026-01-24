//! Analysis Module
//!
//! Multi-layer analysis system:
//! - AST Analysis: Ground-truth extraction via tree-sitter
//! - Deep Analysis: LLM-powered semantic understanding
//! - Distributed Analysis: Parallel chunk-based analysis for large codebases
//! - Synthesis: Hierarchical summarization
//! - Architectural: Module coverage and structural completeness

pub mod architectural_analyzer;
pub mod ast;
pub mod ast_enrichment;
pub mod deep_analyzer;
pub mod distributed;
pub mod multi_agent;
pub mod synthesis;

pub use architectural_analyzer::{
    ArchitecturalAnalyzer, CoverageReport, Module, ModuleCoverage, StructuralCategory,
    StructuralIssue, StructuralValidationResult,
};

pub use deep_analyzer::{
    AnalysisQuality, CodePattern, ConstraintEnforcement, DeepAnalysisResult, DeepAnalyzer,
    FileConstraint, FileDeepAnalysis, FileInsight, Gotcha, ModuleDependency, PatternInstance,
    Relationship,
};

pub use synthesis::{
    AnalysisSynthesizer, ConfidenceScores, CrossValidation, InvalidReference, MergedModule,
    ReanalysisTargets, ReferenceValidationResult, SynthesizedAnalysis,
};

pub use ast_enrichment::{
    AstEnricher, AstFacts, AstStats, AstValidation, AstValidator, FunctionFact, ImportFact,
    ParseFailure, ReferenceCheck, TraitFact, TypeFact, TypeKind, Visibility as AstVisibility,
};

pub use ast::{
    AstAnalysisResult, AstAnalyzerAgent, AstProjectStructure, ComplexityMetrics, DependencyGraph,
    FileAstAnalysis, PublicApiSurface,
};

pub use distributed::{
    AnalysisChunk, AnalysisMerger, ChunkAnalysisResult, ChunkAnalyzer, ChunkConstraint,
    ChunkInsight, ChunkPattern, ConstraintSeverity, LlmChunkAnalyzer, MergedAnalysis,
    ModulePartitioner, ParallelAnalyzer,
};
