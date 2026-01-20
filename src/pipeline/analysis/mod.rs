//! Deep Analysis Module
//!
//! Multi-agent system for thorough codebase analysis:
//! - Multi-agent: Parallel specialist agents for comprehensive analysis
//! - Structure analysis: Directory layout, key files, module dependencies
//! - Pattern extraction: Actual code patterns from file contents
//! - Constraint discovery: Hidden dependencies, anti-patterns from code
//! - Synthesis: Merge findings into coherent project knowledge
//! - Architectural analysis: Module coverage and structural completeness
//! - Top-down validation: Convention consistency with project structure
//! - AST enrichment: Ground-truth facts from tree-sitter parsing

pub mod architectural_analyzer;
pub mod ast_enrichment;
pub mod deep_analyzer;
pub mod multi_agent;
pub mod reconciliation;
pub mod synthesis;
pub mod top_down;

pub use architectural_analyzer::{
    ArchitecturalAnalyzer, CoverageReport, Module, ModuleCoverage, StructuralCategory,
    StructuralIssue, StructuralSeverity, StructuralValidationResult,
};

pub use deep_analyzer::{
    AnalysisQuality, DeepAnalysisResult, DeepAnalyzer, FileInsight, ModuleDependency,
    PatternInstance,
};

pub use synthesis::{
    AnalysisSynthesizer, ConfidenceScores, CrossValidation, InvalidReference, MergedModule,
    ReanalysisTargets, ReferenceValidationResult, SynthesizedAnalysis,
};

pub use top_down::{
    ConventionCategory, ConventionIssue, ConventionValidationResult, ConventionValidator,
    ValidationSeverity,
};

pub use reconciliation::{
    AnalysisClaim, BidirectionalReconciler, ClaimSource, ConflictCategory, FileRef,
    ReconciledAnalysis, ReconciliationConfig, ReconciliationConflict, ResolutionDecision,
};

pub use ast_enrichment::{
    AstEnricher, AstFacts, AstStats, AstValidator, FunctionFact, ImportFact, ParseFailure,
    ReferenceCheck, TraitFact, TypeFact, TypeKind, ValidationResult,
    Visibility as AstVisibility,
};
