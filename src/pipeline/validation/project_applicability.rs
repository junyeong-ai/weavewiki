//! Project Applicability Validation
//!
//! Validates that generated content is specific to the analyzed project,
//! not generic advice that applies to any project.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::ProjectType;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::types::{Agent, Rule, Skill};

static FILE_REF_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@([a-zA-Z0-9_./\-]+)(?::\d+)?").expect("Invalid regex"));

static GENERIC_PATTERNS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "best practice",
        "industry standard",
        "common pattern",
        "typically",
        "generally",
        "usually",
        "consider using",
        "it is recommended",
        "you should always",
        "standard approach",
        "conventional",
    ]
});

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicabilityConfig {
    pub min_project_reference_ratio: f32,
    pub max_generic_content_ratio: f32,
    pub min_project_keywords: usize,
    pub min_file_references: usize,
}

impl Default for ApplicabilityConfig {
    fn default() -> Self {
        Self {
            min_project_reference_ratio: 0.7,
            max_generic_content_ratio: 0.2,
            min_project_keywords: 3,
            min_file_references: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApplicabilityResult {
    pub passed: bool,
    pub project_reference_ratio: f32,
    pub generic_content_ratio: f32,
    pub project_keywords_found: usize,
    pub issues: Vec<ApplicabilityIssue>,
}

#[derive(Debug, Clone)]
pub struct ApplicabilityIssue {
    pub artifact_name: String,
    pub issue_type: ApplicabilityIssueType,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicabilityIssueType {
    NonexistentReference,
    GenericContent,
    NoFileReferences,
    ProjectTypeMismatch,
}

pub struct ProjectApplicabilityValidator {
    project_type: ProjectType,
    file_registry: VerifiedFileRegistry,
    config: ApplicabilityConfig,
}

impl ProjectApplicabilityValidator {
    pub fn new(
        project_type: ProjectType,
        file_registry: VerifiedFileRegistry,
        config: ApplicabilityConfig,
    ) -> Self {
        Self {
            project_type,
            file_registry,
            config,
        }
    }

    pub fn validate(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
    ) -> ApplicabilityResult {
        let mut issues = Vec::new();
        let mut stats = ValidationStats::default();

        for skill in skills {
            self.validate_content(&skill.name, &skill.body, &mut issues, &mut stats);
        }

        for agent in agents {
            self.validate_content(&agent.name, &agent.prompt, &mut issues, &mut stats);
        }

        for rule in rules {
            let content = rule.content.join("\n");
            self.validate_content(&rule.name, &content, &mut issues, &mut stats);
        }

        let project_reference_ratio = stats.project_reference_ratio();
        let generic_content_ratio = stats.generic_content_ratio();

        let passed = project_reference_ratio >= self.config.min_project_reference_ratio
            && generic_content_ratio <= self.config.max_generic_content_ratio
            && stats.project_keywords.len() >= self.config.min_project_keywords
            && !issues.iter().any(|i| i.issue_type == ApplicabilityIssueType::NonexistentReference);

        ApplicabilityResult {
            passed,
            project_reference_ratio,
            generic_content_ratio,
            project_keywords_found: stats.project_keywords.len(),
            issues,
        }
    }

    fn validate_content(
        &self,
        name: &str,
        content: &str,
        issues: &mut Vec<ApplicabilityIssue>,
        stats: &mut ValidationStats,
    ) {
        let references = self.extract_references(content);
        stats.total_references += references.len();

        for reference in &references {
            if self.file_registry.contains(reference) {
                stats.valid_references += 1;
            } else {
                issues.push(ApplicabilityIssue {
                    artifact_name: name.to_string(),
                    issue_type: ApplicabilityIssueType::NonexistentReference,
                    description: format!("References non-existent file: {}", reference),
                });
            }
        }

        if references.is_empty() && content.len() > 100 {
            issues.push(ApplicabilityIssue {
                artifact_name: name.to_string(),
                issue_type: ApplicabilityIssueType::NoFileReferences,
                description: "No file references in substantial content".into(),
            });
        }

        let lines: Vec<_> = content.lines().collect();
        stats.total_lines += lines.len();
        for line in &lines {
            if self.is_generic_line(line) {
                stats.generic_lines += 1;
            }
        }

        self.extract_project_keywords(content, &mut stats.project_keywords);

        if self.detects_wrong_project_type(content) {
            issues.push(ApplicabilityIssue {
                artifact_name: name.to_string(),
                issue_type: ApplicabilityIssueType::ProjectTypeMismatch,
                description: format!("Content for wrong project type (current: {:?})", self.project_type),
            });
        }
    }

    fn extract_references(&self, content: &str) -> Vec<String> {
        FILE_REF_PATTERN
            .captures_iter(content)
            .filter_map(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
            .filter(|r| !r.starts_with("http") && !r.starts_with("CLAUDE"))
            .collect()
    }

    fn is_generic_line(&self, line: &str) -> bool {
        let line_lower = line.to_lowercase();
        GENERIC_PATTERNS.iter().any(|p| line_lower.contains(p))
    }

    fn extract_project_keywords(&self, content: &str, keywords: &mut HashSet<String>) {
        for file in self.file_registry.all_files() {
            let file_name = std::path::Path::new(file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if file_name.len() > 3 && content.contains(file_name) {
                keywords.insert(file_name.to_string());
            }
        }
    }

    fn detects_wrong_project_type(&self, content: &str) -> bool {
        let content_lower = content.to_lowercase();

        

        match self.project_type {
            ProjectType::Cli => {
                content_lower.contains("react component")
                    || content_lower.contains("api endpoint")
                    || content_lower.contains("database migration")
            }
            ProjectType::Frontend => {
                content_lower.contains("database")
                    || content_lower.contains("command line")
                    || content_lower.contains("cli argument")
            }
            ProjectType::Backend => {
                content_lower.contains("react")
                    || content_lower.contains("vue")
                    || content_lower.contains("cli argument")
            }
            ProjectType::Library => {
                content_lower.contains("api endpoint")
                    || content_lower.contains("database migration")
            }
            _ => false,
        }
    }
}

#[derive(Default)]
struct ValidationStats {
    total_references: usize,
    valid_references: usize,
    total_lines: usize,
    generic_lines: usize,
    project_keywords: HashSet<String>,
}

impl ValidationStats {
    fn project_reference_ratio(&self) -> f32 {
        if self.total_references > 0 {
            self.valid_references as f32 / self.total_references as f32
        } else {
            0.0
        }
    }

    fn generic_content_ratio(&self) -> f32 {
        if self.total_lines > 0 {
            self.generic_lines as f32 / self.total_lines as f32
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generic_detection() {
        let line = "This is a best practice for error handling";
        let validator = ProjectApplicabilityValidator::new(
            ProjectType::Cli,
            VerifiedFileRegistry::empty(),
            ApplicabilityConfig::default(),
        );
        assert!(validator.is_generic_line(line));
    }

    #[test]
    fn test_specific_line() {
        let line = "Use the parse_args function in src/cli/mod.rs:42";
        let validator = ProjectApplicabilityValidator::new(
            ProjectType::Cli,
            VerifiedFileRegistry::empty(),
            ApplicabilityConfig::default(),
        );
        assert!(!validator.is_generic_line(line));
    }
}
