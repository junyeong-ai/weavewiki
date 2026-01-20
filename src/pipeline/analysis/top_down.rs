//! Top-Down Convention Validation Module
//!
//! Validates that inferred conventions match the actual project structure.
//! This provides a cross-check between bottom-up code reading and
//! top-down architectural inference.

use std::collections::HashSet;

use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::phases::convention_inference::{
    ArchitectureConvention, FileOrganization, InferredConventions,
};
use crate::pipeline::phases::project_detection::ProjectDetection;

/// Result of convention validation
#[derive(Debug, Clone, Default)]
pub struct ConventionValidationResult {
    /// Whether conventions are consistent with project structure
    pub passed: bool,
    /// Issues found during validation
    pub issues: Vec<ConventionIssue>,
    /// Confidence in the inferred conventions (0.0 - 1.0)
    pub confidence: f32,
    /// Suggestions for improving convention inference
    pub suggestions: Vec<String>,
}

/// Issue found during convention validation
#[derive(Debug, Clone)]
pub struct ConventionIssue {
    pub severity: ValidationSeverity,
    pub category: ConventionCategory,
    pub description: String,
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSeverity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConventionCategory {
    /// Architecture pattern doesn't match directory structure
    ArchitectureMismatch,
    /// Layers described don't exist in the project
    MissingLayers,
    /// Naming conventions don't match actual file names
    NamingInconsistency,
    /// File organization doesn't match claimed structure
    OrganizationMismatch,
}

/// Validator for conventions against project structure
pub struct ConventionValidator {
    file_registry: VerifiedFileRegistry,
}

impl ConventionValidator {
    pub fn new(file_registry: VerifiedFileRegistry) -> Self {
        Self { file_registry }
    }

    /// Validate conventions against project structure
    pub fn validate(
        &self,
        conventions: &InferredConventions,
        detection: &ProjectDetection,
    ) -> ConventionValidationResult {
        let mut issues = Vec::new();
        let mut suggestions = Vec::new();
        let mut confidence = 1.0f32;

        // Validate architecture layers exist
        let layer_issues = self.validate_architecture_layers(&conventions.architecture);
        confidence -= layer_issues.len() as f32 * 0.1;
        issues.extend(layer_issues);

        // Validate file organization
        let org_issues = self.validate_file_organization(&conventions.file_organization);
        confidence -= org_issues.len() as f32 * 0.05;
        issues.extend(org_issues);

        // Validate naming conventions match actual files
        let naming_issues = self.validate_naming_conventions(conventions);
        confidence -= naming_issues.len() as f32 * 0.05;
        issues.extend(naming_issues);

        // Check if architecture matches detected project type
        let type_issues = self.validate_architecture_vs_type(&conventions.architecture, detection);
        confidence -= type_issues.len() as f32 * 0.1;
        issues.extend(type_issues);

        // Generate suggestions based on issues
        for issue in &issues {
            match issue.category {
                ConventionCategory::MissingLayers => {
                    suggestions.push(format!(
                        "Layer '{}' may be misidentified - consider re-analyzing",
                        issue.description
                    ));
                }
                ConventionCategory::ArchitectureMismatch => {
                    suggestions.push(
                        "Architecture pattern may need refinement based on actual structure"
                            .to_string(),
                    );
                }
                _ => {}
            }
        }

        let has_critical = issues
            .iter()
            .any(|i| matches!(i.severity, ValidationSeverity::Critical));

        ConventionValidationResult {
            passed: !has_critical && confidence >= 0.5,
            issues,
            confidence: confidence.max(0.0),
            suggestions,
        }
    }

    /// Validate that architecture layers actually exist in the project
    fn validate_architecture_layers(&self, architecture: &ArchitectureConvention) -> Vec<ConventionIssue> {
        let mut issues = Vec::new();

        for layer in &architecture.layers {
            let path_pattern = &layer.path_pattern;

            // Check if any files match this layer's path
            let matching_files = self.file_registry.files_matching(path_pattern);

            if matching_files.is_empty() {
                // Layer doesn't have any files - might be wrong
                let dir_exists = self.file_registry.directory_exists(path_pattern);

                if !dir_exists {
                    issues.push(ConventionIssue {
                        severity: ValidationSeverity::Warning,
                        category: ConventionCategory::MissingLayers,
                        description: format!(
                            "Layer '{}' path '{}' has no matching files",
                            layer.name, path_pattern
                        ),
                        evidence: None,
                    });
                }
            }
        }

        issues
    }

