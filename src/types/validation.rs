use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

impl DiagnosticLevel {
    pub fn is_error(&self) -> bool {
        matches!(self, DiagnosticLevel::Error)
    }

    pub fn is_warning(&self) -> bool {
        matches!(self, DiagnosticLevel::Warning)
    }
}

impl fmt::Display for DiagnosticLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagnosticLevel::Error => write!(f, "ERROR"),
            DiagnosticLevel::Warning => write!(f, "WARN"),
            DiagnosticLevel::Info => write!(f, "INFO"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: DiagnosticLevel,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

impl ValidationIssue {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticLevel::Error,
            code: code.into(),
            message: message.into(),
            location: None,
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticLevel::Warning,
            code: code.into(),
            message: message.into(),
            location: None,
        }
    }

    pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticLevel::Info,
            code: code.into(),
            message: message.into(),
            location: None,
        }
    }

    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity.is_error()
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(loc) = &self.location {
            write!(
                f,
                "[{}] {}: {} ({})",
                self.severity, self.code, self.message, loc
            )
        } else {
            write!(f, "[{}] {}: {}", self.severity, self.code, self.message)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationResult {
    issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self { issues: Vec::new() }
    }

    pub fn add(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }

    pub fn extend(&mut self, issues: impl IntoIterator<Item = ValidationIssue>) {
        self.issues.extend(issues);
    }

    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.is_error())
    }

    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.is_error())
    }

    pub fn has_warnings(&self) -> bool {
        self.issues.iter().any(|i| i.severity.is_warning())
    }

    pub fn error_count(&self) -> usize {
        self.issues.iter().filter(|i| i.is_error()).count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity.is_warning())
            .count()
    }

    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }

    pub fn into_issues(self) -> Vec<ValidationIssue> {
        self.issues
    }
}

impl IntoIterator for ValidationResult {
    type Item = ValidationIssue;
    type IntoIter = std::vec::IntoIter<ValidationIssue>;

    fn into_iter(self) -> Self::IntoIter {
        self.issues.into_iter()
    }
}

impl<'a> IntoIterator for &'a ValidationResult {
    type Item = &'a ValidationIssue;
    type IntoIter = std::slice::Iter<'a, ValidationIssue>;

    fn into_iter(self) -> Self::IntoIter {
        self.issues.iter()
    }
}

impl FromIterator<ValidationIssue> for ValidationResult {
    fn from_iter<T: IntoIterator<Item = ValidationIssue>>(iter: T) -> Self {
        Self {
            issues: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_level_ordering() {
        assert!(DiagnosticLevel::Error > DiagnosticLevel::Warning);
        assert!(DiagnosticLevel::Warning > DiagnosticLevel::Info);
    }

    #[test]
    fn test_validation_result() {
        let mut result = ValidationResult::new();
        assert!(result.is_valid());

        result.add(ValidationIssue::warning("WARN", "warning"));
        assert!(result.is_valid());
        assert!(result.has_warnings());

        result.add(ValidationIssue::error("ERR", "error"));
        assert!(!result.is_valid());
        assert!(result.has_errors());
    }
}
