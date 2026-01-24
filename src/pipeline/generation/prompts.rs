//! Flexible Prompt Builders
//!
//! Builds prompts for LLM-driven artifact generation.
//! Key principles:
//! - No fixed templates - structure emerges from content
//! - Full context preservation
//! - Project-specific focus

use super::types::{GenerationContext, SynthesisSlice};
use crate::pipeline::insight::ExtractedInsight;
use crate::types::insight::{DomainContext, ModuleContext};

/// Builds prompts for skill generation
pub struct SkillPromptBuilder;

impl SkillPromptBuilder {
    pub fn build(ctx: &GenerationContext) -> String {
        let mut prompt = String::with_capacity(4096);

        prompt.push_str(
            "Generate a high-quality Claude Code skill based on the following context.\n\n",
        );

        // Source insights section
        prompt.push_str("## SOURCE INSIGHTS\n\n");
        prompt.push_str(&format_insights(&ctx.source_insights));
        prompt.push('\n');

        // Project context
        prompt.push_str("## PROJECT CONTEXT\n\n");
        prompt.push_str(&format!(
            "- Project Type: {}\n",
            ctx.detection.primary_type.as_str()
        ));
        prompt.push_str(&format!(
            "- Languages: {}\n",
            ctx.detection
                .languages
                .iter()
                .map(|l| l.language.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        prompt.push_str(&format!(
            "- Architecture: {}\n\n",
            ctx.conventions.architecture.pattern_name
        ));

        // Module context if available
        if let Some(module) = &ctx.module_context {
            prompt.push_str("## MODULE CONTEXT\n\n");
            prompt.push_str(&format_module_context(module));
            prompt.push('\n');
        }

        // Domain context if available
        if let Some(domain) = &ctx.domain_context {
            prompt.push_str("## DOMAIN CONTEXT\n\n");
            prompt.push_str(&format_domain_context(domain));
            prompt.push('\n');
        }

        // Synthesis slice
        if !ctx.synthesis.is_empty() {
            prompt.push_str("## SYNTHESIZED ANALYSIS\n\n");
            prompt.push_str(&format_synthesis(&ctx.synthesis));
            prompt.push('\n');
        }

        // Available file references
        prompt.push_str("## AVAILABLE FILE REFERENCES\n\n");
        let file_refs = ctx.file_registry.to_prompt_context(30);
        prompt.push_str(&file_refs);
        prompt.push_str("\n\n");

        // Requirements
        prompt.push_str(SKILL_REQUIREMENTS);

        // Output format
        prompt.push_str(SKILL_OUTPUT_FORMAT);

        prompt
    }
}

/// Builds prompts for agent generation
pub struct AgentPromptBuilder;

impl AgentPromptBuilder {
    pub fn build(ctx: &GenerationContext) -> String {
        let mut prompt = String::with_capacity(4096);

        prompt.push_str(
            "Generate a specialized Claude Code agent based on the following context.\n\n",
        );

        // Source insights
        prompt.push_str("## SOURCE INSIGHTS (Domain Knowledge)\n\n");
        prompt.push_str(&format_insights(&ctx.source_insights));
        prompt.push('\n');

        // Project context
        prompt.push_str("## PROJECT CONTEXT\n\n");
        prompt.push_str(&format!(
            "- Project Type: {}\n",
            ctx.detection.primary_type.as_str()
        ));
        prompt.push_str(&format!(
            "- Architecture: {}\n\n",
            ctx.conventions.architecture.pattern_name
        ));

        // Domain context is critical for agents
        if let Some(domain) = &ctx.domain_context {
            prompt.push_str("## DOMAIN EXPERTISE REQUIRED\n\n");
            prompt.push_str(&format_domain_context(domain));
            prompt.push('\n');
        }

        // Module context
        if let Some(module) = &ctx.module_context {
            prompt.push_str("## MODULE SCOPE\n\n");
            prompt.push_str(&format_module_context(module));
            prompt.push('\n');
        }

        // Related artifacts for cross-reference
        if !ctx.related_artifacts.is_empty() {
            prompt.push_str("## RELATED ARTIFACTS\n\n");
            for artifact in &ctx.related_artifacts {
                prompt.push_str(&format!(
                    "- {} ({}): {}\n",
                    artifact.name,
                    artifact.artifact_type.as_str(),
                    artifact.summary
                ));
            }
            prompt.push('\n');
        }

        // Requirements
        prompt.push_str(AGENT_REQUIREMENTS);

        // Output format
        prompt.push_str(AGENT_OUTPUT_FORMAT);

        prompt
    }
}

/// Builds prompts for rule generation
pub struct RulePromptBuilder;

impl RulePromptBuilder {
    pub fn build(ctx: &GenerationContext, target_paths: Option<&[String]>) -> String {
        let mut prompt = String::with_capacity(4096);

        prompt.push_str("Generate a Claude Code rule based on the following constraints.\n\n");

        // Source insights (constraints)
        prompt.push_str("## CONSTRAINTS TO ENFORCE\n\n");
        prompt.push_str(&format_insights(&ctx.source_insights));
        prompt.push('\n');

        // Target paths
        if let Some(paths) = target_paths {
            prompt.push_str("## TARGET PATHS\n\n");
            for path in paths {
                prompt.push_str(&format!("- {}\n", path));
            }
            prompt.push('\n');
        }

        // Project context
        prompt.push_str("## PROJECT CONTEXT\n\n");
        prompt.push_str(&format!(
            "- Architecture: {}\n\n",
            ctx.conventions.architecture.pattern_name
        ));

        // Evidence files
        prompt.push_str("## EVIDENCE FILES\n\n");
        let evidence: Vec<_> = ctx
            .source_insights
            .iter()
            .flat_map(|i| i.insight.evidence.iter())
            .take(20)
            .collect();
        for e in evidence {
            prompt.push_str(&format!("- @{}\n", e));
        }
        prompt.push('\n');

        // Requirements
        prompt.push_str(RULE_REQUIREMENTS);

        // Output format
        prompt.push_str(RULE_OUTPUT_FORMAT);

        prompt
    }
}

/// Builds prompts for CLAUDE.md generation
pub struct ClaudeMdPromptBuilder;

impl ClaudeMdPromptBuilder {
    pub fn build(ctx: &GenerationContext) -> String {
        let mut prompt = String::with_capacity(8192);

        prompt.push_str(
            "Generate a CLAUDE.md project memory file based on the following analysis.\n\n",
        );

        // Project overview
        prompt.push_str("## PROJECT ANALYSIS\n\n");
        prompt.push_str(&format!(
            "- Name: {}\n",
            ctx.project_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
        ));
        prompt.push_str(&format!(
            "- Type: {}\n",
            ctx.detection.primary_type.as_str()
        ));
        prompt.push_str(&format!(
            "- Languages: {}\n",
            ctx.detection
                .languages
                .iter()
                .map(|l| format!("{} ({:.0}%)", l.language.as_str(), l.percentage * 100.0))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        prompt.push_str(&format!(
            "- Architecture: {}\n\n",
            ctx.conventions.architecture.pattern_name
        ));

        // All insights grouped by category
        prompt.push_str("## EXTRACTED INSIGHTS\n\n");
        prompt.push_str(&format_insights(&ctx.source_insights));
        prompt.push('\n');

        // Synthesis
        if !ctx.synthesis.is_empty() {
            prompt.push_str("## SYNTHESIZED KNOWLEDGE\n\n");
            prompt.push_str(&format_synthesis(&ctx.synthesis));
            prompt.push('\n');
        }

        // Critical files
        prompt.push_str("## KEY FILES\n\n");
        let key_files = ctx.file_registry.to_prompt_context(50);
        prompt.push_str(&key_files);
        prompt.push_str("\n\n");

        // Requirements
        prompt.push_str(CLAUDE_MD_REQUIREMENTS);

        // Output format
        prompt.push_str(CLAUDE_MD_OUTPUT_FORMAT);

        prompt
    }
}

// Helper functions

fn format_insights(insights: &[ExtractedInsight]) -> String {
    if insights.is_empty() {
        return "No insights available.\n".to_string();
    }

    insights
        .iter()
        .map(|i| {
            let mut s = format!(
                "### {} [{}]\n\n{}\n\n",
                i.insight.title, i.tier, i.insight.description
            );

            if !i.insight.evidence.is_empty() {
                s.push_str("Evidence:\n");
                for e in i.insight.evidence.iter().take(5) {
                    s.push_str(&format!("- @{}\n", e));
                }
            }

            s.push_str(&format!("Value: {:.0}%\n\n", i.value.overall * 100.0));
            s
        })
        .collect()
}

fn format_module_context(ctx: &ModuleContext) -> String {
    let mut s = format!("Module: {} ({})\n", ctx.name, ctx.path);
    s.push_str(&format!("Responsibility: {}\n", ctx.responsibility));

    if !ctx.constraints.is_empty() {
        s.push_str("Constraints:\n");
        for c in &ctx.constraints {
            s.push_str(&format!("- {}\n", c));
        }
    }

    if !ctx.dependencies.is_empty() {
        s.push_str("Dependencies:\n");
        for d in &ctx.dependencies {
            s.push_str(&format!("- {}\n", d));
        }
    }

    s
}

fn format_domain_context(ctx: &DomainContext) -> String {
    let mut s = format!("Domain: {}\n", ctx.domain);

    if !ctx.business_rules.is_empty() {
        s.push_str("Business Rules:\n");
        for r in &ctx.business_rules {
            s.push_str(&format!("- {}\n", r));
        }
    }

    if !ctx.terminology.is_empty() {
        s.push_str("Terminology:\n");
        for (term, def) in &ctx.terminology {
            s.push_str(&format!("- {}: {}\n", term, def));
        }
    }

    if !ctx.compliance.is_empty() {
        s.push_str("Compliance:\n");
        for c in &ctx.compliance {
            s.push_str(&format!("- {}\n", c));
        }
    }

    s
}

fn format_synthesis(synthesis: &SynthesisSlice) -> String {
    let mut s = String::new();

    if !synthesis.modules.is_empty() {
        s.push_str("Modules:\n");
        for m in &synthesis.modules {
            s.push_str(&format!("- {}: {}\n", m.name, m.responsibility));
        }
    }

    if !synthesis.architectural_decisions.is_empty() {
        s.push_str("Architectural Decisions:\n");
        for d in &synthesis.architectural_decisions {
            s.push_str(&format!("- {}: {}\n", d.title, d.description));
        }
    }

    if !synthesis.cross_cutting_concerns.is_empty() {
        s.push_str("Cross-Cutting Concerns:\n");
        for c in &synthesis.cross_cutting_concerns {
            s.push_str(&format!("- {}: {}\n", c.name, c.description));
        }
    }

    s
}

// Requirement strings (kept separate for maintainability)

const SKILL_REQUIREMENTS: &str = r#"## REQUIREMENTS

1. Use directive language: "You must...", "Always...", "Never...", "Avoid..."
2. Include @file:line references from AVAILABLE FILES only
3. Be project-specific, NOT generic language/framework advice
4. Let structure emerge naturally from the insights
5. DO NOT use fixed section templates
6. Preserve ALL domain-specific and business-specific information
7. Minimum 3 @file:line references required
8. Minimum 3 actionable directives required

"#;

const SKILL_OUTPUT_FORMAT: &str = "## OUTPUT FORMAT

Return JSON with:
```json
{
  \"name\": \"kebab-case-skill-name\",
  \"description\": \"Detailed description for triggering (multiple sentences OK)\",
  \"body\": \"Markdown content starting with Title heading and @file:line refs\"
}
```

Let the content structure emerge naturally. Do NOT force sections.
";

const AGENT_REQUIREMENTS: &str = r#"## REQUIREMENTS

1. Define clear domain expertise and specialized role
2. Include project-specific business knowledge
3. Reference relevant files and modules
4. Specify interaction patterns and tool usage if applicable
5. NO generic advice - only domain-specific guidance
6. Include examples of proper agent behavior

"#;

const AGENT_OUTPUT_FORMAT: &str = r#"## OUTPUT FORMAT

Return JSON with:
```json
{
  "name": "domain-expert-name",
  "description": "What this agent specializes in",
  "prompt": "Full agent instructions with domain knowledge..."
}
```
"#;

const RULE_REQUIREMENTS: &str = r#"## REQUIREMENTS

1. Each rule must be actionable and enforceable
2. Include @file:line references as evidence
3. Specify severity if constraint violation is critical
4. Focus on THIS project's constraints, not generic best practices
5. Include prevention guidance

"#;

const RULE_OUTPUT_FORMAT: &str = r#"## OUTPUT FORMAT

Return JSON with:
```json
{
  "name": "kebab-case-rule-name",
  "paths": ["src/path/**"],
  "content": ["Rule statement 1", "Rule statement 2"]
}
```
"#;

const CLAUDE_MD_REQUIREMENTS: &str = r#"## REQUIREMENTS

1. Prioritize Tier 3 (constraints, gotchas) over Tier 2 (conventions)
2. NO Tier 1 content (generic commands, language basics)
3. Include architecture overview with key file references
4. Include critical constraints and gotchas
5. Be concise - focus on what's unique to this project
6. Use @file:line references throughout

"#;

const CLAUDE_MD_OUTPUT_FORMAT: &str = r#"## OUTPUT FORMAT

Return JSON with:
```json
{
  "overview": "Project name - brief description",
  "architecture": "Architecture description with @file refs",
  "standards": ["Standard 1 with @file ref", "Standard 2"]
}
```
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::insight::{
        ArtifactClassification, Insight, TierClassification, ValueScore,
    };
    use std::path::PathBuf;

    fn create_test_context() -> GenerationContext {
        let insight = ExtractedInsight::new(
            Insight::new("Test Constraint", "Must validate all inputs")
                .with_evidence(vec!["src/api/validate.rs:42".to_string()]),
            TierClassification::Tier3Constraint,
            ArtifactClassification::Skill,
        )
        .with_value(ValueScore::new(0.9, 0.8, 0.85));

        crate::pipeline::generation::context::GenerationContextBuilder::new(PathBuf::from("/test"))
            .with_insights(vec![insight])
            .build()
    }

    #[test]
    fn test_skill_prompt_contains_insights() {
        let ctx = create_test_context();
        let prompt = SkillPromptBuilder::build(&ctx);

        assert!(prompt.contains("Test Constraint"));
        assert!(prompt.contains("@src/api/validate.rs:42"));
    }

    #[test]
    fn test_agent_prompt_structure() {
        let ctx = create_test_context();
        let prompt = AgentPromptBuilder::build(&ctx);

        assert!(prompt.contains("SOURCE INSIGHTS"));
        assert!(prompt.contains("PROJECT CONTEXT"));
    }

    #[test]
    fn test_rule_prompt_with_paths() {
        let ctx = create_test_context();
        let paths = vec!["src/api/**".to_string()];
        let prompt = RulePromptBuilder::build(&ctx, Some(&paths));

        assert!(prompt.contains("src/api/**"));
        assert!(prompt.contains("CONSTRAINTS TO ENFORCE"));
    }
}
