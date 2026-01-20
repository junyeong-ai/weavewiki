//! Response Validation
//!
//! Validates LLM response structure and content quality.

use serde_json::Value;
use std::collections::HashSet;

use crate::types::ValidationIssue;

/// Response validation result with statistics
#[derive(Debug, Clone, Default)]
pub struct ResponseValidationResult {
    pub issues: Vec<ValidationIssue>,
    pub files_validated: usize,
    pub sections_validated: usize,
}

impl ResponseValidationResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.severity.is_error())
    }

    pub fn is_acceptable(&self) -> bool {
        self.is_valid()
    }

    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity.is_error())
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity.is_warning())
            .count()
    }

    fn add(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }
}

/// Response validator for batch analysis output
pub struct ResponseValidator {
    valid_complexities: HashSet<&'static str>,
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

    pub fn validate_batch_response(&self, response: &Value) -> ResponseValidationResult {
        let mut result = ResponseValidationResult::new();

        if !response.is_object() {
            result.add(ValidationIssue::error("RESP001", "Response must be a JSON object"));
            return result;
        }

        let files = match response.get("files") {
            Some(Value::Array(arr)) => arr,
            Some(_) => {
                result.add(ValidationIssue::error("RESP002", "'files' must be an array"));
                return result;
            }
            None => {
                result.add(ValidationIssue::error("RESP003", "Missing required 'files' field"));
                return result;
            }
        };

        if files.is_empty() {
            result.add(ValidationIssue::warning("RESP004", "'files' array is empty"));
            return result;
        }

        for (idx, file) in files.iter().enumerate() {
            self.validate_file_analysis(file, idx, &mut result);
        }

        if let Some(metadata) = response.get("analysis_metadata") {
            self.validate_metadata(metadata, files.len(), &mut result);
        }

        result
    }

    fn validate_file_analysis(
        &self,
        file: &Value,
        idx: usize,
        result: &mut ResponseValidationResult,
    ) {
        let location = format!("files[{idx}]");

        match file.get("path") {
            Some(Value::String(s)) if !s.is_empty() => {}
            Some(Value::String(_)) => {
                result.add(
                    ValidationIssue::error("FILE001", "Empty file path").with_location(&location),
                );
            }
            _ => {
                result.add(
                    ValidationIssue::error("FILE002", "Missing or invalid 'path'")
                        .with_location(&location),
                );
            }
        }

        let file_path = file
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        match file.get("sections") {
            Some(Value::Array(sections)) => {
                for (sec_idx, section) in sections.iter().enumerate() {
                    self.validate_section(section, sec_idx, file_path, result);
                    result.sections_validated += 1;
                }
            }
            _ => {
                result.add(
                    ValidationIssue::error("FILE003", "Missing or invalid 'sections' array")
                        .with_location(format!("files[{idx}].sections")),
                );
            }
        }

        if let Some(complexity) = file.get("complexity").and_then(|v| v.as_str())
            && !self.valid_complexities.contains(complexity)
        {
            result.add(
                ValidationIssue::warning(
                    "FILE004",
                    format!(
                        "Invalid complexity '{complexity}', expected: low, medium, high, critical"
                    ),
                )
                .with_location(format!("files[{idx}].complexity")),
            );
        }

        if let Some(confidence) = file.get("confidence")
            && let Some(c) = confidence.as_f64()
            && !(0.0..=1.0).contains(&c)
        {
            result.add(
                ValidationIssue::warning("FILE005", format!("Confidence {c} out of range [0.0, 1.0]"))
                    .with_location(format!("files[{idx}].confidence")),
            );
        }

        if file
            .get("purpose_summary")
            .and_then(|v| v.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            result.add(
                ValidationIssue::info("FILE006", "Missing purpose_summary")
                    .with_location(format!("files[{idx}]")),
            );
        }

        if file
            .get("key_insights")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
        {
            result.add(
                ValidationIssue::info("FILE007", "No key_insights provided")
                    .with_location(format!("files[{idx}]")),
            );
        }

        if file
            .get("hidden_assumptions")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
        {
            result.add(
                ValidationIssue::info("FILE008", "No hidden_assumptions identified")
                    .with_location(format!("files[{idx}]")),
            );
        }

        if file
            .get("modification_risks")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
        {
            result.add(
                ValidationIssue::info("FILE009", "No modification_risks identified")
                    .with_location(format!("files[{idx}]")),
            );
        }

        result.files_validated += 1;
    }

