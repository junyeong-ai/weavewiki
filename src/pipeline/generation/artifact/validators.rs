//! Artifact Validators
//!
//! Validates generated artifacts against quality standards:
//! - Minimum value thresholds
//! - Content quality checks
//! - Tier filtering
//! - Reference validation

use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::insight::TierClassification;
use crate::types::validation::ValidationIssue;
use crate::types::{Agent, Rule, Skill};

/// Validation result
#[derive(Debug, Clone)]
pub struct ArtifactValidation {
    pub is_valid: bool,
    pub score: f32,
    pub issues: Vec<ValidationIssue>,
}

impl ArtifactValidation {
    pub fn pass(score: f32) -> Self {
        Self {
            is_valid: true,
            score,
            issues: Vec::new(),
        }
    }

    pub fn pass_with_warnings(score: f32, issues: Vec<ValidationIssue>) -> Self {
        Self {
            is_valid: true,
            score,
            issues,
        }
    }

    pub fn fail(score: f32, issues: Vec<ValidationIssue>) -> Self {
        Self {
            is_valid: false,
            score,
            issues,
        }
    }
}

/// Validates generated artifacts
pub struct ArtifactValidator {
    min_tier: TierClassification,
    min_evidence_count: usize,
    min_content_length: usize,
}

impl ArtifactValidator {
    pub fn new() -> Self {
        Self {
            min_tier: TierClassification::Tier2Convention,
            min_evidence_count: 1,
            min_content_length: 50,
        }
    }

    pub fn min_tier(mut self, tier: TierClassification) -> Self {
        self.min_tier = tier;
        self
    }

    pub fn min_evidence(mut self, count: usize) -> Self {
        self.min_evidence_count = count;
        self
    }

    /// Validate a Rule artifact
    pub fn validate_rule(
        &self,
        rule: &Rule,
        registry: Option<&VerifiedFileRegistry>,
    ) -> ArtifactValidation {
        let mut issues = Vec::new();
        let mut score = 1.0f32;

        // Check content length
        let content_len: usize = rule.content.iter().map(|s| s.len()).sum();
        if content_len < self.min_content_length {
            issues.push(ValidationIssue::warning(
                "RULE_CONTENT_SHORT",
                format!(
                    "Rule content too short ({} chars, min {})",
                    content_len, self.min_content_length
                ),
            ));
            score -= 0.2;
        }

        // Check for evidence
        if rule.evidence.is_empty() {
            issues.push(ValidationIssue::warning(
                "RULE_NO_EVIDENCE",
                "Rule lacks evidence references",
            ));
            score -= 0.2;
        }

        // Validate evidence paths if registry provided
        if let Some(reg) = registry {
            for evidence in &rule.evidence {
                if !reg.contains(&evidence.file) {
                    issues.push(ValidationIssue::warning(
                        "RULE_EVIDENCE_NOT_FOUND",
                        format!("Evidence file not found: {}", evidence.file),
                    ));
                    score -= 0.1;
                }
            }
        }

        // Tier classification is purely informational - LLM has already classified during generation
        // We log for visibility but do NOT penalize score - LLM judgment is authoritative
        if !rule.tier.should_keep() {
            issues.push(ValidationIssue::info(
                "RULE_LOW_TIER",
                format!(
                    "Rule classified as {} - LLM determined this is appropriate for context",
                    rule.tier
                ),
            ));
            // No score penalty - tier classification is advisory only
        }

        let is_valid = score >= 0.5 && !issues.iter().any(|i| i.is_error());

        if is_valid {
            if issues.is_empty() {
                ArtifactValidation::pass(score.max(0.0))
            } else {
                ArtifactValidation::pass_with_warnings(score.max(0.0), issues)
            }
        } else {
            ArtifactValidation::fail(score.max(0.0), issues)
        }
    }

