//! Claude Code Rule types - Official spec compliant
//!
//! Rules are modular, topic-specific project instructions stored in `.claude/rules/`

use super::node::EvidenceLocation;
use super::utils::is_kebab_case;
use super::validation::ValidationIssue;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    pub content: Vec<String>,
    #[serde(skip)]
    pub evidence: Vec<EvidenceLocation>,
}

impl Rule {
    pub fn new(name: impl Into<String>, content: Vec<String>) -> Self {
        Self {
            name: name.into(),
            paths: None,
            content,
            evidence: Vec::new(),
        }
    }

    pub fn with_paths(mut self, paths: Vec<String>) -> Self {
        self.paths = Some(paths);
        self
    }

    pub fn with_evidence(mut self, evidence: Vec<EvidenceLocation>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn to_markdown(&self) -> String {
        let mut output = String::new();

        if let Some(paths) = &self.paths {
            output.push_str("---\n");
            output.push_str("paths:\n");
            for path in paths {
                output.push_str(&format!("  - \"{path}\"\n"));
            }
            output.push_str("---\n\n");
        }

        // Content is already formatted markdown, join directly
        output.push_str(&self.content.join("\n"));

        output
    }

    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if self.name.is_empty() {
            issues.push(ValidationIssue::error("RULE_NAME_REQUIRED", "rule name is required"));
        }

        if !is_kebab_case(&self.name) {
            issues.push(ValidationIssue::error(
                "RULE_NAME_INVALID",
                format!("name '{}' must be kebab-case", self.name),
            ));
        }

        if self.content.is_empty() {
            issues.push(ValidationIssue::warning(
                "RULE_CONTENT_EMPTY",
                "rule has no content",
            ));
        }

        if let Some(paths) = &self.paths {
            for path in paths {
                if !path.contains('*') && !path.contains('/') {
                    issues.push(ValidationIssue::warning(
                        "RULE_PATH_SIMPLE",
                        format!("path '{path}' should be a glob pattern"),
                    ));
                }
            }
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_to_markdown_with_paths() {
        let rule = Rule::new(
            "error-handling",
            vec![
                "# Error Handling".to_string(),
                "".to_string(),
                "- Use `?` operator for error propagation".to_string(),
                "- Custom errors via `ClaudegenError`".to_string(),
            ],
        )
        .with_paths(vec!["src/**/*.rs".to_string()]);

        let md = rule.to_markdown();
        assert!(md.contains("---"));
        assert!(md.contains("paths:"));
        assert!(md.contains("src/**/*.rs"));
        assert!(md.contains("# Error Handling"));
        assert!(md.contains("- Use `?` operator"));
    }

    #[test]
    fn test_rule_to_markdown_global() {
        let rule = Rule::new(
            "testing",
            vec![
                "# Testing".to_string(),
                "".to_string(),
                "Run tests before commit".to_string(),
            ],
        );

        let md = rule.to_markdown();
        assert!(!md.contains("---"));
        assert!(md.contains("# Testing"));
    }

    #[test]
    fn test_rule_validation() {
        let rule = Rule::new("INVALID", vec![]);
        let issues = rule.validate();
        assert!(issues.iter().any(|i| i.code == "RULE_NAME_INVALID"));
        assert!(issues.iter().any(|i| i.code == "RULE_CONTENT_EMPTY"));
    }
}
