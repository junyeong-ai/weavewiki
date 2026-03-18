//! LLM Agent Discovery
//!
//! LLM-driven agent discovery that leverages full project analysis.
//! LLM determines valuable domain-specific agents based on project structure,
//! patterns, and domain knowledge.
//!
//! Key principle: Extend base agents with specialized LLM-discovered agents,
//! don't replace them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::ai::LlmProvider;
use crate::pipeline::generation::context::GenerationContext;
use crate::pipeline::generation::context_enricher::{enrich_context, EnrichedContext};
use crate::pipeline::analysis::AstFacts;
use crate::types::agent::{Agent, AgentColor, AgentModel, PermissionMode};
use crate::types::Result;
use crate::pipeline::generation::discovery_fmt::{self, DiscoveryFormat};

/// Evidence supporting why an agent is needed
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentEvidence {
    /// Type of evidence (pattern, constraint, cross-cutting, domain)
    pub evidence_type: String,
    /// Description of the evidence
    pub description: String,
    /// File references supporting this evidence
    pub references: Vec<String>,
}

/// An agent discovered by LLM analysis
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveredAgent {
    /// Agent name (kebab-case)
    pub name: String,
    /// Agent's specialized role
    pub role: String,
    /// Files and modules this agent focuses on
    pub scope: Vec<String>,
    /// Skills this agent should have access to
    pub skills: Vec<String>,
    /// Tools this agent needs (Read, Grep, Edit, etc.)
    pub tools: Vec<String>,
    /// Evidence supporting why this agent is valuable
    pub evidence: Vec<AgentEvidence>,
    /// Agent color for visual distinction
    pub color: Option<String>,
    /// Whether this agent should have veto power
    pub can_veto: bool,
    /// Priority (0-100)
    pub priority: u8,
    /// Agent model preference
    pub model: Option<String>,
    /// Permission mode
    pub permission_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentDiscoveryResponse {
    pub agents: Vec<DiscoveredAgent>,
}

pub struct AgentDiscovery;

impl AgentDiscovery {
    /// Discover project-specific agents using LLM analysis
    pub async fn discover(
        ctx: &GenerationContext<'_>,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<Vec<Agent>> {
        Self::discover_with_ast(ctx, provider, None).await
    }

    /// Discover agents with optional AST facts for richer context
    pub async fn discover_with_ast(
        ctx: &GenerationContext<'_>,
        provider: Arc<dyn LlmProvider>,
        ast_facts: Option<&AstFacts>,
    ) -> Result<Vec<Agent>> {
        let enriched = enrich_context(ctx.file_registry, ast_facts, ctx.deep_analysis);
        let system_prompt = ctx.build_system_prompt();
        let discovery_prompt = Self::build_discovery_prompt(ctx, &enriched);
        let schema = schemars::schema_for!(AgentDiscoveryResponse);
        let schema_value = serde_json::to_value(&schema)?;

        let response = provider
            .generate(&format!("{}\n\n{}", system_prompt, discovery_prompt), &schema_value)
            .await?;

        let suggestions: AgentDiscoveryResponse = serde_json::from_value(response.content)?;

        let agents: Vec<Agent> = suggestions
            .agents
            .into_iter()
            .map(Self::create_agent)
            .collect();

        Ok(agents)
    }

    fn build_discovery_prompt(ctx: &GenerationContext<'_>, enriched: &EnrichedContext) -> String {
        let fmt = DiscoveryFormat::for_agents();
        let project_summary = discovery_fmt::format_project_summary(ctx, &fmt);
        let structural_section = enriched.format_structural_section();
        let ast_section = enriched.format_ast_section();
        let confidence_section = enriched.format_confidence_section();
        let modules_section = discovery_fmt::format_modules(ctx, &fmt);
        let patterns_section = discovery_fmt::format_patterns(ctx, enriched, &fmt);
        let constraints_section = ctx.format_constraints();
        let insights_section = discovery_fmt::format_insights(
            ctx, enriched, "Agents should address these",
            discovery_fmt::format_structural_insights_fallback,
        );
        let domain_section = Self::format_domain_knowledge_for_agents(ctx);
        let cross_synthesis_section = Self::format_cross_synthesis(ctx);

        // Budget guidance if available
        let budget_guidance = if let Some(ref budget) = ctx.budget {
            let total = budget.total_tokens();
            format!(
                "\n## CONTEXT BUDGET\nTotal context: ~{} tokens. \
                 Focus agents on Tier 1 (essential) content.\n",
                total
            )
        } else {
            String::new()
        };

        format!(
            r#"Analyze this project and propose 3-6 specialized agents that would add VALUE beyond the base agents (reviewer, coder, architect).

## BASE AGENTS (Already Generated)
- **reviewer**: Code quality gatekeeper (read-only, veto power)
- **coder**: Feature implementation specialist (write access)
- **architect**: System design specialist (read-only, veto power)

CRITICAL: Propose COMPLEMENTARY agents that extend the base capabilities:
- Module specialists for complex, high-value modules
- Domain experts for business logic and compliance
- Integration coordinators for cross-cutting concerns
- Technology specialists for specific frameworks/technologies

{project_summary}
{budget_guidance}
## ANALYSIS CONFIDENCE
{confidence_section}

## PROJECT STRUCTURE (verified)
{structural_section}

## CODE FACTS (from AST)
{ast_section}

{modules_section}

{patterns_section}

{constraints_section}

{insights_section}

{domain_section}

{cross_synthesis_section}

---

## AGENT DISCOVERY GUIDELINES

### Types of Valuable Agents

1. **Module Specialists** (for complex modules)
   - Deep expertise in one module's patterns and constraints
   - Example: "auth-specialist" for authentication module
   - Scope: Limited to specific module paths
   - Purpose: Maintain module-specific conventions

2. **Domain Experts** (for business logic)
   - Knowledge of domain concepts and business rules
   - Example: "billing-expert" for financial logic
   - Scope: Cross-module domain boundaries
   - Purpose: Enforce domain invariants

3. **Integration Coordinators** (for cross-cutting concerns)
   - Oversight of interactions between modules
   - Example: "api-coordinator" for API consistency
   - Scope: Module interfaces and boundaries
   - Purpose: Ensure integration patterns

4. **Technology Specialists** (for framework-specific concerns)
   - Deep knowledge of specific technology patterns
   - Example: "async-specialist" for async/await patterns
   - Scope: Technology-related code paths
   - Purpose: Enforce technology best practices

### Evidence Requirements

Each proposed agent MUST have evidence supporting its value:
- Pattern references (detected patterns that justify the agent)
- Constraint references (constraints the agent should enforce)
- File references (specific files within agent's scope)
- Cross-cutting concerns (hidden dependencies, architecture violations)

### Agent Quality Criteria

Each agent MUST:
- Address a gap not covered by base agents
- Have clear, limited scope (not overlap with base agents)
- Reference actual project patterns/constraints
- Include specific @file:line references in evidence
- Define appropriate tools and permissions

### Example High-Value Agents

For a Rust async API project:
```json
{{
  "name": "api-specialist",
  "role": "API endpoint design and review",
  "scope": ["src/api/", "src/handlers/"],
  "skills": ["code-review", "implement"],
  "tools": ["Read", "Grep", "Glob", "Edit"],
  "evidence": [
    {{
      "evidence_type": "pattern",
      "description": "Async request handling pattern",
      "references": ["@src/api/mod.rs:42"]
    }}
  ],
  "can_veto": true,
  "priority": 60
}}
```

---

Return agents as JSON matching the schema. Focus on agents that prevent mistakes and enforce project-specific patterns."#,
            project_summary = project_summary,
            budget_guidance = budget_guidance,
            confidence_section = confidence_section,
            structural_section = structural_section,
            ast_section = ast_section,
            modules_section = modules_section,
            patterns_section = patterns_section,
            constraints_section = constraints_section,
            insights_section = insights_section,
            domain_section = domain_section,
            cross_synthesis_section = cross_synthesis_section,
        )
    }

    fn format_domain_knowledge_for_agents(ctx: &GenerationContext<'_>) -> String {
        let domain = match ctx.domain_knowledge() {
            Some(d) => d,
            None => {
                // Check for domains from detection
                if ctx.domains.is_empty() {
                    return String::new();
                }
                // Format detected domains
                let domain_list: Vec<_> = ctx
                    .domains
                    .iter()
                    .map(|d| {
                        format!(
                            "- **{}** ({}): {}",
                            d.name, d.id, d.responsibility
                        )
                    })
                    .collect();
                return format!("## DETECTED DOMAINS (Candidates for Experts)\n{}", domain_list.join("\n"));
            }
        };

        let mut parts = Vec::new();
        if !domain.policies.is_empty() {
            parts.push(format!("**Policies**: {}", domain.policies.join("; ")));
        }
        if !domain.core_logic.is_empty() {
            parts.push(format!("**Core Logic**: {}", domain.core_logic.join("; ")));
        }
        if !domain.terminology.is_empty() {
            parts.push(format!("**Domain Terms**: {}", domain.terminology.join(", ")));
        }

        if parts.is_empty() {
            String::new()
        } else {
            format!("## DOMAIN KNOWLEDGE\n{}", parts.join("\n"))
        }
    }

    fn format_cross_synthesis(ctx: &GenerationContext<'_>) -> String {
        let mut sections = Vec::new();

        // Hidden dependencies
        let hidden_deps = ctx.all_hidden_dependencies();
        if !hidden_deps.is_empty() {
            let deps: Vec<_> = hidden_deps
                .iter()
                .map(|hd| {
                    format!(
                        "- {} → {}: {} (impact: {})",
                        hd.from_module, hd.to_module, hd.description, hd.impact
                    )
                })
                .collect();
            sections.push(format!("### Hidden Dependencies\n{}", deps.join("\n")));
        }

        // Architecture violations
        let violations = ctx.all_architecture_violations();
        if !violations.is_empty() {
            let viols: Vec<_> = violations
                .iter()
                .map(|v| {
                    format!(
                        "- [{}] {} → {}: {}",
                        v.violation_type, v.from_layer, v.to_layer, v.description
                    )
                })
                .collect();
            sections.push(format!("### Architecture Violations\n{}", viols.join("\n")));
        }

        // Cross-module constraints
        let constraints = ctx.all_cross_constraints();
        if !constraints.is_empty() {
            let constrs: Vec<_> = constraints
                .iter()
                .map(|c| {
                    format!(
                        "- [{}] {}: {} (modules: {})",
                        c.constraint_type,
                        c.name,
                        c.description,
                        c.affected_modules.join(", ")
                    )
                })
                .collect();
            sections.push(format!("### Cross-Module Constraints\n{}", constrs.join("\n")));
        }

        if sections.is_empty() {
            String::new()
        } else {
            format!("## CROSS-SYNTHESIS INSIGHTS\n{}", sections.join("\n\n"))
        }
    }

    fn create_agent(discovered: DiscoveredAgent) -> Agent {
        // Build prompt from discovered agent
        let prompt = Self::build_agent_prompt(&discovered);

        let color = discovered
            .color
            .as_ref()
            .and_then(|c| c.parse::<AgentColor>().ok())
            .unwrap_or(AgentColor::Orange);

        let model = discovered
            .model
            .as_ref()
            .and_then(|m| m.parse::<AgentModel>().ok())
            .unwrap_or(AgentModel::Sonnet);

        let permission_mode = discovered
            .permission_mode
            .as_ref()
            .and_then(|p| p.parse::<PermissionMode>().ok())
            .unwrap_or_else(|| {
                // Default based on tools
                if discovered.tools.iter().any(|t| t == "Edit" || t == "Write") {
                    PermissionMode::AcceptEdits
                } else {
                    PermissionMode::Default
                }
            });

        let mut agent = Agent::new(&discovered.name, &discovered.role, &prompt)
            .color(color)
            .model(model)
            .tools(discovered.tools)
            .permission_mode(permission_mode);

        if !discovered.skills.is_empty() {
            agent = agent.skills(discovered.skills);
        }

        agent
    }

    fn build_agent_prompt(discovered: &DiscoveredAgent) -> String {
        let scope_list = if discovered.scope.is_empty() {
            "Project-wide".to_string()
        } else {
            discovered.scope.join(", ")
        };

        let evidence_section = if discovered.evidence.is_empty() {
            String::new()
        } else {
            let items: Vec<_> = discovered
                .evidence
                .iter()
                .map(|e| {
                    let refs = if e.references.is_empty() {
                        String::new()
                    } else {
                        format!("\n  References: {}", e.references.join(", "))
                    };
                    format!("- **[{}]** {}{}", e.evidence_type, e.description, refs)
                })
                .collect();
            format!("\n## Evidence\n\n{}\n", items.join("\n"))
        };

        let skills_section = if discovered.skills.is_empty() {
            String::new()
        } else {
            format!("\n## Available Skills\n\n- {}\n", discovered.skills.join("\n- "))
        };

        let veto_note = if discovered.can_veto {
            "\n- **Veto Power**: Can block changes violating domain/module constraints"
        } else {
            ""
        };

        format!(
            r#"# {name}

## Role

{role}

## Scope

- Coverage: {scope}
{evidence_section}{skills_section}
## Context

Rules for this agent's scope are auto-injected based on file paths.

## Guidelines

- Deep expertise in assigned scope
- Enforce patterns and constraints from evidence
- Cite specific @file:line references when providing guidance{veto_note}

## Workflow

1. Receive task affecting scope ({scope})
2. Rules auto-injected for context
3. Apply relevant skill
4. Validate against scope conventions"#,
            name = discovered.name.replace('-', " ").to_uppercase(),
            role = discovered.role,
            scope = scope_list,
            evidence_section = evidence_section,
            skills_section = skills_section,
            veto_note = veto_note,
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

    #[test]
    fn test_convert_discovered_agent() {
        let discovered = DiscoveredAgent {
            name: "auth-specialist".into(),
            role: "Authentication module specialist".into(),
            scope: vec!["src/auth/".into()],
            skills: vec!["code-review".into(), "implement".into()],
            tools: vec!["Read".into(), "Grep".into(), "Edit".into()],
            evidence: vec![AgentEvidence {
                evidence_type: "pattern".into(),
                description: "Token validation pattern".into(),
                references: vec!["@src/auth/token.rs:42".into()],
            }],
            color: Some("orange".into()),
            can_veto: true,
            priority: 60,
            model: Some("sonnet".into()),
            permission_mode: Some("acceptEdits".into()),
        };

        let agent = AgentDiscovery::create_agent(discovered);

        assert_eq!(agent.name, "auth-specialist");
        assert!(agent.prompt.contains("Authentication module specialist"));
        assert!(agent.prompt.contains("src/auth/"));
        assert!(agent.prompt.contains("Token validation pattern"));
        assert!(agent.prompt.contains("Veto Power"));
        assert_eq!(agent.color, Some(AgentColor::Orange));
        assert_eq!(agent.model, Some(AgentModel::Sonnet));
        assert_eq!(agent.color, Some(AgentColor::Orange));
    }

    #[test]
    fn test_build_agent_prompt_minimal() {
        let discovered = DiscoveredAgent {
            name: "api-coordinator".into(),
            role: "API integration coordinator".into(),
            scope: vec![],
            skills: vec![],
            tools: vec!["Read".into()],
            evidence: vec![],
            color: None,
            can_veto: false,
            priority: 50,
            model: None,
            permission_mode: None,
        };

        let prompt = AgentDiscovery::build_agent_prompt(&discovered);

        assert!(prompt.contains("API COORDINATOR"));
        assert!(prompt.contains("API integration coordinator"));
        assert!(prompt.contains("Project-wide"));
        assert!(!prompt.contains("Veto Power"));
    }

    #[test]
    fn test_format_project_summary() {
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

        let fmt = DiscoveryFormat::for_agents();
        let summary = discovery_fmt::format_project_summary(&ctx, &fmt);

        assert!(summary.contains("PROJECT SUMMARY"));
        assert!(summary.contains("test-project"));
        assert!(summary.contains("rust"));
        assert!(summary.contains("Domain Count"));
    }
}