    /// Validate a Skill artifact
    pub fn validate_skill(
        &self,
        skill: &Skill,
        registry: Option<&VerifiedFileRegistry>,
    ) -> ArtifactValidation {
        let mut issues = Vec::new();
        let mut score = 1.0f32;

        // Check body length
        if skill.body.len() < self.min_content_length {
            issues.push(ValidationIssue::warning(
                "SKILL_BODY_SHORT",
                format!(
                    "Skill body too short ({} chars, min {})",
                    skill.body.len(),
                    self.min_content_length
                ),
            ));
            score -= 0.2;
        }

        // Check for file references
        let file_ref_count = skill.body.matches("@").count();
        if file_ref_count < self.min_evidence_count {
            issues.push(ValidationIssue::warning(
                "SKILL_NO_REFS",
                format!(
                    "Skill lacks file references ({} found, min {})",
                    file_ref_count, self.min_evidence_count
                ),
            ));
            score -= 0.2;
        }

        // Validate file references if registry provided
        if let Some(reg) = registry {
            let refs = crate::utils::patterns::extract_paths(&skill.body);
            for file_ref in refs {
                if !reg.contains(&file_ref) {
                    issues.push(ValidationIssue::warning(
                        "SKILL_REF_NOT_FOUND",
                        format!("Referenced file not found: {}", file_ref),
                    ));
                    score -= 0.1;
                }
            }
        }

        // Check description
        if skill.description.is_empty() {
            issues.push(ValidationIssue::warning(
                "SKILL_NO_DESC",
                "Skill lacks description",
            ));
            score -= 0.1;
        }

        // Tier classification is purely informational - LLM has already classified during generation
        // We log for visibility but do NOT penalize score - LLM judgment is authoritative
        if !skill.quality.tier.should_keep() {
            issues.push(ValidationIssue::info(
                "SKILL_LOW_TIER",
                format!(
                    "Skill classified as {} - LLM determined this is appropriate for context",
                    skill.quality.tier
                ),
            ));
            // No score penalty - tier classification is advisory only
        }

        let is_valid = score >= 0.5 && !issues.iter().any(|i| i.is_error());

        if is_valid {
            if issues.is_empty() {
                ArtifactValidation::pass(score.max(0.0))
            } else {
                ArtifactValidation::pass_with_warnings(score.max(0.0), issues)
            }
        } else {
            ArtifactValidation::fail(score.max(0.0), issues)
        }
    }

    /// Validate an Agent artifact
    pub fn validate_agent(
        &self,
        agent: &Agent,
        _registry: Option<&VerifiedFileRegistry>,
    ) -> ArtifactValidation {
        let mut issues = Vec::new();
        let mut score = 1.0f32;

        // Check instructions length
        if agent.prompt.len() < self.min_content_length {
            issues.push(ValidationIssue::warning(
                "AGENT_PROMPT_SHORT",
                format!(
                    "Agent instructions too short ({} chars, min {})",
                    agent.prompt.len(),
                    self.min_content_length
                ),
            ));
            score -= 0.2;
        }

        // Check description
        if agent.description.is_empty() {
            issues.push(ValidationIssue::warning(
                "AGENT_NO_DESC",
                "Agent lacks description",
            ));
            score -= 0.1;
        }

        let is_valid = score >= 0.5;

        if is_valid {
            if issues.is_empty() {
                ArtifactValidation::pass(score.max(0.0))
            } else {
                ArtifactValidation::pass_with_warnings(score.max(0.0), issues)
            }
        } else {
            ArtifactValidation::fail(score.max(0.0), issues)
        }
    }
}

impl Default for ArtifactValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Batch validator for multiple artifacts
pub struct BatchValidator {
    validator: ArtifactValidator,
    registry: Option<VerifiedFileRegistry>,
}

impl BatchValidator {
    pub fn new(registry: Option<VerifiedFileRegistry>) -> Self {
        Self {
            validator: ArtifactValidator::new(),
            registry,
        }
    }

    pub fn validate_rules(&self, rules: &[Rule]) -> BatchArtifactValidation {
        let results: Vec<_> = rules
            .iter()
            .map(|r| {
                (
                    r.name.clone(),
                    self.validator.validate_rule(r, self.registry.as_ref()),
                )
            })
            .collect();

        BatchArtifactValidation::from_results(results)
    }

    pub fn validate_skills(&self, skills: &[Skill]) -> BatchArtifactValidation {
        let results: Vec<_> = skills
            .iter()
            .map(|s| {
                (
                    s.name.clone(),
                    self.validator.validate_skill(s, self.registry.as_ref()),
                )
            })
            .collect();

        BatchArtifactValidation::from_results(results)
    }

    pub fn validate_agents(&self, agents: &[Agent]) -> BatchArtifactValidation {
        let results: Vec<_> = agents
            .iter()
            .map(|a| {
                (
                    a.name.clone(),
                    self.validator.validate_agent(a, self.registry.as_ref()),
                )
            })
            .collect();

        BatchArtifactValidation::from_results(results)
    }
}

/// Result of batch validation
#[derive(Debug, Clone)]
pub struct BatchArtifactValidation {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub average_score: f32,
    pub failed_items: Vec<(String, Vec<ValidationIssue>)>,
}

impl BatchArtifactValidation {
    fn from_results(results: Vec<(String, ArtifactValidation)>) -> Self {
        let total = results.len();
        let passed = results.iter().filter(|(_, r)| r.is_valid).count();
        let failed = total - passed;

        let total_score: f32 = results.iter().map(|(_, r)| r.score).sum();
        let average_score = if total > 0 {
            total_score / total as f32
        } else {
            0.0
        };

        let failed_items: Vec<_> = results
            .into_iter()
            .filter(|(_, r)| !r.is_valid)
            .map(|(name, r)| (name, r.issues))
            .collect();

        Self {
            total,
            passed,
            failed,
            average_score,
            failed_items,
        }
    }

    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EvidenceLocation;

