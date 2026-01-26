//! Claude Code Skill types - Official spec compliant

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml_bw as serde_yaml;

use super::hook::ToolHooks;
use super::node::EvidenceLocation;
use super::utils::is_kebab_case;
use super::validation::ValidationIssue;
use crate::utils::is_valid_tool;
use crate::utils::patterns;

use super::insight::{ContentTier, TierClassification};

/// Quality metrics for generated content
/// Note: actionable_count removed - LLM judges actionability, not pattern matching
#[derive(Debug, Clone, Default)]
pub struct QualityMetrics {
    pub file_refs: usize,
    pub tier: TierClassification,
    pub score: f32,
    pub meets_requirements: bool,
}

impl PartialEq for QualityMetrics {
    fn eq(&self, other: &Self) -> bool {
        self.file_refs == other.file_refs
            && self.tier == other.tier
            && (self.score - other.score).abs() < f32::EPSILON
            && self.meets_requirements == other.meets_requirements
    }
}

impl Eq for QualityMetrics {}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(skip)]
    pub quality: QualityMetrics,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

impl PartialEq for Skill {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.description == other.description
            && self.version == other.version
            && self.allowed_tools == other.allowed_tools
            && self.model == other.model
            && self.context == other.context
            && self.agent == other.agent
            && self.user_invocable == other.user_invocable
            && self.argument_hint == other.argument_hint
            && self.disable_model_invocation == other.disable_model_invocation
            && self.hooks == other.hooks
            && self.body == other.body
            && self.additional_files == other.additional_files
            && self.quality == other.quality
    }
}

impl Eq for Skill {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContextMode {
    Fork,
}

impl std::str::FromStr for ContextMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "fork" => Ok(Self::Fork),
            _ => Err(format!("unknown context mode: {s}")),
        }
    }
}

impl std::fmt::Display for ContextMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fork => write!(f, "fork"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFile {
    pub name: String,
    pub content: String,
}

fn yaml_value<T: serde::Serialize>(v: &T) -> serde_yaml::Value {
    serde_yaml::to_value(v).unwrap_or(serde_yaml::Value::Null(None))
}

/// Minimum required @file:line references for a skill to pass quality
pub const MIN_FILE_REFS: usize = 2;

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
    /// Note: Tier classification is set by LLM during generation, not by static patterns
    pub fn calculate_quality(body: &str) -> QualityMetrics {
        let file_refs = patterns::count_file_line_refs(body);

        // Score based on deterministic metrics only (file references)
        // Base score 0.3, logarithmic scaling for refs (no arbitrary cap)
        // log2(refs + 1) * 0.15 gives diminishing returns without hard cutoff
        // 1 ref → 0.15, 3 refs → 0.30, 7 refs → 0.45, 15 refs → 0.60
        let mut score = 0.3f32;
        if file_refs > 0 {
            score += ((file_refs as f32 + 1.0).log2() * 0.15).min(0.7);
        }
        score = score.clamp(0.0, 1.0);

        // Tier is set by LLM, default to Tier1 until classification
        let tier = ContentTier::default();

        let meets_requirements = file_refs >= MIN_FILE_REFS;

        QualityMetrics {
            file_refs,
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
            // Claude Code expects comma-separated string, not YAML array
            frontmatter.insert("allowed-tools", yaml_value(&tools.join(", ")));
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

        if self.description.is_empty() {
            issues.push(ValidationIssue::error(
                "SKILL_DESC_EMPTY",
                "description is required",
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
        // Claude Code format: comma-separated string, not YAML array
        assert!(md.contains("allowed-tools: Bash, Read"));
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
        let skill =
            Skill::new("test-skill", "Use this skill when testing", "body").with_tools(vec![
                "Read".to_string(),
                "InvalidTool".to_string(),
                "Grep".to_string(),
            ]);

        let issues = skill.validate();
        assert!(issues.iter().any(|i| i.code == "UNKNOWN_TOOL"));
        assert!(issues.iter().any(|i| i.message.contains("InvalidTool")));
    }
}
