//! Module Specialist Agent Generator
//!
//! Generates specialist agents for high-value modules.
//! LLM-First: Provides full module context, LLM decides relevance.

use crate::config::Config;
use crate::pipeline::generation::context::GenerationContext;
use crate::pipeline::generation::skills::ModuleSkillResolver;
use crate::types::agent::{Agent, AgentColor, AgentModel, PermissionMode};
use crate::types::module_map::{DetectedModule, EvidenceLocation, Module};
use crate::pipeline::evidence::artifact_ref;

pub struct ModuleAgentGenerator;

impl ModuleAgentGenerator {
    pub fn generate(ctx: &GenerationContext<'_>, config: &Config) -> Vec<Agent> {
        if !config.generation.module_agents {
            return vec![];
        }

        ctx.modules
            .iter()
            .filter(|m| Self::should_generate(m, config))
            .map(|m| Self::create_agent(m, ctx, config))
            .collect()
    }

    fn should_generate(module: &DetectedModule, config: &Config) -> bool {
        module.value_score >= config.generation.module_agent_threshold as f64
            && module.coverage_ratio >= config.generation.module_agent_min_coverage as f64
            && !module.paths.is_empty()
    }

    fn create_agent(module: &DetectedModule, ctx: &GenerationContext<'_>, config: &Config) -> Agent {
        let as_module: Module = module.clone().into();
        let available = ctx.available_skill_names();
        let skills = ModuleSkillResolver::resolve(&as_module, available, &config.skill_mapping);

        Agent::new(
            format!("{}-specialist", module.module_id),
            format!("{} module specialist", module.module_id),
            Self::generate_prompt(module, ctx),
        )
        .color(AgentColor::Orange)
        .model(AgentModel::Sonnet)
        .tools(super::tool_sets::full_access())
        .skills(skills)
        .permission_mode(PermissionMode::AcceptEdits)
    }

    fn generate_prompt(module: &DetectedModule, ctx: &GenerationContext<'_>) -> String {
        let paths = module.paths.join(", ");

        let key_files_section = if !module.key_files.is_empty() {
            let files = module
                .key_files
                .iter()
                .map(|f| format!("- [Verified: @{}]", f))
                .collect::<Vec<_>>()
                .join("\n");
            format!("\n## Key Files\n\n{}\n", files)
        } else {
            String::new()
        };

        let evidence_section = {
            let mut refs = Vec::new();
            let format_evidence = |ev: &EvidenceLocation, label: &str| {
                if ev.is_file_level() {
                    format!("- [Verified: @{}] - {}", ev.file, label)
                } else {
                    format!("- [Verified: @{}:{}] - {}", ev.file, ev.start_line, label)
                }
            };
            for conv in &module.conventions {
                for ev in &conv.evidence {
                    refs.push(format_evidence(ev, &conv.name));
                }
            }
            for issue in &module.known_issues {
                for ev in &issue.evidence {
                    refs.push(format_evidence(ev, &issue.id));
                }
            }
            for ev in &module.evidence {
                refs.push(format_evidence(ev, "module evidence"));
            }
            if refs.is_empty() {
                String::new()
            } else {
                format!("\n## Evidence\n\n{}\n", refs.join("\n"))
            }
        };

        // Get module-related constraints from discovered insights
        let module_insights: Vec<_> = ctx
            .all_discovered_insights()
            .into_iter()
            .filter(|i| {
                i.evidence.iter().any(|e| {
                    module
                        .paths
                        .iter()
                        .any(|p| e.file.contains(p.trim_end_matches('/')))
                })
            })
            .collect();

        let insights_section = if module_insights.is_empty() {
            String::new()
        } else {
            let items: Vec<_> = module_insights
                .iter()
                .map(|i| {
                    let evidence: Vec<_> = i
                        .evidence
                        .iter()
                        .map(|e| artifact_ref(&e.file, e.start_line))
                        .collect();
                    format!(
                        "### [{}] {}\n{}\n**Prevention**: {}\nEvidence: {}",
                        i.category,
                        i.title,
                        i.description,
                        i.prevention_guidance,
                        evidence.join(", ")
                    )
                })
                .collect();
            format!("\n## Critical Insights\n\n{}\n", items.join("\n\n"))
        };

        // Get hidden dependencies for this module
        let hidden_deps = ctx.hidden_deps_for_module(&module.module_id);
        let deps_section = if hidden_deps.is_empty() {
            String::new()
        } else {
            let items: Vec<_> = hidden_deps
                .iter()
                .map(|hd| {
                    format!(
                        "- **{} → {}**: {} (Impact: {})",
                        hd.from_module, hd.to_module, hd.description, hd.impact
                    )
                })
                .collect();
            format!("\n## Hidden Dependencies\n\n{}\n", items.join("\n"))
        };

        format!(
            r#"# {id} Module Specialist

## Scope

- Paths: {paths}
- Responsibility: {responsibility}
{key_files_section}{evidence_section}{insights_section}{deps_section}
## Context

Rules for this module are auto-injected based on file paths.
- Module rules: rules/modules/{id}.md
- Group rules: rules/groups/{{group}}.md (if applicable)
- Domain rules: rules/domains/{{domain}}.md (if applicable)

## Role

You are the specialist for the {id} module.
- Deep knowledge of this module's patterns and constraints
- Advocate for this module in review decisions
- Ensure changes align with module conventions

## Workflow

1. Receive task affecting {id} module
2. Rules auto-injected for context
3. Apply relevant skill (implement/debug/refactor)
4. Validate against module conventions

## Guidelines

- Work within module boundaries: {paths}
- Follow module-specific conventions (from rules)
- Flag cross-module impacts for architect review
- Cite specific @file:line references when providing guidance"#,
            id = module.module_id,
            paths = paths,
            responsibility = module.responsibility,
            key_files_section = key_files_section,
            evidence_section = evidence_section,
            insights_section = insights_section,
            deps_section = deps_section,
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
    use crate::types::module_map::{
        Convention, EvidenceLocation, IssueCategory, IssueSeverity, KnownIssue, TechStack,
    };

    fn test_config() -> Config {
        let mut config = Config::default();
        config.generation.module_agents = true;
        config.generation.module_agent_threshold = 0.7;
        config.generation.module_agent_min_coverage = 0.02;
        config
    }

    fn test_context<'a>(
        detection: &'a ProjectDetection,
        tech_stack: &'a TechStack,
        modules: &'a [DetectedModule],
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        registry: &'a VerifiedFileRegistry,
    ) -> GenerationContext<'a> {
        GenerationContext::new(
            detection,
            tech_stack,
            "test-project",
            modules,
            &[],
            &[],
            conventions,
            constraints,
            registry,
        )
    }

