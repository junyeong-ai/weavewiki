//! Claude Code Agent types - Official spec compliant

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml_bw as serde_yaml;

use super::hook::ToolHooks;
use super::insight::ContentTier;
use super::node::EvidenceLocation;
use super::skill::QualityMetrics;
use super::utils::{is_kebab_case, yaml_value};
use super::validation::ValidationIssue;
use crate::utils::{is_valid_tool, patterns};

// Tier classification is handled by LLM, not by static name patterns

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<AgentColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(rename = "disallowedTools", skip_serializing_if = "Option::is_none")]
    pub disallowed_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<AgentModel>,
    #[serde(rename = "permissionMode", skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consensus: Option<ConsensusRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<ToolHooks>,
    pub prompt: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub examples: Vec<AgentExample>,
    #[serde(skip)]
    pub evidence: Vec<EvidenceLocation>,
    #[serde(skip)]
    pub quality: QualityMetrics,
}

impl PartialEq for Agent {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.description == other.description
            && self.color == other.color
            && self.tools == other.tools
            && self.disallowed_tools == other.disallowed_tools
            && self.model == other.model
            && self.permission_mode == other.permission_mode
            && self.skills == other.skills
            && self.consensus == other.consensus
            && self.hooks == other.hooks
            && self.prompt == other.prompt
            && self.examples == other.examples
            && self.quality == other.quality
    }
}

impl Eq for Agent {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentColor {
    Blue,
    Green,
    Purple,
    Orange,
    Red,
}

impl std::str::FromStr for AgentColor {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "blue" => Self::Blue,
            "green" => Self::Green,
            "purple" => Self::Purple,
            "orange" => Self::Orange,
            "red" => Self::Red,
            _ => Self::Blue,
        })
    }
}

impl std::fmt::Display for AgentColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Blue => write!(f, "blue"),
            Self::Green => write!(f, "green"),
            Self::Purple => write!(f, "purple"),
            Self::Orange => write!(f, "orange"),
            Self::Red => write!(f, "red"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExample {
    pub context: String,
    pub user: String,
    pub assistant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commentary: Option<String>,
}

impl AgentExample {
    pub fn new(context: &str, user: &str, assistant: &str) -> Self {
        Self {
            context: context.to_string(),
            user: user.to_string(),
            assistant: assistant.to_string(),
            commentary: None,
        }
    }

    pub fn with_commentary(mut self, commentary: &str) -> Self {
        self.commentary = Some(commentary.to_string());
        self
    }

    fn to_example_block(&self) -> String {
        let mut block = String::new();
        block.push_str("<example>\n");
        block.push_str(&format!("Context: {}\n", self.context));
        block.push_str(&format!("user: \"{}\"\n", self.user));
        block.push_str(&format!("assistant: \"{}\"\n", self.assistant));
        if let Some(commentary) = &self.commentary {
            block.push_str("<commentary>\n");
            block.push_str(commentary);
            block.push_str("\n</commentary>\n");
        }
        block.push_str("</example>");
        block
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentModel {
    Sonnet,
    Opus,
    Haiku,
    Inherit,
}

impl std::fmt::Display for AgentModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentModel::Sonnet => write!(f, "sonnet"),
            AgentModel::Opus => write!(f, "opus"),
            AgentModel::Haiku => write!(f, "haiku"),
            AgentModel::Inherit => write!(f, "inherit"),
        }
    }
}

impl std::str::FromStr for AgentModel {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "sonnet" => Self::Sonnet,
            "opus" => Self::Opus,
            "haiku" => Self::Haiku,
            "inherit" => Self::Inherit,
            _ => Self::Inherit, // Default fallback
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    DontAsk,
    BypassPermissions,
    Plan,
}

impl std::str::FromStr for PermissionMode {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        // Case-insensitive matching for robustness
        Ok(match s.to_lowercase().replace('_', "").as_str() {
            "acceptedits" => Self::AcceptEdits,
            "dontask" => Self::DontAsk,
            "bypasspermissions" => Self::BypassPermissions,
            "plan" => Self::Plan,
            "default" => Self::Default,
            _ => Self::Default,
        })
    }
}

