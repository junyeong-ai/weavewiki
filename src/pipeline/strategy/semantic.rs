//! Semantic Strategy
//!
//! Refines artifacts using semantic understanding and source insights.
//! Uses full GenerationContext to preserve original intent while improving quality.

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;

use crate::ai::LlmProvider;
use crate::types::{Agent, Result, Skill};

use super::{
    IssueKind, RefinementStrategy, StrategyContext, StrategyResult, calculate_validated_quality,
};

/// JSON schema for enhanced body response
static ENHANCED_BODY_SCHEMA: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::json!({
        "type": "object",
        "properties": {
            "enhanced_body": {"type": "string"}
        },
        "required": ["enhanced_body"]
    })
});

pub struct SemanticStrategy {
    provider: Arc<dyn LlmProvider>,
}

impl SemanticStrategy {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    fn build_enhancement_prompt(
        &self,
        content_type: &str,
        name: &str,
        description: &str,
        body: &str,
        context: &StrategyContext<'_>,
    ) -> String {
        let file_context = context.file_registry.to_prompt_context(50);
        let source_insights = context.format_source_insights();
        let project_context = context.format_project_context();
        let issues = context.format_issues();

        const DEFAULT_SUGGESTIONS: &str =
            "- Add more specific file references\n- Use stronger directive language";

        // Build prompt with full context
        let mut prompt = format!(
            r##"Improve this {content_type} for a Claude Code plugin. Preserve the original insights while making it more actionable and specific.

## QUALITY ISSUES TO ADDRESS
{issues}

{feedback_section}
"##,
            content_type = content_type,
            issues = issues,
            feedback_section = context.feedback_section(),
        );

        // Add source insights if available
        if !source_insights.is_empty() {
            prompt.push_str(&source_insights);
            prompt.push('\n');
        }

        // Add project context if available
        if !project_context.is_empty() {
            prompt.push_str(&project_context);
            prompt.push('\n');
        }

        // Add file context and current content
        prompt.push_str(&format!(
            r##"## AVAILABLE PROJECT FILES
{file_context}

## CURRENT CONTENT
Name: {name}
Description: {description}
---
{body}
---

## ENHANCEMENT REQUIREMENTS
1. PRESERVE all insights from SOURCE INSIGHTS section
2. Use directive language: 'You must...', 'Always...', 'Never...', 'Avoid...', 'Prefer...'
3. Add @file:line references from AVAILABLE FILES (e.g., '@src/main.rs:42')
4. Add concrete examples with good/bad comparison where helpful
5. Remove generic phrases: 'typically', 'usually', 'best practices', 'as needed'
6. Let structure emerge naturally - do NOT force fixed sections

## SUGGESTIONS
{suggestions}

Return JSON with enhanced content in 'enhanced_body' field."##,
            file_context = file_context,
            name = name,
            description = description,
            body = body,
            suggestions = context.suggestions_section(DEFAULT_SUGGESTIONS),
        ));

        prompt
    }
}

#[async_trait]
impl RefinementStrategy for SemanticStrategy {
    fn name(&self) -> &str {
        "semantic"
    }

    fn applicable_to(&self, issue: &IssueKind) -> bool {
        matches!(
            issue,
            IssueKind::LowActionability
                | IssueKind::TooGeneric
                | IssueKind::Shallow
                | IssueKind::Redundant
        )
    }

    fn priority(&self) -> u8 {
        80
    }

    async fn refine_skill(
        &self,
        skill: &mut Skill,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult> {
        let old_score = calculate_validated_quality(&skill.body, context.file_registry);
        let prompt = self.build_enhancement_prompt(
            "skill",
            &skill.name,
            &skill.description,
            &skill.body,
            context,
        );

        let schema = &*ENHANCED_BODY_SCHEMA;

        match self.provider.generate(&prompt, schema).await {
            Ok(response) => {
                if let Some(body) = response
                    .content
                    .get("enhanced_body")
                    .and_then(|v| v.as_str())
                {
                    let new_score = calculate_validated_quality(body, context.file_registry);
                    let acceptance_delta = context.quality_acceptance_delta;

                    // Only accept if quality improves by meaningful amount
                    if new_score > old_score + acceptance_delta {
                        skill.body = body.to_string();
                        return Ok(StrategyResult {
                            success: true,
                            quality_delta: new_score - old_score,
                            changes_made: vec![format!(
                                "Enhanced skill '{}' body (quality: {:.0}% -> {:.0}%)",
                                skill.name,
                                old_score * 100.0,
                                new_score * 100.0
                            )],
                        });
                    }
                }
                Ok(StrategyResult::default())
            }
            Err(e) => {
                tracing::warn!(skill = skill.name, error = %e, "Semantic enhancement failed");
                Ok(StrategyResult::default())
            }
        }
    }

    async fn refine_agent(
        &self,
        agent: &mut Agent,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult> {
        let old_score = calculate_validated_quality(&agent.prompt, context.file_registry);
        let prompt = self.build_enhancement_prompt(
            "agent",
            &agent.name,
            &agent.description,
            &agent.prompt,
            context,
        );

        let schema = &*ENHANCED_BODY_SCHEMA;

        match self.provider.generate(&prompt, schema).await {
            Ok(response) => {
                if let Some(body) = response
                    .content
                    .get("enhanced_body")
                    .and_then(|v| v.as_str())
                {
                    let new_score = calculate_validated_quality(body, context.file_registry);
                    let acceptance_delta = context.quality_acceptance_delta;

                    // Only accept if quality improves by meaningful amount
                    if new_score > old_score + acceptance_delta {
                        agent.prompt = body.to_string();
                        return Ok(StrategyResult {
                            success: true,
                            quality_delta: new_score - old_score,
                            changes_made: vec![format!(
                                "Enhanced agent '{}' prompt (quality: {:.0}% -> {:.0}%)",
                                agent.name,
                                old_score * 100.0,
                                new_score * 100.0
                            )],
                        });
                    }
                }
                Ok(StrategyResult::default())
            }
            Err(e) => {
                tracing::warn!(agent = agent.name, error = %e, "Semantic enhancement failed");
                Ok(StrategyResult::default())
            }
        }
    }
}
