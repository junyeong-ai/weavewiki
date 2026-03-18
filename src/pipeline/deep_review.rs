//! Deep Review Engine - Two-Pass Quality Guarantee
//!
//! Ensures generated artifacts meet quality standards through:
//! - Pass 1: Full quality audit (programmatic + LLM)
//! - Pass 2: Regression check (no new issues introduced)
//!
//! Validation Strategy:
//! - Programmatic: file references, format (100% reliable)
//! - LLM: semantic quality, tier classification, cross-artifact consistency

use std::collections::HashSet;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::ai::LlmProvider;
use crate::ai::response::generate_schema;
use crate::config::DeepReviewConfig;
use crate::pipeline::context::{FileRegistryExt, VerifiedFileRegistry};
use crate::pipeline::file_reference;
use crate::types::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CheckType {
    SemanticQuality,
    EvidenceValid,
    CrossArtifactConsistent,
    FormatCompliant,
}

impl CheckType {
    pub fn is_programmatic(&self) -> bool {
        matches!(self, CheckType::EvidenceValid | CheckType::FormatCompliant)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CheckType::SemanticQuality => "semantic_quality",
            CheckType::EvidenceValid => "evidence_valid",
            CheckType::CrossArtifactConsistent => "cross_artifact",
            CheckType::FormatCompliant => "format_compliant",
        }
    }
}

use crate::types::Severity;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewIssue {
    pub check_type: CheckType,
    pub severity: Severity,
    pub artifact: String,
    pub message: String,
    pub location: Option<String>,
    pub suggestion: Option<String>,
}

impl ReviewIssue {
    pub fn error(check_type: CheckType, artifact: &str, message: &str) -> Self {
        Self {
            check_type,
            severity: Severity::High,
            artifact: artifact.to_string(),
            message: message.to_string(),
            location: None,
            suggestion: None,
        }
    }

    pub fn warning(check_type: CheckType, artifact: &str, message: &str) -> Self {
        Self {
            check_type,
            severity: Severity::Medium,
            artifact: artifact.to_string(),
            message: message.to_string(),
            location: None,
            suggestion: None,
        }
    }

    pub fn location(mut self, location: &str) -> Self {
        self.location = Some(location.to_string());
        self
    }

