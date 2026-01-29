//! Regeneration Strategy
//!
//! Complete regeneration of artifacts using full GenerationContext.
//! Uses source insights and project context for context-aware regeneration.
//! Falls back to regeneration when other strategies fail repeatedly.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::ai::LlmProvider;
use crate::ai::response::generate_schema;
use crate::ai::validation::deserialize_llm_response;
use crate::types::{Agent, Result, Skill};

use super::{
    IssueKind, RefinementStrategy, StrategyContext, StrategyResult, calculate_validated_quality,
};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct SkillBodyOutput {
    #[serde(default)]
    skill_body: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct AgentPromptOutput {
    #[serde(default)]
    agent_prompt: String,
}

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
        let issues = context.format_issues();
        let default_suggestions = "- Focus on project-specific implementation details\n- Add concrete @file:line references";

        format!(
            r##"Regenerate this skill from scratch.

## PREVIOUS ISSUES
{issues}

{feedback_section}

## AVAILABLE FILES
{file_context}

## SKILL METADATA
Name: {name}
Description: {description}

## GUIDELINES
1. Use clear, actionable language appropriate for the project
2. Include @file:line references from AVAILABLE FILES when relevant
3. Focus on project-specific information rather than generic advice
4. Let structure emerge naturally from the content

## SUGGESTIONS
{suggestions}

Return JSON with skill_body containing the regenerated content."##,
            issues = issues,
            feedback_section = context.feedback_section(),
            file_context = file_context,
            name = name,
            description = description,
            suggestions = context.suggestions_section(default_suggestions),
        )
    }

    fn build_agent_regeneration_prompt(
        &self,
        name: &str,
        description: &str,
        context: &StrategyContext<'_>,
    ) -> String {
        let file_context = context.file_registry.to_prompt_context(100);
        let issues = context.format_issues();
        let default_suggestions =
            "- Define clear domain expertise\n- Include project-specific knowledge";

        format!(
            r##"Regenerate this agent from scratch.

## PREVIOUS ISSUES
{issues}

{feedback_section}

## AVAILABLE FILES
{file_context}

## AGENT METADATA
Name: {name}
Description: {description}

## GUIDELINES
1. Define clear domain expertise and specialized role
2. Include @file:line references from AVAILABLE FILES when relevant
3. Focus on project-specific information rather than generic advice
4. Let structure emerge naturally from the content

## SUGGESTIONS
{suggestions}

Return JSON with agent_prompt containing the regenerated content."##,
            issues = issues,
            feedback_section = context.feedback_section(),
            file_context = file_context,
            name = name,
            description = description,
            suggestions = context.suggestions_section(default_suggestions),
        )
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

        let schema = generate_schema::<SkillBodyOutput>();

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let output: SkillBodyOutput =
                    deserialize_llm_response(&response.content, "skill_regeneration")?;

                if !output.skill_body.is_empty() {
                    let new_score =
                        calculate_validated_quality(&output.skill_body, context.file_registry);
                    let acceptance_delta = context.quality_acceptance_delta;

                    if new_score > old_score + acceptance_delta {
                        skill.body = output.skill_body;
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

        let schema = generate_schema::<AgentPromptOutput>();

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let output: AgentPromptOutput =
                    deserialize_llm_response(&response.content, "agent_regeneration")?;

                if !output.agent_prompt.is_empty() {
                    let new_score =
                        calculate_validated_quality(&output.agent_prompt, context.file_registry);
                    let acceptance_delta = context.quality_acceptance_delta;

                    if new_score > old_score + acceptance_delta {
                        agent.prompt = output.agent_prompt;
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
