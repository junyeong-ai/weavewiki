//! Pipeline Validation Module
//!
//! 5-Layer Validation Architecture:
//! - Layer 0: Format (100% programmatic, structure validation)
//! - Layer 1: Evidence (programmatic + file I/O, reference validity)
//! - Layer 2: Semantic Context (LLM + file reading, claim-context match)
//! - Layer 3: Value Assessment (LLM + few-shot, tier classification)
//! - Layer 4: Cross-Artifact (LLM, consistency between artifacts)
//!
//! Clean Pass Guarantee: Requires N consecutive passes with zero issues.

// Core validation types
pub mod layers;
pub mod clean_pass;
pub mod pipeline;

// Validation layers
pub mod semantic_context;
pub mod few_shot_examples;
pub mod value_assessor;

// Legacy validators (to be integrated)
pub mod content;
pub mod cross_artifact;
pub mod cross_specialist;
pub mod cross_validation;
pub mod evidence;
pub mod multi_perspective;
pub mod project_applicability;
pub mod project_consistency;
pub mod quality_validator;
pub mod semantic_validator;
pub mod tier_filter;
pub mod usability;

pub use content::{
    assess_agent_content, assess_memory_content, assess_skill_content, contains_absolute_paths,
    contains_raw_json, evidence_requirements, is_truncated, ContentAssessment, ContentType,
    EvidenceIssue,
};

pub use cross_validation::{
    validate as validate_cross, ArtifactConsistencyResult, ArtifactInconsistency, ArtifactOverlap,
    ArtifactRedundancy, CrossValidationIssue, CrossValidationResult, CrossValidator,
    EvidenceTraceabilityResult, InvalidReference, OverlapSeverity, PlanConsistencyResult,
    ValidationCategory, ValidationSeverity,
};

pub use project_consistency::{
    check as check_project_consistency, ConsistencyIssue, ConsistencyResult, IssueSeverity,
    ProjectConsistencyChecker,
};

pub use semantic_validator::{
    validate as validate_semantic, IssueCategory, IssueSeverity as SemanticIssueSeverity,
    SemanticIssue, SemanticQualityResult, SemanticScore, SemanticValidator,
};

pub use tier_filter::{
    filter as filter_tier1, ContentTier, FilteredContent, ItemType, Tier1Violation, TierFilter,
    TierFilterResult, ValueScore,
};

pub use quality_validator::{QualityThresholds, QualityValidator};

pub use evidence::{
    validate_evidence, ArtifactEvidenceResult, DepthComplianceResult, EnhancedEvidenceResult,
    EnhancedEvidenceValidator, EvidenceIssue as EnhancedEvidenceIssue, EvidenceSummary,
    IssueCategory as EvidenceIssueCategory, IssueSeverity as EvidenceIssueSeverity, ParsedReference,
};

pub use cross_specialist::{
    ConflictType, CrossSpecialistConfig, CrossSpecialistResult, CrossSpecialistValidator,
    SpecialistAgreement, SpecialistConflict,
};

pub use project_applicability::{
    ApplicabilityConfig, ApplicabilityIssue, ApplicabilityIssueType, ApplicabilityResult,
    ProjectApplicabilityValidator,
};

pub use multi_perspective::{
    ClaimSeverity, CompletenessResult, HallucinationResult, MultiPerspectiveResult,
    MultiPerspectiveValidator, PerspectiveResult, SuspiciousClaim,
};

// New validation system exports
pub use layers::{
    IssueCode, IssueSeverity as LayerIssueSeverity, LayerResult, ValidationIssue, ValidationLayer,
    ValidationResults,
};

pub use clean_pass::{
    CleanPassAttempt, CleanPassStatus, CleanPassTracker, FailureReason, PassTrend, ProgressSummary,
};

pub use pipeline::ValidationPipeline;

pub use semantic_context::{ClaimContext, ContextMatch, SemanticContextResult, SemanticContextValidator};

pub use few_shot_examples::{FewShotExamples, TierExample, TierLevel, ValueDimensions};

pub use value_assessor::{ContentAssessment as ValueContentAssessment, ValueAssessmentResult, ValueAssessor};
