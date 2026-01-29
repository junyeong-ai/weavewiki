//! Claude Code Rule types
//!
//! Rules represent domain knowledge that is auto-injected based on context:
//! - Path patterns (e.g., "src/**/*.rs")
//! - Keyword triggers (e.g., "async", "auth")
//! - Priority ordering (higher = injected first)

use serde::{Deserialize, Serialize};

use super::insight::ContentTier;
use super::node::EvidenceLocation;
use super::utils::is_kebab_case;
use super::validation::ValidationIssue;

/// Rule category for hierarchical organization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuleCategory {
    /// Project-wide rules (priority 100, always inject)
    #[default]
    Project,
    /// Language/tech-specific rules (priority 90, by extension)
    Tech,
    /// Framework-specific rules (priority 85, by path/keywords)
    Framework,
    /// Module-specific rules (priority 80, by module path)
    Module,
    /// Cross-module group rules (priority 70, by member paths)
    Group,
    /// Domain-specific rules (priority 60, by keyword trigger)
    Domain,
}

impl RuleCategory {
    pub fn default_priority(self) -> u8 {
        match self {
            Self::Project => 100,
            Self::Tech => 90,
            Self::Framework => 85,
            Self::Module => 80,
            Self::Group => 70,
            Self::Domain => 60,
        }
    }

    pub fn subdirectory(self) -> &'static str {
        match self {
            Self::Project => "",
            Self::Tech => "tech",
            Self::Framework => "frameworks",
            Self::Module => "modules",
            Self::Group => "groups",
            Self::Domain => "domains",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    /// Path patterns for auto-injection (glob syntax)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    /// Keyword triggers for auto-injection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggers: Option<Vec<String>>,
    /// Injection priority (higher = injected first)
    #[serde(default = "default_priority")]
    pub priority: u8,
    /// Rule category
    #[serde(default)]
    pub category: RuleCategory,
    /// Whether this rule is always injected regardless of context
    #[serde(default)]
    pub always_inject: bool,
    /// Markdown content
    pub content: Vec<String>,
    #[serde(skip)]
    pub evidence: Vec<EvidenceLocation>,
    #[serde(skip)]
    pub tier: ContentTier,
}

fn default_priority() -> u8 {
    50
}

impl PartialEq for Rule {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.paths == other.paths
            && self.content == other.content
            && self.tier == other.tier
            && self.category == other.category
    }
}

impl Eq for Rule {}

impl Rule {
    pub fn new(name: impl Into<String>, content: Vec<String>) -> Self {
        Self {
            name: name.into(),
            paths: None,
            triggers: None,
            priority: default_priority(),
            category: RuleCategory::default(),
            always_inject: false,
            content,
            evidence: Vec::new(),
            tier: ContentTier::default(),
        }
    }

    pub fn project(name: impl Into<String>, content: Vec<String>) -> Self {
        Self {
            name: name.into(),
            paths: Some(vec!["**/*".into()]),
            triggers: None,
            priority: RuleCategory::Project.default_priority(),
            category: RuleCategory::Project,
            always_inject: true,
            content,
            evidence: Vec::new(),
            tier: ContentTier::default(),
        }
    }

    pub fn tech(name: impl Into<String>, paths: Vec<String>, content: Vec<String>) -> Self {
        Self {
            name: name.into(),
            paths: Some(paths),
            triggers: None,
            priority: RuleCategory::Tech.default_priority(),
            category: RuleCategory::Tech,
            always_inject: false,
            content,
            evidence: Vec::new(),
            tier: ContentTier::default(),
        }
    }

    pub fn module(name: impl Into<String>, paths: Vec<String>, content: Vec<String>) -> Self {
        Self {
            name: name.into(),
            paths: Some(paths),
            triggers: None,
            priority: RuleCategory::Module.default_priority(),
            category: RuleCategory::Module,
            always_inject: false,
            content,
            evidence: Vec::new(),
            tier: ContentTier::default(),
        }
    }

    pub fn group(name: impl Into<String>, paths: Vec<String>, content: Vec<String>) -> Self {
        Self {
            name: name.into(),
            paths: Some(paths),
            triggers: None,
            priority: RuleCategory::Group.default_priority(),
            category: RuleCategory::Group,
            always_inject: false,
            content,
            evidence: Vec::new(),
            tier: ContentTier::default(),
        }
    }

    pub fn domain(name: impl Into<String>, triggers: Vec<String>, content: Vec<String>) -> Self {
        Self {
            name: name.into(),
            paths: None,
            triggers: Some(triggers),
            priority: RuleCategory::Domain.default_priority(),
            category: RuleCategory::Domain,
            always_inject: false,
            content,
            evidence: Vec::new(),
            tier: ContentTier::default(),
        }
    }

