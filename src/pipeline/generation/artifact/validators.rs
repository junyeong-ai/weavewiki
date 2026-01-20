//! Artifact Validators
//!
//! Validates generated artifacts against quality standards:
//! - Minimum value thresholds
//! - Content quality checks
//! - Tier filtering
//! - Reference validation

use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::insight::TierClassification;
use crate::types::{Agent, Rule, Skill};

/// Validation result
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub score: f32,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
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

/// Specific validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub message: String,
}

impl ValidationIssue {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            message: message.into(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Warning,
    Error,
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
            min_tier: TierClassification::Tier2,
            min_evidence_count: 1,
            min_content_length: 50,
        }
    }

    pub fn with_min_tier(mut self, tier: TierClassification) -> Self {
        self.min_tier = tier;
        self
    }

    pub fn with_min_evidence(mut self, count: usize) -> Self {
        self.min_evidence_count = count;
        self
    }

    /// Validate a Rule artifact
    pub fn validate_rule(&self, rule: &Rule, registry: Option<&VerifiedFileRegistry>) -> ValidationResult {
        let mut issues = Vec::new();
        let mut score = 1.0f32;

        // Check content length
        let content_len: usize = rule.content.iter().map(|s| s.len()).sum();
        if content_len < self.min_content_length {
            issues.push(ValidationIssue::warning(format!(
                "Rule content too short ({} chars, min {})",
                content_len, self.min_content_length
            )));
            score -= 0.2;
        }

        // Check for evidence
        if rule.evidence.is_empty() {
            issues.push(ValidationIssue::warning("Rule lacks evidence references"));
            score -= 0.2;
        }

        // Validate evidence paths if registry provided
        if let Some(reg) = registry {
            for evidence in &rule.evidence {
                if !reg.contains(&evidence.file) {
                    issues.push(ValidationIssue::warning(format!(
                        "Evidence file not found: {}",
                        evidence.file
                    )));
                    score -= 0.1;
                }
            }
        }

        // Check for actionable language
        let content_text = rule.content.join(" ").to_lowercase();
        let has_actionable = ["must", "should", "avoid", "never", "always"]
            .iter()
            .any(|w| content_text.contains(w));

        if !has_actionable {
            issues.push(ValidationIssue::warning(
                "Rule lacks actionable language (must/should/avoid)",
            ));
            score -= 0.15;
        }

        // Check for generic content (Tier 1 indicators)
        let tier1_indicators = [
            "use best practices",
            "follow conventions",
            "write clean code",
            "use cargo",
            "use npm",
        ];
        for indicator in tier1_indicators {
            if content_text.contains(indicator) {
                issues.push(ValidationIssue::error(format!(
                    "Rule contains generic Tier 1 content: '{}'",
                    indicator
                )));
                score -= 0.3;
            }
        }

        let is_valid = score >= 0.5 && !issues.iter().any(|i| i.severity == IssueSeverity::Error);

        if is_valid {
            if issues.is_empty() {
                ValidationResult::pass(score.max(0.0))
            } else {
                ValidationResult::pass_with_warnings(score.max(0.0), issues)
            }
        } else {
            ValidationResult::fail(score.max(0.0), issues)
        }
    }

    /// Validate a Skill artifact
    pub fn validate_skill(&self, skill: &Skill, registry: Option<&VerifiedFileRegistry>) -> ValidationResult {
        let mut issues = Vec::new();
        let mut score = 1.0f32;

        // Check body length
        if skill.body.len() < self.min_content_length {
            issues.push(ValidationIssue::warning(format!(
                "Skill body too short ({} chars, min {})",
                skill.body.len(),
                self.min_content_length
            )));
            score -= 0.2;
        }

        // Check for file references
        let file_ref_count = skill.body.matches("@").count();
        if file_ref_count < self.min_evidence_count {
            issues.push(ValidationIssue::warning(format!(
                "Skill lacks file references ({} found, min {})",
                file_ref_count, self.min_evidence_count
            )));
            score -= 0.2;
        }

        // Validate file references if registry provided
        if let Some(reg) = registry {
            let refs = extract_file_refs(&skill.body);
            for file_ref in refs {
                if !reg.contains(&file_ref) {
                    issues.push(ValidationIssue::warning(format!(
                        "Referenced file not found: {}",
                        file_ref
                    )));
                    score -= 0.1;
                }
            }
        }

        // Check description
        if skill.description.is_empty() {
            issues.push(ValidationIssue::warning("Skill lacks description"));
            score -= 0.1;
        }

        // Check for Tier 1 content
        let body_lower = skill.body.to_lowercase();
        let tier1_indicators = ["cargo build", "npm install", "pip install"];
        for indicator in tier1_indicators {
            if body_lower.contains(indicator) {
                issues.push(ValidationIssue::error(format!(
                    "Skill contains generic command: '{}'",
                    indicator
                )));
                score -= 0.3;
            }
        }

        let is_valid = score >= 0.5 && !issues.iter().any(|i| i.severity == IssueSeverity::Error);

        if is_valid {
            if issues.is_empty() {
                ValidationResult::pass(score.max(0.0))
            } else {
                ValidationResult::pass_with_warnings(score.max(0.0), issues)
            }
        } else {
            ValidationResult::fail(score.max(0.0), issues)
        }
    }

    /// Validate an Agent artifact
    pub fn validate_agent(&self, agent: &Agent, _registry: Option<&VerifiedFileRegistry>) -> ValidationResult {
        let mut issues = Vec::new();
        let mut score = 1.0f32;

        // Check instructions length
        if agent.prompt.len() < self.min_content_length {
            issues.push(ValidationIssue::warning(format!(
                "Agent instructions too short ({} chars, min {})",
                agent.prompt.len(),
                self.min_content_length
            )));
            score -= 0.2;
        }

        // Check description
        if agent.description.is_empty() {
            issues.push(ValidationIssue::warning("Agent lacks description"));
            score -= 0.1;
        }

        // Check for domain specificity
        let instructions_lower = agent.prompt.to_lowercase();
        let domain_indicators = [
            "domain", "business", "rule", "policy", "compliance",
            "expert", "specialist", "knowledge",
        ];
        let has_domain_content = domain_indicators
            .iter()
            .any(|w| instructions_lower.contains(w));

        if !has_domain_content {
            issues.push(ValidationIssue::warning(
                "Agent may lack domain-specific content",
            ));
            score -= 0.15;
        }

        let is_valid = score >= 0.5;

        if is_valid {
            if issues.is_empty() {
                ValidationResult::pass(score.max(0.0))
            } else {
                ValidationResult::pass_with_warnings(score.max(0.0), issues)
            }
        } else {
            ValidationResult::fail(score.max(0.0), issues)
        }
    }
}