impl std::fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::AcceptEdits => write!(f, "acceptEdits"),
            Self::DontAsk => write!(f, "dontAsk"),
            Self::BypassPermissions => write!(f, "bypassPermissions"),
            Self::Plan => write!(f, "plan"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsensusRole {
    pub priority: u8,
    #[serde(default)]
    pub can_veto: bool,
    #[serde(default = "default_vote_threshold")]
    pub vote_threshold: f64,
}

fn default_vote_threshold() -> f64 {
    0.67
}

impl Default for ConsensusRole {
    fn default() -> Self {
        Self {
            priority: 50,
            can_veto: false,
            vote_threshold: 0.67,
        }
    }
}

impl ConsensusRole {
    pub fn new(priority: u8) -> Self {
        Self {
            priority,
            can_veto: false,
            vote_threshold: 0.67,
        }
    }

    pub fn with_veto(mut self) -> Self {
        self.can_veto = true;
        self
    }

    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.vote_threshold = threshold;
        self
    }
}

/// Minimum required @file:line references for an agent to pass quality
pub const MIN_AGENT_FILE_REFS: usize = 2;

impl Agent {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        let prompt_str = prompt.into();
        let quality = Self::calculate_quality(&prompt_str);
        Self {
            name: name.into(),
            description: description.into(),
            color: None,
            tools: None,
            disallowed_tools: None,
            model: None,
            permission_mode: None,
            skills: None,
            consensus: None,
            hooks: None,
            prompt: prompt_str,
            examples: Vec::new(),
            evidence: Vec::new(),
            quality,
        }
    }

    pub fn calculate_quality(prompt: &str) -> QualityMetrics {
        let file_refs = patterns::count_file_line_refs(prompt);

        let score = if file_refs == 0 {
            0.3
        } else if file_refs < 3 {
            0.5
        } else {
            0.7
        };

        let tier = ContentTier::default();
        let meets_requirements = file_refs >= MIN_AGENT_FILE_REFS;

        QualityMetrics {
            file_refs,
            tier,
            score,
            meets_requirements,
        }
    }

    /// Recalculate quality metrics (call after modifying prompt)
    pub fn update_quality(&mut self) {
        self.quality = Self::calculate_quality(&self.prompt);
    }

    pub fn with_color(mut self, color: AgentColor) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn with_model(mut self, model: AgentModel) -> Self {
        self.model = Some(model);
        self
    }

    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = Some(mode);
        self
    }

    pub fn with_example(mut self, example: AgentExample) -> Self {
        self.examples.push(example);
        self
    }

    pub fn with_evidence(mut self, evidence: Vec<EvidenceLocation>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn with_disallowed_tools(mut self, tools: Vec<String>) -> Self {
        self.disallowed_tools = Some(tools);
        self
    }

    pub fn with_skills(mut self, skills: Vec<String>) -> Self {
        self.skills = Some(skills);
        self
    }

    pub fn with_consensus(mut self, consensus: ConsensusRole) -> Self {
        self.consensus = Some(consensus);
        self
    }

    pub fn with_hooks(mut self, hooks: ToolHooks) -> Self {
        self.hooks = Some(hooks);
        self
    }

    fn build_prompt_with_examples(&self) -> String {
        if self.examples.is_empty() {
            return self.prompt.clone();
        }

        let mut content = self.prompt.clone();
        content.push_str("\n\n## Examples\n\n");
        for example in &self.examples {
            content.push_str(&example.to_example_block());
            content.push('\n');
        }
        content
    }

    pub fn to_markdown(&self) -> String {
        let mut fm: IndexMap<&str, serde_yaml::Value> = IndexMap::new();
        fm.insert("name", yaml_value(&self.name));
        fm.insert("description", yaml_value(&self.description));

        if let Some(model) = &self.model {
            fm.insert("model", yaml_value(&model.to_string()));
        }
        if let Some(color) = &self.color {
            fm.insert("color", yaml_value(&color.to_string()));
        }
        if let Some(tools) = &self.tools {
            fm.insert("tools", serde_yaml::to_value(tools).unwrap_or_default());
        }
        if let Some(disallowed) = &self.disallowed_tools {
            fm.insert("disallowedTools", serde_yaml::to_value(disallowed).unwrap_or_default());
        }
        if let Some(mode) = &self.permission_mode {
            fm.insert("permissionMode", yaml_value(&mode.to_string()));
        }
        if let Some(skills) = &self.skills {
            fm.insert("skills", serde_yaml::to_value(skills).unwrap_or_default());
        }
        if let Some(consensus) = &self.consensus {
            fm.insert("consensus", serde_yaml::to_value(consensus).unwrap_or_default());
        }
        if let Some(hooks) = &self.hooks {
            fm.insert("hooks", serde_yaml::to_value(hooks).unwrap_or_default());
        }

        let yaml = serde_yaml::to_string(&fm).unwrap_or_else(|e| {
            tracing::error!(agent = %self.name, error = %e, "Failed to serialize agent frontmatter");
            format!("name: \"{}\"\n", self.name.replace('"', "\\\""))
        });
        format!("---\n{}---\n\n{}", yaml, self.build_prompt_with_examples())
    }

    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if self.name.is_empty() {
            issues.push(ValidationIssue::error(
                "AGENT_NAME_REQUIRED",
                "agent name is required",
            ));
        }

        if !is_kebab_case(&self.name) {
            issues.push(ValidationIssue::error(
                "AGENT_NAME_INVALID",
                format!("name '{}' must be kebab-case", self.name),
            ));
        }

        if self.description.is_empty() {
            issues.push(ValidationIssue::error(
                "AGENT_DESC_REQUIRED",
                "agent description is required",
            ));
        }

        if self.permission_mode == Some(PermissionMode::BypassPermissions) {
            issues.push(ValidationIssue::warning(
                "BYPASS_PERMISSIONS",
                "bypassPermissions mode should be used with caution",
            ));
        }

        if let Some(tools) = &self.tools {
            for tool in tools {
                if !is_valid_tool(tool) {
                    issues.push(ValidationIssue::warning(
                        "UNKNOWN_TOOL",
                        format!("unknown tool: {tool}"),
                    ));
                }
            }
        }

        // Validate disallowed_tools
        if let Some(tools) = &self.disallowed_tools {
            for tool in tools {
                if !is_valid_tool(tool) {
                    issues.push(ValidationIssue::warning(
                        "UNKNOWN_DISALLOWED_TOOL",
                        format!("unknown disallowed tool: {tool}"),
                    ));
                }
            }
        }

        if let Some(skills) = &self.skills {
            for skill in skills {
                if !is_kebab_case(skill) {
                    issues.push(ValidationIssue::warning(
                        "INVALID_SKILL_NAME",
                        format!("skill name '{}' should be kebab-case", skill),
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
    fn test_agent_to_markdown_basic() {
        let agent = Agent::new(
            "code-reviewer",
            "Use this agent for code review.",
            "You are a code reviewer.",
        )
        .with_color(AgentColor::Blue)
        .with_model(AgentModel::Sonnet)
        .with_tools(vec!["Read".to_string(), "Grep".to_string()]);

        let md = agent.to_markdown();
        assert!(md.contains("name:"), "missing name field");
        assert!(md.contains("code-reviewer"), "missing name value");
        assert!(
            md.contains("Use this agent for code review."),
            "missing description"
        );
        assert!(md.contains("blue"), "missing color");
        assert!(md.contains("sonnet"), "missing model");
        // Tools serialized as YAML array (block style)
        assert!(md.contains("- Read"), "missing tool Read");
        assert!(md.contains("- Grep"), "missing tool Grep");
        assert!(
            md.contains("You are a code reviewer."),
            "missing prompt body"
        );
        assert!(md.starts_with("---\n"), "missing frontmatter opener");
    }

    #[test]
    fn test_agent_examples_in_prompt_body() {
        let agent = Agent::new("test-agent", "Test description", "Test prompt")
            .with_example(AgentExample::new("ctx", "user msg", "assistant msg"));

        let md = agent.to_markdown();
        assert!(md.contains("Test description"), "missing description");
        assert!(md.contains("## Examples"), "missing examples section");
        assert!(md.contains("<example>"), "missing example block");
    }

    #[test]
    fn test_agent_validation() {
        let agent = Agent::new("", "", "");
        let issues = agent.validate();
        assert!(issues.iter().any(|i| i.code == "AGENT_NAME_REQUIRED"));
        assert!(issues.iter().any(|i| i.code == "AGENT_DESC_REQUIRED"));
    }

    #[test]
    fn test_valid_tools() {
        assert!(is_valid_tool("Read"));
        assert!(is_valid_tool("Skill"));
        assert!(!is_valid_tool("InvalidTool"));
    }

    #[test]
    fn test_agent_color_from_str() {
        assert_eq!("blue".parse::<AgentColor>().unwrap(), AgentColor::Blue);
        assert_eq!("GREEN".parse::<AgentColor>().unwrap(), AgentColor::Green);
        assert_eq!("Purple".parse::<AgentColor>().unwrap(), AgentColor::Purple);
        assert_eq!("orange".parse::<AgentColor>().unwrap(), AgentColor::Orange);
        assert_eq!("RED".parse::<AgentColor>().unwrap(), AgentColor::Red);
        assert_eq!("invalid".parse::<AgentColor>().unwrap(), AgentColor::Blue);
        assert_eq!("".parse::<AgentColor>().unwrap(), AgentColor::Blue);
    }
}
