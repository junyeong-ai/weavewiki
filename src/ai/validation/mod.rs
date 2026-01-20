//! AI Response Validation and Quality Assurance
//!
//! Comprehensive validation layer for LLM responses.

mod diagram;
mod json_repair;
mod response;

pub use diagram::{
    DiagramError, DiagramValidation, DiagramValidator, DiagramWarning, is_valid_mermaid,
    validate_mermaid,
};
pub use json_repair::{JsonRepairer, extract_json_from_response, extract_json_with_repair_status};
pub use response::{ResponseValidationResult, ResponseValidator};

use crate::types::Result;
use serde_json::Value;

/// Unified validation pipeline combining all validation steps
pub struct ValidationPipeline {
    repairer: JsonRepairer,
    validator: ResponseValidator,
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
        }
    }

    pub fn process(&self, raw_response: &str) -> Result<ProcessedResponse> {
        let (value, was_repaired) = self.repairer.parse_or_repair(raw_response)?;
        let validation = self.validator.validate_batch_response(&value);

        Ok(ProcessedResponse {
            value,
            was_repaired,
            validation,
        })
    }

    pub fn validate_only(&self, value: &Value) -> ResponseValidationResult {
        self.validator.validate_batch_response(value)
    }
}

#[derive(Debug)]
pub struct ProcessedResponse {
    pub value: Value,
    pub was_repaired: bool,
    pub validation: ResponseValidationResult,
}

impl ProcessedResponse {
    pub fn is_usable(&self) -> bool {
        self.validation.is_acceptable()
    }

    pub fn all_issues(&self) -> Vec<String> {
        self.validation
            .issues
            .iter()
            .map(|issue| format!("[{}] {}: {}", issue.severity, issue.code, issue.message))
            .collect()
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
        let malformed = r#"{"files": [{"path": "test.rs", "sections": []}]"#;

        let result = pipeline.process(malformed).unwrap();
        assert!(result.was_repaired);
    }
}
