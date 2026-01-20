//! Project Consistency Checker
//!
//! Validates that generated content is consistent with the detected project type.
//! Prevents generating backend patterns for frontend projects, etc.

use crate::config::ProjectType;
use crate::types::{Agent, Rule, Skill};

const CLI_KEYWORDS: &[&str] = &[
    "command", "subcommand", "argument", "flag", "option",
    "clap", "structopt", "cli", "terminal", "stdout",
];

const BACKEND_KEYWORDS: &[&str] = &[
    "api", "endpoint", "controller", "service", "repository",
    "database", "migration", "http", "rest", "graphql",
    "middleware", "authentication", "authorization",
    "request", "response", "route",
];

const FRONTEND_KEYWORDS: &[&str] = &[
    "component", "page", "hook", "state", "props",
    "react", "vue", "angular", "dom", "css",
    "style", "render", "ui", "ux", "layout",
];

const LIBRARY_KEYWORDS: &[&str] = &[
    "public api", "export", "semver", "breaking change",
    "version bump", "changelog", "documentation",
    "feature flag", "crate", "package",
];

const MONOREPO_KEYWORDS: &[&str] = &[
    "workspace", "monorepo", "package", "cross-project",
    "shared", "dependency", "turbo", "lerna", "nx",
    "pnpm workspace", "yarn workspace",
];

#[derive(Debug, Clone)]
pub struct ConsistencyResult {
    pub passed: bool,
    pub issues: Vec<ConsistencyIssue>,
}

#[derive(Debug, Clone)]
pub struct ConsistencyIssue {
    pub severity: IssueSeverity,
    pub item_type: String,
    pub item_name: String,
    pub message: String,
    pub expected_type: ProjectType,
    pub detected_type: ProjectType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Error,
    Warning,
}

pub struct ProjectConsistencyChecker {
    project_type: ProjectType,
    is_monorepo: bool,
}

impl ProjectConsistencyChecker {
    pub fn new(project_type: ProjectType, is_monorepo: bool) -> Self {
        Self {
            project_type,
            is_monorepo,
        }
    }

    pub fn check(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
    ) -> ConsistencyResult {
        let mut issues = Vec::new();

        for skill in skills {
            if let Some(issue) = self.check_skill(skill) {
                issues.push(issue);
            }
        }

        for agent in agents {
            if let Some(issue) = self.check_agent(agent) {
                issues.push(issue);
            }
        }

        for rule in rules {
            if let Some(issue) = self.check_rule(rule) {
                issues.push(issue);
            }
        }

        let has_errors = issues.iter().any(|i| i.severity == IssueSeverity::Error);

        ConsistencyResult {
            passed: !has_errors,
            issues,
        }
    }

    fn check_skill(&self, skill: &Skill) -> Option<ConsistencyIssue> {
        let content = format!(
            "{} {} {}",
            skill.name, skill.description, skill.body
        )
        .to_lowercase();

        let detected = self.detect_content_type(&content);

        if !self.is_compatible(detected) {
            return Some(ConsistencyIssue {
                severity: IssueSeverity::Warning,
                item_type: "Skill".to_string(),
                item_name: skill.name.clone(),
                message: format!(
                    "Skill appears to be for {} project but detected project type is {}",
                    detected, self.project_type
                ),
                expected_type: self.project_type,
                detected_type: detected,
            });
        }

        None
    }

    fn check_agent(&self, agent: &Agent) -> Option<ConsistencyIssue> {
        let prompt_content = agent.prompt.as_str();
        let content = format!(
            "{} {} {}",
            agent.name, agent.description, prompt_content
        )
        .to_lowercase();

        let detected = self.detect_content_type(&content);

        if !self.is_compatible(detected) {
            return Some(ConsistencyIssue {
                severity: IssueSeverity::Warning,
                item_type: "Agent".to_string(),
                item_name: agent.name.clone(),
                message: format!(
                    "Agent appears to be for {} project but detected project type is {}",
                    detected, self.project_type
                ),
                expected_type: self.project_type,
                detected_type: detected,
            });
        }

        None
    }

    fn check_rule(&self, rule: &Rule) -> Option<ConsistencyIssue> {
        let content = format!("{} {}", rule.name, rule.content.join(" ")).to_lowercase();

        let detected = self.detect_content_type(&content);

        if !self.is_compatible(detected) {
            return Some(ConsistencyIssue {
                severity: IssueSeverity::Warning,
                item_type: "Rule".to_string(),
                item_name: rule.name.clone(),
                message: format!(
                    "Rule appears to be for {} project but detected project type is {}",
                    detected, self.project_type
                ),
                expected_type: self.project_type,
                detected_type: detected,
            });
        }

        None
    }