impl Default for ArtifactValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract file references from text (format: @path/to/file:line)
fn extract_file_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();

    for word in text.split_whitespace() {
        if let Some(path) = word.strip_prefix('@') {
            let file_path = path.split(':').next().unwrap_or(path);
            if file_path.contains('/') || file_path.contains('.') {
                refs.push(file_path.to_string());
            }
        }
    }

    refs
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

    pub fn validate_rules(&self, rules: &[Rule]) -> BatchValidationResult {
        let results: Vec<_> = rules
            .iter()
            .map(|r| (r.name.clone(), self.validator.validate_rule(r, self.registry.as_ref())))
            .collect();

        BatchValidationResult::from_results(results)
    }

    pub fn validate_skills(&self, skills: &[Skill]) -> BatchValidationResult {
        let results: Vec<_> = skills
            .iter()
            .map(|s| (s.name.clone(), self.validator.validate_skill(s, self.registry.as_ref())))
            .collect();

        BatchValidationResult::from_results(results)
    }

    pub fn validate_agents(&self, agents: &[Agent]) -> BatchValidationResult {
        let results: Vec<_> = agents
            .iter()
            .map(|a| (a.name.clone(), self.validator.validate_agent(a, self.registry.as_ref())))
            .collect();

        BatchValidationResult::from_results(results)
    }
}

