//! Claude Code Memory types (CLAUDE.md)

use serde::{Deserialize, Serialize};

use super::validation::ValidationIssue;

/// Maximum recommended lines for CLAUDE.md.
///
/// Rationale: Claude's context window is limited. A CLAUDE.md over 500 lines
/// consumes significant context budget, leaving less room for actual code
/// analysis. This threshold warns (not blocks) when content is too verbose.
/// Users can override via configuration if their use case requires more.
const MAX_RECOMMENDED_MEMORY_LINES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectMemory {
    #[serde(default)]
    pub overview: String,
    pub architecture: Option<String>,
    #[serde(default)]
    pub commands: Vec<DevelopmentCommand>,
    #[serde(default)]
    pub standards: Vec<String>,
    #[serde(default)]
    pub imports: Vec<String>,
    /// Domain-specific knowledge: policies, terminology, workflows
    pub domain_knowledge: Option<String>,
    /// Critical constraints and gotchas (Tier 3 insights)
    #[serde(default)]
    pub gotchas: Vec<String>,
    /// Navigation map for module-to-artifact mapping
    pub navigation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentCommand {
    pub name: String,
    pub command: String,
    pub description: Option<String>,
}

impl ProjectMemory {
    pub fn new(overview: impl Into<String>) -> Self {
        Self {
            overview: overview.into(),
            architecture: None,
            commands: Vec::new(),
            standards: Vec::new(),
            imports: Vec::new(),
            domain_knowledge: None,
            gotchas: Vec::new(),
            navigation: None,
        }
    }

    pub fn to_markdown(&self) -> String {
        let overview_section = if self.overview.trim_start().starts_with('#') {
            self.overview.clone()
        } else {
            format!("# Project Overview\n\n{}", self.overview)
        };

        let mut sections = vec![overview_section];

        if let Some(arch) = &self.architecture {
            sections.push(format!("## Architecture\n\n{arch}"));
        }

        if let Some(domain) = &self.domain_knowledge {
            sections.push(format!("## Domain Knowledge\n\n{domain}"));
        }

        if !self.gotchas.is_empty() {
            let gotchas = self
                .gotchas
                .iter()
                .map(|g| format!("- {g}"))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## Critical Constraints\n\n{gotchas}"));
        }

        if !self.commands.is_empty() {
            let cmds = self
                .commands
                .iter()
                .map(|c| match &c.description {
                    Some(desc) => format!("- **{}**: `{}` - {}", c.name, c.command, desc),
                    None => format!("- **{}**: `{}`", c.name, c.command),
                })
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## Development Commands\n\n{cmds}"));
        }

        if !self.standards.is_empty() {
            let stds = self
                .standards
                .iter()
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## Code Standards\n\n{stds}"));
        }

        if !self.imports.is_empty() {
            let imports = self
                .imports
                .iter()
                .map(|i| format!("@{i}"))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## References\n\n{imports}"));
        }

        let content = sections.join("\n\n");
        let line_count = content.lines().count();

        if line_count > MAX_RECOMMENDED_MEMORY_LINES {
            tracing::warn!(
                lines = line_count,
                max = MAX_RECOMMENDED_MEMORY_LINES,
                "CLAUDE.md exceeds recommended line count"
            );
        }

        content
    }

    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if self.overview.is_empty() {
            issues.push(ValidationIssue::error(
                "MEMORY_OVERVIEW_REQUIRED",
                "overview is required",
            ));
        }

        let line_count = self.to_markdown().lines().count();
        if line_count > MAX_RECOMMENDED_MEMORY_LINES {
            issues.push(ValidationIssue::warning(
                "MEMORY_TOO_LONG",
                format!(
                    "exceeds {} lines (current: {})",
                    MAX_RECOMMENDED_MEMORY_LINES, line_count
                ),
            ));
        }

        issues
    }
}
