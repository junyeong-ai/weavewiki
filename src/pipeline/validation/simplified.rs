//! Validation Types
//!
//! Validation for generated Claude Code artifacts.

use std::collections::HashSet;

use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::quality::{count_tier1_matches, count_tier3_matches};
use crate::types::{Agent, ContentTier, ProjectMemory, Rule, Skill};
use crate::utils::patterns::{extract_file_refs, FILE_REFERENCE_PATTERN};

const CLAUDE_MD_TIER1_THRESHOLD: usize = 3;
const TIER_PATTERN_THRESHOLD: usize = 2;

#[derive(Debug, Clone, Default)]
pub struct TierFilterResult {
    pub passed: bool,
    pub tier1_count: usize,
    pub tier2_count: usize,
    pub tier3_count: usize,
    pub tier3_ratio: f32,
}

impl TierFilterResult {
    pub fn check(skills: &[Skill], agents: &[Agent], rules: &[Rule], claude_md: &str) -> Self {
        let total = skills.len() + agents.len() + rules.len();
        if total == 0 {
            return Self {
                passed: true,
                ..Self::default()
            };
        }

        let (mut tier1, mut tier2, mut tier3) = (0, 0, 0);

        for skill in skills {
            match skill.quality.tier {
                ContentTier::Tier0Hallucinated | ContentTier::Tier1Generic => tier1 += 1,
                ContentTier::Tier2Convention => tier2 += 1,
                ContentTier::Tier3Constraint => tier3 += 1,
            }
        }

        for agent in agents {
            match agent.quality.tier {
                ContentTier::Tier0Hallucinated | ContentTier::Tier1Generic => tier1 += 1,
                ContentTier::Tier2Convention => tier2 += 1,
                ContentTier::Tier3Constraint => tier3 += 1,
            }
        }

        for rule in rules {
            match Self::classify_rule_tier(rule) {
                ContentTier::Tier0Hallucinated | ContentTier::Tier1Generic => tier1 += 1,
                ContentTier::Tier2Convention => tier2 += 1,
                ContentTier::Tier3Constraint => tier3 += 1,
            }
        }

        let claude_tier1 = count_tier1_matches(claude_md);
        if claude_tier1 >= CLAUDE_MD_TIER1_THRESHOLD {
            tier1 += 1;
        }

        Self {
            passed: tier1 == 0,
            tier1_count: tier1,
            tier2_count: tier2,
            tier3_count: tier3,
            tier3_ratio: tier3 as f32 / total as f32,
        }
    }

