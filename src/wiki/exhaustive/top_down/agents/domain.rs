//! Domain Agent
//!
//! Applies domain terminology to enhance insight quality.

use super::{TopDownAgent, TopDownAgentConfig, TopDownContext, run_top_down_agent};
use crate::types::{DomainTerm, error::WeaveError};
use crate::wiki::exhaustive::top_down::insights::ProjectInsight;
use serde_json::json;

#[derive(Default)]
pub struct DomainAgent;

#[async_trait::async_trait]
impl TopDownAgent for DomainAgent {
    fn name(&self) -> &str {
        "domain"
    }

    async fn run(&self, context: &TopDownContext) -> Result<ProjectInsight, WeaveError> {
        run_top_down_agent(
            context,
            TopDownAgentConfig {
                name: "domain",
                build_context: Box::new(Self::build_domain_context),
                build_prompt: Box::new(Self::build_prompt),
                schema: Self::schema(),
                parse_result: Box::new(Self::parse_result),
                debug_result: Box::new(|insight| {
                    format!(
                        "Found {} terms, {} patterns, {} recommendations",
                        insight.domain_terminology.len(),
                        insight.domain_patterns.len(),
                        insight.domain_recommendations.len()
                    )
                }),
            },
        )
        .await
    }
}

impl DomainAgent {
    fn build_domain_context(context: &TopDownContext) -> String {
        let mut ctx = String::new();

        if !context.profile.purposes.is_empty() {
            ctx.push_str(&format!(
                "Project purposes: {}\n",
                context.profile.purposes.join(", ")
            ));
        }

        if !context.profile.target_users.is_empty() {
            ctx.push_str(&format!(
                "Target users: {}\n",
                context.profile.target_users.join(", ")
            ));
        }

        // Add terminology
        if !context.profile.terminology.is_empty() {
            ctx.push_str("\nKnown Terminology:\n");
            for t in &context.profile.terminology {
                ctx.push_str(&format!("- {}: {}\n", t.term, t.definition));
            }
        } else {
            ctx.push_str("\nNo domain terminology detected yet.\n");
        }

        // Add insights summary
        let total_files = context.file_insights.len();
        let files_with_content = context
            .file_insights
            .iter()
            .filter(|f| f.has_content())
            .count();
        let files_with_diagrams = context
            .file_insights
            .iter()
            .filter(|f| f.has_diagram())
            .count();

        ctx.push_str(&format!(
            "\nFile Insights Summary:\nTotal files analyzed: {}\nFiles with documentation: {}\nFiles with diagrams: {}\n",
            total_files, files_with_content, files_with_diagrams
        ));

        ctx
    }

