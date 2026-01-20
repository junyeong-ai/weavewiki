//! Multi-Layer Validation Architecture
//!
//! Implements a 5-layer validation pipeline:
//! - Layer 0: Format (100% programmatic, structure validation)
//! - Layer 1: Evidence (programmatic + file I/O, reference validity)
//! - Layer 2: Semantic Context (LLM + file reading, claim-context match)
//! - Layer 3: Value Assessment (LLM + few-shot, tier classification)
//! - Layer 4: Cross-Artifact (LLM, consistency between artifacts)

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValidationLayer {
    Format,
    Evidence,
    SemanticContext,
    ValueAssessment,
    CrossArtifact,
}

impl ValidationLayer {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Format,
            Self::Evidence,
            Self::SemanticContext,
            Self::ValueAssessment,
            Self::CrossArtifact,
        ]
    }

    pub fn confidence(&self) -> f32 {
        match self {
            Self::Format => 1.0,
            Self::Evidence => 1.0,
            Self::SemanticContext => 0.85,
            Self::ValueAssessment => 0.80,
            Self::CrossArtifact => 0.80,
        }
    }

    pub fn is_programmatic(&self) -> bool {
        matches!(self, Self::Format | Self::Evidence)
    }

    pub fn requires_llm(&self) -> bool {
        !self.is_programmatic()
    }

    pub fn priority(&self) -> u8 {
        match self {
            Self::Format => 0,
            Self::Evidence => 1,
            Self::SemanticContext => 2,
            Self::ValueAssessment => 3,
            Self::CrossArtifact => 4,
        }
    }
}