    #[test]
    fn test_validate_rule_pass() {
        let validator = ArtifactValidator::new();

        // Rule with Tier2 (convention) should pass validation
        let rule = Rule::new(
            "test-rule",
            vec![
                "# Test Rule".into(),
                "".into(),
                "You must follow this constraint.".into(),
                "Always validate input before processing.".into(),
            ],
        )
        .evidence(vec![EvidenceLocation {
            file: "src/main.rs".into(),
            start_line: 10,
            end_line: 10,
            start_column: None,
            end_column: None,
        }])
        .tier(crate::types::ContentTier::Tier2Convention);

        let result = validator.validate_rule(&rule, None);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_rule_tier1_info() {
        let validator = ArtifactValidator::new();

        // Rule with Tier1 (generic) - purely informational, no score penalty
        // LLM classification is advisory; tier doesn't affect validation outcome
        let rule = Rule::new(
            "generic-rule",
            vec![
                "# Generic Rule".into(),
                "".into(),
                "Use best practices when coding. See @src/main.rs:1 for context.".into(),
            ],
        )
        .evidence(vec![EvidenceLocation {
            file: "src/main.rs".into(),
            start_line: 1,
            end_line: 1,
            start_column: None,
            end_column: None,
        }])
        .tier(crate::types::ContentTier::Tier1Generic);

        let result = validator.validate_rule(&rule, None);
        // Tier1 is purely informational - validation passes, score not affected
        assert!(result.is_valid);
        assert!(result.issues.iter().any(|i| i.code == "RULE_LOW_TIER"));
        // Score is not penalized for tier classification
        assert!(result.score >= 0.9); // May have other minor deductions
    }

    #[test]
    fn test_validate_skill_pass() {
        let validator = ArtifactValidator::new();

        let mut skill = Skill::new(
            "test-skill",
            "A test skill for validation",
            "## Test Skill\n\nYou should check @src/main.rs:10 for implementation details.\n\nAlways validate inputs.",
        );
        skill.quality.tier = TierClassification::Tier2Convention;

        let result = validator.validate_skill(&skill, None);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_skill_tier1_info() {
        let validator = ArtifactValidator::new();

        // Skill classified as Tier 1 by LLM - purely informational, no score penalty
        // LLM classification is advisory; tier doesn't affect validation outcome
        let mut skill = Skill::new(
            "generic-skill",
            "A generic skill",
            "## Build Instructions\n\nRun cargo build to build the project. Check @src/main.rs:1 for details.",
        );
        skill.quality.tier = TierClassification::Tier1Generic;

        let result = validator.validate_skill(&skill, None);
        // Tier1 is purely informational - validation passes, score not affected
        assert!(result.is_valid);
        assert!(result.issues.iter().any(|i| i.code == "SKILL_LOW_TIER"));
        // Score is not penalized for tier classification
        assert!(result.score >= 0.9); // May have other minor deductions
    }

    #[test]
    fn test_validate_skill_warning_short() {
        let validator = ArtifactValidator::new();

        let skill = Skill::new("short-skill", "Short", "Too short");

        let result = validator.validate_skill(&skill, None);
        // Short skill gets warnings but may still pass with reduced score
        assert!(result.score < 0.7);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.message.contains("too short"))
        );
    }

    #[test]
    fn test_batch_validation() {
        let batch = BatchValidator::new(None);

        let rules = vec![
            Rule::new(
                "valid-rule",
                vec![
                    "You must validate all user input. This is a critical security constraint."
                        .into(),
                ],
            )
            .evidence(vec![EvidenceLocation {
                file: "src/validate.rs".into(),
                start_line: 1,
                end_line: 1,
                start_column: None,
                end_column: None,
            }])
            .tier(crate::types::ContentTier::Tier2Convention),
            // Good content and evidence - both should pass (tier is now advisory)
            Rule::new(
                "also-valid-rule",
                vec!["Always check authentication before processing requests.".into()],
            )
            .evidence(vec![EvidenceLocation {
                file: "src/auth.rs".into(),
                start_line: 1,
                end_line: 1,
                start_column: None,
                end_column: None,
            }])
            .tier(crate::types::ContentTier::Tier1Generic), // Tier1 is advisory, not failure
        ];

        let result = batch.validate_rules(&rules);
        assert_eq!(result.total, 2);
        // Both rules pass - tier classification doesn't cause failure
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 0);
    }
}