    fn build_prompt(context: &TopDownContext, domain_context: &str) -> String {
        format!(
            r#"<ROLE>
You are a domain expert analyst specializing in extracting and refining project-specific
terminology and patterns. Your task is to enhance domain understanding by analyzing
code through the lens of domain-specific language and patterns.
</ROLE>

<OBJECTIVES>
Based on the project context, provide:
1. Refined domain terminology with accurate definitions grounded in the codebase
2. Domain-specific patterns and idioms observed in the code
3. Architecture patterns that align with domain best practices
4. Recommendations for domain clarity and terminology consistency
</OBJECTIVES>

## Project Context
Name: {}
Domain Traits: {}

{}

<FOCUS>
IMPORTANT: Focus on domain patterns evident in the code, NOT generic domain knowledge.
- Do NOT explain domain concepts outside the project context
- Do NOT recommend technologies or patterns not present in the codebase
- Do NOT speculate about domain requirements beyond what the code reveals
- ONLY connect observations to actual terminology and patterns found in the code
</FOCUS>

## Analysis Task
Analyze domain context to:
1. Refine terminology definitions based on actual code usage
2. Identify domain-specific patterns (modeling, workflows, data structures)
3. Assess domain-architecture alignment
4. Recommend terminology improvements for clarity

<ANTI-PATTERNS>
WRONG - Generic domain advice:
- "Domain-driven design is a good approach"
- "Terminology should be clear"
- "The domain model could be improved"

WRONG - External domain knowledge:
- Explaining generic concepts like "aggregates" without code reference
- Recommending patterns without evidence they're already used

CORRECT - Code-grounded analysis:
- "The codebase uses 'Workflow' inconsistently: container vs single step. Recommend: 'Workflow' for multi-step, 'WorkflowStep' for individual steps"
- "Pattern: All domain entities inherit from 'Entity' base (src/domain/entity.rs:15)"
- "Term 'Pipeline' is ambiguous: used for data (src/data/) and workflow (src/workflow/)"
</ANTI-PATTERNS>
"#,
            context.profile.name,
            context.profile.domain_traits.join(", "),
            domain_context
        )
    }

    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Domain terminology and pattern analysis with refinements grounded in actual code usage",
            "required": ["refined_terminology"],
            "additionalProperties": false,
            "properties": {
                "refined_terminology": {
                    "type": "array",
                    "description": "Refined domain terms with definitions grounded in actual code usage",
                    "minItems": 0,
                    "maxItems": 50,
                    "items": {
                        "type": "object",
                        "required": ["term", "definition"],
                        "additionalProperties": false,
                        "properties": {
                            "term": {"type": "string", "description": "Domain term", "minLength": 1, "maxLength": 100},
                            "definition": {"type": "string", "description": "Precise definition based on code usage", "minLength": 10, "maxLength": 500},
                            "context": {"type": "string", "description": "Where/when this term is used in the codebase", "minLength": 5, "maxLength": 300},
                            "improvement": {"type": "string", "description": "Suggested refinement if ambiguity exists", "maxLength": 300}
                        }
                    }
                },
                "domain_patterns": {
                    "type": "array",
                    "description": "Domain-specific patterns identified with code evidence",
                    "maxItems": 30,
                    "items": {"type": "string", "minLength": 10, "maxLength": 300}
                },
                "recommendations": {
                    "type": "array",
                    "description": "Recommendations for improving domain clarity",
                    "maxItems": 15,
                    "items": {"type": "string", "minLength": 10, "maxLength": 500}
                },
                "terminology_inconsistencies": {
                    "type": "array",
                    "description": "Terms used inconsistently across the codebase",
                    "maxItems": 20,
                    "items": {
                        "type": "object",
                        "required": ["term", "usages"],
                        "additionalProperties": false,
                        "properties": {
                            "term": {"type": "string", "minLength": 1, "maxLength": 100},
                            "usages": {"type": "array", "description": "Different meanings observed", "minItems": 2, "maxItems": 10, "items": {"type": "string", "maxLength": 200}}
                        }
                    }
                }
            }
        })
    }

    fn parse_result(result: &serde_json::Value, insight: &mut ProjectInsight) {
        // Map domain terminology
        if let Some(terms) = result.get("refined_terminology").and_then(|v| v.as_array()) {
            insight.domain_terminology = terms
                .iter()
                .filter_map(|v| {
                    Some(DomainTerm {
                        term: v.get("term")?.as_str()?.to_string(),
                        definition: v.get("definition")?.as_str()?.to_string(),
                        context: v.get("context").and_then(|c| c.as_str()).map(String::from),
                    })
                })
                .collect();
        }

        // Map domain patterns
        if let Some(patterns) = result.get("domain_patterns").and_then(|v| v.as_array()) {
            insight.domain_patterns = patterns
                .iter()
                .filter_map(|p| p.as_str().map(String::from))
                .collect();
        }

        // Map domain recommendations
        if let Some(recs) = result.get("recommendations").and_then(|v| v.as_array()) {
            insight.domain_recommendations = recs
                .iter()
                .filter_map(|r| r.as_str().map(String::from))
                .collect();
        }
    }
}
