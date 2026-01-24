//! Regeneration Strategy
//!
//! Complete regeneration of artifacts using full GenerationContext.
//! Uses source insights and project context for context-aware regeneration.
//! Falls back to regeneration when other strategies fail repeatedly.

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;

use crate::ai::LlmProvider;
use crate::types::{Agent, Result, Skill};

use super::{
    IssueKind, RefinementStrategy, StrategyContext, StrategyResult, calculate_validated_quality,
};

/// Minimum content thresholds
const SKILL_MIN_CHARS: usize = 200;
const AGENT_MIN_CHARS: usize = 300;

/// JSON schema for skill body response
static SKILL_BODY_SCHEMA: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::json!({
        "type": "object",
        "properties": {
            "skill_body": {"type": "string"}
        },
        "required": ["skill_body"]
    })
});

/// JSON schema for agent prompt response
static AGENT_PROMPT_SCHEMA: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::json!({
        "type": "object",
        "properties": {
            "agent_prompt": {"type": "string"}
        },
        "required": ["agent_prompt"]
    })
});

pub struct RegenerationStrategy {
    provider: Arc<dyn LlmProvider>,
}

impl RegenerationStrategy {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    fn build_skill_regeneration_prompt(
        &self,
        name: &str,
        description: &str,
        context: &StrategyContext<'_>,
    ) -> String {
        let file_context = context.file_registry.to_prompt_context(100);
        let source_insights = context.format_source_insights();
        let project_context = context.format_project_context();
        let issues = context.format_issues();

        let default_suggestions = "- Focus on project-specific implementation details\n- Add concrete @file:line references";

        let mut prompt = format!(
            r##"Regenerate this skill from scratch based on the source insights.

## PREVIOUS ISSUES
{issues}

{feedback_section}
"##,
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

        prompt.push_str(&format!(
            r##"## AVAILABLE FILES
{file_context}

## SKILL METADATA
Name: {name}
Description: {description}

## REQUIREMENTS
1. Preserve ALL insights from SOURCE INSIGHTS section
2. Use directive language: "must", "should", "avoid", "use", "prefer", "ensure", "never"
3. Include @file:line references from AVAILABLE FILES
4. Be project-specific, NOT generic advice
5. Let structure emerge naturally from the content
6. Minimum 400 characters of substantive content

## SUGGESTIONS
{suggestions}

Return JSON with skill_body containing the regenerated content."##,
            file_context = file_context,
            name = name,
            description = description,
            suggestions = context.suggestions_section(default_suggestions),
        ));

        prompt
    }

    fn build_agent_regeneration_prompt(
        &self,
        name: &str,
        description: &str,
        context: &StrategyContext<'_>,
    ) -> String {
        let file_context = context.file_registry.to_prompt_context(100);
        let source_insights = context.format_source_insights();
        let project_context = context.format_project_context();
        let issues = context.format_issues();

        let default_suggestions =
            "- Define clear domain expertise\n- Include project-specific knowledge";

        let mut prompt = format!(
            r##"Regenerate this agent from scratch based on the source insights.

## PREVIOUS ISSUES
{issues}

{feedback_section}
"##,
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

        prompt.push_str(&format!(
            r##"## AVAILABLE FILES
{file_context}

## AGENT METADATA
Name: {name}
Description: {description}

## REQUIREMENTS
1. Preserve ALL insights from SOURCE INSIGHTS section
2. Define clear domain expertise and specialized role
3. Include @file:line references from AVAILABLE FILES
4. Specify what the agent should and should not do
5. Be project-specific, NOT generic advice
6. Let structure emerge naturally from the content

## SUGGESTIONS
{suggestions}

Return JSON with agent_prompt containing the regenerated content."##,
            file_context = file_context,
            name = name,
            description = description,
            suggestions = context.suggestions_section(default_suggestions),
        ));

        prompt
    }
}

#[async_trait]
impl RefinementStrategy for RegenerationStrategy {
    fn name(&self) -> &str {
        "regeneration"
    }

    fn applicable_to(&self, _issue: &IssueKind) -> bool {
        // Regeneration is applicable to all issues as a last resort
        true
    }

    fn priority(&self) -> u8 {
        // Lowest priority - use other strategies first
        10
    }

    async fn refine_skill(
        &self,
        skill: &mut Skill,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult> {
        let old_score = calculate_validated_quality(&skill.body, context.file_registry);
        let prompt = self.build_skill_regeneration_prompt(&skill.name, &skill.description, context);

        let schema = &*SKILL_BODY_SCHEMA;

        match self.provider.generate(&prompt, schema).await {
            Ok(response) => {
                if let Some(body) = response.content.get("skill_body").and_then(|v| v.as_str()) {
                    let new_score = calculate_validated_quality(body, context.file_registry);
                    let acceptance_delta = context.quality_acceptance_delta;

                    // Only accept if new content is better and meets minimum requirements
                    if body.len() >= SKILL_MIN_CHARS && new_score > old_score + acceptance_delta {
                        skill.body = body.to_string();
                        return Ok(StrategyResult {
                            success: true,
                            quality_delta: new_score - old_score,
                            changes_made: vec![format!(
                                "Regenerated skill '{}' (quality: {:.0}% -> {:.0}%)",
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
                tracing::warn!(skill = skill.name, error = %e, "Skill regeneration failed");
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
        let prompt = self.build_agent_regeneration_prompt(&agent.name, &agent.description, context);

        let schema = &*AGENT_PROMPT_SCHEMA;

        match self.provider.generate(&prompt, schema).await {
            Ok(response) => {
                if let Some(body) = response
                    .content
                    .get("agent_prompt")
                    .and_then(|v| v.as_str())
                {
                    let new_score = calculate_validated_quality(body, context.file_registry);
                    let acceptance_delta = context.quality_acceptance_delta;

                    // Only accept if new content is better and meets minimum requirements
                    if body.len() >= AGENT_MIN_CHARS && new_score > old_score + acceptance_delta {
                        agent.prompt = body.to_string();
                        return Ok(StrategyResult {
                            success: true,
                            quality_delta: new_score - old_score,
                            changes_made: vec![format!(
                                "Regenerated agent '{}' (quality: {:.0}% -> {:.0}%)",
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
                tracing::warn!(agent = agent.name, error = %e, "Agent regeneration failed");
                Ok(StrategyResult::default())
            }
        }
    }
}
