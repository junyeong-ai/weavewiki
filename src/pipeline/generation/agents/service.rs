//! Service-Specific Agent Generator
//!
//! Generates specialized agents for each detected service in the project.
//! Each agent is scoped to the service's modules and interfaces.

use crate::config::Config;
use crate::pipeline::generation::context::GenerationContext;
use crate::pipeline::phases::service_detection::{DetectedService, ServiceType};
use crate::types::agent::{Agent, AgentColor, AgentModel, PermissionMode};

struct ServiceAgentConfig {
    description: String,
    color: AgentColor,
    tools: Vec<String>,
    skills: Vec<String>,
}

pub struct ServiceAgentGenerator;

impl ServiceAgentGenerator {
    /// Generate agents for detected services when service detection is available.
    pub fn generate(ctx: &GenerationContext<'_>, _config: &Config) -> Vec<Agent> {
        ctx.services
            .iter()
            .map(|service| Self::create_agent(service, ctx))
            .collect()
    }

    fn create_agent(
        service: &DetectedService,
        ctx: &GenerationContext<'_>,
    ) -> Agent {
        let config = Self::service_type_config(service);
        let name = format!("{}-service", service.service_id);

        Agent::new(name, config.description, Self::generate_prompt(service, ctx))
            .color(config.color)
            .model(AgentModel::Sonnet)
            .tools(config.tools)
            .skills(config.skills)
            .permission_mode(PermissionMode::Default)
    }

    fn generate_prompt(service: &DetectedService, ctx: &GenerationContext<'_>) -> String {
        let mut lines = Vec::new();
        lines.push(format!("# {} Service Specialist", service.name));
        lines.push(String::new());
        lines.push("## Scope".into());
        lines.push(String::new());
        lines.push(format!("Service path: `{}`", service.path));
        lines.push(format!("Service type: {}", service.service_type));
        lines.push(String::new());

        if !service.modules.is_empty() {
            lines.push("## Modules".into());
            lines.push(String::new());
            for module_id in &service.modules {
                if let Some(module) = ctx.modules.iter().find(|m| m.module_id == *module_id) {
                    lines.push(format!(
                        "- **{}**: {}",
                        module.module_id, module.responsibility
                    ));
                } else {
                    lines.push(format!("- {}", module_id));
                }
            }
            lines.push(String::new());
        }

        if !service.interfaces.is_empty() {
            lines.push("## Interfaces".into());
            lines.push(String::new());
            for iface in &service.interfaces {
                lines.push(format!("- {} ({})", iface.interface_type, iface.protocol));
                for endpoint in &iface.endpoints {
                    lines.push(format!("  - `{}`", endpoint));
                }
            }
            lines.push(String::new());
        }

        if !service.dependencies.is_empty() {
            lines.push("## Dependencies".into());
            lines.push(String::new());
            for dep in &service.dependencies {
                lines.push(format!(
                    "- **{}** ({}): {}",
                    dep.target_service, dep.dependency_type, dep.description
                ));
            }
            lines.push(String::new());
        }

        Self::add_type_guidance(&mut lines, service);
        lines.join("\n")
    }

    fn service_type_config(service: &DetectedService) -> ServiceAgentConfig {
        match service.service_type {
            ServiceType::Api => ServiceAgentConfig {
                description: format!("{} API specialist", service.name),
                color: AgentColor::Green,
                tools: super::tool_sets::full_access(),
                skills: vec!["code-review".into()],
            },
            ServiceType::Worker => ServiceAgentConfig {
                description: format!("{} worker specialist", service.name),
                color: AgentColor::Purple,
                tools: super::tool_sets::full_access(),
                skills: vec![],
            },
            ServiceType::Gateway => ServiceAgentConfig {
                description: format!("{} gateway specialist", service.name),
                color: AgentColor::Orange,
                tools: super::tool_sets::full_access(),
                skills: vec!["code-review".into()],
            },
            ServiceType::Library => ServiceAgentConfig {
                description: format!("{} library maintainer", service.name),
                color: AgentColor::Blue,
                tools: super::tool_sets::library(),
                skills: vec![],
            },
            ServiceType::Cli => ServiceAgentConfig {
                description: format!("{} CLI specialist", service.name),
                color: AgentColor::Orange,
                tools: super::tool_sets::full_access(),
                skills: vec![],
            },
            ServiceType::Web => ServiceAgentConfig {
                description: format!("{} frontend specialist", service.name),
                color: AgentColor::Blue,
                tools: super::tool_sets::full_access(),
                skills: vec!["code-review".into()],
            },
        }
    }

