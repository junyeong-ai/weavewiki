//! Analysis Module
//!
//! Multi-layer analysis system for 100% file coverage:
//! - Distributed Analysis: Parallel chunked analysis for full coverage
//! - Aggregator: Map-Reduce aggregation of chunk results
//! - Domain Analysis: Core policies, logic, terminology, workflows
//! - Cross Synthesis: Cross-reference synthesis and tier classification
//! - AST Enrichment: Ground-truth extraction via tree-sitter
//! - Deep Analysis: LLM-powered semantic understanding
//! - Architectural: Module coverage and structural completeness

pub mod aggregator;
pub mod architectural_analyzer;
pub mod ast_enrichment;
pub mod cross_synthesis;
pub mod deep_analyzer;
pub mod distributed;
pub mod domain_analyzer;
pub mod synthesis;

// Distributed Analysis (100% coverage)
pub use distributed::{
    AnalysisChunk, AsyncStyle, ChunkAnalysisResult, ChunkConventions, ChunkingStrategy,
    DistributedAnalyzer, ErrorStyle, NamingCase,
};

// Aggregation (Map-Reduce)
pub use aggregator::{
    AggregatedAnalysis, AggregatedConstraint, AggregatedPattern, AnalysisAggregator, Coverage,
    DependencyEdge, DependencyGraph, ProjectConventions,
};

// Domain Analysis
pub use domain_analyzer::DomainAnalyzer;

// Cross Synthesis
pub use cross_synthesis::{
    ArchitectureViolation, CoverageAnalysis, CoverageGap, CrossModuleConstraint, CrossSynthesizer,
    DomainArchMapping, GapType, HiddenDependency, HiddenDependencyType, PolicyViolation,
    SynthesizedInsights, Tier2Category, Tier2Insight, Tier3Category, Tier3Insight, ViolationType,
};

// Architectural Analysis
pub use architectural_analyzer::{
    ArchitecturalAnalysis, ArchitecturalAnalyzer, CoverageReport, ModuleCoverage,
    StructuralCategory, StructuralIssue, StructuralValidationResult,
};

// Deep Analysis
pub use deep_analyzer::{
    AnalysisQuality, CodePattern, ConstraintEnforcement, ConstraintKind, DeepAnalysisResult,
    DeepAnalyzer, DiscoveredConstraint, FileConstraint, FileDeepAnalysis, FileInsight, Gotcha,
    ModuleDependency, PatternInstance, Relationship, ValueBreakdown,
};

// Synthesis
pub use synthesis::{
    AnalysisSynthesizer, ConfidenceScores, CrossValidation, InvalidReference, MergedModule,
    ReanalysisTargets, ReferenceValidationResult, SynthesizedAnalysis,
};

// AST Enrichment
pub use ast_enrichment::{
    AstEnricher, AstFacts, AstStats, AstValidation, AstValidator, FunctionFact, ImportFact,
    ParseFailure, ReferenceCheck, TraitFact, TypeFact, TypeKind, Visibility as AstVisibility,
};