    fn test_module(id: &str, value_score: f64, coverage: f64) -> DetectedModule {
        DetectedModule {
            module_id: id.into(),
            paths: vec![format!("src/{}", id)],
            key_files: vec![],
            dependencies: vec![],
            dependents: vec![],
            responsibility: format!("{} module", id),
            conventions: vec![],
            known_issues: vec![],
            value_score,
            risk_score: 0.0,
            coverage_ratio: coverage,
            evidence: vec![],
            primary_language: None,
        }
    }

    fn test_module_with_evidence() -> DetectedModule {
        let mut module = test_module("auth", 0.9, 0.1);
        module.key_files = vec!["src/auth/mod.rs".into(), "src/auth/token.rs".into()];
        module.conventions = vec![
            Convention::new("token-validation", "Validate tokens before use")
                .with_evidence(vec![EvidenceLocation::new("src/auth/token.rs", 42)]),
        ];
        module.known_issues = vec![
            KnownIssue::new(
                "race-condition",
                "Token refresh race condition",
                IssueSeverity::High,
                IssueCategory::Concurrency,
            )
            .with_evidence(vec![EvidenceLocation::new("src/auth/refresh.rs", 15)]),
        ];
        module.evidence = vec![EvidenceLocation::file_level("src/auth/mod.rs".to_string())];
        module
    }

    #[test]
    fn test_generates_for_high_value_modules() {
        let config = test_config();
        let modules = vec![
            test_module("auth", 0.8, 0.05),
            test_module("utils", 0.5, 0.03),
        ];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &modules,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = ModuleAgentGenerator::generate(&ctx, &config);

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "auth-specialist");
    }

    #[test]
    fn test_respects_coverage_threshold() {
        let config = test_config();
        let modules = vec![test_module("auth", 0.9, 0.01)];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &modules,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = ModuleAgentGenerator::generate(&ctx, &config);

        assert!(agents.is_empty());
    }

    #[test]
    fn test_disabled_when_config_false() {
        let mut config = test_config();
        config.generation.module_agents = false;

        let modules = vec![test_module("auth", 0.9, 0.1)];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &modules,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = ModuleAgentGenerator::generate(&ctx, &config);

        assert!(agents.is_empty());
    }

    #[test]
    fn test_agent_has_correct_tools() {
        let config = test_config();
        let modules = vec![test_module("auth", 0.9, 0.1)];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &modules,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = ModuleAgentGenerator::generate(&ctx, &config);
        let agent = &agents[0];

        let tools = agent.tools.as_ref().unwrap();
        assert!(tools.contains(&"Edit".to_string()));
        assert!(tools.contains(&"Write".to_string()));
    }

    #[test]
    fn test_agent_has_correct_skills() {
        let config = test_config();
        let modules = vec![test_module("auth", 0.9, 0.1)];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &modules,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = ModuleAgentGenerator::generate(&ctx, &config);
        let agent = &agents[0];

        let skills = agent.skills.as_ref().unwrap();
        assert!(skills.contains(&"implement".to_string()));
        assert!(skills.contains(&"security-audit".to_string()));
    }

    #[test]
    fn test_prompt_includes_key_files_and_evidence() {
        let config = test_config();
        let modules = vec![test_module_with_evidence()];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &modules,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = ModuleAgentGenerator::generate(&ctx, &config);
        let agent = &agents[0];

        assert!(agent.prompt.contains("## Key Files"));
        assert!(agent.prompt.contains("[Verified: @src/auth/mod.rs]"));
        assert!(agent.prompt.contains("[Verified: @src/auth/token.rs]"));

        assert!(agent.prompt.contains("## Evidence"));
        assert!(agent.prompt.contains("[Verified: @src/auth/token.rs:42]"));
        assert!(agent.prompt.contains("token-validation"));

        assert!(agent.prompt.contains("[Verified: @src/auth/refresh.rs:15]"));
        assert!(agent.prompt.contains("race-condition"));

        assert!(agent.prompt.contains("[Verified: @src/auth/mod.rs] - module evidence"));
    }

    #[test]
    fn test_prompt_omits_empty_sections() {
        let config = test_config();
        let modules = vec![test_module("utils", 0.9, 0.1)];
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(
            &detection,
            &tech_stack,
            &modules,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = ModuleAgentGenerator::generate(&ctx, &config);
        let agent = &agents[0];

        assert!(!agent.prompt.contains("## Key Files"));
        assert!(!agent.prompt.contains("## Evidence"));
    }
}
