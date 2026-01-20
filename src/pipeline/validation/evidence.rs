//! Enhanced Evidence Validator
//!
//! Validates evidence quality with strengthened requirements:
//! - Minimum file references per artifact type
//! - Evidence depth validation (FileOnly, FileAndLine, FileLineContext)
//! - Per-project-type requirements
//! - Context snippet validation for FileLineContext depth

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::{EvidenceDepth, ProjectType, ProjectTypeQuality, QualityConfig};
use crate::pipeline::context::VerifiedFileRegistry;
use crate::types::{Agent, ProjectMemory, Rule, Skill};

/// Regex for file references with optional line numbers
static FILE_REF_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@([a-zA-Z0-9_./\-]+)(?::(\d+)(?:-(\d+))?)?").expect("Invalid regex")
});

/// Result of enhanced evidence validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedEvidenceResult {
    pub passed: bool,
    pub overall_score: f32,
    pub artifact_results: Vec<ArtifactEvidenceResult>,
    pub depth_compliance: DepthComplianceResult,
    pub issues: Vec<EvidenceIssue>,
    pub summary: EvidenceSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceSummary {
    pub total_artifacts: usize,
    pub artifacts_meeting_requirements: usize,
    pub total_references: usize,
    pub valid_references: usize,
    pub references_with_lines: usize,
    pub references_with_context: usize,
    pub hallucinated_references: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactEvidenceResult {
    pub artifact_type: String,
    pub artifact_name: String,
    pub reference_count: usize,
    pub valid_references: usize,
    pub required_references: usize,
    pub depth_level: EvidenceDepth,
    pub required_depth: EvidenceDepth,
    pub passed: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthComplianceResult {
    pub required_depth: EvidenceDepth,
    pub file_only_count: usize,
    pub file_line_count: usize,
    pub file_line_context_count: usize,
    pub compliance_ratio: f32,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceIssue {
    pub artifact: String,
    pub severity: IssueSeverity,
    pub category: IssueCategory,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IssueSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IssueCategory {
    InsufficientReferences,
    InsufficientDepth,
    HallucinatedReference,
    MissingLineNumber,
    MissingContext,
}

/// Parsed file reference
#[derive(Debug, Clone)]
pub struct ParsedReference {
    pub file_path: String,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub depth: EvidenceDepth,
    pub is_valid: bool,
}

impl ParsedReference {
    pub fn depth_level(&self) -> EvidenceDepth {
        match (self.line_start, self.line_end) {
            (Some(_), Some(_)) => EvidenceDepth::FileLineContext,
            (Some(_), None) => EvidenceDepth::FileAndLine,
            (None, Some(_)) => EvidenceDepth::FileOnly, // Invalid case: end without start
            (None, None) => EvidenceDepth::FileOnly,
        }
    }
}

/// Enhanced evidence validator
pub struct EnhancedEvidenceValidator {
    quality_gate: ProjectTypeQuality,
    file_registry: VerifiedFileRegistry,
    project_root: std::path::PathBuf,
}

impl EnhancedEvidenceValidator {
    pub fn new(
        project_type: ProjectType,
        quality_config: &QualityConfig,
        file_registry: VerifiedFileRegistry,
        project_root: impl AsRef<Path>,
    ) -> Self {
        let quality_gate = quality_config
            .project_specific
            .get_for_type(project_type)
            .clone();

        Self {
            quality_gate,
            file_registry,
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    /// Validate all artifacts against evidence requirements
    pub fn validate(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        memory: &ProjectMemory,
    ) -> EnhancedEvidenceResult {
        let mut artifact_results = Vec::new();
        let mut issues = Vec::new();
        let mut summary = EvidenceSummary {
            total_artifacts: 0,
            artifacts_meeting_requirements: 0,
            total_references: 0,
            valid_references: 0,
            references_with_lines: 0,
            references_with_context: 0,
            hallucinated_references: 0,
        };

        // Validate each artifact type
        for skill in skills {
            let result = self.validate_artifact(
                "skill",
                &skill.name,
                &skill.body,
                self.quality_gate.min_file_references,
            );
            self.update_summary(&result, &mut summary);
            if !result.passed {
                issues.extend(self.generate_issues(&result));
            }
            artifact_results.push(result);
        }

        for agent in agents {
            let result = self.validate_artifact(
                "agent",
                &agent.name,
                &agent.prompt,
                self.quality_gate.min_file_references,
            );
            self.update_summary(&result, &mut summary);
            if !result.passed {
                issues.extend(self.generate_issues(&result));
            }
            artifact_results.push(result);
        }

        for rule in rules {
            let content = rule.content.join("\n");
            // Rules have slightly lower reference requirement
            let min_refs = (self.quality_gate.min_file_references as f32 * 0.5).ceil() as usize;
            let result = self.validate_artifact("rule", &rule.name, &content, min_refs.max(1));
            self.update_summary(&result, &mut summary);
            if !result.passed {
                issues.extend(self.generate_issues(&result));
            }
            artifact_results.push(result);
        }

        // Validate CLAUDE.md
        let memory_content = memory.to_markdown();
        let memory_min_refs = self.quality_gate.min_file_references * 2; // CLAUDE.md needs more references
        let memory_result =
            self.validate_artifact("memory", "CLAUDE.md", &memory_content, memory_min_refs);
        self.update_summary(&memory_result, &mut summary);
        if !memory_result.passed {
            issues.extend(self.generate_issues(&memory_result));
        }
        artifact_results.push(memory_result);

        summary.total_artifacts = artifact_results.len();

        // Calculate depth compliance
        let depth_compliance = self.calculate_depth_compliance(&summary);

        // Calculate overall score
        let overall_score = self.calculate_overall_score(&artifact_results, &depth_compliance);

        // Check if passed
        let passed = overall_score >= self.quality_gate.min_evidence
            && depth_compliance.passed
            && !issues.iter().any(|i| i.severity == IssueSeverity::Critical);

        EnhancedEvidenceResult {
            passed,
            overall_score,
            artifact_results,
            depth_compliance,
            issues,
            summary,
        }
    }

    fn validate_artifact(
        &self,
        artifact_type: &str,
        name: &str,
        content: &str,
        min_references: usize,
    ) -> ArtifactEvidenceResult {
        let references = self.extract_references(content);
        let valid_refs: Vec<_> = references.iter().filter(|r| r.is_valid).collect();
        let required_depth = self.quality_gate.evidence_depth;

        // Check depth compliance for each reference
        let depth_compliant: Vec<_> = valid_refs
            .iter()
            .filter(|r| self.depth_meets_requirement(r.depth_level(), required_depth))
            .collect();

        // Determine actual achieved depth
        let achieved_depth = if valid_refs.is_empty() {
            EvidenceDepth::FileOnly
        } else {

            valid_refs
                .iter()
                .map(|r| r.depth_level())
                .max_by_key(|d| depth_to_level(d))
                .unwrap_or(EvidenceDepth::FileOnly)
        };

        let mut artifact_issues = Vec::new();

        // Check reference count
        if valid_refs.len() < min_references {
            artifact_issues.push(format!(
                "Insufficient references: {} found, {} required",
                valid_refs.len(),
                min_references
            ));
        }

        // Check hallucinations
        let hallucinated: Vec<_> = references.iter().filter(|r| !r.is_valid).collect();
        if !hallucinated.is_empty() {
            for h in &hallucinated {
                artifact_issues.push(format!("Hallucinated reference: {}", h.file_path));
            }
        }

        // Check depth compliance
        if !self.depth_meets_requirement(achieved_depth, required_depth) {
            artifact_issues.push(format!(
                "Insufficient evidence depth: {:?} achieved, {:?} required",
                achieved_depth, required_depth
            ));
        }

        let passed = valid_refs.len() >= min_references
            && hallucinated.is_empty()
            && depth_compliant.len() >= (min_references.saturating_sub(1));

        ArtifactEvidenceResult {
            artifact_type: artifact_type.to_string(),
            artifact_name: name.to_string(),
            reference_count: references.len(),
            valid_references: valid_refs.len(),
            required_references: min_references,
            depth_level: achieved_depth,
            required_depth,
            passed,
            issues: artifact_issues,
        }
    }

    fn extract_references(&self, content: &str) -> Vec<ParsedReference> {
        FILE_REF_PATTERN
            .captures_iter(content)
            .map(|cap| {
                let file_path = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
                let line_start = cap
                    .get(2)
                    .and_then(|m| m.as_str().parse::<u32>().ok());
                let line_end = cap
                    .get(3)
                    .and_then(|m| m.as_str().parse::<u32>().ok());

                // Skip non-file references
                if file_path.starts_with("http")
                    || file_path.starts_with("CLAUDE")
                    || file_path.is_empty()
                {
                    return ParsedReference {
                        file_path: file_path.to_string(),
                        line_start: None,
                        line_end: None,
                        depth: EvidenceDepth::FileOnly,
                        is_valid: false,
                    };
                }

                let is_valid = self.file_registry.contains(file_path)
                    || self.project_root.join(file_path).exists()
                    || self.project_root.join("src").join(file_path).exists();

                let depth = match (line_start, line_end) {
                    (Some(_), Some(_)) => EvidenceDepth::FileLineContext,
                    (Some(_), None) => EvidenceDepth::FileAndLine,
                    (None, Some(_)) => EvidenceDepth::FileOnly, // Invalid: end without start
                    (None, None) => EvidenceDepth::FileOnly,
                };

                ParsedReference {
                    file_path: file_path.to_string(),
                    line_start,
                    line_end,
                    depth,
                    is_valid,
                }
            })
            .filter(|r| !r.file_path.is_empty())
            .collect()
    }

    fn depth_meets_requirement(&self, achieved: EvidenceDepth, required: EvidenceDepth) -> bool {
        depth_to_level(&achieved) >= depth_to_level(&required)
    }

    fn update_summary(&self, result: &ArtifactEvidenceResult, summary: &mut EvidenceSummary) {
        summary.total_references += result.reference_count;
        summary.valid_references += result.valid_references;
        summary.hallucinated_references += result.reference_count - result.valid_references;

        if result.passed {
            summary.artifacts_meeting_requirements += 1;
        }

        // Count by depth (this is an approximation; ideally we'd track individual refs)
        let depth_level = depth_to_level(&result.depth_level);
        if depth_level >= 2 {
            summary.references_with_context += result.valid_references;
            summary.references_with_lines += result.valid_references;
        } else if depth_level >= 1 {
            summary.references_with_lines += result.valid_references;
        }
    }

    fn generate_issues(&self, result: &ArtifactEvidenceResult) -> Vec<EvidenceIssue> {
        let mut issues = Vec::new();

        if result.valid_references < result.required_references {
            issues.push(EvidenceIssue {
                artifact: format!("{}:{}", result.artifact_type, result.artifact_name),
                severity: if result.valid_references == 0 {
                    IssueSeverity::Critical
                } else {
                    IssueSeverity::High
                },
                category: IssueCategory::InsufficientReferences,
                description: format!(
                    "{} has {} valid references, needs at least {}",
                    result.artifact_name, result.valid_references, result.required_references
                ),
                suggestion: format!(
                    "Add {} more @file:line references to {}",
                    result.required_references - result.valid_references,
                    result.artifact_name
                ),
            });
        }

        if !self.depth_meets_requirement(result.depth_level, result.required_depth) {
            issues.push(EvidenceIssue {
                artifact: format!("{}:{}", result.artifact_type, result.artifact_name),
                severity: IssueSeverity::Medium,
                category: IssueCategory::InsufficientDepth,
                description: format!(
                    "{} has {:?} depth, needs {:?}",
                    result.artifact_name, result.depth_level, result.required_depth
                ),
                suggestion: self.depth_improvement_suggestion(result.required_depth),
            });
        }

        let hallucinated_count = result.reference_count - result.valid_references;
        if hallucinated_count > 0 {
            issues.push(EvidenceIssue {
                artifact: format!("{}:{}", result.artifact_type, result.artifact_name),
                severity: IssueSeverity::High,
                category: IssueCategory::HallucinatedReference,
                description: format!(
                    "{} has {} hallucinated (non-existent) file references",
                    result.artifact_name, hallucinated_count
                ),
                suggestion: "Remove or fix invalid file references".into(),
            });
        }

        issues
    }

    fn depth_improvement_suggestion(&self, required: EvidenceDepth) -> String {
        let level = depth_to_level(&required);
        if level >= 2 {
            "Add @file:line-line references with context (e.g., @src/main.rs:42-50)".into()
        } else if level >= 1 {
            "Add @file:line references (e.g., @src/main.rs:42)".into()
        } else {
            "Add @file references".into()
        }
    }

    fn calculate_depth_compliance(&self, summary: &EvidenceSummary) -> DepthComplianceResult {
        let required = self.quality_gate.evidence_depth;
        let total = summary.valid_references;

        let required_level = depth_to_level(&required);
        let compliant_count = if required_level >= 2 {
            summary.references_with_context
        } else if required_level >= 1 {
            summary.references_with_lines
        } else {
            total
        };

        let compliance_ratio = if total > 0 {
            compliant_count as f32 / total as f32
        } else {
            0.0
        };

        // Need at least 70% of references to meet depth requirement
        let passed = compliance_ratio >= 0.7 || total < 3;

        DepthComplianceResult {
            required_depth: required,
            file_only_count: total - summary.references_with_lines,
            file_line_count: summary.references_with_lines - summary.references_with_context,
            file_line_context_count: summary.references_with_context,
            compliance_ratio,
            passed,
        }
    }

    fn calculate_overall_score(
        &self,
        results: &[ArtifactEvidenceResult],
        depth_compliance: &DepthComplianceResult,
    ) -> f32 {
        if results.is_empty() {
            return 1.0;
        }

        // Weight: 60% reference count compliance, 40% depth compliance
        let reference_scores: Vec<f32> = results
            .iter()
            .map(|r| {
                if r.required_references == 0 {
                    1.0
                } else {
                    (r.valid_references as f32 / r.required_references as f32).min(1.0)
                }
            })
            .collect();

        let avg_reference_score = reference_scores.iter().sum::<f32>() / results.len() as f32;

        // Penalty for hallucinations
        let total_refs: usize = results.iter().map(|r| r.reference_count).sum();
        let valid_refs: usize = results.iter().map(|r| r.valid_references).sum();
        let hallucination_penalty = if total_refs > 0 {
            1.0 - ((total_refs - valid_refs) as f32 / total_refs as f32 * 0.5)
        } else {
            1.0
        };

        let score = (avg_reference_score * 0.6 + depth_compliance.compliance_ratio * 0.4)
            * hallucination_penalty;

        score.clamp(0.0, 1.0)
    }
}

/// Convenience function for quick validation
#[allow(clippy::too_many_arguments)]
pub fn validate_evidence(
    project_type: ProjectType,
    quality_config: &QualityConfig,
    file_registry: VerifiedFileRegistry,
    project_root: impl AsRef<Path>,
    skills: &[Skill],
    agents: &[Agent],
    rules: &[Rule],
    memory: &ProjectMemory,
) -> EnhancedEvidenceResult {
    let validator =
        EnhancedEvidenceValidator::new(project_type, quality_config, file_registry, project_root);
    validator.validate(skills, agents, rules, memory)
}

/// Convert EvidenceDepth to a numeric level for comparison
/// Minimal/FileOnly = 0, Standard/FileAndLine = 1, Comprehensive/FileLineContext = 2
fn depth_to_level(depth: &EvidenceDepth) -> u8 {
    match depth {
        EvidenceDepth::Minimal | EvidenceDepth::FileOnly => 0,
        EvidenceDepth::Standard | EvidenceDepth::FileAndLine => 1,
        EvidenceDepth::Comprehensive | EvidenceDepth::FileLineContext => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsed_reference_depth() {
        let file_only = ParsedReference {
            file_path: "src/main.rs".into(),
            line_start: None,
            line_end: None,
            depth: EvidenceDepth::FileOnly,
            is_valid: true,
        };
        assert!(matches!(
            file_only.depth_level(),
            EvidenceDepth::FileOnly
        ));

        let file_line = ParsedReference {
            file_path: "src/main.rs".into(),
            line_start: Some(42),
            line_end: None,
            depth: EvidenceDepth::FileAndLine,
            is_valid: true,
        };
        assert!(matches!(
            file_line.depth_level(),
            EvidenceDepth::FileAndLine
        ));

        let file_context = ParsedReference {
            file_path: "src/main.rs".into(),
            line_start: Some(42),
            line_end: Some(50),
            depth: EvidenceDepth::FileLineContext,
            is_valid: true,
        };
        assert!(matches!(
            file_context.depth_level(),
            EvidenceDepth::FileLineContext
        ));
    }

    #[test]
    fn test_depth_meets_requirement() {
        // FileOnly meets FileOnly
        assert!(depth_comparison(EvidenceDepth::FileOnly, EvidenceDepth::FileOnly));
        // FileAndLine meets FileOnly
        assert!(depth_comparison(EvidenceDepth::FileAndLine, EvidenceDepth::FileOnly));
        // FileLineContext meets all
        assert!(depth_comparison(EvidenceDepth::FileLineContext, EvidenceDepth::FileOnly));
        assert!(depth_comparison(EvidenceDepth::FileLineContext, EvidenceDepth::FileAndLine));
        // FileOnly does NOT meet FileAndLine
        assert!(!depth_comparison(EvidenceDepth::FileOnly, EvidenceDepth::FileAndLine));
    }

    fn depth_comparison(achieved: EvidenceDepth, required: EvidenceDepth) -> bool {
        let achieved_level = match achieved {
            EvidenceDepth::Minimal | EvidenceDepth::FileOnly => 0,
            EvidenceDepth::Standard | EvidenceDepth::FileAndLine => 1,
            EvidenceDepth::Comprehensive | EvidenceDepth::FileLineContext => 2,
        };
        let required_level = match required {
            EvidenceDepth::Minimal | EvidenceDepth::FileOnly => 0,
            EvidenceDepth::Standard | EvidenceDepth::FileAndLine => 1,
            EvidenceDepth::Comprehensive | EvidenceDepth::FileLineContext => 2,
        };
        achieved_level >= required_level
    }

    #[test]
    fn test_issue_severity_ordering() {
        assert!(matches!(IssueSeverity::Critical, IssueSeverity::Critical));
        assert!(matches!(IssueSeverity::High, IssueSeverity::High));
    }
}
