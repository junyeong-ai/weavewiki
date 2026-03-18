//! Claude Code Plugin types based on official plugin architecture
//!
//! Plugins provide isolated, managed extensions for Claude Code including
//! skills, agents, and hooks with a standardized manifest format.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::agent::Agent;
use super::hook::HooksConfig;
use super::rule::Rule;
use super::skill::Skill;
use super::utils::is_kebab_case;
use crate::constants::plugin as plugin_constants;

/// Repository information for plugin source
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryInfo {
    /// Repository type (e.g., "git")
    #[serde(rename = "type")]
    pub repo_type: String,
    /// Repository URL
    pub url: String,
}

impl RepositoryInfo {
    pub fn git(url: impl Into<String>) -> Self {
        Self {
            repo_type: "git".to_string(),
            url: url.into(),
        }
    }
}

/// Plugin manifest (plugin.json) - the core definition of a Claude Code plugin
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin identifier (kebab-case, e.g., "claudegen-docs")
    pub name: String,
    /// Semantic version (e.g., "1.0.0")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Plugin author or generator
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Homepage or repository URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Repository information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryInfo>,
    /// License identifier (e.g., "MIT", "Apache-2.0")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Keywords for discovery
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    /// Path to MCP servers configuration
    #[serde(rename = "mcpServers", skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<String>,
    /// Path to LSP servers configuration
    #[serde(rename = "lspServers", skip_serializing_if = "Option::is_none")]
    pub lsp_servers: Option<String>,
    /// Tool permissions for plugin
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PluginPermissions>,
    /// Plugin-level hooks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HooksConfig>,
    /// Schema version for multi-agent support
    #[serde(rename = "schemaVersion", skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<String>,
    /// Generator tool identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    /// Required skills for this plugin
    #[serde(rename = "requiredSkills", skip_serializing_if = "Option::is_none")]
    pub required_skills: Option<Vec<String>>,
    /// Required agents for this plugin
    #[serde(rename = "requiredAgents", skip_serializing_if = "Option::is_none")]
    pub required_agents: Option<Vec<String>>,
}

/// Plugin permissions control tool access
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PluginPermissions {
    /// Allowed tools (if set, only these tools are permitted)
    #[serde(rename = "allowedTools", skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    /// Disallowed tools (blocked even if in allowed list)
    #[serde(rename = "disallowedTools", skip_serializing_if = "Option::is_none")]
    pub disallowed_tools: Option<Vec<String>>,
}

impl PluginPermissions {
    /// Validate plugin permissions
    pub fn validate(&self) -> Vec<super::ValidationIssue> {
        use crate::utils::is_valid_tool;
        let mut issues = Vec::new();

        // Validate allowed_tools
        if let Some(tools) = &self.allowed_tools {
            for tool in tools {
                if !is_valid_tool(tool) {
                    issues.push(super::ValidationIssue::warning(
                        "INVALID_ALLOWED_TOOL",
                        format!("unknown tool in allowedTools: {}", tool),
                    ));
                }
            }
        }

        // Validate disallowed_tools
        if let Some(tools) = &self.disallowed_tools {
            for tool in tools {
                if !is_valid_tool(tool) {
                    issues.push(super::ValidationIssue::warning(
                        "INVALID_DISALLOWED_TOOL",
                        format!("unknown tool in disallowedTools: {}", tool),
                    ));
                }
            }
        }

        // Check for conflicts (tool in both allowed and disallowed)
        if let (Some(allowed), Some(disallowed)) = (&self.allowed_tools, &self.disallowed_tools) {
            for tool in allowed {
                if disallowed.contains(tool) {
                    issues.push(super::ValidationIssue::warning(
                        "TOOL_CONFLICT",
                        format!(
                            "tool '{}' appears in both allowedTools and disallowedTools",
                            tool
                        ),
                    ));
                }
            }
        }

        issues
    }
}

/// Complete plugin package ready for output
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plugin {
    pub manifest: PluginManifest,
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub rules: Vec<Rule>,
}

