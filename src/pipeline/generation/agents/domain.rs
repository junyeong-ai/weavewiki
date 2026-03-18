//! Domain Expert Agent Generator
//!
//! Generates expert agents for detected business domains.
//! LLM-First: Provides full context, LLM decides relevance.

use crate::config::Config;
use crate::pipeline::generation::context::GenerationContext;
use crate::types::agent::{Agent, AgentColor, AgentModel, PermissionMode};
use crate::types::module_map::Domain;
use crate::pipeline::evidence::artifact_ref;

pub struct DomainAgentGenerator;

impl DomainAgentGenerator {
    pub fn generate(ctx: &GenerationContext<'_>, config: &Config) -> Vec<Agent> {
        if !config.generation.domain_experts {
            return vec![];
        }

        ctx.domains.iter().map(|d| Self::create_agent(d, ctx)).collect()
    }

    fn create_agent(domain: &Domain, ctx: &GenerationContext<'_>) -> Agent {
        Agent::new(
            format!("{}-expert", domain.id),
            format!("{} domain expert", domain.name),
            Self::generate_prompt(domain, ctx),
        )
        .color(AgentColor::Purple)
        .model(AgentModel::Sonnet)
        .tools(super::tool_sets::read_only())
        .disallowed_tools(super::tool_sets::write_tools())
        .skills(vec!["plan".into(), "code-review".into()])
        .permission_mode(PermissionMode::Default)
    }

    fn generate_prompt(domain: &Domain, ctx: &GenerationContext<'_>) -> String {
        let groups = domain.group_ids.join(", ");
        let boundaries = if domain.boundary_rules.is_empty() {
            "None specified".to_string()
        } else {
            domain.boundary_rules.join("\n- ")
        };

        let mut knowledge_section = String::new();

        // Add domain-related insights
        let domain_insights: Vec<_> = ctx
            .all_discovered_insights()
            .into_iter()
            .filter(|i| {
                i.title.to_lowercase().contains(&domain.id.to_lowercase())
                    || i.description
                        .to_lowercase()
                        .contains(&domain.id.to_lowercase())
            })
            .collect();

        if !domain_insights.is_empty() {
            knowledge_section.push_str("\n## Critical Insights\n\n");
            for insight in &domain_insights {
                knowledge_section.push_str(&format!("### [{}] {}\n", insight.category, insight.title));
                knowledge_section.push_str(&insight.description);
                knowledge_section.push_str(&format!("\n**Prevention**: {}\n", insight.prevention_guidance));
                for ev in &insight.evidence {
                    knowledge_section.push_str(&format!("{}\n", artifact_ref(&ev.file, ev.start_line)));
                }
                knowledge_section.push('\n');
            }
        }

        // Add patterns relevant to domain
        let domain_patterns = ctx.all_patterns();
        let relevant_patterns: Vec<_> = domain_patterns
            .into_iter()
            .filter(|p| {
                p.description
                    .to_lowercase()
                    .contains(&domain.id.to_lowercase())
                    || p.locations.iter().any(|l| {
                        domain
                            .group_ids
                            .iter()
                            .any(|g| l.file.to_lowercase().contains(&g.to_lowercase()))
                    })
            })
            .collect();

        if !relevant_patterns.is_empty() {
            knowledge_section.push_str("\n## Relevant Patterns\n\n");
            knowledge_section.push_str(&ctx.format_patterns(&relevant_patterns));
            knowledge_section.push('\n');
        }

        format!(
            r#"# {name} Domain Expert

## Scope

- Domain: {name}
- Groups: {groups}
- Responsibility: {responsibility}

## Boundary Rules

- {boundaries}
{knowledge_section}
## Context

Domain rules are auto-injected: rules/domains/{id}.md

## Role

You are the domain expert for {name}.
- Deep knowledge of domain concepts and business rules
- Cross-module oversight within this domain
- Veto power for domain boundary violations

## Workflow

1. Receive request affecting {name} domain
2. Verify domain boundary compliance
3. Apply relevant skill (plan/code-review)
4. VETO if domain rules violated

## Guidelines

- Read-only access (advisory role)
- Can veto on domain boundary violations
- Focus on domain integrity over implementation details"#,
            id = domain.id,
            name = domain.name,
            groups = groups,
            responsibility = domain.responsibility,
            boundaries = boundaries,
            knowledge_section = knowledge_section,
        )
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

    fn test_config() -> Config {
        let mut config = Config::default();
        config.generation.domain_experts = true;
        config
    }

    fn test_context<'a>(
        detection: &'a ProjectDetection,
        tech_stack: &'a TechStack,
        domains: &'a [Domain],
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
            domains,
            conventions,
            constraints,
            registry,
        )
    }

    fn test_domain(id: &str) -> Domain {
        Domain {
            id: id.into(),
            name: format!("{} Domain", id),
            group_ids: vec!["group1".into()],
            responsibility: format!("{} business logic", id),
            boundary_rules: vec!["No direct DB access from API layer".into()],
            interfaces: vec![],
            owner: String::new(),
        }
    }

    #[test]
    fn test_generates_for_domains() {
        let config = test_config();
        let domains = vec![test_domain("identity"), test_domain("billing")];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &domains,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = DomainAgentGenerator::generate(&ctx, &config);

        assert_eq!(agents.len(), 2);
        assert!(agents.iter().any(|a| a.name == "identity-expert"));
        assert!(agents.iter().any(|a| a.name == "billing-expert"));
    }

    #[test]
    fn test_disabled_when_config_false() {
        let mut config = test_config();
        config.generation.domain_experts = false;

        let domains = vec![test_domain("identity")];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &domains,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = DomainAgentGenerator::generate(&ctx, &config);

        assert!(agents.is_empty());
    }

    #[test]
    fn test_agent_is_read_only() {
        let config = test_config();
        let domains = vec![test_domain("identity")];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &domains,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = DomainAgentGenerator::generate(&ctx, &config);
        let agent = &agents[0];

        let tools = agent.tools.as_ref().unwrap();
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));

        let disallowed = agent.disallowed_tools.as_ref().unwrap();
        assert!(disallowed.contains(&"Write".to_string()));
        assert!(disallowed.contains(&"Edit".to_string()));
        assert!(disallowed.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_agent_has_veto() {
        let config = test_config();
        let domains = vec![test_domain("identity")];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &domains,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = DomainAgentGenerator::generate(&ctx, &config);
        let agent = &agents[0];

        assert!(agent.permission_mode == Some(PermissionMode::Default));
    }

    #[test]
    fn test_agent_has_planning_skills() {
        let config = test_config();
        let domains = vec![test_domain("identity")];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &domains,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = DomainAgentGenerator::generate(&ctx, &config);
        let agent = &agents[0];

        let skills = agent.skills.as_ref().unwrap();
        assert!(skills.contains(&"plan".to_string()));
        assert!(skills.contains(&"code-review".to_string()));
    }
}
