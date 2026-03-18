//! Agent Generation Module
//!
//! Five-layer agent generation:
//! - Layer 1: Base agents (reviewer, coder, architect) - always generated
//! - Layer 2: Module specialists - for high-value modules
//! - Layer 3: Domain experts - for detected business domains
//! - Layer 4: LLM-discovered agents - domain-specific specialists (optional)
//! - Layer 5: Service specialists - for detected services (conditional)
//!
//! All agents now receive analysis data through GenerationContext.

mod base;
mod discovery;
mod domain;
mod module;
mod service;

pub(crate) mod tool_sets {
    pub fn read_only() -> Vec<String> {
        vec!["Read".into(), "Grep".into(), "Glob".into()]
    }

    pub fn full_access() -> Vec<String> {
        vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
            "Bash".into(),
        ]
    }

    pub fn library() -> Vec<String> {
        vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
        ]
    }

    /// Tools that read-only agents must not use.
    ///
    /// Defense-in-depth: paired with `read_only()` to explicitly block
    /// write-capable tools even if the platform defaults change.
    pub fn write_tools() -> Vec<String> {
        vec!["Write".into(), "Edit".into(), "Bash".into()]
    }
}

pub use base::{BaseAgentSpec, BaseAgentsGenerator};
pub use discovery::{AgentDiscovery, AgentEvidence, DiscoveredAgent};
pub use domain::DomainAgentGenerator;
pub use module::ModuleAgentGenerator;
pub use service::ServiceAgentGenerator;

use std::sync::Arc;

use super::context::GenerationContext;
use crate::ai::LlmProvider;
use crate::config::Config;
use crate::types::agent::Agent;
use crate::types::Result;

pub struct AgentsGenerator;

impl AgentsGenerator {
    /// Generate all agents using GenerationContext for access to analysis data.
    pub fn generate(ctx: &GenerationContext<'_>, config: &Config) -> Vec<Agent> {
        let mut agents = Vec::new();

        // Layer 1: Base agents with context (evidence-based prompts)
        agents.extend(BaseAgentsGenerator::generate(ctx));

        // Layer 2: Module specialists (conditional, with semantic routing)
        agents.extend(ModuleAgentGenerator::generate(ctx, config));

        // Layer 3: Domain experts (conditional, with semantic routing)
        agents.extend(DomainAgentGenerator::generate(ctx, config));

        // Layer 5: Service specialists (conditional, for detected services)
        agents.extend(ServiceAgentGenerator::generate(ctx, config));

        agents
    }

    /// Generate agents with LLM discovery when enabled.
    ///
    /// LLM-discovered agents extend base agents, they don't replace them.
    /// Discovery proposes domain-specific specialists based on project analysis.
    pub async fn generate_with_llm(
        ctx: &GenerationContext<'_>,
        config: &Config,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<Vec<Agent>> {
        // Start with base agents (always generated)
        let mut agents = Self::generate(ctx, config);

        // Layer 4: LLM-discovered agents (conditional)
        if config.discovery.agent_discovery {
            match AgentDiscovery::discover(ctx, provider).await {
                Ok(discovered) if !discovered.is_empty() => {
                    tracing::info!(
                        count = discovered.len(),
                        "LLM discovered project-specific agents"
                    );
                    // Merge discovered agents, avoiding duplicates by name
                    let existing_names: std::collections::HashSet<_> =
                        agents.iter().map(|a| a.name.clone()).collect();
                    for agent in discovered {
                        if !existing_names.contains(&agent.name) {
                            agents.push(agent);
                        } else {
                            tracing::debug!(
                                name = %agent.name,
                                "Skipping discovered agent - already exists"
                            );
                        }
                    }
                }
                Ok(_) => {
                    tracing::debug!("LLM returned no discovered agents");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Agent discovery failed, using base agents only");
                }
            }
        }

        Ok(agents)
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
    use crate::types::module_map::{DetectedModule, Domain, TechStack};

    fn test_config() -> Config {
        let mut config = Config::default();
        config.generation.module_agents = true;
        config.generation.module_agent_threshold = 0.7;
        config.generation.module_agent_min_coverage = 0.02;
        config.generation.domain_experts = true;
        config
    }

    fn test_module(id: &str, value_score: f64) -> DetectedModule {
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
            coverage_ratio: 0.1,
            evidence: vec![],
            primary_language: None,
        }
    }

    fn test_domain(id: &str) -> Domain {
        Domain {
            id: id.into(),
            name: format!("{} Domain", id),
            group_ids: vec![],
            responsibility: format!("{} logic", id),
            boundary_rules: vec![],
            interfaces: vec![],
            owner: String::new(),
        }
    }

    #[test]
    fn test_generates_all_layers() {
        let config = test_config();
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();

        let modules = vec![test_module("auth", 0.9)];
        let domains = vec![test_domain("identity")];

        let ctx = GenerationContext::new(
            &detection,
            &tech_stack,
            "test-project",
            &modules,
            &[],
            &domains,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = AgentsGenerator::generate(&ctx, &config);

        // 3 base + 1 module specialist + 1 domain expert = 5
        assert_eq!(agents.len(), 5);

        let names: Vec<_> = agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"reviewer"));
        assert!(names.contains(&"coder"));
        assert!(names.contains(&"architect"));
        assert!(names.contains(&"auth-specialist"));
        assert!(names.contains(&"identity-expert"));
    }

    #[test]
    fn test_respects_disabled_config() {
        let mut config = test_config();
        config.generation.module_agents = false;
        config.generation.domain_experts = false;

        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();

        let modules = vec![test_module("auth", 0.9)];
        let domains = vec![test_domain("identity")];

        let ctx = GenerationContext::new(
            &detection,
            &tech_stack,
            "test-project",
            &modules,
            &[],
            &domains,
            &conventions,
            &constraints,
            &registry,
        );

        let agents = AgentsGenerator::generate(&ctx, &config);

        // Only 3 base agents
        assert_eq!(agents.len(), 3);
    }

    #[test]
    fn test_discovery_config_default() {
        let config = Config::default();
        assert!(config.discovery.agent_discovery);
        assert!(config.discovery.skill_discovery);
    }

    #[test]
    fn test_discovery_exports() {
        // Ensure discovery types are properly exported
        let evidence = AgentEvidence {
            evidence_type: "pattern".into(),
            description: "test".into(),
            references: vec![],
        };
        assert_eq!(evidence.evidence_type, "pattern");

        let discovered = DiscoveredAgent {
            name: "test-agent".into(),
            role: "test role".into(),
            scope: vec![],
            skills: vec![],
            tools: vec!["Read".into()],
            evidence: vec![evidence],
            color: None,
            can_veto: false,
            priority: 50,
            model: None,
            permission_mode: None,
        };
        assert_eq!(discovered.name, "test-agent");
    }
}