    /// Validate file organization matches claimed structure
    fn validate_file_organization(&self, organization: &FileOrganization) -> Vec<ConventionIssue> {
        let mut issues = Vec::new();

        for dir_role in &organization.key_directories {
            // Check if the directory exists
            let dir_exists = self.file_registry.directory_exists(&dir_role.path);
            let files_exist = !self.file_registry.files_matching(&dir_role.path).is_empty();

            if !dir_exists && !files_exist {
                issues.push(ConventionIssue {
                    severity: ValidationSeverity::Info,
                    category: ConventionCategory::OrganizationMismatch,
                    description: format!(
                        "Key directory '{}' ({}) not found in project",
                        dir_role.path, dir_role.role
                    ),
                    evidence: None,
                });
            }
        }

        issues
    }

    /// Validate naming conventions against actual file names
    fn validate_naming_conventions(&self, conventions: &InferredConventions) -> Vec<ConventionIssue> {
        let mut issues = Vec::new();

        // Check file naming patterns
        if !conventions.naming.file_naming.suffix_patterns.is_empty() {
            let suffix_patterns: HashSet<_> = conventions
                .naming
                .file_naming
                .suffix_patterns
                .iter()
                .map(|p| p.suffix.as_str())
                .collect();

            // Sample some files and check if they follow the naming convention
            let all_files: Vec<_> = self.file_registry.all_files().take(50).collect();
            let mut matching_count = 0;

            for file in &all_files {
                for suffix in &suffix_patterns {
                    if file.contains(suffix) {
                        matching_count += 1;
                        break;
                    }
                }
            }

            // If very few files match the claimed patterns, it might be wrong
            if all_files.len() > 10 && matching_count < all_files.len() / 10 {
                issues.push(ConventionIssue {
                    severity: ValidationSeverity::Info,
                    category: ConventionCategory::NamingInconsistency,
                    description: format!(
                        "File naming patterns {:?} match very few files ({}/{})",
                        suffix_patterns, matching_count, all_files.len()
                    ),
                    evidence: None,
                });
            }
        }

        issues
    }

    /// Validate architecture pattern fits detected project type
    fn validate_architecture_vs_type(
        &self,
        architecture: &ArchitectureConvention,
        detection: &ProjectDetection,
    ) -> Vec<ConventionIssue> {
        let mut issues = Vec::new();

        let pattern = architecture.pattern_name.to_lowercase();
        let project_type = detection.primary_type;

        // Some architecture patterns are unusual for certain project types
        let unusual_combinations = [
            // CLIs typically don't use hexagonal architecture
            (crate::config::ProjectType::Cli, "hexagonal"),
            (crate::config::ProjectType::Cli, "microservice"),
            // Libraries typically don't use MVC
            (crate::config::ProjectType::Library, "mvc"),
            (crate::config::ProjectType::Library, "microservice"),
        ];

        for (ptype, arch_pattern) in unusual_combinations {
            if project_type == ptype && pattern.contains(arch_pattern) {
                issues.push(ConventionIssue {
                    severity: ValidationSeverity::Info,
                    category: ConventionCategory::ArchitectureMismatch,
                    description: format!(
                        "Architecture '{}' is unusual for {} projects",
                        architecture.pattern_name, project_type
                    ),
                    evidence: None,
                });
            }
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_default() {
        let result = ConventionValidationResult::default();
        assert!(!result.passed);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_convention_issue_creation() {
        let issue = ConventionIssue {
            severity: ValidationSeverity::Warning,
            category: ConventionCategory::MissingLayers,
            description: "Test issue".to_string(),
            evidence: Some("test evidence".to_string()),
        };

        assert!(matches!(issue.severity, ValidationSeverity::Warning));
        assert!(matches!(issue.category, ConventionCategory::MissingLayers));
    }
}