impl PluginManifest {
    /// Create a new plugin manifest with name only (version/description optional per spec)
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: None,
            mcp_servers: None,
            lsp_servers: None,
            permissions: None,
            hooks: None,
            schema_version: None,
            generator: None,
            required_skills: None,
            required_agents: None,
        }
    }

    /// Create with version and description for convenience
    pub fn with_version_and_description(
        name: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: Some(version.into()),
            description: Some(description.into()),
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: None,
            mcp_servers: None,
            lsp_servers: None,
            permissions: None,
            hooks: None,
            schema_version: None,
            generator: None,
            required_skills: None,
            required_agents: None,
        }
    }

    /// Set the version
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set the description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the author
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Set the homepage
    pub fn homepage(mut self, homepage: impl Into<String>) -> Self {
        self.homepage = Some(homepage.into());
        self
    }

    /// Set repository info
    pub fn repository(mut self, repository: RepositoryInfo) -> Self {
        self.repository = Some(repository);
        self
    }

    /// Set license
    pub fn license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    /// Set keywords
    pub fn keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = Some(keywords);
        self
    }

    /// Set MCP servers config path
    pub fn mcp_servers(mut self, path: impl Into<String>) -> Self {
        self.mcp_servers = Some(path.into());
        self
    }

    /// Set LSP servers config path
    pub fn lsp_servers(mut self, path: impl Into<String>) -> Self {
        self.lsp_servers = Some(path.into());
        self
    }

    /// Set permissions
    pub fn permissions(mut self, permissions: PluginPermissions) -> Self {
        self.permissions = Some(permissions);
        self
    }

    /// Set hooks
    pub fn hooks(mut self, hooks: HooksConfig) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn schema_version(mut self, version: impl Into<String>) -> Self {
        self.schema_version = Some(version.into());
        self
    }

    pub fn generator(mut self, generator: impl Into<String>) -> Self {
        self.generator = Some(generator.into());
        self
    }

    pub fn required_skills(mut self, skills: Vec<String>) -> Self {
        self.required_skills = Some(skills);
        self
    }

    pub fn required_agents(mut self, agents: Vec<String>) -> Self {
        self.required_agents = Some(agents);
        self
    }

    /// Serialize to JSON for plugin.json
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Validate the manifest
    pub fn validate(&self) -> Vec<super::ValidationIssue> {
        let mut errors = Vec::new();

        // Name validation (required)
        if self.name.is_empty() {
            errors.push(super::ValidationIssue::error(
                "MANIFEST_NAME_REQUIRED",
                "plugin name is required",
            ));
        } else if self.name.len() > plugin_constants::MAX_NAME_LENGTH {
            errors.push(super::ValidationIssue::error(
                "MANIFEST_NAME_TOO_LONG",
                format!(
                    "plugin name exceeds {} characters",
                    plugin_constants::MAX_NAME_LENGTH
                ),
            ));
        } else if !is_valid_plugin_name(&self.name) {
            errors.push(super::ValidationIssue::error(
                "MANIFEST_NAME_INVALID",
                "plugin name must be kebab-case (lowercase letters, numbers, hyphens)",
            ));
        }

        // Description validation (optional, but if present must be valid)
        if let Some(ref desc) = self.description
            && desc.len() > 1024
        {
            errors.push(super::ValidationIssue::error(
                "MANIFEST_DESC_TOO_LONG",
                "plugin description exceeds 1024 characters",
            ));
        }

        errors
    }
}

impl Plugin {
    pub fn new(manifest: PluginManifest) -> Self {
        Self {
            manifest,
            skills: Vec::new(),
            agents: Vec::new(),
            rules: Vec::new(),
        }
    }

    pub fn add_skill(&mut self, skill: Skill) {
        self.skills.push(skill);
    }

    pub fn add_agent(&mut self, agent: Agent) {
        self.agents.push(agent);
    }

    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Get the plugin root directory (.claude/plugins/{project-name}/)
    ///
    /// Unified plugin structure for claude-pilot integration:
    /// ```text
    /// .claude/plugins/{project-name}/
    /// ├── .claude-plugin/
    /// │   └── plugin.json
    /// ├── module_map.json
    /// ├── rules/
    /// │   ├── project.md
    /// │   ├── tech/{lang}.md
    /// │   ├── frameworks/{fw}.md
    /// │   ├── modules/{module}.md
    /// │   ├── groups/{group}.md
    /// │   └── domains/{domain}.md
    /// ├── skills/
    /// │   └── {skill}/SKILL.md
    /// └── agents/
    ///     └── {agent}.md
    /// ```
    pub fn plugin_dir(&self, base: &Path) -> PathBuf {
        base.join(".claude").join("plugins").join(&self.manifest.name)
    }

    /// Get the rules directory path (rules/)
    pub fn rules_dir(&self, base: &Path) -> PathBuf {
        self.plugin_dir(base).join("rules")
    }

    /// Get full path to plugin.json (.claude-plugin/plugin.json)
    pub fn manifest_path(&self, base: &Path) -> PathBuf {
        self.plugin_dir(base)
            .join(".claude-plugin")
            .join("plugin.json")
    }

    /// Get the skills directory path (skills/)
    pub fn skills_dir(&self, base: &Path) -> PathBuf {
        self.plugin_dir(base).join("skills")
    }