    pub fn framework(
        name: impl Into<String>,
        paths: Vec<String>,
        triggers: Vec<String>,
        content: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            paths: if paths.is_empty() {
                None
            } else {
                Some(paths)
            },
            triggers: if triggers.is_empty() {
                None
            } else {
                Some(triggers)
            },
            priority: RuleCategory::Framework.default_priority(),
            category: RuleCategory::Framework,
            always_inject: false,
            content,
            evidence: Vec::new(),
            tier: ContentTier::default(),
        }
    }

    pub fn with_tier(mut self, tier: ContentTier) -> Self {
        self.tier = tier;
        self
    }

    pub fn with_paths(mut self, paths: Vec<String>) -> Self {
        self.paths = Some(paths);
        self
    }

    pub fn with_triggers(mut self, triggers: Vec<String>) -> Self {
        self.triggers = Some(triggers);
        self
    }

    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_category(mut self, category: RuleCategory) -> Self {
        self.priority = category.default_priority();
        self.category = category;
        self
    }

    pub fn with_evidence(mut self, evidence: Vec<EvidenceLocation>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn to_markdown(&self) -> String {
        let mut output = String::new();

        // Generate frontmatter
        let has_frontmatter = self.paths.is_some()
            || self.triggers.is_some()
            || self.always_inject
            || self.priority != default_priority();

        if has_frontmatter {
            output.push_str("---\n");

            if let Some(paths) = &self.paths {
                output.push_str("paths:\n");
                for path in paths {
                    output.push_str(&format!("  - \"{path}\"\n"));
                }
            }

            if let Some(triggers) = &self.triggers {
                output.push_str("triggers:\n");
                for trigger in triggers {
                    output.push_str(&format!("  - \"{trigger}\"\n"));
                }
            }

            if self.priority != default_priority() {
                output.push_str(&format!("priority: {}\n", self.priority));
            }

            if self.always_inject {
                output.push_str("always_inject: true\n");
            }

            output.push_str("---\n\n");
        }

        output.push_str(&self.content.join("\n"));
        output
    }

    pub fn output_path(&self) -> String {
        let subdir = self.category.subdirectory();
        if subdir.is_empty() {
            format!("{}.md", self.name)
        } else {
            format!("{}/{}.md", subdir, self.name)
        }
    }

    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if self.name.is_empty() {
            issues.push(ValidationIssue::error(
                "RULE_NAME_REQUIRED",
                "rule name is required",
            ));
        }

        if !is_kebab_case(&self.name) {
            issues.push(ValidationIssue::error(
                "RULE_NAME_INVALID",
                format!("name '{}' must be kebab-case", self.name),
            ));
        }

        if self.content.is_empty() {
            issues.push(ValidationIssue::warning(
                "RULE_CONTENT_EMPTY",
                "rule has no content",
            ));
        }

        if let Some(paths) = &self.paths {
            for path in paths {
                if !path.contains('*') && !path.contains('/') {
                    issues.push(ValidationIssue::warning(
                        "RULE_PATH_SIMPLE",
                        format!("path '{path}' should be a glob pattern"),
                    ));
                }
            }
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_to_markdown_with_paths() {
        let rule = Rule::tech(
            "error-handling",
            vec!["src/**/*.rs".into()],
            vec![
                "# Error Handling".into(),
                "".into(),
                "- Use `?` operator for error propagation".into(),
                "- Custom errors via `ClaudegenError`".into(),
            ],
        );

        let md = rule.to_markdown();
        assert!(md.contains("---"));
        assert!(md.contains("paths:"));
        assert!(md.contains("src/**/*.rs"));
        assert!(md.contains("# Error Handling"));
        assert!(md.contains("- Use `?` operator"));
    }

    #[test]
    fn test_rule_to_markdown_project() {
        let rule = Rule::project(
            "project",
            vec![
                "# Project Rules".into(),
                "".into(),
                "Always follow conventions".into(),
            ],
        );

        let md = rule.to_markdown();
        assert!(md.contains("---"));
        assert!(md.contains("always_inject: true"));
        assert!(md.contains("priority: 100"));
        assert!(md.contains("# Project Rules"));
    }

    #[test]
    fn test_rule_to_markdown_domain() {
        let rule = Rule::domain(
            "security",
            vec!["auth".into(), "password".into(), "token".into()],
            vec!["# Security".into(), "".into(), "Validate all inputs".into()],
        );

        let md = rule.to_markdown();
        assert!(md.contains("triggers:"));
        assert!(md.contains("\"auth\""));
        assert!(md.contains("priority: 60"));
    }

    #[test]
    fn test_rule_validation() {
        let rule = Rule::new("INVALID", vec![]);
        let issues = rule.validate();
        assert!(issues.iter().any(|i| i.code == "RULE_NAME_INVALID"));
        assert!(issues.iter().any(|i| i.code == "RULE_CONTENT_EMPTY"));
    }

    #[test]
    fn test_rule_output_path() {
        assert_eq!(Rule::project("proj", vec![]).output_path(), "proj.md");
        assert_eq!(
            Rule::tech("rust", vec![], vec![]).output_path(),
            "tech/rust.md"
        );
        assert_eq!(
            Rule::module("auth", vec![], vec![]).output_path(),
            "modules/auth.md"
        );
        assert_eq!(
            Rule::domain("security", vec![], vec![]).output_path(),
            "domains/security.md"
        );
    }

    #[test]
    fn test_category_priority() {
        assert_eq!(RuleCategory::Project.default_priority(), 100);
        assert_eq!(RuleCategory::Tech.default_priority(), 90);
        assert_eq!(RuleCategory::Framework.default_priority(), 85);
        assert_eq!(RuleCategory::Module.default_priority(), 80);
        assert_eq!(RuleCategory::Group.default_priority(), 70);
        assert_eq!(RuleCategory::Domain.default_priority(), 60);
    }
}