impl std::fmt::Display for ValidationLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format => write!(f, "format"),
            Self::Evidence => write!(f, "evidence"),
            Self::SemanticContext => write!(f, "semantic_context"),
            Self::ValueAssessment => write!(f, "value_assessment"),
            Self::CrossArtifact => write!(f, "cross_artifact"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl IssueSeverity {
    pub fn resets_clean_pass(&self) -> bool {
        matches!(self, Self::Error | Self::Critical)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub layer: ValidationLayer,
    pub severity: IssueSeverity,
    pub artifact: String,
    pub code: IssueCode,
    pub message: String,
    pub location: Option<String>,
    pub suggestion: Option<String>,
}

impl ValidationIssue {
    pub fn new(
        layer: ValidationLayer,
        severity: IssueSeverity,
        artifact: impl Into<String>,
        code: IssueCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            layer,
            severity,
            artifact: artifact.into(),
            code,
            message: message.into(),
            location: None,
            suggestion: None,
        }
    }

    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn critical(
        layer: ValidationLayer,
        artifact: impl Into<String>,
        code: IssueCode,
        message: impl Into<String>,
    ) -> Self {
        Self::new(layer, IssueSeverity::Critical, artifact, code, message)
    }

    pub fn error(
        layer: ValidationLayer,
        artifact: impl Into<String>,
        code: IssueCode,
        message: impl Into<String>,
    ) -> Self {
        Self::new(layer, IssueSeverity::Error, artifact, code, message)
    }

    pub fn warning(
        layer: ValidationLayer,
        artifact: impl Into<String>,
        code: IssueCode,
        message: impl Into<String>,
    ) -> Self {
        Self::new(layer, IssueSeverity::Warning, artifact, code, message)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IssueCode {
    // Format (Layer 0)
    MissingFrontmatter,
    MissingRequiredField,
    InvalidYaml,
    InvalidStructure,

    // Evidence (Layer 1)
    FileNotFound,
    LineOutOfRange,
    InvalidReference,
    InsufficientReferences,

    // Semantic Context (Layer 2)
    ClaimContextMismatch,
    UnsupportedClaim,
    MisleadingReference,

    // Value Assessment (Layer 3)
    Tier1Content,
    LowMistakePrevention,
    LowDiscoverability,
    GenericContent,

    // Cross-Artifact (Layer 4)
    InconsistentDescription,
    MissingDependency,
    ConflictingInstructions,
    DuplicateContent,

    // LLM Validation
    LlmValidationFailed,
}

impl std::fmt::Display for IssueCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::MissingFrontmatter => "MISSING_FRONTMATTER",
            Self::MissingRequiredField => "MISSING_REQUIRED_FIELD",
            Self::InvalidYaml => "INVALID_YAML",
            Self::InvalidStructure => "INVALID_STRUCTURE",
            Self::FileNotFound => "FILE_NOT_FOUND",
            Self::LineOutOfRange => "LINE_OUT_OF_RANGE",
            Self::InvalidReference => "INVALID_REFERENCE",
            Self::InsufficientReferences => "INSUFFICIENT_REFERENCES",
            Self::ClaimContextMismatch => "CLAIM_CONTEXT_MISMATCH",
            Self::UnsupportedClaim => "UNSUPPORTED_CLAIM",
            Self::MisleadingReference => "MISLEADING_REFERENCE",
            Self::Tier1Content => "TIER1_CONTENT",
            Self::LowMistakePrevention => "LOW_MISTAKE_PREVENTION",
            Self::LowDiscoverability => "LOW_DISCOVERABILITY",
            Self::GenericContent => "GENERIC_CONTENT",
            Self::InconsistentDescription => "INCONSISTENT_DESCRIPTION",
            Self::MissingDependency => "MISSING_DEPENDENCY",
            Self::ConflictingInstructions => "CONFLICTING_INSTRUCTIONS",
            Self::DuplicateContent => "DUPLICATE_CONTENT",
            Self::LlmValidationFailed => "LLM_VALIDATION_FAILED",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Default)]
pub struct LayerResult {
    pub layer: Option<ValidationLayer>,
    pub passed: bool,
    pub score: f32,
    pub issues: Vec<ValidationIssue>,
    pub metadata: HashMap<String, String>,
}

impl LayerResult {
    pub fn pass(layer: ValidationLayer) -> Self {
        Self {
            layer: Some(layer),
            passed: true,
            score: 1.0,
            issues: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn fail(layer: ValidationLayer, issues: Vec<ValidationIssue>) -> Self {
        let score = Self::calculate_score(&issues);
        Self {
            layer: Some(layer),
            passed: false,
            score,
            issues,
            metadata: HashMap::new(),
        }
    }

    /// Set score (does NOT recalculate passed - use with_score_threshold for that)
    pub fn with_score(mut self, score: f32) -> Self {
        self.score = score;
        self
    }

    /// Set score with threshold-based pass determination
    pub fn with_score_threshold(mut self, score: f32, threshold: f32) -> Self {
        self.score = score;
        self.passed = score >= threshold && self.critical_count() == 0;
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Add issues to a pass result (e.g., warnings that don't cause failure)
    pub fn with_issues(mut self, issues: Vec<ValidationIssue>) -> Self {
        self.issues = issues;
        self
    }

    /// Add warning count method for completeness
    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .count()
    }

    fn calculate_score(issues: &[ValidationIssue]) -> f32 {
        if issues.is_empty() {
            return 1.0;
        }

        let critical = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Critical)
            .count();
        let errors = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .count();
        let warnings = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .count();

        let penalty = critical as f32 * 0.3 + errors as f32 * 0.15 + warnings as f32 * 0.05;
        (1.0 - penalty).clamp(0.0, 1.0)
    }

    pub fn critical_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Critical)
            .count()
    }

    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .count()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ValidationResults {
    pub layer_results: HashMap<ValidationLayer, LayerResult>,
    pub overall_passed: bool,
    pub overall_score: f32,
    pub total_issues: usize,
}

impl ValidationResults {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_layer_result(&mut self, result: LayerResult) {
        if let Some(layer) = result.layer {
            self.total_issues += result.issues.len();
            self.layer_results.insert(layer, result);
            self.recalculate();
        }
    }

    fn recalculate(&mut self) {
        if self.layer_results.is_empty() {
            self.overall_passed = true;
            self.overall_score = 1.0;
            return;
        }

        let total_weight: f32 = self
            .layer_results
            .keys()
            .map(|l| l.confidence())
            .sum();

        self.overall_score = self
            .layer_results
            .iter()
            .map(|(layer, result)| result.score * layer.confidence())
            .sum::<f32>()
            / total_weight;

        self.overall_passed = self.layer_results.values().all(|r| r.passed)
            && self.critical_issues() == 0
            && self.error_issues() == 0;
    }

    pub fn critical_issues(&self) -> usize {
        self.layer_results.values().map(|r| r.critical_count()).sum()
    }

    pub fn error_issues(&self) -> usize {
        self.layer_results.values().map(|r| r.error_count()).sum()
    }

    pub fn all_issues(&self) -> Vec<&ValidationIssue> {
        self.layer_results
            .values()
            .flat_map(|r| &r.issues)
            .collect()
    }

    pub fn issues_for_layer(&self, layer: ValidationLayer) -> Vec<&ValidationIssue> {
        self.layer_results
            .get(&layer)
            .map(|r| r.issues.iter().collect())
            .unwrap_or_default()
    }

    pub fn is_clean(&self) -> bool {
        self.total_issues == 0
    }

    pub fn get_layer_score(&self, layer: ValidationLayer) -> Option<f32> {
        self.layer_results.get(&layer).map(|r| r.score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_priority_order() {
        assert!(ValidationLayer::Format.priority() < ValidationLayer::Evidence.priority());
        assert!(ValidationLayer::Evidence.priority() < ValidationLayer::SemanticContext.priority());
    }

    #[test]
    fn test_layer_result_pass() {
        let result = LayerResult::pass(ValidationLayer::Format);
        assert!(result.passed);
        assert_eq!(result.score, 1.0);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_layer_result_fail_with_issues() {
        let issues = vec![
            ValidationIssue::error(
                ValidationLayer::Evidence,
                "skill:test",
                IssueCode::FileNotFound,
                "File not found",
            ),
        ];
        let result = LayerResult::fail(ValidationLayer::Evidence, issues);
        assert!(!result.passed);
        assert!(result.score < 1.0);
    }

    #[test]
    fn test_validation_results_clean() {
        let mut results = ValidationResults::new();
        results.add_layer_result(LayerResult::pass(ValidationLayer::Format));
        results.add_layer_result(LayerResult::pass(ValidationLayer::Evidence));
        assert!(results.is_clean());
        assert!(results.overall_passed);
    }

    #[test]
    fn test_validation_results_with_issues() {
        let mut results = ValidationResults::new();
        results.add_layer_result(LayerResult::pass(ValidationLayer::Format));

        let issues = vec![ValidationIssue::error(
            ValidationLayer::Evidence,
            "test",
            IssueCode::FileNotFound,
            "error",
        )];
        results.add_layer_result(LayerResult::fail(ValidationLayer::Evidence, issues));

        assert!(!results.is_clean());
        assert!(!results.overall_passed);
        assert_eq!(results.total_issues, 1);
    }

    #[test]
    fn test_severity_resets_clean_pass() {
        assert!(!IssueSeverity::Info.resets_clean_pass());
        assert!(!IssueSeverity::Warning.resets_clean_pass());
        assert!(IssueSeverity::Error.resets_clean_pass());
        assert!(IssueSeverity::Critical.resets_clean_pass());
    }
}