    /// Get the agents directory path (agents/)
    pub fn agents_dir(&self, base: &Path) -> PathBuf {
        self.plugin_dir(base).join("agents")
    }

    /// Get the commands directory path (commands/)
    pub fn commands_dir(&self, base: &Path) -> PathBuf {
        self.plugin_dir(base).join("commands")
    }

    pub fn hooks_dir(&self, base: &Path) -> PathBuf {
        self.plugin_dir(base).join("hooks")
    }

    pub fn validate(&self) -> PluginValidationResult {
        use super::validation::ValidationIssue;
        use std::collections::HashSet;

        let manifest_errors = self.manifest.validate();

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

        PluginValidationResult {
            manifest_errors,
            skill_errors,
            agent_errors,
            rule_errors,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PluginValidationResult {
    pub manifest_errors: Vec<super::ValidationIssue>,
    pub skill_errors: Vec<(String, Vec<super::ValidationIssue>)>,
    pub agent_errors: Vec<(String, Vec<super::ValidationIssue>)>,
    pub rule_errors: Vec<(String, Vec<super::ValidationIssue>)>,
}

impl PluginValidationResult {
    pub fn is_valid(&self) -> bool {
        !self.manifest_errors.iter().any(|e| e.is_error())
            && self.skill_errors.is_empty()
            && self.agent_errors.is_empty()
            && self.rule_errors.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.manifest_errors.iter().filter(|e| e.is_error()).count()
            + self
                .skill_errors
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
        self.manifest_errors
            .iter()
            .filter(|e| e.severity.is_warning())
            .count()
            + self
                .skill_errors
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

/// Validate plugin name format (kebab-case, optionally hidden with leading dot)
fn is_valid_plugin_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    // Handle hidden directory names (e.g., ".claudegen")
    let check_name = if let Some(stripped) = name.strip_prefix('.') {
        if stripped.is_empty() {
            return false; // Just "." is invalid
        }
        stripped
    } else {
        name
    };

    is_kebab_case(check_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_plugin_names() {
        assert!(is_valid_plugin_name("claudegen"));
        assert!(is_valid_plugin_name("claudegen-docs"));
        assert!(is_valid_plugin_name("my-plugin-v2"));
        // Hidden directory names
        assert!(is_valid_plugin_name(".claudegen"));
        assert!(is_valid_plugin_name(".my-plugin"));
    }

    #[test]
    fn test_invalid_plugin_names() {
        assert!(!is_valid_plugin_name(""));
        assert!(!is_valid_plugin_name("-plugin"));
        assert!(!is_valid_plugin_name("plugin-"));
        assert!(!is_valid_plugin_name("my--plugin"));
        assert!(!is_valid_plugin_name("MyPlugin"));
        assert!(!is_valid_plugin_name("my_plugin"));
        // Invalid hidden names
        assert!(!is_valid_plugin_name("."));
        assert!(!is_valid_plugin_name(".-plugin"));
        assert!(!is_valid_plugin_name(".plugin-"));
    }

    #[test]
    fn test_manifest_validation() {
        let manifest =
            PluginManifest::with_version_and_description("claudegen", "1.0.0", "A test plugin");
        assert!(manifest.validate().is_empty());

        // Empty name is the only required field error now (version/description are optional)
        let invalid = PluginManifest::new("");
        let errors = invalid.validate();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "MANIFEST_NAME_REQUIRED");
    }

    #[test]
    fn test_plugin_paths() {
        // Plugin output structure: .claude/plugins/{project-name}/
        let manifest =
            PluginManifest::with_version_and_description("myproject", "1.0.0", "Test");
        let plugin = Plugin::new(manifest);
        let base = Path::new("/project");

        // Plugin directory at .claude/plugins/{project}/
        assert_eq!(
            plugin.plugin_dir(base),
            PathBuf::from("/project/.claude/plugins/myproject")
        );
        // Plugin manifest at .claude-plugin/plugin.json
        assert_eq!(
            plugin.manifest_path(base),
            PathBuf::from("/project/.claude/plugins/myproject/.claude-plugin/plugin.json")
        );
        // Skills at skills/
        assert_eq!(
            plugin.skills_dir(base),
            PathBuf::from("/project/.claude/plugins/myproject/skills")
        );
        // Agents at agents/
        assert_eq!(
            plugin.agents_dir(base),
            PathBuf::from("/project/.claude/plugins/myproject/agents")
        );
        // Rules at rules/
        assert_eq!(
            plugin.rules_dir(base),
            PathBuf::from("/project/.claude/plugins/myproject/rules")
        );
    }
}
