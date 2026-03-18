//! Base Agents Generator
//!
//! Generates base agents from configurable specs with project-specific context injection.
//! Uses LLM-First GenerationContext - no filtering, LLM decides relevance.

use crate::pipeline::analysis::PatternInstance;
use crate::pipeline::generation::context::GenerationContext;
use crate::types::agent::{Agent, AgentColor, AgentModel, PermissionMode};
use crate::types::artifact_category::{AGENT_REVIEWER, AGENT_CODER, AGENT_ARCHITECT};

/// Specification for a base agent.
///
/// Captures all configuration needed to generate an agent without hardcoding.
/// Use `BaseAgentSpec::default_specs()` for the standard reviewer/coder/architect trio,
/// or construct custom specs for project-specific agents.
#[derive(Debug, Clone)]
pub struct BaseAgentSpec {
    pub name: String,
    pub role: String,
    pub color: AgentColor,
    pub model: AgentModel,
    pub tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub skills: Vec<String>,
    pub permission_mode: PermissionMode,
    pub prompt_header: String,
}

impl BaseAgentSpec {
    /// Returns the default 3 agent specs: reviewer, coder, architect.
    pub fn default_specs() -> Vec<BaseAgentSpec> {
        vec![
            BaseAgentSpec {
                name: AGENT_REVIEWER.into(),
                role: "Code quality gatekeeper".into(),
                color: AgentColor::Blue,
                model: AgentModel::Sonnet,
                tools: super::tool_sets::read_only(),
                disallowed_tools: super::tool_sets::write_tools(),
                skills: vec!["code-review".into()],
                permission_mode: PermissionMode::Default,

                prompt_header: r#"# Reviewer Agent

## Role

Code quality gatekeeper. Validates correctness, security, and convention compliance.

## Workflow

1. Receive files for review
2. Rules auto-injected based on file paths
3. Execute `code-review` skill
4. Output PASS/ISSUES

## Guidelines

- **Read-Only**: No file modifications
- **Evidence-Based**: All issues require @file:line evidence
- **Rule-Referenced**: Cite violated rule

## Output Format

```
PASS
```
or
```
ISSUES

[SEVERITY] file:line - description
  Rule: {violated rule path}
  Fix: {suggested fix}
```
"#
                .into(),
            },
            BaseAgentSpec {
                name: AGENT_CODER.into(),
                role: "Feature implementation specialist".into(),
                color: AgentColor::Green,
                model: AgentModel::Sonnet,
                tools: super::tool_sets::full_access(),
                disallowed_tools: vec![],
                skills: vec!["implement".into(), "debug".into(), "refactor".into()],
                permission_mode: PermissionMode::AcceptEdits,

                prompt_header: r#"# Coder Agent

## Role

Feature implementation specialist. Transforms requirements into working code.

## Workflow

1. Receive implementation request
2. Rules auto-injected for context
3. Execute appropriate skill (implement/debug/refactor)
4. Verify with tests

## Guidelines

- **Module Boundaries**: Work within module responsibility
- **Convention Compliance**: Follow rules
- **Minimal Footprint**: Only necessary changes
- **Test Coverage**: Test new code

## Output

1. List of changed files
2. Summary of changes
3. Test execution results
"#
                .into(),
            },
            BaseAgentSpec {
                name: AGENT_ARCHITECT.into(),
                role: "System design and planning specialist".into(),
                color: AgentColor::Purple,
                model: AgentModel::Sonnet,
                tools: super::tool_sets::read_only(),
                disallowed_tools: super::tool_sets::write_tools(),
                skills: vec!["plan".into()],
                permission_mode: PermissionMode::Plan,

                prompt_header: r#"# Architect Agent

## Role

System design specialist. Designs features to align with architecture.

## Workflow

1. Receive requirements
2. Check architecture rules (rules/project.md, rules/groups/*.md)
3. Execute `plan` skill
4. Output implementation plan

## Guidelines

- **Architecture Alignment**: Consistent with existing architecture
- **Minimal Complexity**: Only necessary complexity
- **Future-Proof**: Extensible design

## Output Format

```markdown
## Plan: {title}

### Affected Modules
- {module}: {impact}

### Tasks
1. [ ] {task} - {files}

### Risks
- {risk}: {mitigation}

### Verification
- {how to verify completion}
```
"#
                .into(),
            },
        ]
    }
}

fn append_analysis_sections(
    body: &mut String,
    ctx: &GenerationContext<'_>,
    discovered_insights: &[&crate::pipeline::analysis::cross_synthesis::Tier3Insight],
    patterns: &[&PatternInstance],
    pattern_section_title: &str,
) {
    if !discovered_insights.is_empty() {
        body.push_str("\n## Critical Insights\n\n");
        body.push_str(&ctx.format_discovered_insights());
        body.push('\n');
    }

    if !patterns.is_empty() {
        body.push_str(&format!("\n## {}\n\n", pattern_section_title));
        body.push_str(&ctx.format_patterns(patterns));
        body.push('\n');
    }

    let constraints_text = ctx.format_constraints();
    if !constraints_text.is_empty() {
        body.push_str("\n## Project Constraints\n\n");
        body.push_str(&constraints_text);
        body.push('\n');
    }
}

/// Appends agent-specific dynamic sections based on agent name.
///
/// The coder gets domain context; the architect gets core files.
/// Other/custom agents get the standard analysis sections only.
fn append_agent_specific_sections(body: &mut String, ctx: &GenerationContext<'_>, name: &str) {
    match name {
        AGENT_CODER => {
            let domain = ctx.domain_knowledge();
            if let Some(d) = &domain {
                let domain_text = ctx.format_domain(&Some(d.clone()));
                if !domain_text.is_empty() {
                    body.push_str("\n## Domain Context\n\n");
                    body.push_str(&domain_text);
                    body.push('\n');
                }
            }
        }
        AGENT_ARCHITECT => {
            let files = ctx.all_files_with_context();
            if !files.is_empty() {
                body.push_str("\n## Core Files\n\n");
                for file in &files {
                    body.push_str(&format!("- @{}\n", file.path));
                }
            }
        }
        _ => {}
    }
}

/// Appends applicable rules section referencing the generated rules directory.
///
/// Each agent gets a reference to the rules directory so Claude Code's context system
/// can auto-inject relevant rules based on file paths during the agent's operation.
fn append_applicable_rules(body: &mut String, agent_name: &str) {
    body.push_str("\n## Applicable Rules\n\n");
    body.push_str("Rules are auto-injected from `.claude/rules/` based on file paths.\n");
    match agent_name {
        AGENT_REVIEWER => {
            body.push_str("- @.claude/rules/project.md\n");
            body.push_str("- @.claude/rules/modules/\n");
        }
        AGENT_CODER => {
            body.push_str("- @.claude/rules/tech/\n");
            body.push_str("- @.claude/rules/frameworks/\n");
            body.push_str("- @.claude/rules/modules/\n");
        }
        AGENT_ARCHITECT => {
            body.push_str("- @.claude/rules/project.md\n");
            body.push_str("- @.claude/rules/groups/\n");
            body.push_str("- @.claude/rules/cross-cutting/\n");
        }
        _ => {
            body.push_str("- @.claude/rules/project.md\n");
        }
    }
}

/// Appends a Related Resources section that cross-references related agents, skills, and rules.
///
/// Creates an agent→skill→rule navigation graph so Claude Code can preload relevant
/// artifacts during agent operation.
fn append_related_resources(body: &mut String, ctx: &GenerationContext<'_>, agent_name: &str) {
    let skill_names = ctx.available_skill_names();
    if skill_names.is_empty() {
        return;
    }

    let related_skills: Vec<&String> = match agent_name {
        AGENT_REVIEWER => skill_names
            .iter()
            .filter(|s| {
                let l = s.to_lowercase();
                l.contains("review") || l.contains("audit") || l.contains("lint") || l.contains("check")
            })
            .collect(),
        AGENT_CODER => skill_names
            .iter()
            .filter(|s| {
                let l = s.to_lowercase();
                l.contains("implement") || l.contains("debug") || l.contains("refactor")
                    || l.contains("fix") || l.contains("create") || l.contains("migrate")
                    || l.contains("add") || l.contains("build")
            })
            .collect(),
        AGENT_ARCHITECT => skill_names
            .iter()
            .filter(|s| {
                let l = s.to_lowercase();
                l.contains("plan") || l.contains("design") || l.contains("architect") || l.contains("rfc")
            })
            .collect(),
        _ => Vec::new(),
    };

    if related_skills.is_empty() {
        return;
    }

    body.push_str("\n## Related Resources\n\n");
    body.push_str("### Preload Skills\n");
    for skill in &related_skills {
        body.push_str(&format!("- /{}\n", skill));
    }

    // Cross-reference other agents for handoff scenarios
    let handoff_agents: Vec<&str> = match agent_name {
        AGENT_REVIEWER => vec![AGENT_CODER],
        AGENT_CODER => vec![AGENT_REVIEWER, AGENT_ARCHITECT],
        AGENT_ARCHITECT => vec![AGENT_CODER],
        _ => Vec::new(),
    };

    if !handoff_agents.is_empty() {
        body.push_str("\n### Handoff Agents\n");
        for agent in handoff_agents {
            body.push_str(&format!("- @.claude/agents/{}.md\n", agent));
        }
    }

    // Cross-reference specific rule categories relevant to this agent's role
    let rule_refs: Vec<&str> = match agent_name {
        AGENT_REVIEWER => vec![
            ".claude/rules/project.md",
            ".claude/rules/cross-cutting/",
        ],
        AGENT_CODER => vec![
            ".claude/rules/tech/",
            ".claude/rules/frameworks/",
            ".claude/rules/modules/",
        ],
        AGENT_ARCHITECT => vec![
            ".claude/rules/project.md",
            ".claude/rules/groups/",
            ".claude/rules/domains/",
        ],
        _ => Vec::new(),
    };

    if !rule_refs.is_empty() {
        body.push_str("\n### Related Rules\n");
        for rule_ref in rule_refs {
            body.push_str(&format!("- @{}\n", rule_ref));
        }
    }
}

/// Appends available skills section from the generation context.
fn append_available_skills(body: &mut String, ctx: &GenerationContext<'_>) {
    let skill_names = ctx.available_skill_names();
    if skill_names.is_empty() {
        return;
    }

    body.push_str("\n## Available Skills\n\n");
    for name in skill_names {
        body.push_str(&format!("- /{}\n", name));
    }
}

pub struct BaseAgentsGenerator;

impl BaseAgentsGenerator {
    /// Generate agents from the default specs.
    pub fn generate(ctx: &GenerationContext<'_>) -> Vec<Agent> {
        Self::generate_from_specs(&BaseAgentSpec::default_specs(), ctx)
    }

    /// Generate agents from custom specs.
    ///
    /// For each spec, builds the prompt body from the spec's `prompt_header`,
    /// appends analysis sections from GenerationContext, and applies all
    /// configuration (tools, skills, color, model, permissions).
    pub fn generate_from_specs(
        specs: &[BaseAgentSpec],
        ctx: &GenerationContext<'_>,
    ) -> Vec<Agent> {
        specs
            .iter()
            .map(|spec| Self::generate_from_spec(spec, ctx))
            .collect()
    }

    fn generate_from_spec(
        spec: &BaseAgentSpec,
        ctx: &GenerationContext<'_>,
    ) -> Agent {
        let patterns = ctx.all_patterns();
        let insights = ctx.all_discovered_insights();

        let pattern_section_title = match spec.name.as_str() {
            AGENT_CODER => "Implementation Patterns",
            _ => "Architecture Patterns",
        };

        let mut body = spec.prompt_header.clone();
        append_analysis_sections(&mut body, ctx, &insights, &patterns, pattern_section_title);
        append_agent_specific_sections(&mut body, ctx, &spec.name);
        append_applicable_rules(&mut body, &spec.name);
        append_related_resources(&mut body, ctx, &spec.name);
        append_available_skills(&mut body, ctx);

        let mut agent = Agent::new(&spec.name, &spec.role, &body)
            .color(spec.color)
            .model(spec.model)
            .tools(spec.tools.clone())
            .skills(spec.skills.clone())
            .permission_mode(spec.permission_mode);

        if !spec.disallowed_tools.is_empty() {
            agent = agent.disallowed_tools(spec.disallowed_tools.clone());
        }

        agent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::context::VerifiedFileRegistry;
    use crate::pipeline::phases::{
        constraint_extraction::ExtractedConstraints, convention_inference::InferredConventions,
        project_detection::ProjectDetection,
    };
    use crate::types::module_map::TechStack;

    fn test_context<'a>(
        detection: &'a ProjectDetection,
        tech_stack: &'a TechStack,
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        registry: &'a VerifiedFileRegistry,
    ) -> GenerationContext<'a> {
        GenerationContext::new(
            detection,
            tech_stack,
            "test-project",
            &[],
            &[],
            &[],
            conventions,
            constraints,
            registry,
        )
    }

    fn make_agents() -> Vec<Agent> {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );
        BaseAgentsGenerator::generate(&ctx)
    }

    #[test]
    fn test_generates_three_agents() {
        let agents = make_agents();
        assert_eq!(agents.len(), 3);
    }

    #[test]
    fn test_agent_names() {
        let agents = make_agents();
        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"reviewer"));
        assert!(names.contains(&"coder"));
        assert!(names.contains(&"architect"));
    }

    #[test]
    fn test_reviewer_is_read_only() {
        let agents = make_agents();
        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        let tools = reviewer.tools.as_ref().unwrap();
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));

        let disallowed = reviewer.disallowed_tools.as_ref().unwrap();
        assert!(disallowed.contains(&"Write".to_string()));
        assert!(disallowed.contains(&"Edit".to_string()));
        assert!(disallowed.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_coder_has_edit_tools() {
        let agents = make_agents();
        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        let tools = coder.tools.as_ref().unwrap();
        assert!(tools.contains(&"Edit".to_string()));
        assert!(tools.contains(&"Write".to_string()));
        assert!(tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_architect_is_read_only() {
        let agents = make_agents();
        let architect = agents.iter().find(|a| a.name == "architect").unwrap();
        let tools = architect.tools.as_ref().unwrap();
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));

        let disallowed = architect.disallowed_tools.as_ref().unwrap();
        assert!(disallowed.contains(&"Write".to_string()));
        assert!(disallowed.contains(&"Edit".to_string()));
        assert!(disallowed.contains(&"Bash".to_string()));
        assert_eq!(architect.permission_mode, Some(PermissionMode::Plan));
    }

    #[test]
    fn test_agent_skills() {
        let agents = make_agents();

        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        assert_eq!(
            reviewer.skills.as_ref().unwrap(),
            &vec!["code-review".to_string()]
        );

        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        let coder_skills = coder.skills.as_ref().unwrap();
        assert!(coder_skills.contains(&"implement".to_string()));
        assert!(coder_skills.contains(&"debug".to_string()));
        assert!(coder_skills.contains(&"refactor".to_string()));

        let architect = agents.iter().find(|a| a.name == "architect").unwrap();
        assert_eq!(
            architect.skills.as_ref().unwrap(),
            &vec!["plan".to_string()]
        );
    }

    #[test]
    fn test_coder_permission_mode() {
        let agents = make_agents();
        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        assert_eq!(coder.permission_mode, Some(PermissionMode::AcceptEdits));
    }

    #[test]
    fn test_agents_have_colors() {
        let agents = make_agents();
        for agent in &agents {
            assert!(
                agent.color.is_some(),
                "Agent {} should have a color",
                agent.name
            );
        }
    }

    #[test]
    fn test_agents_validation() {
        let agents = make_agents();
        for agent in &agents {
            let issues = agent.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(
                errors.is_empty(),
                "Agent {} has validation errors: {:?}",
                agent.name,
                errors
            );
        }
    }

    // =========================================================================
    // New tests for BaseAgentSpec and generate_from_specs
    // =========================================================================

    #[test]
    fn test_default_specs_count() {
        let specs = BaseAgentSpec::default_specs();
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].name, "reviewer");
        assert_eq!(specs[1].name, "coder");
        assert_eq!(specs[2].name, "architect");
    }

    #[test]
    fn test_custom_spec_generates_agent() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );
        let custom_spec = BaseAgentSpec {
            name: "security-auditor".into(),
            role: "Security analysis specialist".into(),
            color: AgentColor::Red,
            model: AgentModel::Opus,
            tools: vec!["Read".into(), "Grep".into(), "Glob".into()],
            disallowed_tools: vec!["Write".into(), "Edit".into(), "Bash".into()],
            skills: vec!["security-audit".into()],
            permission_mode: PermissionMode::Default,
            prompt_header: "# Security Auditor\n\nAnalyze code for security vulnerabilities.\n"
                .into(),
        };

        let agents = BaseAgentsGenerator::generate_from_specs(&[custom_spec], &ctx);

        assert_eq!(agents.len(), 1);
        let agent = &agents[0];
        assert_eq!(agent.name, "security-auditor");
        assert_eq!(agent.description, "Security analysis specialist");
        assert_eq!(agent.color, Some(AgentColor::Red));
        assert_eq!(agent.model, Some(AgentModel::Opus));
        assert_eq!(agent.permission_mode, Some(PermissionMode::Default));
        assert!(agent.prompt.contains("Security Auditor"));

        let tools = agent.tools.as_ref().unwrap();
        assert!(tools.contains(&"Read".to_string()));
        assert!(!tools.contains(&"Edit".to_string()));

        let disallowed = agent.disallowed_tools.as_ref().unwrap();
        assert!(disallowed.contains(&"Write".to_string()));

        let skills = agent.skills.as_ref().unwrap();
        assert_eq!(skills, &vec!["security-audit".to_string()]);

        // Validate the agent
        let issues = agent.validate();
        let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
        assert!(errors.is_empty(), "Custom agent has validation errors: {:?}", errors);
    }

    #[test]
    fn test_generate_from_specs_matches_generate() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &conventions,
            &constraints,
            &registry,
        );
        let from_generate = BaseAgentsGenerator::generate(&ctx);
        let from_specs = BaseAgentsGenerator::generate_from_specs(
            &BaseAgentSpec::default_specs(),
            &ctx,
        );

        assert_eq!(from_generate.len(), from_specs.len());
        for (a, b) in from_generate.iter().zip(from_specs.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.description, b.description);
            assert_eq!(a.color, b.color);
            assert_eq!(a.model, b.model);
            assert_eq!(a.tools, b.tools);
            assert_eq!(a.disallowed_tools, b.disallowed_tools);
            assert_eq!(a.skills, b.skills);
            assert_eq!(a.permission_mode, b.permission_mode);
            assert_eq!(a.prompt, b.prompt);
        }
    }
}