    fn validate_section(
        &self,
        section: &Value,
        idx: usize,
        file_path: &str,
        result: &mut ResponseValidationResult,
    ) {
        let location = format!("{file_path}:sections[{idx}]");

        if section
            .get("section_name")
            .and_then(|v| v.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            result.add(
                ValidationIssue::warning("SEC001", "Missing or empty section_name")
                    .with_location(&location),
            );
        }

        if section.get("content").is_none() {
            result.add(
                ValidationIssue::warning("SEC002", "Missing content field").with_location(&location),
            );
        }

        match section.get("evidence_lines") {
            Some(Value::Array(lines)) if lines.is_empty() => {
                result.add(
                    ValidationIssue::warning(
                        "SEC003",
                        "Empty evidence_lines - sections should cite line numbers",
                    )
                    .with_location(&location),
                );
            }
            Some(Value::Array(lines)) => {
                for (line_idx, line) in lines.iter().enumerate() {
                    match line.as_u64() {
                        Some(0) => {
                            result.add(
                                ValidationIssue::warning(
                                    "SEC004",
                                    "Line number 0 is invalid (lines start at 1)",
                                )
                                .with_location(format!("{location}:evidence_lines[{line_idx}]")),
                            );
                        }
                        None if !line.is_u64() => {
                            result.add(
                                ValidationIssue::warning(
                                    "SEC005",
                                    format!("Invalid line number: {line:?}"),
                                )
                                .with_location(format!("{location}:evidence_lines[{line_idx}]")),
                            );
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                result.add(
                    ValidationIssue::warning(
                        "SEC006",
                        "Missing evidence_lines - all claims need evidence",
                    )
                    .with_location(&location),
                );
            }
        }

        if let Some(importance) = section.get("importance").and_then(|v| v.as_str())
            && !self.valid_importances.contains(importance)
        {
            result.add(
                ValidationIssue::info(
                    "SEC007",
                    format!(
                        "Non-standard importance '{importance}', expected: critical, high, medium, low"
                    ),
                )
                .with_location(&location),
            );
        }
    }

    fn validate_metadata(
        &self,
        metadata: &Value,
        file_count: usize,
        result: &mut ResponseValidationResult,
    ) {
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

            result.add(ValidationIssue::warning(
                "META001",
                format!("Incomplete coverage: {analyzed}/{input} files analyzed"),
            ));
        }

        if let Some(analyzed) = metadata.get("files_analyzed").and_then(|v| v.as_u64())
            && analyzed as usize != file_count
        {
            result.add(ValidationIssue::info(
                "META002",
                format!("Metadata claims {analyzed} files analyzed but {file_count} in response"),
            ));
        }

        if let Some(low_conf) = metadata
            .get("low_confidence_files")
            .and_then(|v| v.as_array())
        {
            for file in low_conf {
                if let Some(path) = file.as_str() {
                    result.add(ValidationIssue::info(
                        "META003",
                        format!("Low confidence analysis: {path}"),
                    ));
                }
            }
        }
    }

    pub fn validate_coverage(
        &self,
        response: &Value,
        expected_paths: &[String],
    ) -> ResponseValidationResult {
        let mut result = ResponseValidationResult::new();

        let files = match response.get("files").and_then(|v| v.as_array()) {
            Some(f) => f,
            None => {
                result.add(ValidationIssue::error("COV001", "No files array in response"));
                return result;
            }
        };

        let response_paths: HashSet<String> = files
            .iter()
            .filter_map(|f| f.get("path").and_then(|v| v.as_str()))
            .map(normalize_path)
            .collect();

        for expected in expected_paths {
            let normalized = normalize_path(expected);
            if !response_paths.contains(&normalized) {
                result.add(ValidationIssue::warning(
                    "COV002",
                    format!("Missing analysis for: {expected}"),
                ));
            }
        }

        let expected_normalized: HashSet<String> =
            expected_paths.iter().map(|p| normalize_path(p)).collect();

        for path in &response_paths {
            if !expected_normalized.contains(path) {
                result.add(ValidationIssue::info(
                    "COV003",
                    format!("Extra file in response: {path}"),
                ));
            }
        }

        result
    }
}

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
        assert!(result.is_valid());
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
        assert!(result
            .issues
            .iter()
            .any(|i| i.message.contains("Missing analysis for") && i.message.contains("c.rs")));
    }
}
