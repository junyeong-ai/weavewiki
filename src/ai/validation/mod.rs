//! AI Response Validation and Quality Assurance
//!
//! Comprehensive validation layer for LLM responses ensuring:
//! - Structural integrity (required fields, valid enums)
//! - JSON repair for malformed responses
//! - Mermaid diagram syntax validation (CodeWiki-style strict checking)
//!
//! ## Design Philosophy
//! - Fail fast on structural errors, repair on format issues
//! - Strict diagram validation: fail loudly on invalid diagrams

mod diagram;
mod json_repair;
mod response;
mod scoring;

pub use diagram::{
    DiagramError, DiagramValidation, DiagramValidator, DiagramWarning, is_valid_mermaid,
    validate_mermaid,
};
pub use json_repair::{JsonRepairer, extract_json_from_response, extract_json_with_repair_status};
pub use response::{IssueSeverity, ResponseValidator, ValidationIssue, ValidationResult};
pub use scoring::{ConfidenceScorer, QualityMetrics, ScoringConfig};

use crate::types::Result;
use serde_json::Value;

/// Unified validation pipeline combining all validation steps
pub struct ValidationPipeline {
    repairer: JsonRepairer,
    validator: ResponseValidator,
    scorer: ConfidenceScorer,
}

impl Default for ValidationPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationPipeline {
    pub fn new() -> Self {
        Self {
            repairer: JsonRepairer::new(),
            validator: ResponseValidator::new(),
            scorer: ConfidenceScorer::new(),
        }
    }

    /// Configure scoring parameters
    pub fn with_scoring(mut self, config: ScoringConfig) -> Self {
        self.scorer = ConfidenceScorer::with_config(config);
        self
    }

    /// Process raw LLM response through full validation pipeline
    ///
    /// Steps:
    /// 1. Attempt JSON repair if malformed
    /// 2. Validate structure and required fields
    /// 3. Calculate confidence scores with penalties
    /// 4. Return validated and scored response
    pub fn process(&self, raw_response: &str) -> Result<ProcessedResponse> {
        // Step 1: Parse/repair JSON
        let (value, was_repaired) = self.repairer.parse_or_repair(raw_response)?;

        // Step 2: Validate structure
        let validation = self.validator.validate_batch_response(&value);

        // Step 3: Calculate scores
        let quality = self.scorer.calculate_quality(&value);

        Ok(ProcessedResponse {
            value,
            was_repaired,
            validation,
            quality,
        })
    }

    /// Quick validation check without full processing
    pub fn validate_only(&self, value: &Value) -> ValidationResult {
        self.validator.validate_batch_response(value)
    }
}

/// Result of full validation pipeline
#[derive(Debug)]
pub struct ProcessedResponse {
    /// Parsed (and possibly repaired) JSON value
    pub value: Value,
    /// Whether JSON repair was needed
    pub was_repaired: bool,
    /// Validation result with issues
    pub validation: ValidationResult,
    /// Quality metrics
    pub quality: QualityMetrics,
}

impl ProcessedResponse {
    /// Check if response is usable (may have warnings but no errors)
    pub fn is_usable(&self) -> bool {
        self.validation.is_acceptable()
    }

    /// Get overall confidence score
    pub fn confidence(&self) -> f32 {
        self.quality.overall_confidence
    }

    /// Get list of all issues (validation + quality)
    pub fn all_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();

        for issue in &self.validation.issues {
            issues.push(format!("[{}] {}", issue.severity, issue.message));
        }

        for warning in &self.quality.warnings {
            issues.push(format!("[QUALITY] {}", warning));
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_valid_response() {
        let pipeline = ValidationPipeline::new();

        let valid_json = r#"{
            "files": [{
                "path": "src/main.rs",
                "language": "rust",
                "purpose_summary": "Application entry point",
                "complexity": "medium",
                "confidence": 0.9,
                "sections": [{
                    "section_name": "Main Function",
                    "section_type": "behavior",
                    "importance": "high",
                    "content": {"summary": "Entry point"},
                    "evidence_lines": [1, 5, 10]
                }],
                "key_insights": ["Handles CLI args"],
                "hidden_assumptions": ["Requires config file"],
                "modification_risks": ["Breaking changes to CLI"]
            }]
        }"#;

        let result = pipeline.process(valid_json).unwrap();
        assert!(result.is_usable());
        assert!(!result.was_repaired);
    }

    #[test]
    fn test_pipeline_repairs_json() {
        let pipeline = ValidationPipeline::new();

        // Missing closing brace
        let malformed = r#"{"files": [{"path": "test.rs", "sections": []}]"#;

        let result = pipeline.process(malformed).unwrap();
        assert!(result.was_repaired);
    }
}