    fn detect_content_type(&self, content: &str) -> ProjectType {
        let mut scores: Vec<(ProjectType, usize)> = vec![
            (ProjectType::Cli, self.count_keywords(content, CLI_KEYWORDS)),
            (ProjectType::Backend, self.count_keywords(content, BACKEND_KEYWORDS)),
            (ProjectType::Frontend, self.count_keywords(content, FRONTEND_KEYWORDS)),
            (ProjectType::Library, self.count_keywords(content, LIBRARY_KEYWORDS)),
            (ProjectType::Monorepo, self.count_keywords(content, MONOREPO_KEYWORDS)),
        ];

        scores.sort_by(|a, b| b.1.cmp(&a.1));

        if scores[0].1 == 0 {
            return ProjectType::Auto;
        }

        if scores[0].1 > scores[1].1 * 2 {
            return scores[0].0;
        }

        ProjectType::Hybrid
    }

    fn count_keywords(&self, content: &str, keywords: &[&str]) -> usize {
        keywords.iter().filter(|kw| content.contains(*kw)).count()
    }

    fn is_compatible(&self, content_type: ProjectType) -> bool {
        if content_type == ProjectType::Auto || content_type == ProjectType::Hybrid {
            return true;
        }

        if self.project_type == ProjectType::Auto || self.project_type == ProjectType::Hybrid {
            return true;
        }

        // Monorepo-specific content is compatible with monorepo projects
        if self.is_monorepo && content_type == ProjectType::Monorepo {
            return true;
        }

        // For monorepos, we're more permissive but still require some relationship
        // Allow cross-type content only if it's related to the primary type
        // or involves monorepo-specific patterns
        if self.is_monorepo {
            // Allow if the content type matches the project's primary type
            if content_type == self.project_type {
                return true;
            }

            // Allow CLI tools in any monorepo (build scripts, dev tools)
            if content_type == ProjectType::Cli {
                return true;
            }

            // Allow Library content in any monorepo (shared packages)
            if content_type == ProjectType::Library {
                return true;
            }

            // For other types, require explicit match to avoid
            // generating backend patterns for frontend projects
            return false;
        }

        content_type == self.project_type
    }
}

pub fn check(
    project_type: ProjectType,
    is_monorepo: bool,
    skills: &[Skill],
    agents: &[Agent],
    rules: &[Rule],
) -> ConsistencyResult {
    let checker = ProjectConsistencyChecker::new(project_type, is_monorepo);
    checker.check(skills, agents, rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_detection() {
        let checker = ProjectConsistencyChecker::new(ProjectType::Cli, false);
        let content = "add new command with arguments and flags for cli";
        let detected = checker.detect_content_type(content);
        assert_eq!(detected, ProjectType::Cli);
    }

    #[test]
    fn test_backend_detection() {
        let checker = ProjectConsistencyChecker::new(ProjectType::Backend, false);
        let content = "add new api endpoint with controller and service layer";
        let detected = checker.detect_content_type(content);
        assert_eq!(detected, ProjectType::Backend);
    }

    #[test]
    fn test_frontend_detection() {
        let checker = ProjectConsistencyChecker::new(ProjectType::Frontend, false);
        let content = "add react component with hooks and state management";
        let detected = checker.detect_content_type(content);
        assert_eq!(detected, ProjectType::Frontend);
    }

    #[test]
    fn test_compatible_monorepo() {
        let checker = ProjectConsistencyChecker::new(ProjectType::Backend, true);

        // Primary type should be compatible
        assert!(checker.is_compatible(ProjectType::Backend));

        // CLI tools are allowed in any monorepo
        assert!(checker.is_compatible(ProjectType::Cli));

        // Library content is allowed in any monorepo
        assert!(checker.is_compatible(ProjectType::Library));

        // Monorepo-specific content is allowed
        assert!(checker.is_compatible(ProjectType::Monorepo));

        // Frontend is NOT compatible with a Backend monorepo project
        // (more strict than before to prevent mismatched content)
        assert!(!checker.is_compatible(ProjectType::Frontend));
    }

    #[test]
    fn test_monorepo_primary_type_match() {
        // Frontend monorepo should accept Frontend content
        let frontend_mono = ProjectConsistencyChecker::new(ProjectType::Frontend, true);
        assert!(frontend_mono.is_compatible(ProjectType::Frontend));
        assert!(!frontend_mono.is_compatible(ProjectType::Backend));

        // Backend monorepo should accept Backend content
        let backend_mono = ProjectConsistencyChecker::new(ProjectType::Backend, true);
        assert!(backend_mono.is_compatible(ProjectType::Backend));
        assert!(!backend_mono.is_compatible(ProjectType::Frontend));
    }
}
