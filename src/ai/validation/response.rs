//! Response Validation
//!
//! Validates LLM response structure and content quality:
//! - Required fields presence
//! - Valid enum values
//! - Evidence line numbers exist
//! - Path matching with input files
//! - Section structure integrity
//!
//! Based on CodeWiki's post-processing validation pattern.

use serde_json::Value;
use std::collections::HashSet;
use std::fmt;

/// Severity levels for validation issues
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Critical error - response is unusable
    Error,
    /// Warning - response usable but degraded quality
    Warning,
    /// Info - observation that doesn't affect usability
    Info,
}

impl fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IssueSeverity::Error => write!(f, "ERROR"),
            IssueSeverity::Warning => write!(f, "WARN"),
            IssueSeverity::Info => write!(f, "INFO"),
        }
    }
}

/// A single validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub message: String,
    pub location: Option<String>,
}

impl ValidationIssue {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            message: message.into(),
            location: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            message: message.into(),
            location: None,
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Info,
            message: message.into(),
            location: None,
        }
    }

    pub fn at(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }
}

/// Validation result containing all issues found
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub issues: Vec<ValidationIssue>,
    pub files_validated: usize,
    pub sections_validated: usize,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            issues: Vec::new(),
            files_validated: 0,
            sections_validated: 0,
        }
    }

    /// Check if response is valid (no errors)
    pub fn is_valid(&self) -> bool {
        !self
            .issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error)
    }

    /// Check if response is acceptable (no errors, warnings are OK)
    pub fn is_acceptable(&self) -> bool {
        self.is_valid()
    }

    /// Count errors
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .count()
    }

    /// Count warnings
    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .count()
    }

    fn add(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Response validator for batch analysis output
pub struct ResponseValidator {
    /// Valid complexity values
    valid_complexities: HashSet<&'static str>,
    /// Valid importance values
    valid_importances: HashSet<&'static str>,
}

impl Default for ResponseValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseValidator {
    pub fn new() -> Self {
        Self {
            valid_complexities: ["low", "medium", "high", "critical"].into_iter().collect(),
            valid_importances: ["critical", "high", "medium", "low"].into_iter().collect(),
        }
    }

    /// Validate batch analysis response
    pub fn validate_batch_response(&self, response: &Value) -> ValidationResult {
        let mut result = ValidationResult::new();

        // Check top-level structure
        if !response.is_object() {
            result.add(ValidationIssue::error("Response must be a JSON object"));
            return result;
        }

        // Check files array
        let files = match response.get("files") {
            Some(Value::Array(arr)) => arr,
            Some(_) => {
                result.add(ValidationIssue::error("'files' must be an array"));
                return result;
            }
            None => {
                result.add(ValidationIssue::error("Missing required 'files' field"));
                return result;
            }
        };

        if files.is_empty() {
            result.add(ValidationIssue::warning("'files' array is empty"));
            return result;
        }

        // Validate each file
        for (idx, file) in files.iter().enumerate() {
            self.validate_file_analysis(file, idx, &mut result);
        }

        // Validate metadata if present
        if let Some(metadata) = response.get("analysis_metadata") {
            self.validate_metadata(metadata, files.len(), &mut result);
        }

        result
    }

    /// Validate a single file analysis
    fn validate_file_analysis(&self, file: &Value, idx: usize, result: &mut ValidationResult) {
        let location = format!("files[{}]", idx);

        // Required: path
        match file.get("path") {
            Some(Value::String(s)) if !s.is_empty() => {}
            Some(Value::String(_)) => {
                result.add(ValidationIssue::error("Empty file path").at(&location));
            }
            _ => {
                result.add(ValidationIssue::error("Missing or invalid 'path'").at(&location));
            }
        }

        let file_path = file
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Required: sections
        match file.get("sections") {
            Some(Value::Array(sections)) => {
                for (sec_idx, section) in sections.iter().enumerate() {
                    self.validate_section(section, sec_idx, file_path, result);
                    result.sections_validated += 1;
                }
            }
            _ => {
                result.add(
                    ValidationIssue::error("Missing or invalid 'sections' array")
                        .at(format!("files[{}].sections", idx)),
                );
            }
        }

        // Optional with validation: complexity
        if let Some(complexity) = file.get("complexity").and_then(|v| v.as_str())
            && !self.valid_complexities.contains(complexity)
        {
            result.add(
                ValidationIssue::warning(format!(
                    "Invalid complexity '{}', expected one of: low, medium, high, critical",
                    complexity
                ))
                .at(format!("files[{}].complexity", idx)),
            );
        }

        // Optional with validation: confidence
        if let Some(confidence) = file.get("confidence")
            && let Some(c) = confidence.as_f64()
            && !(0.0..=1.0).contains(&c)
        {
            result.add(
                ValidationIssue::warning(format!("Confidence {} out of range [0.0, 1.0]", c))
                    .at(format!("files[{}].confidence", idx)),
            );
        }

        // Quality indicators (warnings if missing)
        if file
            .get("purpose_summary")
            .and_then(|v| v.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            result.add(
                ValidationIssue::info("Missing purpose_summary").at(format!("files[{}]", idx)),
            );
        }

        if file
            .get("key_insights")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
        {
            result.add(
                ValidationIssue::info("No key_insights provided").at(format!("files[{}]", idx)),
            );
        }

        // v2.0 fields - check for completeness
        if file
            .get("hidden_assumptions")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
        {
            result.add(
                ValidationIssue::info("No hidden_assumptions identified")
                    .at(format!("files[{}]", idx)),
            );
        }

        if file
            .get("modification_risks")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
        {
            result.add(
                ValidationIssue::info("No modification_risks identified")
                    .at(format!("files[{}]", idx)),
            );
        }

        result.files_validated += 1;
    }

    /// Validate a single section
    fn validate_section(
        &self,
        section: &Value,
        idx: usize,
        file_path: &str,
        result: &mut ValidationResult,
    ) {
        let location = format!("{}:sections[{}]", file_path, idx);

        // Required: section_name
        if section
            .get("section_name")
            .and_then(|v| v.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            result.add(ValidationIssue::warning("Missing or empty section_name").at(&location));
        }

        // Required: content
        if section.get("content").is_none() {
            result.add(ValidationIssue::warning("Missing content field").at(&location));
        }

        // Critical: evidence_lines (this is NON-NEGOTIABLE per prompt)
        match section.get("evidence_lines") {
            Some(Value::Array(lines)) if lines.is_empty() => {
                result.add(
                    ValidationIssue::warning(
                        "Empty evidence_lines - sections should cite line numbers",
                    )
                    .at(&location),
                );
            }
            Some(Value::Array(lines)) => {
                // Validate line numbers are positive integers
                for (line_idx, line) in lines.iter().enumerate() {
                    match line.as_u64() {
                        Some(0) => {
                            result.add(
                                ValidationIssue::warning(
                                    "Line number 0 is invalid (lines start at 1)",
                                )
                                .at(format!("{}:evidence_lines[{}]", location, line_idx)),
                            );
                        }
                        None if !line.is_u64() => {
                            result.add(
                                ValidationIssue::warning(format!(
                                    "Invalid line number: {:?}",
                                    line
                                ))
                                .at(format!("{}:evidence_lines[{}]", location, line_idx)),
                            );
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                result.add(
                    ValidationIssue::warning("Missing evidence_lines - all claims need evidence")
                        .at(&location),
                );
            }
        }

        // Optional with validation: importance
        if let Some(importance) = section.get("importance").and_then(|v| v.as_str())
            && !self.valid_importances.contains(importance)
        {
            result.add(
                ValidationIssue::info(format!(
                    "Non-standard importance '{}', expected: critical, high, medium, low",
                    importance
                ))
                .at(&location),
            );
        }
    }

    /// Validate analysis metadata
    fn validate_metadata(
        &self,
        metadata: &Value,
        file_count: usize,
        result: &mut ValidationResult,
    ) {
        // Check coverage completeness
        if let Some(complete) = metadata.get("coverage_complete").and_then(|v| v.as_bool())
            && !complete
        {
            let analyzed = metadata
                .get("files_analyzed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let input = metadata
                .get("files_in_input")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            result.add(ValidationIssue::warning(format!(
                "Incomplete coverage: {}/{} files analyzed",
                analyzed, input
            )));
        }

        // Check for consistency
        if let Some(analyzed) = metadata.get("files_analyzed").and_then(|v| v.as_u64())
            && analyzed as usize != file_count
        {
            result.add(ValidationIssue::info(format!(
                "Metadata claims {} files analyzed but {} in response",
                analyzed, file_count
            )));
        }

        // Report low confidence files
        if let Some(low_conf) = metadata
            .get("low_confidence_files")
            .and_then(|v| v.as_array())
        {
            for file in low_conf {
                if let Some(path) = file.as_str() {
                    result.add(ValidationIssue::info(format!(
                        "Low confidence analysis: {}",
                        path
                    )));
                }
            }
        }
    }

    /// Validate response against expected file paths
    pub fn validate_coverage(
        &self,
        response: &Value,
        expected_paths: &[String],
    ) -> ValidationResult {
        let mut result = ValidationResult::new();

        let files = match response.get("files").and_then(|v| v.as_array()) {
            Some(f) => f,
            None => {
                result.add(ValidationIssue::error("No files array in response"));
                return result;
            }
        };

        // Collect response paths (normalized)
        let response_paths: HashSet<String> = files
            .iter()
            .filter_map(|f| f.get("path").and_then(|v| v.as_str()))
            .map(normalize_path)
            .collect();

        // Check for missing files
        for expected in expected_paths {
            let normalized = normalize_path(expected);
            if !response_paths.contains(&normalized) {
                result.add(ValidationIssue::warning(format!(
                    "Missing analysis for: {}",
                    expected
                )));
            }
        }

        // Check for extra files
        let expected_normalized: HashSet<String> =
            expected_paths.iter().map(|p| normalize_path(p)).collect();

        for path in &response_paths {
            if !expected_normalized.contains(path) {
                result.add(ValidationIssue::info(format!(
                    "Extra file in response: {}",
                    path
                )));
            }
        }

        result
    }
}

/// Normalize file path for comparison
fn normalize_path(path: &str) -> String {
    path.trim()
        .trim_start_matches("./")
        .trim_start_matches("../")
        .trim_start_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_valid_response() {
        let validator = ResponseValidator::new();

        let response = json!({
            "files": [{
                "path": "src/main.rs",
                "sections": [{
                    "section_name": "Entry Point",
                    "content": {"summary": "Main function"},
                    "evidence_lines": [1, 5]
                }]
            }]
        });

        let result = validator.validate_batch_response(&response);
        assert!(result.is_valid());
    }

    #[test]
    fn test_missing_files_field() {
        let validator = ResponseValidator::new();
        let response = json!({"data": []});

        let result = validator.validate_batch_response(&response);
        assert!(!result.is_valid());
        assert!(result.error_count() > 0);
    }

    #[test]
    fn test_empty_evidence_warning() {
        let validator = ResponseValidator::new();

        let response = json!({
            "files": [{
                "path": "test.rs",
                "sections": [{
                    "section_name": "Test",
                    "content": {},
                    "evidence_lines": []
                }]
            }]
        });

        let result = validator.validate_batch_response(&response);
        assert!(result.is_valid()); // Warnings don't make it invalid
        assert!(result.warning_count() > 0);
    }

    #[test]
    fn test_invalid_complexity() {
        let validator = ResponseValidator::new();

        let response = json!({
            "files": [{
                "path": "test.rs",
                "complexity": "super-complex",
                "sections": []
            }]
        });

        let result = validator.validate_batch_response(&response);
        assert!(result.warning_count() > 0);
    }

    #[test]
    fn test_coverage_validation() {
        let validator = ResponseValidator::new();

        let response = json!({
            "files": [
                {"path": "src/a.rs", "sections": []},
                {"path": "src/b.rs", "sections": []}
            ]
        });

        let expected = vec![
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
            "src/c.rs".to_string(),
        ];

        let result = validator.validate_coverage(&response, &expected);
        // Should warn about missing c.rs
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.message.contains("Missing analysis for") && i.message.contains("c.rs"))
        );
    }
}
