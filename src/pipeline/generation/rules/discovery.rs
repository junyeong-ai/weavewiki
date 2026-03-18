//! LLM Rule Discovery
//!
//! LLM-driven rule discovery that discovers domain-specific rules
//! the procedural generators cannot detect. Examples:
//! - HIPAA compliance rules
//! - Financial regulation constraints
//! - Custom architecture invariants
//! - Cross-cutting security policies
//!
//! Follows the same pattern as SkillDiscovery and AgentDiscovery.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::ai::LlmProvider;
use crate::pipeline::generation::context::GenerationContext;
use crate::pipeline::generation::context_enricher::{enrich_context, EnrichedContext};
use crate::pipeline::generation::discovery_fmt::{self, DiscoveryFormat};
use crate::types::{Result, Rule};

/// A rule discovered by LLM analysis
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveredRule {
    /// Rule name (kebab-case)
    pub name: String,
    /// What this rule enforces
    pub description: String,
    /// Why this rule is important (evidence-based justification)
    pub why_important: String,
    /// File glob patterns this rule applies to
    pub paths: Vec<String>,
    /// Rule content lines (markdown)
    pub content: Vec<String>,
    /// Priority (0-100, higher = more important)
    pub priority: u8,
    /// File references supporting this rule
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RuleDiscoveryResponse {
    pub rules: Vec<DiscoveredRule>,
}

pub struct RuleDiscovery;

impl RuleDiscovery {
    /// Discover domain-specific rules using LLM analysis.
    ///
    /// Returns discovered rules on success, empty Vec on failure (graceful fallback).
    pub async fn discover(
        ctx: &GenerationContext<'_>,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<Vec<Rule>> {
        let enriched = enrich_context(ctx.file_registry, None, ctx.deep_analysis);
        let system_prompt = ctx.build_system_prompt();
        let discovery_prompt = Self::build_discovery_prompt(ctx, &enriched);
        let schema = schemars::schema_for!(RuleDiscoveryResponse);
        let schema_value = serde_json::to_value(&schema)?;

        let response = provider
            .generate(
                &format!("{}\n\n{}", system_prompt, discovery_prompt),
                &schema_value,
            )
            .await?;

        let suggestions: RuleDiscoveryResponse = serde_json::from_value(response.content)?;

        let rules: Vec<Rule> = suggestions
            .rules
            .into_iter()
            .filter(|r| !r.content.is_empty() && !r.name.is_empty())
            .map(Self::create_rule)
            .collect();

        tracing::info!(count = rules.len(), "LLM discovered domain-specific rules");
        Ok(rules)
    }

    fn build_discovery_prompt(ctx: &GenerationContext<'_>, enriched: &EnrichedContext) -> String {
        let fmt = DiscoveryFormat::for_rules();
        let project_summary = discovery_fmt::format_project_summary(ctx, &fmt);
        let structural_section = enriched.format_structural_section();
        let ast_section = enriched.format_ast_section();
        let confidence_section = enriched.format_confidence_section();
        let modules_section = discovery_fmt::format_modules(ctx, &fmt);
        let patterns_section = discovery_fmt::format_patterns(ctx, enriched, &fmt);
        let constraints_section = ctx.format_constraints();
        let insights_section = discovery_fmt::format_insights(
            ctx,
            enriched,
            "Rules MUST address these",
            discovery_fmt::format_structural_insights_fallback,
        );

        let domain_section = discovery_fmt::format_domain_knowledge(ctx);

        format!(
            r##"Analyze this project and discover 3-8 domain-specific RULES that Claude Code should enforce.

CRITICAL: These rules should capture project-specific knowledge that procedural analysis CANNOT detect.
The following rule types are ALREADY generated procedurally - do NOT duplicate them:
- Project-level rules (always-inject, global conventions)
- Language/tech rules (Rust, Python, TypeScript patterns)
- Framework rules (React, Django, Spring patterns)
- Module rules (per-module conventions)
- Group rules (cross-module groupings)

Focus ONLY on:
- **Domain invariants**: Business logic constraints (e.g., "all financial calculations MUST use Decimal, never f64")
- **Compliance requirements**: HIPAA, PCI-DSS, GDPR, SOX patterns detected in code
- **Architecture policies**: Detected but undocumented architectural decisions
- **Cross-cutting concerns**: Security, performance, or reliability constraints spanning multiple modules
- **Hidden gotchas**: Implicit rules that new developers would violate

{project_summary}

## ANALYSIS CONFIDENCE
{confidence_section}

## PROJECT STRUCTURE (verified)
{structural_section}

## CODE FACTS (from AST)
{ast_section}

{modules_section}

{insights_section}

{patterns_section}

{domain_section}

## PROJECT CONSTRAINTS
{constraints_section}

---

## RULE QUALITY CRITERIA

Each rule MUST:
- Reference actual files from THIS project using @file:line format
- Explain WHY this rule exists (what goes wrong without it)
- Be PRESCRIPTIVE ("MUST", "NEVER", not "Consider" or "Should")
- Include `paths` globs matching the files this rule applies to
- Not duplicate any procedurally-generated rule (project/tech/framework/module/group)

### Example High-Value Discovered Rules

For a financial API:
```json
{{
  "name": "decimal-precision",
  "description": "All monetary calculations must use Decimal type",
  "why_important": "Float arithmetic causes rounding errors in financial calculations. @src/billing/calculator.rs:42 shows the Decimal pattern.",
  "paths": ["src/billing/**", "src/payments/**"],
  "content": ["# Decimal Precision", "", "MUST use `Decimal` type for all monetary values.", "NEVER use `f64` or `f32` for amounts.", "", "Reference: @src/billing/calculator.rs:42"],
  "priority": 75,
  "evidence_refs": ["@src/billing/calculator.rs:42", "@src/payments/processor.rs:18"]
}}
```

For a healthcare system:
```json
{{
  "name": "phi-data-handling",
  "description": "Protected Health Information handling rules",
  "why_important": "HIPAA compliance requires specific data handling patterns. @src/patient/record.rs:15 implements the encryption pattern.",
  "paths": ["src/patient/**", "src/records/**"],
  "content": ["# PHI Data Handling", "", "MUST encrypt all PHI at rest and in transit.", "NEVER log PHI fields to standard output.", "", "Reference: @src/patient/record.rs:15"],
  "priority": 80,
  "evidence_refs": ["@src/patient/record.rs:15"]
}}
```

Return rules as JSON matching the schema."##,
            project_summary = project_summary,
            confidence_section = confidence_section,
            structural_section = structural_section,
            ast_section = ast_section,
            modules_section = modules_section,
            insights_section = insights_section,
            patterns_section = patterns_section,
            domain_section = domain_section,
            constraints_section = constraints_section,
        )
    }

    fn create_rule(discovered: DiscoveredRule) -> Rule {
        let paths: Vec<String> = discovered
            .paths
            .iter()
            .map(|p| p.trim().replace('\\', "/"))
            .collect();

        let mut rule = Rule::new(&discovered.name, discovered.content)
            .priority(discovered.priority);

        if !paths.is_empty() {
            rule = rule.paths(paths);
        }

        rule
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_rule_from_discovered() {
        let discovered = DiscoveredRule {
            name: "decimal-precision".into(),
            description: "Use Decimal for money".into(),
            why_important: "Prevents rounding errors".into(),
            paths: vec!["src/billing/**".into(), "src/payments/**".into()],
            content: vec![
                "# Decimal Precision".into(),
                String::new(),
                "MUST use Decimal for money.".into(),
            ],
            priority: 75,
            evidence_refs: vec!["@src/billing/calc.rs:42".into()],
        };

        let rule = RuleDiscovery::create_rule(discovered);

        assert_eq!(rule.name, "decimal-precision");
        assert_eq!(rule.priority, 75);
        assert!(rule.paths.as_ref().unwrap().len() == 2);
        assert!(rule.content.iter().any(|c| c.contains("Decimal")));
    }

    #[test]
    fn test_create_rule_no_paths() {
        let discovered = DiscoveredRule {
            name: "global-policy".into(),
            description: "A global policy".into(),
            why_important: "Required everywhere".into(),
            paths: vec![],
            content: vec!["# Global Policy".into(), String::new(), "Always do X.".into()],
            priority: 90,
            evidence_refs: vec![],
        };

        let rule = RuleDiscovery::create_rule(discovered);

        assert_eq!(rule.name, "global-policy");
        assert!(rule.paths.is_none());
    }

    #[test]
    fn test_discovered_rule_schema_is_valid() {
        let schema = schemars::schema_for!(RuleDiscoveryResponse);
        let json = serde_json::to_value(&schema).unwrap();
        assert!(json.get("properties").is_some() || json.get("$defs").is_some());
    }
}
