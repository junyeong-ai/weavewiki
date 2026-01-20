//! Pipeline Validation Module
//!
//! Two-tier validation architecture:
//! - Tier 1: Pattern-based pre-filter (fast, no LLM)
//! - Tier 2: AI-based quality validation (single LLM call, self-review perspective)
//!
//! Enhanced evidence validation:
//! - Per-project-type minimum references
//! - Evidence depth validation (FileOnly, FileAndLine, FileLineContext)

pub mod content;
pub mod cross_artifact;
pub mod cross_specialist;
pub mod cross_validation;
pub mod evidence;
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