    pub fn suggestion(mut self, suggestion: &str) -> Self {
        self.suggestion = Some(suggestion.to_string());
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct CheckResult {
    pub passed: bool,
    pub score: f32,
    pub issues: Vec<ReviewIssue>,
}

impl CheckResult {
    pub fn pass() -> Self {
        Self {
            passed: true,
            score: 1.0,
            issues: Vec::new(),
        }
    }

    pub fn fail(issues: Vec<ReviewIssue>) -> Self {
        Self {
            passed: false,
            score: 0.0,
            issues,
        }
    }

    pub fn score(mut self, score: f32) -> Self {
        self.score = score;
        // Don't override passed status - it's already set by pass()/fail()
        // Score is informational, passing is determined by check logic
        self
    }
}

#[derive(Debug, Clone)]
pub struct DeepReviewChecks {
    pub semantic_quality: CheckResult,
    pub evidence_valid: CheckResult,
    pub cross_artifact_consistent: CheckResult,
    pub format_compliant: CheckResult,
}

impl DeepReviewChecks {
    pub fn all_passed(&self) -> bool {
        self.semantic_quality.passed
            && self.evidence_valid.passed
            && self.cross_artifact_consistent.passed
            && self.format_compliant.passed
    }

    pub fn collect_issues(&self) -> Vec<ReviewIssue> {
        let mut issues = Vec::new();
        issues.extend(self.semantic_quality.issues.clone());
        issues.extend(self.evidence_valid.issues.clone());
        issues.extend(self.cross_artifact_consistent.issues.clone());
        issues.extend(self.format_compliant.issues.clone());
        issues
    }

    pub fn overall_score(&self) -> f32 {
        (self.semantic_quality.score
            + self.evidence_valid.score
            + self.cross_artifact_consistent.score
            + self.format_compliant.score)
            / 4.0
    }
}

#[derive(Debug, Clone)]
pub struct DeepReviewResult {
    pub pass_number: u32,
    pub passed: bool,
    pub issues: Vec<ReviewIssue>,
    pub quality_score: f32,
    pub checks: DeepReviewChecks,
}

#[derive(Debug, Clone)]
pub struct RegressionCheck {
    pub has_regression: bool,
    pub new_issues: Vec<ReviewIssue>,
    pub resolved_issues: Vec<ReviewIssue>,
}

#[derive(Debug, Clone)]
pub enum TwoPassResult {
    Passed {
        total_attempts: u32,
        final_quality: f32,
    },
    Failed {
        total_attempts: u32,
        remaining_issues: Vec<ReviewIssue>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ReviewArtifacts {
    pub claude_md: Option<String>,
    pub skills: Vec<(String, String)>,
    pub agents: Vec<(String, String)>,
    pub rules: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct LlmReviewResponse {
    #[serde(default)]
    passed: bool,
    #[serde(default)]
    score: f32,
    #[serde(default)]
    issues: Vec<LlmIssue>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct LlmIssue {
    #[serde(default)]
    artifact: String,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    suggestion: Option<String>,
}

pub struct DeepReviewEngine {
    provider: Arc<dyn LlmProvider>,
    config: DeepReviewConfig,
    file_registry: VerifiedFileRegistry,
}

impl DeepReviewEngine {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        config: &DeepReviewConfig,
        file_registry: VerifiedFileRegistry,
    ) -> Self {
        Self {
            provider,
            config: config.clone(),
            file_registry,
        }
    }

    pub async fn execute_two_pass_review(
        &self,
        artifacts: &ReviewArtifacts,
    ) -> Result<TwoPassResult> {
        let required_passes = self.config.required_passes;
        let max_attempts = self.config.max_attempts;

        let mut consecutive_passes = 0u32;
        let mut total_attempts = 0u32;
        let mut baseline_issues: Option<Vec<ReviewIssue>> = None;

        info!(
            required_passes,
            max_attempts, "Starting two-pass deep review"
        );

        while consecutive_passes < required_passes && total_attempts < max_attempts {
            total_attempts += 1;

            let result = self.execute_single_pass(artifacts, total_attempts).await?;

            if result.passed {
                if consecutive_passes > 0 && self.config.check_regression {
                    let regression = self.check_regression(&result, &baseline_issues);
                    if regression.has_regression {
                        warn!(
                            attempt = total_attempts,
                            new_issues = regression.new_issues.len(),
                            "Regression detected"
                        );
                        consecutive_passes = 0;
                        baseline_issues = None;
                        continue;
                    }
                }

                consecutive_passes += 1;
                baseline_issues = Some(result.issues.clone());

                info!(
                    attempt = total_attempts,
                    consecutive = consecutive_passes,
                    required = required_passes,
                    "Pass succeeded"
                );
            } else {
                warn!(
                    attempt = total_attempts,
                    issues = result.issues.len(),
                    score = format!("{:.1}%", result.quality_score * 100.0),
                    "Pass failed"
                );

                for issue in result.issues.iter().take(3) {
                    debug!(
                        artifact = %issue.artifact,
                        message = %issue.message,
                        "Issue"
                    );
                }

                consecutive_passes = 0;
                baseline_issues = None;
            }
        }

        if consecutive_passes >= required_passes {
            info!(total_attempts, "Deep review PASSED");
            Ok(TwoPassResult::Passed {
                total_attempts,
                final_quality: self.calculate_final_quality(artifacts).await?,
            })
        } else {
            warn!(total_attempts, "Deep review FAILED");
            Ok(TwoPassResult::Failed {
                total_attempts,
                remaining_issues: baseline_issues.unwrap_or_default(),
            })
        }
    }

    pub async fn execute_single_pass(
        &self,
        artifacts: &ReviewArtifacts,
        pass_number: u32,
    ) -> Result<DeepReviewResult> {
        let mut all_issues = Vec::new();

        // Programmatic checks (100% reliable)
        let evidence = self.validate_evidence(artifacts);
        all_issues.extend(evidence.issues.clone());

        let format = self.validate_format(artifacts);
        all_issues.extend(format.issues.clone());

        // LLM-based checks (contextual judgment)
        let semantic = self.check_semantic_quality(artifacts).await?;
        all_issues.extend(semantic.issues.clone());

        let cross = self.check_cross_artifact_consistency(artifacts).await?;
        all_issues.extend(cross.issues.clone());

        let checks = DeepReviewChecks {
            semantic_quality: semantic,
            evidence_valid: evidence,
            cross_artifact_consistent: cross,
            format_compliant: format,
        };

        let passed = checks.all_passed();
        let quality_score = checks.overall_score();

        Ok(DeepReviewResult {
            pass_number,
            passed,
            issues: all_issues,
            quality_score,
            checks,
        })
    }

    fn validate_evidence(&self, artifacts: &ReviewArtifacts) -> CheckResult {
        let mut issues = Vec::new();
        let mut total_refs = 0;
        let mut valid_refs = 0;

        let all_content = self.collect_all_content(artifacts);

        for (artifact_name, content) in &all_content {
            for file_ref in file_reference::extract_references(content) {
                // Only count references with line numbers for validation
                let Some(line_num) = file_ref.line_start else {
                    continue;
                };

                total_refs += 1;
                let file_path = &file_ref.path;

                if !self.file_registry.file_exists(file_path) {
                    issues.push(
                        ReviewIssue::error(
                            CheckType::EvidenceValid,
                            artifact_name,
                            &format!("File not found: {}", file_path),
                        )
                        .location(&format!("@{}:{}", file_path, line_num))
                        .suggestion("Remove or update the file reference"),
                    );
                    continue;
                }

                if let Ok(max_lines) = self.file_registry.get_line_count(file_path)
                    && (line_num as usize) > max_lines
                {
                    issues.push(
                        ReviewIssue::error(
                            CheckType::EvidenceValid,
                            artifact_name,
                            &format!(
                                "Invalid line {} (file has {} lines): {}",
                                line_num, max_lines, file_path
                            ),
                        )
                        .location(&format!("@{}:{}", file_path, line_num)),
                    );
                    continue;
                }

                valid_refs += 1;
            }
        }

        if total_refs > 0 {
            let ratio = valid_refs as f32 / total_refs as f32;
            if ratio < self.config.min_evidence_ratio {
                issues.push(ReviewIssue::error(
                    CheckType::EvidenceValid,
                    "overall",
                    &format!(
                        "Evidence ratio {:.1}% below minimum {:.1}%",
                        ratio * 100.0,
                        self.config.min_evidence_ratio * 100.0
                    ),
                ));
            }
        }

        if issues.is_empty() {
            CheckResult::pass()
        } else {
            let score = if total_refs > 0 {
                valid_refs as f32 / total_refs as f32
            } else {
                0.5
            };
            CheckResult::fail(issues).score(score)
        }
    }

    /// Validate artifact format using simple heuristics.
    ///
    /// Note: This is a quick syntactic check, not full YAML parsing.
    /// Limitations:
    /// - `starts_with("---")` may match non-YAML content (e.g., markdown horizontal rules)
    /// - `contains("name:")` doesn't verify field is in frontmatter section
    /// - Doesn't validate YAML syntax or required field completeness
    ///
    /// For strict validation, use the actual YAML parser in artifact serialization.
    /// This check catches obvious formatting errors before LLM semantic review.
    fn validate_format(&self, artifacts: &ReviewArtifacts) -> CheckResult {
        let mut issues = Vec::new();

        for (name, content) in &artifacts.skills {
            if !content.starts_with("---") {
                issues.push(
                    ReviewIssue::error(
                        CheckType::FormatCompliant,
                        name,
                        "Missing YAML frontmatter",
                    )
                    .suggestion("Add YAML frontmatter with name and description"),
                );
            } else if !content.contains("name:") {
                issues.push(ReviewIssue::error(
                    CheckType::FormatCompliant,
                    name,
                    "Missing 'name' in frontmatter",
                ));
            }
        }

        for (name, content) in &artifacts.agents {
            if !content.starts_with("---") {
                issues.push(
                    ReviewIssue::error(
                        CheckType::FormatCompliant,
                        name,
                        "Missing YAML frontmatter",
                    )
                    .suggestion("Add YAML frontmatter with name and description"),
                );
            } else if !content.contains("name:") {
                issues.push(ReviewIssue::error(
                    CheckType::FormatCompliant,
                    name,
                    "Missing 'name' in frontmatter",
                ));
            }
        }

        for (name, content) in &artifacts.rules {
            if content.starts_with("---") && !content.contains("paths:") {
                issues.push(
                    ReviewIssue::error(
                        CheckType::FormatCompliant,
                        name,
                        "Frontmatter missing 'paths' field",
                    )
                    .suggestion("Add 'paths:' with glob patterns"),
                );
            }
        }

        if issues.is_empty() {
            CheckResult::pass()
        } else {
            CheckResult::fail(issues).score(0.5)
        }
    }

    async fn check_semantic_quality(&self, artifacts: &ReviewArtifacts) -> Result<CheckResult> {
        let prompt = self.build_semantic_quality_prompt(artifacts);
        let schema = self.review_response_schema();

        let response = match self.provider.generate(&prompt, &schema).await {
            Ok(resp) => resp,
            Err(e) => {
                warn!(error = %e, "LLM semantic check failed");
                return Ok(CheckResult::fail(vec![
                    ReviewIssue::error(
                        CheckType::SemanticQuality,
                        "llm_validation",
                        &format!("LLM validation failed: {}", e),
                    )
                    .suggestion("Check LLM provider connectivity and retry"),
                ]));
            }
        };

        self.parse_llm_review_response(&response.content, CheckType::SemanticQuality)
    }

    fn build_semantic_quality_prompt(&self, artifacts: &ReviewArtifacts) -> String {
        let mut content_summary = String::new();

        if let Some(ref claude_md) = artifacts.claude_md {
            let preview: String = claude_md
                .chars()
                .take(self.config.claude_md_preview_chars)
                .collect();
            content_summary.push_str(&format!("## CLAUDE.md\n{}\n\n", preview));
        }

        // Use config for skill limits (0 = unlimited)
        let skill_iter: Box<dyn Iterator<Item = _>> = if self.config.max_skills_in_review == 0 {
            Box::new(artifacts.skills.iter())
        } else {
            Box::new(
                artifacts
                    .skills
                    .iter()
                    .take(self.config.max_skills_in_review),
            )
        };
        for (name, body) in skill_iter {
            let preview: String = body.chars().take(self.config.skill_preview_chars).collect();
            content_summary.push_str(&format!("## Skill: {}\n{}\n\n", name, preview));
        }

        // Use config for agent limits (0 = unlimited)
        let agent_iter: Box<dyn Iterator<Item = _>> = if self.config.max_agents_in_review == 0 {
            Box::new(artifacts.agents.iter())
        } else {
            Box::new(
                artifacts
                    .agents
                    .iter()
                    .take(self.config.max_agents_in_review),
            )
        };
        for (name, body) in agent_iter {
            let preview: String = body.chars().take(self.config.agent_preview_chars).collect();
            content_summary.push_str(&format!("## Agent: {}\n{}\n\n", name, preview));
        }

        format!(
            r#"You are a quality reviewer for Claude Code plugin artifacts.

Evaluate the following generated content for SEMANTIC QUALITY:

1. **Actionability** (0-100): Are instructions specific and actionable?
   - Bad: "Follow best practices"
   - Good: "Use Arc::clone(&provider) when sharing providers across threads"

2. **Specificity** (0-100): Does it contain project-specific knowledge?
   - Bad: Generic advice applicable to any project
   - Good: References actual files, patterns, constraints unique to this project

3. **Value-Add** (0-100): Does it provide value beyond Claude's existing knowledge?
   - Bad: "Use cargo build to compile" (Claude knows this)
   - Good: "This project requires --features=cli for binary builds"

4. **Evidence Quality** (0-100): Are file references meaningful?
   - Bad: Random file references without context
   - Good: "@src/main.rs:42 is the entry point for CLI parsing"

CONTENT TO REVIEW:
{content_summary}

Respond in JSON format:
{{
  "passed": true/false,
  "score": 0.0-1.0,
  "issues": [
    {{
      "artifact": "artifact name",
      "severity": "warning|error|critical",
      "message": "specific issue description",
      "suggestion": "how to fix"
    }}
  ]
}}

Report all issues with their severity. Use your judgment on what constitutes passing quality based on project context."#
        )
    }

    async fn check_cross_artifact_consistency(
        &self,
        artifacts: &ReviewArtifacts,
    ) -> Result<CheckResult> {
        let prompt = self.build_cross_artifact_prompt(artifacts);
        let schema = self.review_response_schema();

        let response = match self.provider.generate(&prompt, &schema).await {
            Ok(resp) => resp,
            Err(e) => {
                warn!(error = %e, "LLM cross-artifact check failed");
                return Ok(CheckResult::fail(vec![
                    ReviewIssue::error(
                        CheckType::CrossArtifactConsistent,
                        "llm_validation",
                        &format!("LLM cross-artifact validation failed: {}", e),
                    )
                    .suggestion("Check LLM provider connectivity and retry"),
                ]));
            }
        };

        self.parse_llm_review_response(&response.content, CheckType::CrossArtifactConsistent)
    }

    fn build_cross_artifact_prompt(&self, artifacts: &ReviewArtifacts) -> String {
        let mut artifact_list = String::new();

        if artifacts.claude_md.is_some() {
            artifact_list.push_str("- CLAUDE.md (project conventions)\n");
        }

        for (name, _) in &artifacts.skills {
            artifact_list.push_str(&format!("- Skill: {}\n", name));
        }

        for (name, _) in &artifacts.agents {
            artifact_list.push_str(&format!("- Agent: {}\n", name));
        }

        for (name, _) in &artifacts.rules {
            artifact_list.push_str(&format!("- Rule: {}\n", name));
        }

        // Use ~75% of full preview size for cross-artifact (to fit both CLAUDE.md and skills)
        let claude_preview_chars = (self.config.claude_md_preview_chars * 3) / 4;
        let skill_preview_chars = self.config.skill_preview_chars / 2;

        let claude_preview = artifacts
            .claude_md
            .as_ref()
            .map(|c| c.chars().take(claude_preview_chars).collect::<String>())
            .unwrap_or_default();

        // Use config for skill limits in cross-artifact check
        let skill_iter: Box<dyn Iterator<Item = _>> = if self.config.max_skills_in_review == 0 {
            Box::new(artifacts.skills.iter())
        } else {
            Box::new(
                artifacts
                    .skills
                    .iter()
                    .take(self.config.max_skills_in_review),
            )
        };
        let skills_preview: String = skill_iter
            .map(|(n, b)| {
                format!(
                    "### {}\n{}",
                    n,
                    b.chars().take(skill_preview_chars).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        format!(
            r#"You are reviewing Claude Code plugin artifacts for CROSS-ARTIFACT CONSISTENCY.

Check for:
1. **Logical Consistency**: Do skills/agents align with CLAUDE.md conventions?
2. **No Contradictions**: Are there conflicting instructions between artifacts?
3. **Completeness**: Does CLAUDE.md reference skills/agents that exist?
4. **Coherent Terminology**: Same concepts use same names across artifacts?

ARTIFACTS:
{artifact_list}

CLAUDE.md PREVIEW:
{claude_preview}

SKILLS PREVIEW:
{skills_preview}

Respond in JSON format:
{{
  "passed": true/false,
  "score": 0.0-1.0,
  "issues": [
    {{
      "artifact": "artifact name or 'cross-reference'",
      "severity": "warning|error|critical",
      "message": "consistency issue description",
      "suggestion": "how to resolve"
    }}
  ]
}}

Determine if artifacts are logically consistent based on your analysis of the relationships."#
        )
    }

    fn review_response_schema(&self) -> Value {
        generate_schema::<LlmReviewResponse>()
    }

    fn parse_llm_review_response(
        &self,
        content: &Value,
        check_type: CheckType,
    ) -> Result<CheckResult> {
        let parsed: LlmReviewResponse = match serde_json::from_value(content.clone()) {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "Failed to parse LLM review response");
                return Ok(CheckResult::fail(vec![
                    ReviewIssue::error(
                        check_type,
                        "llm_response_parse",
                        &format!("Failed to parse LLM response: {}", e),
                    )
                    .suggestion("LLM response format may be invalid, retry validation"),
                ]));
            }
        };

        let issues: Vec<ReviewIssue> = parsed
            .issues
            .into_iter()
            .map(|i| {
                let severity = match i.severity.to_lowercase().as_str() {
                    "critical" => Severity::Critical,
                    "error" => Severity::High,
                    _ => Severity::Medium,
                };

                ReviewIssue {
                    check_type,
                    severity,
                    artifact: i.artifact,
                    message: i.message,
                    location: None,
                    suggestion: i.suggestion,
                }
            })
            .collect();

        if parsed.passed && issues.is_empty() {
            Ok(CheckResult::pass().score(parsed.score.max(0.7)))
        } else {
            Ok(CheckResult {
                passed: parsed.passed,
                score: parsed.score,
                issues,
            })
        }
    }

    fn check_regression(
        &self,
        current: &DeepReviewResult,
        baseline: &Option<Vec<ReviewIssue>>,
    ) -> RegressionCheck {
        let baseline_issues: HashSet<_> = baseline
            .as_ref()
            .map(|b| b.iter().map(|i| (&i.artifact, &i.message)).collect())
            .unwrap_or_default();

        let current_issues: HashSet<_> = current
            .issues
            .iter()
            .map(|i| (&i.artifact, &i.message))
            .collect();

        let new_issues: Vec<_> = current
            .issues
            .iter()
            .filter(|i| !baseline_issues.contains(&(&i.artifact, &i.message)))
            .cloned()
            .collect();

        let resolved_issues: Vec<_> = baseline
            .as_ref()
            .map(|b| {
                b.iter()
                    .filter(|i| !current_issues.contains(&(&i.artifact, &i.message)))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        RegressionCheck {
            has_regression: !new_issues.is_empty(),
            new_issues,
            resolved_issues,
        }
    }

    async fn calculate_final_quality(&self, artifacts: &ReviewArtifacts) -> Result<f32> {
        let result = self.execute_single_pass(artifacts, 0).await?;
        Ok(result.quality_score)
    }

    fn collect_all_content(&self, artifacts: &ReviewArtifacts) -> Vec<(String, String)> {
        let mut all = Vec::new();

        if let Some(ref claude_md) = artifacts.claude_md {
            all.push(("CLAUDE.md".to_string(), claude_md.clone()));
        }

        for (name, content) in &artifacts.skills {
            all.push((format!("skill:{}", name), content.clone()));
        }

        for (name, content) in &artifacts.agents {
            all.push((format!("agent:{}", name), content.clone()));
        }

        for (name, content) in &artifacts.rules {
            all.push((format!("rule:{}", name), content.clone()));
        }

        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_issue() {
        let issue = ReviewIssue::error(CheckType::EvidenceValid, "test.md", "File not found")
            .location("@missing.rs:10")
            .suggestion("Check file path");

        assert_eq!(issue.severity, Severity::High);
        assert!(issue.location.is_some());
        assert!(issue.suggestion.is_some());
    }

    #[test]
    fn test_check_result() {
        let pass = CheckResult::pass();
        assert!(pass.passed);
        assert_eq!(pass.score, 1.0);

        let fail = CheckResult::fail(vec![ReviewIssue::error(
            CheckType::EvidenceValid,
            "test",
            "error",
        )]);
        assert!(!fail.passed);
    }

    #[test]
    fn test_deep_review_checks() {
        let checks = DeepReviewChecks {
            semantic_quality: CheckResult::pass(),
            evidence_valid: CheckResult::pass(),
            cross_artifact_consistent: CheckResult::pass(),
            format_compliant: CheckResult::pass(),
        };

        assert!(checks.all_passed());
        assert_eq!(checks.overall_score(), 1.0);
    }
}
