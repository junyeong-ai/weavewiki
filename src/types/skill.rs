//! Claude Code Skill types - Official spec compliant
//!
//! Skills follow the Progressive Disclosure philosophy:
//! - CLAUDE.md: Core principles (loaded always)
//! - Rules: Path-scoped conventions (loaded on file access)
//! - Skills: Complex workflows with @file:line references (invoked explicitly)
//!
//! Quality Requirements:
//! - Minimum 2 @file:line references per skill
//! - At least 3 actionable statements (must/should/avoid)
//! - No Tier 1 (generic) content
//! - Project-specific constraints and gotchas

use super::agent::is_valid_tool;
use super::hook::ToolHooks;
use super::node::EvidenceLocation;
use super::utils::is_kebab_case;
use super::validation::ValidationIssue;
use indexmap::IndexMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_yaml_ng as serde_yaml;
use std::sync::LazyLock;

// Quality calculation patterns (defined locally to avoid circular deps)
static FILE_LINE_REF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@([a-zA-Z0-9_\-]+/[a-zA-Z0-9_./\-]+):(\d+)").expect("Invalid regex")
});

static ACTIONABLE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(must|shall|should|always|never|avoid|do not|don't|forbidden|prohibited)\b")
        .expect("Invalid regex")
});

// Tier 1 patterns (subset for basic detection)
const TIER1_KEYWORDS: &[&str] = &[
    "cargo build", "cargo test", "npm install", "npm run", "pip install",
    "go build", "gradle build", "use async/await", "write tests",
];

// Tier 3 indicators (high-value content)
const TIER3_KEYWORDS: &[&str] = &[
    "hidden", "gotcha", "pitfall", "constraint", "dependency",
    "must not", "never", "forbidden", "anti-pattern", "order matters",
];

/// Count @file:line references in content
fn count_file_line_refs(content: &str) -> usize {
    FILE_LINE_REF.captures_iter(content).count()
}

/// Count actionable statements in content
fn count_actionable_statements(content: &str) -> usize {
    ACTIONABLE_PATTERN.find_iter(content).count()
}

/// Count Tier 1 patterns in content
fn count_tier1_patterns(content: &str) -> usize {
    let lower = content.to_lowercase();
    TIER1_KEYWORDS.iter().filter(|k| lower.contains(*k)).count()
}

/// Count Tier 3 indicators in content
fn count_tier3_indicators(content: &str) -> usize {
    let lower = content.to_lowercase();
    TIER3_KEYWORDS.iter().filter(|k| lower.contains(*k)).count()
}

/// Content value tier classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ContentTier {
    /// Generic knowledge Claude already knows (cargo build, npm install)
    #[default]
    Tier1Generic,
    /// Project conventions that need consistency (naming, architecture)
    Tier2Convention,
    /// Hidden constraints, gotchas, dependencies (highest value)
    Tier3Constraint,
}

/// Quality metrics for generated content
#[derive(Debug, Clone, Default)]
pub struct QualityMetrics {
    /// Number of @file:line references
    pub file_refs: usize,
    /// Number of actionable statements (must/should/avoid)
    pub actionable_count: usize,
    /// Content tier classification
    pub tier: ContentTier,
    /// Overall quality score (0.0 - 1.0)
    pub score: f32,
    /// Whether all quality requirements are met
    pub meets_requirements: bool,
}

// Implement PartialEq manually to handle f32 comparison
impl PartialEq for QualityMetrics {
    fn eq(&self, other: &Self) -> bool {
        self.file_refs == other.file_refs
            && self.actionable_count == other.actionable_count
            && self.tier == other.tier
            && (self.score - other.score).abs() < f32::EPSILON
            && self.meets_requirements == other.meets_requirements
    }
}

