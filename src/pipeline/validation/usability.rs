//! Usability Validator
//!
//! Validates that generated content is actually useful for AI coding assistance.
//! Focuses on practical effectiveness rather than surface metrics.

use serde::{Deserialize, Serialize};

use crate::config::{ProjectType, UsabilityConfig};
use crate::types::{Agent, ProjectMemory, Rule, Skill};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsabilityResult {
    pub passed: bool,
    pub score: f32,
    pub progressive_disclosure: ProgressiveDisclosureScore,
    pub context_efficiency: ContextEfficiencyScore,
    pub task_relevance: TaskRelevanceScore,
    pub issues: Vec<UsabilityIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressiveDisclosureScore {
    pub score: f32,
    pub claude_md_is_entry_point: bool,
    pub rules_add_value: bool,
    pub skills_are_actionable: bool,
    pub agents_have_clear_scope: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEfficiencyScore {
    pub score: f32,
    pub total_tokens_estimate: usize,
    pub redundancy_ratio: f32,
    pub essential_content_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRelevanceScore {
    pub score: f32,
    pub covered_tasks: Vec<String>,
    pub missing_common_tasks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsabilityIssue {
    pub category: UsabilityCategory,
    pub severity: Severity,
    pub artifact: String,
    pub description: String,
    pub fix_suggestion: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsabilityCategory {
    TooVerbose,
    TooTerse,
    MissingEntryPoint,
    UnclearScope,
    RedundantContent,
    MissingCommonTask,
    PoorProgression,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

pub struct UsabilityValidator {
    project_type: ProjectType,
    config: UsabilityConfig,
}

impl UsabilityValidator {
    pub fn new(project_type: ProjectType) -> Self {
        Self {
            project_type,
            config: UsabilityConfig::default(),
        }
    }

    pub fn with_config(mut self, config: UsabilityConfig) -> Self {
        self.config = config;
        self
    }

    pub fn validate(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> UsabilityResult {
        let mut issues = Vec::new();
        let t = &self.config.thresholds;

        let progressive_disclosure =
            self.check_progressive_disclosure(skills, agents, rules, claude_md, &mut issues);
        let context_efficiency =
            self.check_context_efficiency(skills, agents, rules, claude_md, &mut issues);
        let task_relevance = self.check_task_relevance(skills, agents, &mut issues);

        let score = (progressive_disclosure.score * t.progressive_disclosure_weight
            + context_efficiency.score * t.context_efficiency_weight
            + task_relevance.score * t.task_relevance_weight)
            .clamp(0.0, 1.0);

        let passed = score >= self.config.min_usability_score;

        UsabilityResult {
            passed,
            score,
            progressive_disclosure,
            context_efficiency,
            task_relevance,
            issues,
        }
    }

    fn check_progressive_disclosure(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
        issues: &mut Vec<UsabilityIssue>,
    ) -> ProgressiveDisclosureScore {
        let claude_md_content = claude_md.to_markdown();

        let claude_md_is_entry_point = self.is_valid_entry_point(&claude_md_content, issues);
        let rules_add_value = self.rules_add_value(rules, &claude_md_content, issues);
        let skills_are_actionable = self.skills_are_actionable(skills, issues);
        let agents_have_clear_scope = self.agents_have_clear_scope(agents, issues);

        let checks = [
            claude_md_is_entry_point,
            rules_add_value,
            skills_are_actionable,
            agents_have_clear_scope,
        ];

        let score = checks.iter().filter(|&&b| b).count() as f32 / checks.len() as f32;

        ProgressiveDisclosureScore {
            score,
            claude_md_is_entry_point,
            rules_add_value,
            skills_are_actionable,
            agents_have_clear_scope,
        }
    }

    fn is_valid_entry_point(&self, content: &str, issues: &mut Vec<UsabilityIssue>) -> bool {
        let t = &self.config.thresholds;
        let lines: Vec<_> = content.lines().collect();
        let has_overview =
            content.contains("# Project") || content.contains("## Overview") || lines.len() > 5;
        let has_architecture =
            content.to_lowercase().contains("architecture") || content.contains("## ");
        let not_too_short = content.len() > t.min_entry_point_length;
        let not_too_long = content.len() < t.max_entry_point_length;

        if !not_too_short {
            issues.push(UsabilityIssue {
                category: UsabilityCategory::TooTerse,
                severity: Severity::Error,
                artifact: "CLAUDE.md".into(),
                description: "CLAUDE.md is too short to be useful as entry point".into(),
                fix_suggestion: "Add project overview, architecture, and key modules".into(),
            });
        }

        if !not_too_long {
            issues.push(UsabilityIssue {
                category: UsabilityCategory::TooVerbose,
                severity: Severity::Warning,
                artifact: "CLAUDE.md".into(),
                description: "CLAUDE.md is too long - details should go to rules".into(),
                fix_suggestion: "Move detailed conventions to .claude/rules/".into(),
            });
        }

        has_overview && has_architecture && not_too_short && not_too_long
    }

    fn rules_add_value(
        &self,
        rules: &[Rule],
        claude_md_content: &str,
        issues: &mut Vec<UsabilityIssue>,
    ) -> bool {
        if rules.is_empty() {
            return true;
        }

        let t = &self.config.thresholds;
        let mut valuable_rules = 0;

        for rule in rules {
            let rule_content = rule.to_markdown();
            let unique_content = self.calculate_unique_content(&rule_content, claude_md_content);

            if unique_content > t.min_unique_content_ratio {
                valuable_rules += 1;
            } else {
                issues.push(UsabilityIssue {
                    category: UsabilityCategory::RedundantContent,
                    severity: Severity::Warning,
                    artifact: format!("rule:{}", rule.name),
                    description: format!(
                        "Rule '{}' has significant overlap with CLAUDE.md",
                        rule.name
                    ),
                    fix_suggestion: "Remove duplicated content or consolidate".into(),
                });
            }
        }

        valuable_rules as f32 / rules.len() as f32 > t.min_valuable_rules_ratio
    }

    fn calculate_unique_content(&self, content: &str, reference: &str) -> f32 {
        let t = &self.config.thresholds;
        let content_words: std::collections::HashSet<_> = content
            .split_whitespace()
            .filter(|w| w.len() > t.min_word_length_for_uniqueness)
            .map(|w| w.to_lowercase())
            .collect();

        let reference_words: std::collections::HashSet<_> = reference
            .split_whitespace()
            .filter(|w| w.len() > t.min_word_length_for_uniqueness)
            .map(|w| w.to_lowercase())
            .collect();

        if content_words.is_empty() {
            return 0.0;
        }

        let unique: std::collections::HashSet<_> =
            content_words.difference(&reference_words).collect();
        unique.len() as f32 / content_words.len() as f32
    }

    fn skills_are_actionable(&self, skills: &[Skill], issues: &mut Vec<UsabilityIssue>) -> bool {
        if skills.is_empty() {
            return true;
        }

        let t = &self.config.thresholds;
        let mut actionable_count = 0;

        for skill in skills {
            let has_steps =
                skill.body.contains("Step") || skill.body.contains("1.") || skill.body.contains("- ");
            let has_references = skill.body.contains("@") || skill.body.contains("src/");
            let has_gotchas = skill.body.to_lowercase().contains("gotcha")
                || skill.body.to_lowercase().contains("note");

            if has_steps && (has_references || has_gotchas) {
                actionable_count += 1;
            } else {
                issues.push(UsabilityIssue {
                    category: UsabilityCategory::UnclearScope,
                    severity: Severity::Warning,
                    artifact: format!("skill:{}", skill.name),
                    description: format!("Skill '{}' lacks clear actionable steps", skill.name),
                    fix_suggestion: "Add numbered steps with @file references".into(),
                });
            }
        }

        actionable_count as f32 / skills.len() as f32 > t.min_actionable_ratio
    }

    fn agents_have_clear_scope(&self, agents: &[Agent], issues: &mut Vec<UsabilityIssue>) -> bool {
        if agents.is_empty() {
            return true;
        }

        let t = &self.config.thresholds;
        let mut clear_scope_count = 0;

        for agent in agents {
            let has_responsibilities = agent.prompt.contains("Responsibilit")
                || agent.prompt.contains("•")
                || agent.prompt.contains("- ");
            let has_references = agent.prompt.contains("@") || agent.prompt.contains("src/");
            let has_clear_description = agent.description.len() > t.min_description_length;

            if has_responsibilities && has_references && has_clear_description {
                clear_scope_count += 1;
            } else {
                issues.push(UsabilityIssue {
                    category: UsabilityCategory::UnclearScope,
                    severity: Severity::Warning,
                    artifact: format!("agent:{}", agent.name),
                    description: format!(
                        "Agent '{}' has unclear scope or responsibilities",
                        agent.name
                    ),
                    fix_suggestion: "Add clear responsibilities with @file:line references".into(),
                });
            }
        }

        clear_scope_count as f32 / agents.len() as f32 > t.min_clear_scope_ratio
    }

    fn check_context_efficiency(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
        issues: &mut Vec<UsabilityIssue>,
    ) -> ContextEfficiencyScore {
        let t = &self.config.thresholds;
        let max_tokens = self.config.max_context_tokens;

        let total_content = format!(
            "{}\n{}\n{}\n{}",
            claude_md.to_markdown(),
            skills
                .iter()
                .map(|s| s.body.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            agents
                .iter()
                .map(|a| a.prompt.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            rules
                .iter()
                .map(|r| r.to_markdown())
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let total_tokens =
            (total_content.split_whitespace().count() as f32 * t.tokens_per_word_estimate) as usize;
        let redundancy_ratio = self.calculate_redundancy(&total_content);
        let essential_ratio = self.calculate_essential_content_ratio(&total_content);

        if total_tokens > max_tokens {
            issues.push(UsabilityIssue {
                category: UsabilityCategory::TooVerbose,
                severity: Severity::Warning,
                artifact: "all".into(),
                description: format!(
                    "Total content ~{} tokens exceeds recommended {}",
                    total_tokens, max_tokens
                ),
                fix_suggestion: "Reduce verbosity, focus on essential information".into(),
            });
        }

        let token_score = if total_tokens <= max_tokens {
            1.0
        } else {
            max_tokens as f32 / total_tokens as f32
        };
        let redundancy_score = 1.0 - redundancy_ratio;
        let score = (token_score * t.token_score_weight
            + redundancy_score * t.redundancy_score_weight
            + essential_ratio * t.essential_ratio_weight)
            .clamp(0.0, 1.0);

        ContextEfficiencyScore {
            score,
            total_tokens_estimate: total_tokens,
            redundancy_ratio,
            essential_content_ratio: essential_ratio,
        }
    }

    fn calculate_redundancy(&self, content: &str) -> f32 {
        let t = &self.config.thresholds;
        let words: Vec<_> = content
            .split_whitespace()
            .filter(|w| w.len() > t.min_word_length_for_uniqueness)
            .map(|w| w.to_lowercase())
            .collect();

        if words.is_empty() {
            return 0.0;
        }

        let unique: std::collections::HashSet<_> = words.iter().collect();
        1.0 - (unique.len() as f32 / words.len() as f32)
    }

    fn calculate_essential_content_ratio(&self, content: &str) -> f32 {
        let total_lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        if total_lines.is_empty() {
            return 0.0;
        }

        let essential_lines = total_lines
            .iter()
            .filter(|l| {
                let t = l.trim();
                t.contains("@") || t.contains("src/") || t.starts_with("#") || t.starts_with("-") || t.starts_with("•") || t.contains("must") || t.contains("should") || t.contains("avoid")
            })
            .count();

        essential_lines as f32 / total_lines.len() as f32
    }

    fn check_task_relevance(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        issues: &mut Vec<UsabilityIssue>,
    ) -> TaskRelevanceScore {
        let common_tasks = self.get_common_tasks();
        let mut covered = Vec::new();
        let mut missing = Vec::new();

        let all_content: String = skills
            .iter()
            .map(|s| s.body.to_lowercase())
            .chain(agents.iter().map(|a| a.prompt.to_lowercase()))
            .collect::<Vec<_>>()
            .join(" ");

        for task in &common_tasks {
            if all_content.contains(&task.to_lowercase()) {
                covered.push(task.clone());
            } else {
                missing.push(task.clone());
            }
        }

        for task in &missing {
            if self.is_task_important_for_project_type(task) {
                issues.push(UsabilityIssue {
                    category: UsabilityCategory::MissingCommonTask,
                    severity: Severity::Info,
                    artifact: "skills/agents".into(),
                    description: format!("Common task '{}' is not covered", task),
                    fix_suggestion: format!("Consider adding skill or agent for '{}'", task),
                });
            }
        }

        let score = if common_tasks.is_empty() {
            1.0
        } else {
            covered.len() as f32 / common_tasks.len() as f32
        };

        TaskRelevanceScore {
            score,
            covered_tasks: covered,
            missing_common_tasks: missing,
        }
    }

    fn get_common_tasks(&self) -> Vec<String> {
        match self.project_type {
            ProjectType::Cli => vec![
                "add command".into(),
                "debug".into(),
                "test".into(),
                "build".into(),
            ],
            ProjectType::Backend => vec![
                "add endpoint".into(),
                "database".into(),
                "debug".into(),
                "test".into(),
            ],
            ProjectType::Frontend => vec![
                "add component".into(),
                "state".into(),
                "test".into(),
                "build".into(),
            ],
            ProjectType::Library => vec![
                "add api".into(),
                "document".into(),
                "test".into(),
                "release".into(),
            ],
            _ => vec![
                "debug".into(),
                "test".into(),
                "build".into(),
            ],
        }
    }

    fn is_task_important_for_project_type(&self, task: &str) -> bool {
        let important_tasks = self.get_common_tasks();
        important_tasks.iter().take(2).any(|t| t == task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usability_validator_creation() {
        let validator = UsabilityValidator::new(ProjectType::Cli);
        assert_eq!(validator.config.min_usability_score, 0.7);
    }

    #[test]
    fn test_empty_validation() {
        let validator = UsabilityValidator::new(ProjectType::Auto);
        let claude_md = ProjectMemory::default();
        let result = validator.validate(&[], &[], &[], &claude_md);
        assert!(result.score >= 0.0);
    }
}
