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
        let issues = context.format_issues();

        const DEFAULT_SUGGESTIONS: &str =
            "- Add more specific file references\n- Use stronger directive language";

        let prompt = format!(
            r##"Improve this {content_type} for a Claude Code plugin. Preserve the original insights while making it more actionable and specific.

## QUALITY ISSUES TO ADDRESS
{issues}

{feedback_section}

## AVAILABLE PROJECT FILES
{file_context}

## CURRENT CONTENT
Name: {name}
Description: {description}
---
{body}
---

## ENHANCEMENT GUIDELINES
1. Use clear, actionable language appropriate for the project context
2. Add @file:line references from AVAILABLE FILES when relevant
3. Include concrete examples where helpful
4. Be specific to the project rather than giving generic advice
5. Let structure emerge naturally from the content

## SUGGESTIONS
{suggestions}

Return JSON with enhanced content in 'enhanced_body' field."##,
            content_type = content_type,
            issues = issues,
            feedback_section = context.feedback_section(),
            file_context = file_context,
            name = name,
            description = description,
            body = body,
            suggestions = context.suggestions_section(DEFAULT_SUGGESTIONS),
        );

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