impl Eq for QualityMetrics {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(rename = "allowed-tools", skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(rename = "user-invocable", skip_serializing_if = "Option::is_none")]
    pub user_invocable: Option<bool>,
    #[serde(rename = "argument-hint", skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    #[serde(
        rename = "disable-model-invocation",
        skip_serializing_if = "Option::is_none"
    )]
    pub disable_model_invocation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<ToolHooks>,
    pub body: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub additional_files: Vec<SkillFile>,
    #[serde(skip)]
    pub evidence: Vec<EvidenceLocation>,
    /// Quality metrics (not serialized to output)
    #[serde(skip)]
    pub quality: QualityMetrics,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContextMode {
    Fork,
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFile {
    pub name: String,
    pub content: String,
}

fn yaml_value<T: serde::Serialize>(v: &T) -> serde_yaml::Value {
    serde_yaml::to_value(v).unwrap_or(serde_yaml::Value::Null)
}

/// Minimum required @file:line references for a skill to pass quality
pub const MIN_FILE_REFS: usize = 2;
/// Minimum required actionable statements for a skill to pass quality
pub const MIN_ACTIONABLE_COUNT: usize = 3;

impl Skill {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        let body_str = body.into();
        let quality = Self::calculate_quality(&body_str);
        Self {
            name: name.into(),
            description: description.into(),
            version: default_version(),
            allowed_tools: None,
            model: None,
            context: None,
            agent: None,
            user_invocable: None,
            argument_hint: None,
            disable_model_invocation: None,
            hooks: None,
            body: body_str,
            additional_files: Vec::new(),
            evidence: Vec::new(),
            quality,
        }
    }

    /// Calculate quality metrics from body content
    pub fn calculate_quality(body: &str) -> QualityMetrics {
        let file_refs = count_file_line_refs(body);
        let actionable_count = count_actionable_statements(body);
        let tier1_count = count_tier1_patterns(body);
        let tier3_count = count_tier3_indicators(body);

        // Determine tier based on content
        let tier = if tier1_count > 2 {
            ContentTier::Tier1Generic
        } else if tier3_count > 0 {
            ContentTier::Tier3Constraint
        } else if file_refs >= MIN_FILE_REFS {
            ContentTier::Tier2Convention
        } else {
            ContentTier::Tier1Generic
        };

        // Calculate score
        let mut score = 0.0f32;
        score += 0.15 * (file_refs.min(5) as f32);           // Max 0.75 for refs
        score += 0.05 * (actionable_count.min(5) as f32);    // Max 0.25 for actionable
        score -= 0.1 * (tier1_count as f32);                  // Penalty for tier1
        score += 0.1 * (tier3_count.min(3) as f32);          // Bonus for tier3
        score = score.clamp(0.0, 1.0);

        let meets_requirements = file_refs >= MIN_FILE_REFS
            && actionable_count >= MIN_ACTIONABLE_COUNT
            && tier != ContentTier::Tier1Generic;

        QualityMetrics {
            file_refs,
            actionable_count,
            tier,
            score,
            meets_requirements,
        }
    }

    /// Recalculate quality metrics (call after modifying body)
    pub fn update_quality(&mut self) {
        self.quality = Self::calculate_quality(&self.body);
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = Some(tools);
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_user_invocable(mut self, invocable: bool) -> Self {
        self.user_invocable = Some(invocable);
        self
    }

    pub fn with_argument_hint(mut self, hint: impl Into<String>) -> Self {
        self.argument_hint = Some(hint.into());
        self
    }

    pub fn with_evidence(mut self, evidence: Vec<EvidenceLocation>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn with_context(mut self, context: ContextMode) -> Self {
        self.context = Some(context);
        self
    }

    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    pub fn with_disable_model_invocation(mut self, disable: bool) -> Self {
        self.disable_model_invocation = Some(disable);
        self
    }

    pub fn with_hooks(mut self, hooks: ToolHooks) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn to_markdown(&self) -> String {
        let mut frontmatter: IndexMap<&str, serde_yaml::Value> = IndexMap::new();
        frontmatter.insert("name", yaml_value(&self.name));
        frontmatter.insert("description", yaml_value(&self.description));
        frontmatter.insert("version", yaml_value(&self.version));

        if let Some(tools) = &self.allowed_tools {
            frontmatter.insert("allowed-tools", yaml_value(tools));
        }
        if let Some(model) = &self.model {
            frontmatter.insert("model", yaml_value(model));
        }
        if let Some(context) = &self.context {
            frontmatter.insert("context", yaml_value(context));
        }
        if let Some(agent) = &self.agent {
            frontmatter.insert("agent", yaml_value(agent));
        }
        if let Some(invocable) = self.user_invocable {
            frontmatter.insert("user-invocable", yaml_value(&invocable));
        }
        if let Some(hint) = &self.argument_hint {
            frontmatter.insert("argument-hint", yaml_value(hint));
        }
        if let Some(disable) = self.disable_model_invocation {
            frontmatter.insert("disable-model-invocation", yaml_value(&disable));
        }

        let yaml = serde_yaml::to_string(&frontmatter).unwrap_or_else(|e| {
            tracing::error!(skill = %self.name, error = %e, "Failed to serialize skill frontmatter");
            format!("name: \"{}\"\n", self.name.replace('"', "\\\""))
        });
        format!("---\n{}---\n\n{}", yaml, self.body)
    }

    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if self.name.len() > 64 {
            issues.push(ValidationIssue::error(
                "SKILL_NAME_TOO_LONG",
                format!("name '{}' exceeds 64 characters", self.name),
            ));
        }

        if !is_kebab_case(&self.name) {
            issues.push(ValidationIssue::error(
                "SKILL_NAME_INVALID",
                format!("name '{}' must be kebab-case", self.name),
            ));
        }

        if self.description.len() > 1024 {
            issues.push(ValidationIssue::error(
                "SKILL_DESC_TOO_LONG",
                "description exceeds 1024 characters",
            ));
        }

        if self.description.is_empty() {
            issues.push(ValidationIssue::error(
                "SKILL_DESC_EMPTY",
                "description is required",
            ));
        } else if !self.has_usage_context() {
            issues.push(ValidationIssue::warning(
                "SKILL_DESC_MISSING_CONTEXT",
                "description should explain when to use this skill",
            ));
        }

        if self.agent.is_some() && self.context != Some(ContextMode::Fork) {
            issues.push(ValidationIssue::warning(
                "AGENT_WITHOUT_FORK",
                "agent field requires context: fork",
            ));
        }

        // Validate allowed-tools (consistent with Agent validation)
        if let Some(tools) = &self.allowed_tools {
            for tool in tools {
                if !is_valid_tool(tool) {
                    issues.push(ValidationIssue::warning(
                        "UNKNOWN_TOOL",
                        format!("unknown tool: {tool}"),
                    ));
                }
            }
        }

        issues
    }

    fn has_usage_context(&self) -> bool {
        let desc_lower = self.description.to_lowercase();
        let usage_keywords = [
            "when",
            "use",
            "trigger",
            "invoke",
            "run",
            "execute",
            "should be used",
        ];
        usage_keywords.iter().any(|kw| desc_lower.contains(kw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_to_markdown() {
        let skill = Skill::new(
            "rust-build",
            "This skill should be used when building Rust projects.",
            "# Rust Build\n\nBuild commands...",
        )
        .with_tools(vec!["Bash".to_string(), "Read".to_string()])
        .with_user_invocable(true);

        let md = skill.to_markdown();
        assert!(md.contains("name: rust-build"));
        assert!(md.contains("allowed-tools:"));
        assert!(md.contains("- Bash"));
        assert!(md.contains("- Read"));
        assert!(md.contains("user-invocable: true"));
    }

    #[test]
    fn test_skill_validation() {
        let skill = Skill::new("INVALID_NAME", "", "body");
        let issues = skill.validate();
        assert!(issues.iter().any(|i| i.code == "SKILL_NAME_INVALID"));
    }

    #[test]
    fn test_skill_with_argument_hint() {
        let skill =
            Skill::new("my-skill", "desc", "body").with_argument_hint("[file-path] [options]");

        let md = skill.to_markdown();
        assert!(md.contains("argument-hint:"));
        assert!(md.contains("[file-path] [options]"));
    }

    #[test]
    fn test_skill_tool_validation() {
        let skill = Skill::new("test-skill", "Use this skill when testing", "body")
            .with_tools(vec![
                "Read".to_string(),
                "InvalidTool".to_string(),
                "Grep".to_string(),
            ]);

        let issues = skill.validate();
        assert!(issues.iter().any(|i| i.code == "UNKNOWN_TOOL"));
        assert!(issues.iter().any(|i| i.message.contains("InvalidTool")));
    }
}
