//! LLM Skill Discovery
//!
//! LLM-driven skill discovery that leverages full project analysis.
//! LLM determines valuable skills based on project structure, patterns, and domain.
//!
//! Key principle: Always provide rich context even when analysis is sparse.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::pipeline::context::FileRegistryExt;
use crate::pipeline::generation::context::GenerationContext;
use crate::pipeline::generation::context_enricher::{enrich_context, EnrichedContext};
use crate::pipeline::generation::evidence_gate::EvidenceMetrics;
use crate::pipeline::generation::discovery_fmt::{self, DiscoveryFormat};
use super::prompt::SkillPromptBuilder;
use crate::ai::LlmProvider;
use crate::pipeline::analysis::AstFacts;
use crate::types::{Result, Skill};
use crate::types::artifact_category::{AGENT_REVIEWER, AGENT_CODER, AGENT_ARCHITECT};
use crate::types::skill::ContextMode;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkillSuggestion {
    pub name: String,
    pub description: String,
    pub why_valuable: String,
    pub focus_areas: Vec<String>,
    pub tools: Vec<String>,
    pub has_argument: bool,
    pub argument_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkillDiscoveryResponse {
    pub skills: Vec<SkillSuggestion>,
}

pub struct SkillDiscovery;

impl SkillDiscovery {
    pub async fn discover(
        ctx: &GenerationContext<'_>,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<Vec<Skill>> {
        Self::discover_with_ast(ctx, provider, None).await
    }

    pub async fn discover_with_negative_feedback(
        ctx: &GenerationContext<'_>,
        provider: Arc<dyn LlmProvider>,
        rejected_skills: &[String],
    ) -> Result<Vec<Skill>> {
        let enriched = enrich_context(ctx.file_registry, None, ctx.deep_analysis);
        let system_prompt = ctx.build_system_prompt();
        let mut discovery_prompt = Self::build_discovery_prompt_with_context(ctx, &enriched);

        if !rejected_skills.is_empty() {
            discovery_prompt.push_str(&format!(
                "\n\n## PREVIOUS ATTEMPT FEEDBACK\n\
                 The following skills were already attempted and rejected or returned empty.\n\
                 Do NOT suggest these again. Instead, explore different aspects of the project:\n\
                 {}\n\
                 Focus on skills that address different concerns than the rejected ones.",
                rejected_skills.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n")
            ));
        }

        let schema = schemars::schema_for!(SkillDiscoveryResponse);
        let schema_value = serde_json::to_value(&schema)?;
        let response = provider
            .generate(&format!("{}\n\n{}", system_prompt, discovery_prompt), &schema_value)
            .await?;

        let suggestions: SkillDiscoveryResponse = serde_json::from_value(response.content)?;
        let mut skills = Vec::with_capacity(suggestions.skills.len());
        for suggestion in suggestions.skills {
            let skill =
                Self::generate_skill_from_suggestion(ctx, &provider, &suggestion, &enriched)
                    .await?;
            skills.push(skill);
        }
        Ok(skills)
    }

    pub async fn discover_with_ast(
        ctx: &GenerationContext<'_>,
        provider: Arc<dyn LlmProvider>,
        ast_facts: Option<&AstFacts>,
    ) -> Result<Vec<Skill>> {
        let enriched = enrich_context(ctx.file_registry, ast_facts, ctx.deep_analysis);
        let system_prompt = ctx.build_system_prompt();
        let discovery_prompt = Self::build_discovery_prompt_with_context(ctx, &enriched);
        let schema = schemars::schema_for!(SkillDiscoveryResponse);
        let schema_value = serde_json::to_value(&schema)?;

        let response = provider
            .generate(&format!("{}\n\n{}", system_prompt, discovery_prompt), &schema_value)
            .await?;

        let suggestions: SkillDiscoveryResponse = serde_json::from_value(response.content)?;

        let mut skills = Vec::with_capacity(suggestions.skills.len());
        for suggestion in suggestions.skills {
            let skill =
                Self::generate_skill_from_suggestion(ctx, &provider, &suggestion, &enriched)
                    .await?;
            skills.push(skill);
        }

        Ok(skills)
    }

    fn build_discovery_prompt_with_context(
        ctx: &GenerationContext<'_>,
        enriched: &EnrichedContext,
    ) -> String {
        let fmt = DiscoveryFormat::for_skills();
        let project_summary = discovery_fmt::format_project_summary(ctx, &fmt);
        let structural_section = enriched.format_structural_section();
        let ast_section = enriched.format_ast_section();
        let confidence_section = enriched.format_confidence_section();
        let modules_section = discovery_fmt::format_modules(ctx, &fmt);
        let insights_section = discovery_fmt::format_insights(
            ctx, enriched, "Skills MUST address these",
            discovery_fmt::format_structural_insights_fallback_with_ast,
        );
        let patterns_section = discovery_fmt::format_patterns(ctx, enriched, &fmt);
        let constraints_section = ctx.format_constraints();

        // Budget-aware: conditionally include Tier 2/3 sections
        let domain_section = if let Some(ref budget) = ctx.budget {
            if budget.tier3.domain_knowledge.is_empty() {
                String::new()
            } else {
                discovery_fmt::format_domain_knowledge(ctx)
            }
        } else {
            discovery_fmt::format_domain_knowledge(ctx)
        };

        let budget_guidance = if let Some(ref budget) = ctx.budget {
            let total = budget.total_tokens();
            format!(
                "\n## CONTEXT BUDGET\nTotal context: ~{} tokens. \
                 Focus skills on Tier 1 (essential) content. \
                 Reference Tier 2/3 content only when directly relevant.\n",
                total
            )
        } else {
            String::new()
        };

        let dynamic_commands = Self::build_dynamic_commands(ctx);

        let categories_section = if ctx.discovered_categories.is_empty() {
            String::new()
        } else {
            let entries: Vec<String> = ctx
                .discovered_categories
                .iter()
                .map(|c| {
                    format!(
                        "- **{}** (priority {}): {}\n  Triggers: {}",
                        c.name,
                        c.suggested_priority,
                        c.description,
                        c.trigger_patterns.join(", ")
                    )
                })
                .collect();
            format!(
                "## DISCOVERED DOMAIN CATEGORIES\n\
                 The following domain-specific categories were discovered from analysis.\n\
                 Create skills that address these categories:\n\n{}\n",
                entries.join("\n")
            )
        };

        format!(
            r#"Analyze this project and suggest 5-8 high-value, PROJECT-SPECIFIC skills for Claude Code.

CRITICAL: Prioritize skills unique to THIS project's domain and patterns.
- Prefer domain-specific skills (e.g., "api-endpoint" for API projects, "migration" for DB-heavy projects, "pipeline-step" for data pipelines)
- Only suggest generic skills (code-review, debug, refactor) if this project has non-obvious patterns that make them uniquely valuable
- Each skill MUST address something a developer could get wrong without project-specific knowledge

{project_summary}
{budget_guidance}
## ANALYSIS CONFIDENCE
{confidence_section}

## PROJECT STRUCTURE (verified - always available)
{structural_section}

## CODE FACTS (from AST - ground truth)
{ast_section}

{modules_section}

{insights_section}

{patterns_section}

{domain_section}

## PROJECT CONSTRAINTS
{constraints_section}

{categories_section}

{dynamic_commands}

---

## HOW TO CREATE PROJECT-SPECIFIC SKILLS

### Transform Analysis Into Actionable Skills

1. **From Detected Patterns → Skill Instructions**
   Pattern: "async-await in @src/api/"
   → Skill: "api-implementation" with instruction:
     "ALL API handlers MUST use async/await pattern. See @src/api/router.rs for reference."

2. **From Constraints → Preventive Guidance**
   Constraint: "Provider requires Arc wrapping"
   → Skill: "implement" with instruction:
     "ALWAYS wrap LlmProvider with Arc::new(). FAILURE: Thread-unsafe sharing causes panics at runtime."

3. **From Module Structure → Domain Skills**
   Module: "auth" with 15 files, session management
   → Skill: "auth-implementation" specific to this project's auth patterns

4. **From Hidden Dependencies → Gotcha Sections**
   Dependency: "config must load before service init"
   → Skill: "debug" with instruction:
     "CHECK initialization order first. Config loads at @src/config/mod.rs:42 BEFORE services."

### Domain-Specific Skill Discovery

Based on project type, prioritize these patterns:
- **API/Web Service**: api-endpoint, middleware, request-validation, error-response
- **Data Pipeline**: pipeline-step, data-validation, transformation, schema-migration
- **CLI Tool**: command-handler, argument-parsing, output-formatting
- **Library/SDK**: public-api-design, backward-compatibility, documentation
- **Frontend**: component-creation, state-management, routing, styling
- **Monorepo**: workspace-setup, cross-package, shared-config
- **ML/AI**: model-training, data-preprocessing, evaluation, deployment

IMPORTANT: These are examples, not constraints. Discover skills unique to THIS project.

### Skill Quality Criteria

Each skill MUST:
- Reference actual files from THIS project using @file:line
- Include specific gotchas discovered from analysis
- Provide PRESCRIPTIVE instructions ("DO X" not "Consider X")
- Explain FAILURE modes ("WITHOUT this, Y will happen")

### Example High-Value Skills

For a Rust async API project:
```
name: "api-endpoint"
description: "Add new API endpoint following project patterns"
focus_areas: ["Route definition", "Handler implementation", "Error handling"]
why_valuable: "Enforces async patterns and error handling from @src/api/error.rs"
```

For a React frontend:
```
name: "feature-component"
description: "Create feature component with state management"
focus_areas: ["Component structure", "State hooks", "API integration"]
why_valuable: "Ensures TanStack Query usage and design system compliance"
```

---

Return skills as JSON with: name, description, why_valuable, focus_areas, tools, has_argument, argument_hint."#,
            project_summary = project_summary,
            budget_guidance = budget_guidance,
            confidence_section = confidence_section,
            structural_section = structural_section,
            ast_section = ast_section,
            modules_section = modules_section,
            insights_section = insights_section,
            patterns_section = patterns_section,
            domain_section = domain_section,
            constraints_section = constraints_section,
            categories_section = categories_section,
            dynamic_commands = dynamic_commands,
        )
    }


    async fn generate_skill_from_suggestion(
        ctx: &GenerationContext<'_>,
        provider: &Arc<dyn LlmProvider>,
        suggestion: &SkillSuggestion,
        enriched: &EnrichedContext,
    ) -> Result<Skill> {
        let discovered_insights = ctx.all_discovered_insights();
        let patterns = ctx.all_patterns();
        let enriched_domain = ctx.enriched_domain_knowledge();
        let verified_refs = ctx.verified_references_for_skill(&suggestion.name);
        let constraints_text = ctx.format_constraints();
        let files = ctx.all_files_with_context();

        let focus = suggestion.focus_areas.join(", ");

        let metrics = EvidenceMetrics::from_context(ctx);
        let evidence_instructions = format!(
            "Evidence available: {} verified references, {} patterns, {} constraints (confidence: {:.0}%)",
            metrics.verified_refs, metrics.patterns, metrics.constraints, metrics.confidence * 100.0
        );

        let prompt = SkillPromptBuilder::new(&suggestion.name, &suggestion.description)
            .project_type(ctx.detection.primary_type)
            .skill_focus(focus)
            .verified_refs(verified_refs)
            .discovered_insights(discovered_insights)
            .constraints_text(constraints_text)
            .patterns(patterns)
            .files(files)
            .enriched_domain(enriched_domain)
            .tools(suggestion.tools.clone())
            .argument_hint(suggestion.argument_hint.as_deref())
            .enriched_context(enriched.clone())
            .evidence_instructions(evidence_instructions)
            .build();

        let system_prompt = ctx.build_system_prompt();
        let response = provider
            .generate(&format!("{}\n\n{}", system_prompt, prompt), &serde_json::json!({}))
            .await?;

        let content_str = response
            .content
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| response.content.to_string());

        let body = super::extract_skill_body(&content_str, &suggestion.name);
        let body = Self::inject_dynamic_commands(body, ctx);

        let mut skill = Skill::new(&suggestion.name, &suggestion.description, &body)
            .tools(suggestion.tools.clone())
            .user_invocable(true);

        if let Some(hint) = &suggestion.argument_hint {
            skill = skill.argument_hint(hint);
        }

        // Set context and agent for skills designed for sub-agent execution
        let agent = Self::infer_agent_for_skill(&suggestion.name, &suggestion.tools);
        if let Some(agent_name) = agent {
            skill = skill.context(ContextMode::Fork).agent(agent_name);
        }

        Ok(skill)
    }

    fn infer_agent_for_skill(skill_name: &str, tools: &[String]) -> Option<String> {
        let name_lower = skill_name.to_lowercase();

        // Read-only analysis skills → reviewer agent
        let review_patterns = ["review", "audit", "lint", "check", "validate", "inspect"];
        if review_patterns.iter().any(|p| name_lower.contains(p)) {
            return Some(AGENT_REVIEWER.into());
        }

        // Planning/architecture skills → architect agent
        let plan_patterns = ["plan", "architect", "design", "rfc"];
        if plan_patterns.iter().any(|p| name_lower.contains(p)) {
            return Some(AGENT_ARCHITECT.into());
        }

        // Write/edit tools present → coder agent
        let has_write_tools = tools
            .iter()
            .any(|t| matches!(t.as_str(), "Edit" | "Write" | "Bash"));
        if has_write_tools {
            return Some(AGENT_CODER.into());
        }

        // Implementation-related skills → coder agent
        let impl_patterns = [
            "implement",
            "debug",
            "refactor",
            "fix",
            "create",
            "add",
            "build",
            "migrate",
        ];
        if impl_patterns.iter().any(|p| name_lower.contains(p)) {
            return Some(AGENT_CODER.into());
        }

        None
    }

    fn build_dynamic_commands(ctx: &GenerationContext<'_>) -> String {
        let mut commands: Vec<String> = Vec::new();
        let primary_lang = ctx.tech_stack.primary_language.as_str();

        match primary_lang {
            "rust" => {
                if ctx.file_registry.file_exists("Cargo.toml") {
                    commands.push("!cat Cargo.toml".into());
                    commands.push("!cargo test --list 2>&1 | head -20".into());
                }
            }
            "typescript" | "javascript" => {
                if ctx.file_registry.file_exists("package.json") {
                    commands.push("!cat package.json".into());
                }
                if ctx.file_registry.file_exists("tsconfig.json") {
                    commands.push("!cat tsconfig.json".into());
                }
            }
            "python" => {
                if ctx.file_registry.file_exists("pyproject.toml") {
                    commands.push("!cat pyproject.toml".into());
                } else if ctx.file_registry.file_exists("setup.py") {
                    commands.push("!cat setup.py".into());
                }
                if ctx.file_registry.file_exists("requirements.txt") {
                    commands.push("!cat requirements.txt".into());
                }
            }
            "go" => {
                if ctx.file_registry.file_exists("go.mod") {
                    commands.push("!cat go.mod".into());
                }
            }
            "java" | "kotlin" => {
                if ctx.file_registry.file_exists("pom.xml") {
                    commands.push("!head -50 pom.xml".into());
                } else if ctx.file_registry.file_exists("build.gradle") {
                    commands.push("!cat build.gradle".into());
                } else if ctx.file_registry.file_exists("build.gradle.kts") {
                    commands.push("!cat build.gradle.kts".into());
                }
            }
            _ => {}
        }

        let ci_files = [
            ".github/workflows/ci.yml",
            ".github/workflows/main.yml",
            ".gitlab-ci.yml",
            "Jenkinsfile",
        ];
        for ci_file in &ci_files {
            if ctx.file_registry.file_exists(ci_file) {
                commands.push(format!("!cat {}", ci_file));
                break;
            }
        }

        if ctx.file_registry.file_exists("Dockerfile") {
            commands.push("!cat Dockerfile".into());
        } else if ctx.file_registry.file_exists("docker-compose.yml") {
            commands.push("!head -30 docker-compose.yml".into());
        }

        if commands.is_empty() {
            return String::new();
        }

        format!(
            "## DYNAMIC CONTEXT COMMANDS\nSkills can use these !command blocks for live context:\n{}",
            commands
                .iter()
                .map(|c| format!("- `{}`", c))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    fn inject_dynamic_commands(body: String, ctx: &GenerationContext<'_>) -> String {
        let primary_lang = ctx.tech_stack.primary_language.as_str();
        let mut dynamic_block = String::new();

        match primary_lang {
            "rust" if ctx.file_registry.file_exists("Cargo.toml") => {
                dynamic_block.push_str("\n## Project Context\n\n");
                dynamic_block.push_str("!cat Cargo.toml\n");
            }
            "typescript" | "javascript" if ctx.file_registry.file_exists("package.json") => {
                dynamic_block.push_str("\n## Project Context\n\n");
                dynamic_block.push_str("!cat package.json\n");
            }
            "python" if ctx.file_registry.file_exists("pyproject.toml") => {
                dynamic_block.push_str("\n## Project Context\n\n");
                dynamic_block.push_str("!cat pyproject.toml\n");
            }
            "go" if ctx.file_registry.file_exists("go.mod") => {
                dynamic_block.push_str("\n## Project Context\n\n");
                dynamic_block.push_str("!cat go.mod\n");
            }
            _ => {}
        }

        if dynamic_block.is_empty() {
            body
        } else {
            format!("{}{}", body, dynamic_block)
        }
    }
}