    fn add_type_guidance(content: &mut Vec<String>, service: &DetectedService) {
        content.push("## Guidelines".into());
        content.push(String::new());

        match service.service_type {
            ServiceType::Api => {
                content.push("- Validate API contracts and endpoint schemas".into());
                content.push("- Check request/response validation at boundaries".into());
                content.push("- Review error handling and status codes".into());
                content
                    .push("- Ensure authentication/authorization middleware is applied".into());
            }
            ServiceType::Worker => {
                content.push("- Validate idempotency of job handlers".into());
                content.push("- Check retry logic and dead letter handling".into());
                content.push("- Review resource cleanup after job completion".into());
                content.push("- Ensure graceful shutdown behavior".into());
            }
            ServiceType::Gateway => {
                content.push("- Validate routing rules and load balancing".into());
                content.push("- Check rate limiting and circuit breaker configs".into());
                content.push("- Review protocol translation accuracy".into());
                content.push("- Ensure upstream health checks are configured".into());
            }
            ServiceType::Library => {
                content
                    .push("- Validate public API stability and backwards compatibility".into());
                content.push("- Check documentation completeness for exported items".into());
                content.push("- Review dependency hygiene (minimize transitive deps)".into());
            }
            ServiceType::Cli => {
                content.push("- Validate argument parsing and help text".into());
                content.push("- Check exit codes follow conventions".into());
                content.push("- Review error messages for user-friendliness".into());
                content.push("- Ensure signal handling (SIGINT, SIGTERM)".into());
            }
            ServiceType::Web => {
                content.push("- Validate component composition and state management".into());
                content.push("- Check accessibility (a11y) compliance".into());
                content.push("- Review client-side security (XSS, CSRF)".into());
                content.push("- Ensure responsive design patterns".into());
            }
        }
        content.push(String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::context::VerifiedFileRegistry;
    use crate::pipeline::phases::{
        constraint_extraction::ExtractedConstraints,
        convention_inference::InferredConventions,
        project_detection::ProjectDetection,
        service_detection::{
            DependencyType, DetectedService, InterfaceType, ServiceDependency, ServiceInterface,
        },
    };
    use crate::types::module_map::{DetectedModule, TechStack};

    fn create_test_service(id: &str, service_type: ServiceType) -> DetectedService {
        DetectedService {
            service_id: id.into(),
            name: id.replace('-', " ").to_string(),
            path: format!("services/{}", id),
            service_type,
            modules: vec![id.into()],
            interfaces: vec![],
            dependencies: vec![],
        }
    }

    #[test]
    fn test_generates_api_service_agent() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("typescript");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();

        let services = vec![create_test_service("user-api", ServiceType::Api)];
        let modules = vec![DetectedModule::new("user-api", "User management API")
            .paths(vec!["services/user-api/".into()])];

        let ctx = GenerationContext::new(
            &detection,
            &tech_stack,
            "test-project",
            &modules,
            &[],
            &[],
            &conventions,
            &constraints,
            &registry,
        )
        .services(&services);

        let agents = ServiceAgentGenerator::generate(&ctx, &Config::default());
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "user-api-service");
        assert!(agents[0].prompt.contains("API"));
        assert_eq!(agents[0].color, Some(AgentColor::Green));
    }

    #[test]
    fn test_generates_multiple_service_agents() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("go");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();

        let services = vec![
            create_test_service("api-gateway", ServiceType::Gateway),
            create_test_service("payment-worker", ServiceType::Worker),
            create_test_service("web-app", ServiceType::Web),
        ];

        let ctx = GenerationContext::new(
            &detection,
            &tech_stack,
            "test-project",
            &[],
            &[],
            &[],
            &conventions,
            &constraints,
            &registry,
        )
        .services(&services);

        let agents = ServiceAgentGenerator::generate(&ctx, &Config::default());
        assert_eq!(agents.len(), 3);

        let names: Vec<_> = agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"api-gateway-service"));
        assert!(names.contains(&"payment-worker-service"));
        assert!(names.contains(&"web-app-service"));
    }

    #[test]
    fn test_no_agents_without_services() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();

        let ctx = GenerationContext::new(
            &detection,
            &tech_stack,
            "test-project",
            &[],
            &[],
            &[],
            &conventions,
            &constraints,
            &registry,
        );

        let agents = ServiceAgentGenerator::generate(&ctx, &Config::default());
        assert!(agents.is_empty());
    }

    #[test]
    fn test_service_agent_includes_interfaces() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();

        let services = vec![DetectedService {
            service_id: "auth-api".into(),
            name: "Auth API".into(),
            path: "services/auth".into(),
            service_type: ServiceType::Api,
            modules: vec!["auth".into()],
            interfaces: vec![ServiceInterface {
                interface_type: InterfaceType::Http,
                endpoints: vec!["/auth/login".into(), "/auth/refresh".into()],
                protocol: "HTTP/1.1".into(),
            }],
            dependencies: vec![ServiceDependency {
                target_service: "user-db".into(),
                dependency_type: DependencyType::Database,
                description: "Reads user credentials".into(),
            }],
        }];

        let ctx = GenerationContext::new(
            &detection,
            &tech_stack,
            "test-project",
            &[],
            &[],
            &[],
            &conventions,
            &constraints,
            &registry,
        )
        .services(&services);

        let agents = ServiceAgentGenerator::generate(&ctx, &Config::default());
        assert_eq!(agents.len(), 1);
        assert!(agents[0].prompt.contains("/auth/login"));
        assert!(agents[0].prompt.contains("user-db"));
        assert!(agents[0].prompt.contains("Database"));
    }
}