    fn classify_rule_tier(rule: &Rule) -> ContentTier {
        let content = rule.content.join("\n");
        let tier1_count = count_tier1_matches(&content);
        let tier3_count = count_tier3_matches(&content);
        let has_file_refs = FILE_REFERENCE_PATTERN.is_match(&content);

        if tier1_count >= TIER_PATTERN_THRESHOLD && tier3_count == 0 && !has_file_refs {
            ContentTier::Tier1Generic
        } else if tier3_count >= TIER_PATTERN_THRESHOLD || has_file_refs {
            ContentTier::Tier3Constraint
        } else {
            ContentTier::Tier2Convention
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConsistencyResult {
    pub passed: bool,
    pub issues: Vec<String>,
}

impl ConsistencyResult {
    pub fn check(is_monorepo: bool, skills: &[Skill], agents: &[Agent], rules: &[Rule]) -> Self {
        let mut issues = Vec::new();

        let mut skill_names: HashSet<&str> = HashSet::new();
        for skill in skills {
            if !skill_names.insert(&skill.name) {
                issues.push(format!("Duplicate skill name: {}", skill.name));
            }
        }

        let mut agent_names: HashSet<&str> = HashSet::new();
        for agent in agents {
            if !agent_names.insert(&agent.name) {
                issues.push(format!("Duplicate agent name: {}", agent.name));
            }
        }

        let mut rule_names: HashSet<&str> = HashSet::new();
        for rule in rules {
            if !rule_names.insert(&rule.name) {
                issues.push(format!("Duplicate rule name: {}", rule.name));
            }
        }

        for agent in agents {
            if let Some(ref agent_skills) = agent.skills {
                for skill_ref in agent_skills {
                    if !skill_names.contains(skill_ref.as_str()) {
                        issues.push(format!(
                            "Agent '{}' references non-existent skill: {}",
                            agent.name, skill_ref
                        ));
                    }
                }
            }
        }

        if is_monorepo && rules.is_empty() {
            issues.push("Monorepo detected but no path-based rules generated".to_string());
        }

        Self {
            passed: issues.is_empty(),
            issues,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CrossValidationResult {
    pub passed: bool,
    pub evidence_traceability: EvidenceTraceabilityResult,
    pub plan_consistency: PlanConsistencyResult,
}

impl CrossValidationResult {
    pub fn check(
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
        file_registry: &VerifiedFileRegistry,
    ) -> Self {
        let evidence_traceability =
            EvidenceTraceabilityResult::check(skills, agents, rules, claude_md, file_registry);
        let plan_consistency = PlanConsistencyResult::check(skills, agents, rules, claude_md);

        let passed =
            evidence_traceability.invalid_references == 0 && plan_consistency.passed;

        Self {
            passed,
            evidence_traceability,
            plan_consistency,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvidenceTraceabilityResult {
    pub coverage_score: f32,
    pub valid_references: usize,
    pub invalid_references: usize,
}

impl Default for EvidenceTraceabilityResult {
    fn default() -> Self {
        Self {
            coverage_score: 1.0,
            valid_references: 0,
            invalid_references: 0,
        }
    }
}

impl EvidenceTraceabilityResult {
    pub fn check(
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
        file_registry: &VerifiedFileRegistry,
    ) -> Self {
        let mut valid = 0;
        let mut invalid = 0;

        for skill in skills {
            let (v, i) = Self::validate_refs(&skill.body, file_registry);
            valid += v;
            invalid += i;
        }

        for agent in agents {
            let (v, i) = Self::validate_refs(&agent.prompt, file_registry);
            valid += v;
            invalid += i;
        }

        for rule in rules {
            let content = rule.content.join("\n");
            let (v, i) = Self::validate_refs(&content, file_registry);
            valid += v;
            invalid += i;
        }

        if let Some(ref arch) = claude_md.architecture {
            let (v, i) = Self::validate_refs(arch, file_registry);
            valid += v;
            invalid += i;
        }

        for standard in &claude_md.standards {
            let (v, i) = Self::validate_refs(standard, file_registry);
            valid += v;
            invalid += i;
        }

        let total = valid + invalid;
        let coverage_score = if total > 0 {
            valid as f32 / total as f32
        } else {
            1.0
        };

        Self {
            coverage_score,
            valid_references: valid,
            invalid_references: invalid,
        }
    }

    fn validate_refs(content: &str, registry: &VerifiedFileRegistry) -> (usize, usize) {
        let refs = extract_file_refs(content);
        let mut valid = 0;
        let mut invalid = 0;

        for file_ref in refs {
            if registry.contains(&file_ref.path) {
                if let Some(line) = file_ref.line_start {
                    if let Some(max_lines) = registry.line_count(&file_ref.path) {
                        if (line as usize) <= max_lines {
                            valid += 1;
                        } else {
                            invalid += 1;
                        }
                    } else {
                        valid += 1;
                    }
                } else {
                    valid += 1;
                }
            } else {
                invalid += 1;
            }
        }

        (valid, invalid)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlanConsistencyResult {
    pub passed: bool,
    pub missing_coverage: Vec<String>,
}

impl PlanConsistencyResult {
    pub fn check(
        skills: &[Skill],
        agents: &[Agent],
        _rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> Self {
        let mut missing_coverage = Vec::new();

        if claude_md.overview.is_empty() {
            missing_coverage.push("CLAUDE.md has no overview".to_string());
        }

        if skills.is_empty() && agents.is_empty() {
            missing_coverage.push("No skills or agents generated".to_string());
        }

        Self {
            passed: missing_coverage.is_empty(),
            missing_coverage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_filter_result_empty() {
        let result = TierFilterResult::check(&[], &[], &[], "");
        assert!(result.passed);
        assert_eq!(result.tier1_count, 0);
    }

    #[test]
    fn test_consistency_result_duplicate_names() {
        let skills = vec![
            Skill::new("test-skill", "desc", "body"),
            Skill::new("test-skill", "desc2", "body2"),
        ];

        let result = ConsistencyResult::check(false, &skills, &[], &[]);

        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.contains("Duplicate")));
    }

    #[test]
    fn test_plan_consistency_empty_overview() {
        let claude_md = ProjectMemory {
            overview: String::new(),
            architecture: None,
            commands: Vec::new(),
            standards: Vec::new(),
            imports: Vec::new(),
        };

        let result = PlanConsistencyResult::check(&[], &[], &[], &claude_md);

        assert!(!result.passed);
        assert!(result.missing_coverage.iter().any(|m| m.contains("overview")));
    }
}
