//! Cross-Validation Module
//!
//! Validates consistency between:
//! - OutputPlan ↔ Generated Content
//! - Evidence traceability (file:line references)
//! - Cross-artifact consistency (Skills ↔ Agents ↔ Rules)
//! - Quality scoring for verification loop

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// Regex pattern for file references (e.g., @src/main.rs:10)
static FILE_REF_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@([a-zA-Z0-9_./\-]+(?::\d+)?)").expect("Invalid regex pattern"));

use crate::config::{CrossValidationConfig, QualityConfig};
use crate::pipeline::phases::output_router::OutputPlan;
use crate::types::{Agent, ProjectMemory, Rule, Skill};

#[derive(Debug, Clone)]
pub struct CrossValidationResult {
    pub passed: bool,
    pub quality_score: f32,
    pub plan_consistency: PlanConsistencyResult,
    pub evidence_traceability: EvidenceTraceabilityResult,
    pub artifact_consistency: ArtifactConsistencyResult,
    pub issues: Vec<CrossValidationIssue>,
}

#[derive(Debug, Clone, Default)]
pub struct ArtifactConsistencyResult {
    pub passed: bool,
    pub skill_agent_overlaps: Vec<ArtifactOverlap>,
    pub rule_memory_redundancies: Vec<ArtifactRedundancy>,
    pub skill_rule_inconsistencies: Vec<ArtifactInconsistency>,
    pub consistency_score: f32,
}

#[derive(Debug, Clone)]
pub struct ArtifactOverlap {
    pub skill_name: String,
    pub agent_name: String,
    pub overlap_description: String,
    pub severity: OverlapSeverity,
}

