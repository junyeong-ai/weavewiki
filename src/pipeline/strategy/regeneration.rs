//! Regeneration Strategy

use std::sync::Arc;

use async_trait::async_trait;

use crate::ai::LlmProvider;
use crate::config::RefinementConfig;
use crate::pipeline::validation::content::thresholds;
use crate::types::{Agent, Result, Skill};

use super::{IssueKind, RefinementStrategy, StrategyContext, StrategyResult, calculate_validated_quality};

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

        format!(
            r###"Generate a high-quality skill document from scratch.

{file_context}

SKILL NAME: {name}
SKILL DESCRIPTION: {description}

PREVIOUS ISSUES: {issues}

REQUIREMENTS:
1. Minimum 400 characters of substantive content
2. Use directive language: "must", "should", "avoid", "use", "prefer", "ensure", "never"
3. Include at least 3 @file:line references from the available files list
4. Include step-by-step instructions with numbered steps
5. Add a "## Common Mistakes" or "## Gotchas" section
6. Add at least one code example with ```

TEMPLATE:
## Overview
[Brief description of the skill]

## Steps
1. First step with @file:line reference
2. Second step
3. Third step

## Example
```language
// Example code
```

## Gotchas
- Common mistake 1
- Common mistake 2

Return ONLY the skill body content."###,
            file_context = file_context,
            name = name,
            description = description,
            issues = context.issue_description,
        )
    }

    fn build_agent_regeneration_prompt(
        &self,
        name: &str,
        description: &str,
        context: &StrategyContext<'_>,
    ) -> String {
        let file_context = context.file_registry.to_prompt_context(100);

        format!(
            r###"Generate a high-quality agent prompt from scratch.

{file_context}

AGENT NAME: {name}
AGENT DESCRIPTION: {description}

PREVIOUS ISSUES: {issues}

REQUIREMENTS:
1. Clear statement of the agent's purpose and responsibilities
2. At least 3 ## section headers
3. Include @file:line references to relevant code locations
4. Specify what the agent should and should not do
5. Include example scenarios

TEMPLATE:
## Purpose
[What this agent does and when to use it]

## Responsibilities
- Must: [specific actions]
- Should: [recommended actions]
- Avoid: [things to not do]

## Key Files
- @file:line - description
- @file:line - description

## Decision Criteria
[When to take which action]

Return ONLY the agent prompt content."###,
            file_context = file_context,
            name = name,
            description = description,
            issues = context.issue_description,
        )
    }
}

#[async_trait]
impl RefinementStrategy for RegenerationStrategy {
    fn name(&self) -> &str {
        "regeneration"
    }

    fn applicable_to(&self, _issue: &IssueKind) -> bool {
        true
    }

    fn priority(&self) -> u8 {
        10
    }

    async fn refine_skill(
        &self,
        skill: &mut Skill,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult> {
        let old_score = calculate_validated_quality(&skill.body, context.file_registry);
        let prompt = self.build_skill_regeneration_prompt(&skill.name, &skill.description, context);

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "skill_body": {"type": "string"}
            },
            "required": ["skill_body"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                if let Some(body) = response
                    .content
                    .get("skill_body")
                    .and_then(|v| v.as_str())
                {
                    let new_score = calculate_validated_quality(body, context.file_registry);
                    let t = thresholds::get();
                    let acceptance_delta = RefinementConfig::default().quality_acceptance_delta;

                    // Only accept if new content is better
                    if body.len() >= t.skill_min_chars && new_score > old_score + acceptance_delta {
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

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "agent_prompt": {"type": "string"}
            },
            "required": ["agent_prompt"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                if let Some(body) = response
                    .content
                    .get("agent_prompt")
                    .and_then(|v| v.as_str())
                {
                    let new_score = calculate_validated_quality(body, context.file_registry);
                    let t = thresholds::get();
                    let acceptance_delta = RefinementConfig::default().quality_acceptance_delta;

                    // Only accept if new content is better
                    if body.len() >= t.agent_min_chars && new_score > old_score + acceptance_delta {
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
