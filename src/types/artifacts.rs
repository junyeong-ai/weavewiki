//! Generated artifacts container
//!
//! Simple container for generated rules, skills, and agents.
//! Path helpers for .claude/ directory structure.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::agent::Agent;
use super::rule::Rule;
use super::skill::Skill;
use super::validation::ValidationIssue;

/// Container for generated Claude Code artifacts
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedArtifacts {
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub rules: Vec<Rule>,
}

impl GeneratedArtifacts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn skills(mut self, skills: Vec<Skill>) -> Self {
        self.skills = skills;
        self
    }

    pub fn agents(mut self, agents: Vec<Agent>) -> Self {
        self.agents = agents;
        self
    }

    pub fn rules(mut self, rules: Vec<Rule>) -> Self {
        self.rules = rules;
        self
    }

    pub fn validate(&self) -> ArtifactValidationResult {
        let mut skill_names = HashSet::new();
        let skill_errors: Vec<_> = self
            .skills
            .iter()
            .filter_map(|skill| {
                let mut issues = skill.validate();
                if !skill_names.insert(skill.name.clone()) {
                    issues.push(ValidationIssue::error(
                        "DUPLICATE_SKILL_NAME",
                        format!("duplicate skill name: '{}'", skill.name),
                    ));
                }
                if issues.is_empty() {
                    None
                } else {
                    Some((skill.name.clone(), issues))
                }
            })
            .collect();

        let mut agent_names = HashSet::new();
        let agent_errors: Vec<_> = self
            .agents
            .iter()
            .filter_map(|agent| {
                let mut issues = agent.validate();
                if !agent_names.insert(agent.name.clone()) {
                    issues.push(ValidationIssue::error(
                        "DUPLICATE_AGENT_NAME",
                        format!("duplicate agent name: '{}'", agent.name),
                    ));
                }
                if issues.is_empty() {
                    None
                } else {
                    Some((agent.name.clone(), issues))
                }
            })
            .collect();

        let mut rule_names = HashSet::new();
        let rule_errors: Vec<_> = self
            .rules
            .iter()
            .filter_map(|rule| {
                let mut issues = rule.validate();
                if !rule_names.insert(rule.name.clone()) {
                    issues.push(ValidationIssue::error(
                        "DUPLICATE_RULE_NAME",
                        format!("duplicate rule name: '{}'", rule.name),
                    ));
                }
                if issues.is_empty() {
                    None
                } else {
                    Some((rule.name.clone(), issues))
                }
            })
            .collect();

        ArtifactValidationResult {
            skill_errors,
            agent_errors,
            rule_errors,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArtifactValidationResult {
    pub skill_errors: Vec<(String, Vec<ValidationIssue>)>,
    pub agent_errors: Vec<(String, Vec<ValidationIssue>)>,
    pub rule_errors: Vec<(String, Vec<ValidationIssue>)>,
}

impl ArtifactValidationResult {
    pub fn is_valid(&self) -> bool {
        self.skill_errors.is_empty() && self.agent_errors.is_empty() && self.rule_errors.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.skill_errors
            .iter()
            .flat_map(|(_, e)| e)
            .filter(|e| e.is_error())
            .count()
            + self
                .agent_errors
                .iter()
                .flat_map(|(_, e)| e)
                .filter(|e| e.is_error())
                .count()
            + self
                .rule_errors
                .iter()
                .flat_map(|(_, e)| e)
                .filter(|e| e.is_error())
                .count()
    }

    pub fn warning_count(&self) -> usize {
        self.skill_errors
            .iter()
            .flat_map(|(_, e)| e)
            .filter(|e| e.severity.is_warning())
            .count()
            + self
                .agent_errors
                .iter()
                .flat_map(|(_, e)| e)
                .filter(|e| e.severity.is_warning())
                .count()
            + self
                .rule_errors
                .iter()
                .flat_map(|(_, e)| e)
                .filter(|e| e.severity.is_warning())
                .count()
    }
}

/// Content structure for CLAUDE.md generation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeMdContent {
    pub overview: String,
    pub architecture: Option<String>,
    pub commands: Vec<String>,
    pub standards: Vec<String>,
    pub imports: Vec<String>,
    pub domain_knowledge: Option<String>,
    pub gotchas: Vec<String>,
    pub navigation: Option<String>,
}

impl ClaudeMdContent {
    pub fn new(overview: impl Into<String>) -> Self {
        Self {
            overview: overview.into(),
            ..Default::default()
        }
    }

    pub fn to_markdown(&self) -> String {
        let mut sections = Vec::new();

        sections.push(format!("# Project Overview\n\n{}", self.overview));

        if let Some(ref arch) = self.architecture {
            sections.push(format!("## Architecture\n\n{arch}"));
        }

        if !self.commands.is_empty() {
            sections.push(format!("## Commands\n\n{}", self.commands.join("\n")));
        }

        if !self.standards.is_empty() {
            sections.push(format!("## Standards\n\n{}", self.standards.join("\n")));
        }

        if let Some(ref domain) = self.domain_knowledge {
            sections.push(format!("## Domain Knowledge\n\n{domain}"));
        }

        if !self.gotchas.is_empty() {
            sections.push(format!("## Gotchas\n\n{}", self.gotchas.join("\n")));
        }

        if let Some(ref nav) = self.navigation {
            sections.push(format!("## Navigation\n\n{nav}"));
        }

        if !self.imports.is_empty() {
            sections.push(format!("## Imports\n\n{}", self.imports.join("\n")));
        }

        sections.join("\n\n")
    }
}

/// Output directory structure helper
pub struct OutputPaths;

impl OutputPaths {
    /// Claude Code configuration directory (.claude/)
    ///
    /// ```text
    /// .claude/
    /// ├── rules/                   # Auto-loaded by Claude Code
    /// │   ├── project.md
    /// │   ├── tech/{lang}.md
    /// │   ├── frameworks/{fw}.md
    /// │   ├── modules/{module}.md
    /// │   ├── groups/{group}.md
    /// │   └── domains/{domain}.md
    /// ├── skills/                  # Model-invoked skills
    /// │   └── {skill}/SKILL.md
    /// └── agents/                  # Agent definitions
    ///     └── {agent}.md
    /// ```
    pub fn claude_dir(base: &Path) -> PathBuf {
        base.join(".claude")
    }

    /// claudegen internal state directory (.claudegen/)
    /// Contains manifest.json for tracking generated state
    pub fn metadata_dir(base: &Path) -> PathBuf {
        base.join(".claudegen")
    }

    /// Rules directory (.claude/rules/)
    pub fn rules_dir(base: &Path) -> PathBuf {
        Self::claude_dir(base).join("rules")
    }

    /// Skills directory (.claude/skills/)
    pub fn skills_dir(base: &Path) -> PathBuf {
        Self::claude_dir(base).join("skills")
    }

    /// Agents directory (.claude/agents/)
    pub fn agents_dir(base: &Path) -> PathBuf {
        Self::claude_dir(base).join("agents")
    }

    /// Internal manifest path (.claudegen/manifest.json)
    pub fn manifest_path(base: &Path) -> PathBuf {
        Self::metadata_dir(base).join("manifest.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_paths() {
        let base = Path::new("/project");

        assert_eq!(
            OutputPaths::claude_dir(base),
            PathBuf::from("/project/.claude")
        );
        assert_eq!(
            OutputPaths::metadata_dir(base),
            PathBuf::from("/project/.claudegen")
        );
        assert_eq!(
            OutputPaths::rules_dir(base),
            PathBuf::from("/project/.claude/rules")
        );
        assert_eq!(
            OutputPaths::skills_dir(base),
            PathBuf::from("/project/.claude/skills")
        );
        assert_eq!(
            OutputPaths::agents_dir(base),
            PathBuf::from("/project/.claude/agents")
        );
        assert_eq!(
            OutputPaths::manifest_path(base),
            PathBuf::from("/project/.claudegen/manifest.json")
        );
    }

    #[test]
    fn test_generated_artifacts_builder() {
        let artifacts = GeneratedArtifacts::new()
            .skills(vec![])
            .agents(vec![])
            .rules(vec![]);

        assert!(artifacts.skills.is_empty());
        assert!(artifacts.agents.is_empty());
        assert!(artifacts.rules.is_empty());
    }

    #[test]
    fn test_validation_result() {
        let result = ArtifactValidationResult::default();
        assert!(result.is_valid());
        assert_eq!(result.error_count(), 0);
        assert_eq!(result.warning_count(), 0);
    }
}