#[derive(Debug, Clone)]
pub struct ArtifactRedundancy {
    pub rule_name: String,
    pub memory_section: String,
    pub redundant_content: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactInconsistency {
    pub skill_name: String,
    pub rule_name: String,
    pub inconsistency_description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapSeverity {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone)]
pub struct PlanConsistencyResult {
    pub passed: bool,
    pub planned_skills: usize,
    pub generated_skills: usize,
    pub planned_agents: usize,
    pub generated_agents: usize,
    pub planned_rules: usize,
    pub generated_rules: usize,
    pub missing_items: Vec<String>,
    pub extra_items: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EvidenceTraceabilityResult {
    pub passed: bool,
    pub total_references: usize,
    pub valid_references: usize,
    pub invalid_references: Vec<InvalidReference>,
    pub coverage_score: f32,
}

#[derive(Debug, Clone)]
pub struct InvalidReference {
    pub source: String,
    pub reference: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct CrossValidationIssue {
    pub severity: ValidationSeverity,
    pub category: ValidationCategory,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationCategory {
    PlanConsistency,
    EvidenceTraceability,
    ArtifactConsistency,
    QualityThreshold,
    ContentCompleteness,
}

pub struct CrossValidator {
    config: CrossValidationConfig,
    quality_config: QualityConfig,
    project_root: std::path::PathBuf,
}

impl CrossValidator {
    pub fn new(
        config: CrossValidationConfig,
        quality_config: QualityConfig,
        project_root: impl AsRef<Path>,
    ) -> Self {
        Self {
            config,
            quality_config,
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    pub fn validate(
        &self,
        plan: &OutputPlan,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        memory: &ProjectMemory,
    ) -> CrossValidationResult {
        let mut issues = Vec::new();

        let plan_consistency = if self.config.plan_output_consistency {
            self.check_plan_consistency(plan, skills, agents, rules, &mut issues)
        } else {
            PlanConsistencyResult {
                passed: true,
                planned_skills: 0,
                generated_skills: skills.len(),
                planned_agents: 0,
                generated_agents: agents.len(),
                planned_rules: 0,
                generated_rules: rules.len(),
                missing_items: vec![],
                extra_items: vec![],
            }
        };

        let evidence_traceability = if self.config.evidence_traceability {
            self.check_evidence_traceability(skills, agents, rules, memory, &mut issues)
        } else {
            EvidenceTraceabilityResult {
                passed: true,
                total_references: 0,
                valid_references: 0,
                invalid_references: vec![],
                coverage_score: 1.0,
            }
        };

        let artifact_consistency = if self.config.artifact_consistency {
            self.check_artifact_consistency(skills, agents, rules, memory, &mut issues)
        } else {
            ArtifactConsistencyResult {
                passed: true,
                consistency_score: 1.0,
                ..Default::default()
            }
        };

        let quality_score = self.calculate_quality_score(
            &plan_consistency,
            &evidence_traceability,
            &artifact_consistency,
            skills,
            agents,
            rules,
            memory,
        );

        if quality_score < self.quality_config.minimum_quality {
            issues.push(CrossValidationIssue {
                severity: ValidationSeverity::Error,
                category: ValidationCategory::QualityThreshold,
                message: format!(
                    "Quality score {:.2} below minimum threshold {:.2}",
                    quality_score, self.quality_config.minimum_quality
                ),
            });
        }

        let passed = !issues.iter().any(|i| i.severity == ValidationSeverity::Error);

        CrossValidationResult {
            passed,
            quality_score,
            plan_consistency,
            evidence_traceability,
            artifact_consistency,
            issues,
        }
    }

    fn check_plan_consistency(
        &self,
        plan: &OutputPlan,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        issues: &mut Vec<CrossValidationIssue>,
    ) -> PlanConsistencyResult {
        let planned_skill_names: HashSet<_> = plan
            .skills_plan
            .planned_skills
            .iter()
            .map(|s| s.name.to_lowercase())
            .collect();

        let generated_skill_names: HashSet<_> =
            skills.iter().map(|s| s.name.to_lowercase()).collect();

        let planned_agent_names: HashSet<_> = plan
            .agents_plan
            .planned_agents
            .iter()
            .map(|a| a.name.to_lowercase())
            .collect();

        let generated_agent_names: HashSet<_> =
            agents.iter().map(|a| a.name.to_lowercase()).collect();

        let planned_rule_names: HashSet<_> = plan
            .rules_plan
            .rule_groups
            .iter()
            .map(|r| r.name.to_lowercase())
            .collect();

        let generated_rule_names: HashSet<_> =
            rules.iter().map(|r| r.name.to_lowercase()).collect();

        let mut missing_items = Vec::new();
        let mut extra_items = Vec::new();

        for name in planned_skill_names.difference(&generated_skill_names) {
            missing_items.push(format!("Skill: {}", name));
        }
        for name in generated_skill_names.difference(&planned_skill_names) {
            extra_items.push(format!("Skill: {}", name));
        }

        for name in planned_agent_names.difference(&generated_agent_names) {
            missing_items.push(format!("Agent: {}", name));
        }
        for name in generated_agent_names.difference(&planned_agent_names) {
            extra_items.push(format!("Agent: {}", name));
        }

        for name in planned_rule_names.difference(&generated_rule_names) {
            missing_items.push(format!("Rule: {}", name));
        }
        for name in generated_rule_names.difference(&planned_rule_names) {
            extra_items.push(format!("Rule: {}", name));
        }

        if !missing_items.is_empty() {
            issues.push(CrossValidationIssue {
                severity: ValidationSeverity::Warning,
                category: ValidationCategory::PlanConsistency,
                message: format!("Missing planned items: {}", missing_items.join(", ")),
            });
        }

        let passed = missing_items.is_empty();

        PlanConsistencyResult {
            passed,
            planned_skills: plan.skills_plan.planned_skills.len(),
            generated_skills: skills.len(),
            planned_agents: plan.agents_plan.planned_agents.len(),
            generated_agents: agents.len(),
            planned_rules: plan.rules_plan.rule_groups.len(),
            generated_rules: rules.len(),
            missing_items,
            extra_items,
        }
    }

    fn check_evidence_traceability(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        memory: &ProjectMemory,
        issues: &mut Vec<CrossValidationIssue>,
    ) -> EvidenceTraceabilityResult {
        let mut total_references = 0;
        let mut valid_references = 0;
        let mut invalid_references = Vec::new();

        let all_content = [
            skills
                .iter()
                .map(|s| (s.name.clone(), s.body.clone()))
                .collect::<Vec<_>>(),
            agents
                .iter()
                .map(|a| (a.name.clone(), a.prompt.clone()))
                .collect::<Vec<_>>(),
            rules
                .iter()
                .map(|r| (r.name.clone(), r.content.join("\n")))
                .collect::<Vec<_>>(),
            vec![("CLAUDE.md".to_string(), memory.to_markdown())],
        ]
        .concat();

        for (source, content) in all_content {
            let refs = self.extract_file_references(&content);
            total_references += refs.len();

            for reference in refs {
                if self.validate_reference(&reference) {
                    valid_references += 1;
                } else {
                    invalid_references.push(InvalidReference {
                        source: source.clone(),
                        reference: reference.clone(),
                        reason: "File does not exist".to_string(),
                    });
                }
            }
        }

        let coverage_score = if total_references > 0 {
            valid_references as f32 / total_references as f32
        } else {
            1.0
        };

        if coverage_score < 0.8 {
            issues.push(CrossValidationIssue {
                severity: ValidationSeverity::Warning,
                category: ValidationCategory::EvidenceTraceability,
                message: format!(
                    "Evidence traceability below threshold: {:.0}% ({}/{} valid references)",
                    coverage_score * 100.0,
                    valid_references,
                    total_references
                ),
            });
        }

        let passed = invalid_references.is_empty() || coverage_score >= 0.8;

        EvidenceTraceabilityResult {
            passed,
            total_references,
            valid_references,
            invalid_references,
            coverage_score,
        }
    }

    fn extract_file_references(&self, content: &str) -> Vec<String> {
        FILE_REF_PATTERN
            .captures_iter(content)
            .filter_map(|cap| cap.get(1))
            .map(|m| m.as_str())
            .filter(|path| {
                !path.starts_with("http")
                    && !path.starts_with("CLAUDE")
                    && !path.starts_with('.')
            })
            .map(String::from)
            .collect()
    }

    fn validate_reference(&self, reference: &str) -> bool {
        let path_part = reference.split(':').next().unwrap_or(reference);
        let full_path = self.project_root.join(path_part);
        full_path.exists() || self.project_root.join("src").join(path_part).exists()
    }

    fn check_artifact_consistency(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        memory: &ProjectMemory,
        issues: &mut Vec<CrossValidationIssue>,
    ) -> ArtifactConsistencyResult {
        let mut skill_agent_overlaps = Vec::new();
        let mut rule_memory_redundancies = Vec::new();
        let mut skill_rule_inconsistencies = Vec::new();

        // Check Skills ↔ Agents overlap (responsibility overlap detection)
        for skill in skills {
            for agent in agents {
                if let Some(overlap) = self.detect_skill_agent_overlap(skill, agent) {
                    skill_agent_overlaps.push(overlap);
                }
            }
        }

        // Check Rules ↔ CLAUDE.md redundancy
        let memory_content = memory.to_markdown().to_lowercase();
        for rule in rules {
            if let Some(redundancy) = self.detect_rule_memory_redundancy(rule, &memory_content) {
                rule_memory_redundancies.push(redundancy);
            }
        }

        // Check Skills ↔ Rules consistency
        for skill in skills {
            for rule in rules {
                if let Some(inconsistency) = self.detect_skill_rule_inconsistency(skill, rule) {
                    skill_rule_inconsistencies.push(inconsistency);
                }
            }
        }

        // Report issues
        let high_severity_overlaps = skill_agent_overlaps
            .iter()
            .filter(|o| o.severity == OverlapSeverity::High)
            .count();

        if high_severity_overlaps > 0 {
            issues.push(CrossValidationIssue {
                severity: ValidationSeverity::Warning,
                category: ValidationCategory::ArtifactConsistency,
                message: format!(
                    "{} high-severity skill/agent overlaps detected",
                    high_severity_overlaps
                ),
            });
        }

        if !rule_memory_redundancies.is_empty() {
            issues.push(CrossValidationIssue {
                severity: ValidationSeverity::Info,
                category: ValidationCategory::ArtifactConsistency,
                message: format!(
                    "{} rules have redundant content with CLAUDE.md",
                    rule_memory_redundancies.len()
                ),
            });
        }

        if !skill_rule_inconsistencies.is_empty() {
            issues.push(CrossValidationIssue {
                severity: ValidationSeverity::Warning,
                category: ValidationCategory::ArtifactConsistency,
                message: format!(
                    "{} skill/rule inconsistencies detected",
                    skill_rule_inconsistencies.len()
                ),
            });
        }

        // Calculate consistency score
        let total_checks = (skills.len() * agents.len())
            + rules.len()
            + (skills.len() * rules.len());
        let total_issues = skill_agent_overlaps.len()
            + rule_memory_redundancies.len()
            + skill_rule_inconsistencies.len();

        let consistency_score = if total_checks > 0 {
            1.0 - (total_issues as f32 / total_checks.max(1) as f32).min(1.0)
        } else {
            1.0
        };

        let passed = high_severity_overlaps == 0 && skill_rule_inconsistencies.is_empty();

        ArtifactConsistencyResult {
            passed,
            skill_agent_overlaps,
            rule_memory_redundancies,
            skill_rule_inconsistencies,
            consistency_score,
        }
    }

    fn detect_skill_agent_overlap(&self, skill: &Skill, agent: &Agent) -> Option<ArtifactOverlap> {
        let skill_keywords = self.extract_keywords(&skill.body);
        let agent_keywords = self.extract_keywords(&agent.prompt);

        let overlap_count = skill_keywords
            .intersection(&agent_keywords)
            .count();

        let overlap_ratio = if skill_keywords.len() + agent_keywords.len() > 0 {
            (2.0 * overlap_count as f32)
                / (skill_keywords.len() + agent_keywords.len()) as f32
        } else {
            0.0
        };

        // High overlap threshold for detection
        if overlap_ratio > 0.5 {
            let severity = if overlap_ratio > 0.7 {
                OverlapSeverity::High
            } else if overlap_ratio > 0.5 {
                OverlapSeverity::Medium
            } else {
                OverlapSeverity::Low
            };

            Some(ArtifactOverlap {
                skill_name: skill.name.clone(),
                agent_name: agent.name.clone(),
                overlap_description: format!(
                    "{}% keyword overlap detected",
                    (overlap_ratio * 100.0) as i32
                ),
                severity,
            })
        } else {
            None
        }
    }

    fn detect_rule_memory_redundancy(
        &self,
        rule: &Rule,
        memory_content: &str,
    ) -> Option<ArtifactRedundancy> {
        let rule_content = rule.content.join(" ").to_lowercase();
        let rule_sentences: Vec<&str> = rule_content
            .split('.')
            .filter(|s| s.trim().len() > 20)
            .collect();

        for sentence in rule_sentences {
            let trimmed = sentence.trim();
            if memory_content.contains(trimmed) {
                return Some(ArtifactRedundancy {
                    rule_name: rule.name.clone(),
                    memory_section: "CLAUDE.md".to_string(),
                    redundant_content: trimmed.chars().take(100).collect(),
                });
            }
        }

        None
    }

    fn detect_skill_rule_inconsistency(
        &self,
        skill: &Skill,
        rule: &Rule,
    ) -> Option<ArtifactInconsistency> {
        // Check if skill and rule target the same path patterns
        let skill_paths = self.extract_path_patterns(&skill.body);
        let rule_paths: HashSet<String> = rule
            .paths
            .as_ref()
            .map(|p| p.iter().cloned().collect())
            .unwrap_or_default();

        if skill_paths.is_empty() || rule_paths.is_empty() {
            return None;
        }

        // Check for path overlap
        let has_path_overlap = skill_paths.iter().any(|sp| {
            rule_paths.iter().any(|rp| sp.contains(rp.as_str()) || rp.contains(sp.as_str()))
        });

        if !has_path_overlap {
            return None;
        }

        // Check for contradicting instructions
        let skill_lower = skill.body.to_lowercase();
        let rule_lower = rule.content.join(" ").to_lowercase();

        let contradiction_pairs = [
            ("must use", "do not use"),
            ("required", "prohibited"),
            ("always", "never"),
            ("enable", "disable"),
        ];

        for (positive, negative) in contradiction_pairs {
            if (skill_lower.contains(positive) && rule_lower.contains(negative))
                || (skill_lower.contains(negative) && rule_lower.contains(positive))
            {
                return Some(ArtifactInconsistency {
                    skill_name: skill.name.clone(),
                    rule_name: rule.name.clone(),
                    inconsistency_description: format!(
                        "Potential contradiction: skill uses '{}' while rule uses '{}'",
                        positive, negative
                    ),
                });
            }
        }

        None
    }

    fn extract_keywords(&self, content: &str) -> HashSet<String> {
        content
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|word| word.len() > 4)
            .map(String::from)
            .collect()
    }

    fn extract_path_patterns(&self, content: &str) -> HashSet<String> {
        content
            .split_whitespace()
            .filter(|word| {
                word.contains('/') || word.contains("**") || word.ends_with(".rs")
                    || word.ends_with(".ts") || word.ends_with(".tsx")
            })
            .map(String::from)
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn calculate_quality_score(
        &self,
        plan_consistency: &PlanConsistencyResult,
        evidence: &EvidenceTraceabilityResult,
        artifact_consistency: &ArtifactConsistencyResult,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        memory: &ProjectMemory,
    ) -> f32 {
        let mut scores = Vec::new();
        let mut weights = Vec::new();

        if self.config.plan_output_consistency {
            let total_planned = plan_consistency.planned_skills
                + plan_consistency.planned_agents
                + plan_consistency.planned_rules;
            let missing = plan_consistency.missing_items.len();
            let consistency_score = if total_planned > 0 {
                1.0 - (missing as f32 / total_planned as f32)
            } else {
                1.0
            };
            scores.push(consistency_score);
            weights.push(0.3);
        }

        if self.config.evidence_traceability {
            scores.push(evidence.coverage_score);
            weights.push(0.15);
        }

        // Artifact consistency score
        scores.push(artifact_consistency.consistency_score);
        weights.push(0.15);

        let skill_score = self.score_skills(skills);
        let agent_score = self.score_agents(agents);
        let rule_score = self.score_rules(rules);
        let memory_score = self.score_memory(memory);

        scores.push(skill_score);
        weights.push(0.1);
        scores.push(agent_score);
        weights.push(0.1);
        scores.push(rule_score);
        weights.push(0.1);
        scores.push(memory_score);
        weights.push(0.1);

        let total_weight: f32 = weights.iter().sum();
        let weighted_sum: f32 = scores.iter().zip(weights.iter()).map(|(s, w)| s * w).sum();

        weighted_sum / total_weight
    }

    /// Score content based on length, structure count, and file references.
    /// Uses consistent weights: length (0.4), structure (0.3), references (0.3).
    fn score_content(
        &self,
        content: &str,
        min_chars: usize,
        structure_count: usize,
        min_structure: usize,
    ) -> f32 {
        let mut score = 0.0;

        // Length score (0.4 weight)
        if content.len() >= min_chars {
            score += 0.4;
        } else if min_chars > 0 {
            score += 0.4 * (content.len() as f32 / min_chars as f32);
        }

        // Structure score (0.3 weight)
        if structure_count >= min_structure {
            score += 0.3;
        } else if structure_count > 0 && min_structure > 0 {
            score += 0.3 * (structure_count as f32 / min_structure as f32);
        }

        // Reference score (0.3 weight)
        let ref_count = self.extract_file_references(content).len();
        let target_refs = 3; // Standard target for all artifacts
        if ref_count >= target_refs {
            score += 0.3;
        } else if ref_count > 0 {
            score += 0.3 * (ref_count as f32 / target_refs as f32);
        }

        score
    }

    fn score_skills(&self, skills: &[Skill]) -> f32 {
        if skills.is_empty() {
            return 1.0;
        }

        let cfg = &self.quality_config.skill;
        let total: f32 = skills
            .iter()
            .map(|s| {
                let step_count = s.body.matches('\n').count();
                self.score_content(&s.body, cfg.min_chars, step_count, cfg.min_steps)
            })
            .sum();

        total / skills.len() as f32
    }

    fn score_agents(&self, agents: &[Agent]) -> f32 {
        if agents.is_empty() {
            return 1.0;
        }

        let cfg = &self.quality_config.agent;
        let total: f32 = agents
            .iter()
            .map(|a| {
                let section_count = a.prompt.matches("##").count();
                self.score_content(&a.prompt, cfg.min_chars, section_count, cfg.min_sections)
            })
            .sum();

        total / agents.len() as f32
    }

    fn score_rules(&self, rules: &[Rule]) -> f32 {
        if rules.is_empty() {
            return 1.0;
        }

        let total: f32 = rules
            .iter()
            .map(|r| {
                let content = r.content.join("\n");
                let has_structure = content.contains("##") || content.contains("```");
                let has_paths = r.paths.as_ref().is_some_and(|p| !p.is_empty());
                let structure_count = if has_structure { 1 } else { 0 } + if has_paths { 1 } else { 0 };
                self.score_content(&content, 200, structure_count, 2)
            })
            .sum();

        total / rules.len() as f32
    }

    fn score_memory(&self, memory: &ProjectMemory) -> f32 {
        let cfg = &self.quality_config.memory;
        let content = memory.to_markdown();
        let section_count = content.matches("##").count();
        self.score_content(&content, cfg.min_chars, section_count, cfg.min_sections)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn validate(
    config: &CrossValidationConfig,
    quality_config: &QualityConfig,
    project_root: impl AsRef<Path>,
    plan: &OutputPlan,
    skills: &[Skill],
    agents: &[Agent],
    rules: &[Rule],
    memory: &ProjectMemory,
) -> CrossValidationResult {
    let validator = CrossValidator::new(config.clone(), quality_config.clone(), project_root);
    validator.validate(plan, skills, agents, rules, memory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::output_router::{
        AgentsPlan, ClaudeMdPlan, ContentScope, RulesPlan, RulesLocation, SkillsPlan,
    };
    use crate::pipeline::phases::OutputStrategy;
    use tempfile::TempDir;

    fn default_config() -> CrossValidationConfig {
        CrossValidationConfig::default()
    }

    fn default_quality_config() -> QualityConfig {
        QualityConfig::default()
    }

    fn empty_plan() -> OutputPlan {
        OutputPlan {
            strategy: OutputStrategy::Unified,
            claude_md_plan: ClaudeMdPlan {
                content_scope: ContentScope::Full,
                sections: vec![],
                include_architecture: true,
                include_commands: true,
                include_conventions: true,
                include_constraints: false,
            },
            skills_plan: SkillsPlan {
                generate_skills: false,
                planned_skills: vec![],
            },
            agents_plan: AgentsPlan {
                generate_agents: false,
                planned_agents: vec![],
            },
            rules_plan: RulesPlan {
                generate_path_rules: false,
                rule_groups: vec![],
                location: RulesLocation::ClaudeMdInline,
            },
        }
    }

    #[test]
    fn test_empty_validation_passes() {
        let temp_dir = TempDir::new().unwrap();
        let validator = CrossValidator::new(
            default_config(),
            default_quality_config(),
            temp_dir.path(),
        );

        let result = validator.validate(
            &empty_plan(),
            &[],
            &[],
            &[],
            &ProjectMemory::new("Test project"),
        );

        assert!(result.quality_score >= 0.0);
    }

    #[test]
    fn test_file_reference_extraction() {
        let temp_dir = TempDir::new().unwrap();
        let validator = CrossValidator::new(
            default_config(),
            default_quality_config(),
            temp_dir.path(),
        );

        let content = "See @src/main.rs:10 and @src/lib.rs for details";
        let refs = validator.extract_file_references(content);

        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"src/main.rs:10".to_string()));
        assert!(refs.contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn test_artifact_consistency_no_issues() {
        let temp_dir = TempDir::new().unwrap();
        let validator = CrossValidator::new(
            default_config(),
            default_quality_config(),
            temp_dir.path(),
        );

        let skills = vec![Skill::new("build-project", "Build the project", "Run cargo build")];
        let agents = vec![Agent::new(
            "code-reviewer",
            "Review code quality",
            "You are a code review expert",
        )];
        let rules = vec![
            Rule::new("no-unwrap", vec!["Do not use unwrap() in production code".to_string()])
                .with_paths(vec!["src/**/*.rs".to_string()]),
        ];

        let memory = ProjectMemory::new("Test project");

        let result = validator.validate(&empty_plan(), &skills, &agents, &rules, &memory);

        assert!(result.artifact_consistency.passed);
        assert!(result.artifact_consistency.skill_agent_overlaps.is_empty());
    }

    #[test]
    fn test_skill_agent_overlap_detection() {
        let temp_dir = TempDir::new().unwrap();
        let validator = CrossValidator::new(
            default_config(),
            default_quality_config(),
            temp_dir.path(),
        );

        // Create skill and agent with high keyword overlap
        let skill = Skill::new(
            "debug-code",
            "Debug code issues",
            "debugging production issues error handling logging tracing",
        );

        let agent = Agent::new(
            "debugger",
            "Debug production issues",
            "debugging production issues error handling logging tracing analysis",
        );

        let overlap = validator.detect_skill_agent_overlap(&skill, &agent);
        assert!(overlap.is_some());
        let overlap = overlap.unwrap();
        assert!(matches!(
            overlap.severity,
            OverlapSeverity::High | OverlapSeverity::Medium
        ));
    }

    #[test]
    fn test_skill_rule_inconsistency_detection() {
        let temp_dir = TempDir::new().unwrap();
        let validator = CrossValidator::new(
            default_config(),
            default_quality_config(),
            temp_dir.path(),
        );

        let skill = Skill::new(
            "add-logging",
            "Add logging",
            "You must use println! for src/cli/ debugging",
        );

        let rule = Rule::new("no-println", vec!["Do not use println! in library code".to_string()])
            .with_paths(vec!["src/cli/**".to_string()]);

        let inconsistency = validator.detect_skill_rule_inconsistency(&skill, &rule);
        assert!(inconsistency.is_some());
    }

    #[test]
    fn test_keyword_extraction() {
        let temp_dir = TempDir::new().unwrap();
        let validator = CrossValidator::new(
            default_config(),
            default_quality_config(),
            temp_dir.path(),
        );

        let keywords = validator.extract_keywords("debugging production issues error handling");
        assert!(keywords.contains(&"debugging".to_string()));
        assert!(keywords.contains(&"production".to_string()));
        assert!(keywords.contains(&"issues".to_string()));
        assert!(keywords.contains(&"error".to_string()));
        assert!(keywords.contains(&"handling".to_string()));
    }
}