/// Result of batch validation
#[derive(Debug, Clone)]
pub struct BatchValidationResult {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub average_score: f32,
    pub failed_items: Vec<(String, Vec<ValidationIssue>)>,
}

impl BatchValidationResult {
    fn from_results(results: Vec<(String, ValidationResult)>) -> Self {
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

        let rule = Rule {
            name: "test-rule".to_string(),
            paths: None,
            content: vec![
                "# Test Rule".to_string(),
                "".to_string(),
                "You must follow this constraint.".to_string(),
                "Always validate input before processing.".to_string(),
            ],
            evidence: vec![EvidenceLocation {
                file: "src/main.rs".to_string(),
                start_line: 10,
                end_line: 10,
                start_column: None,
                end_column: None,
            }],
        };

        let result = validator.validate_rule(&rule, None);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_rule_fail_tier1() {
        let validator = ArtifactValidator::new();

        let rule = Rule {
            name: "generic-rule".to_string(),
            paths: None,
            content: vec![
                "# Generic Rule".to_string(),
                "".to_string(),
                "Use best practices when coding.".to_string(),
            ],
            evidence: Vec::new(),
        };

        let result = validator.validate_rule(&rule, None);
        assert!(!result.is_valid);
        assert!(result.issues.iter().any(|i| i.message.contains("Tier 1")));
    }

    #[test]
    fn test_validate_skill_pass() {
        let validator = ArtifactValidator::new();

        let skill = Skill::new(
            "test-skill",
            "A test skill for validation",
            "## Test Skill\n\nYou should check @src/main.rs:10 for implementation details.\n\nAlways validate inputs.",
        );

        let result = validator.validate_skill(&skill, None);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_skill_fail_tier1() {
        let validator = ArtifactValidator::new();

        // Skill with generic Tier 1 command
        let skill = Skill::new(
            "generic-skill",
            "A generic skill",
            "## Build Instructions\n\nRun cargo build to build the project.",
        );

        let result = validator.validate_skill(&skill, None);
        assert!(!result.is_valid);
        assert!(result.issues.iter().any(|i| i.message.contains("generic command")));
    }

    #[test]
    fn test_validate_skill_warning_short() {
        let validator = ArtifactValidator::new();

        let skill = Skill::new("short-skill", "Short", "Too short");

        let result = validator.validate_skill(&skill, None);
        // Short skill gets warnings but may still pass with reduced score
        assert!(result.score < 0.7);
        assert!(result.issues.iter().any(|i| i.message.contains("too short")));
    }

    #[test]
    fn test_extract_file_refs() {
        let text = "Check @src/main.rs:10 and @lib/utils.rs for details.";
        let refs = extract_file_refs(text);

        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"src/main.rs".to_string()));
        assert!(refs.contains(&"lib/utils.rs".to_string()));
    }

    #[test]
    fn test_batch_validation() {
        let batch = BatchValidator::new(None);

        let rules = vec![
            Rule {
                name: "valid-rule".to_string(),
                paths: None,
                content: vec!["You must validate all user input.".to_string()],
                evidence: vec![EvidenceLocation {
                    file: "src/validate.rs".to_string(),
                    start_line: 1,
                    end_line: 1,
                    start_column: None,
                    end_column: None,
                }],
            },
            Rule {
                name: "invalid-rule".to_string(),
                paths: None,
                content: vec!["Use best practices.".to_string()],
                evidence: Vec::new(),
            },
        ];

        let result = batch.validate_rules(&rules);
        assert_eq!(result.total, 2);
        assert_eq!(result.passed, 1);
        assert_eq!(result.failed, 1);
    }
}
