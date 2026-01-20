//! Semantic Strategy

use std::sync::Arc;

use async_trait::async_trait;

use crate::ai::LlmProvider;
use crate::types::{Agent, Result, Skill};

use super::{IssueKind, RefinementStrategy, StrategyContext, StrategyResult, calculate_validated_quality};

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

        format!(
            r##"You are improving a {content_type} for a Claude Code plugin. Make it more actionable and specific.

QUALITY ISSUE TO FIX: {issue}

AVAILABLE PROJECT FILES (use these for @file:line references):
{file_context}

CURRENT CONTENT:
Name: {name}
Description: {description}
---
{body}
---

ENHANCEMENT REQUIREMENTS:
1. Use directive language throughout: 'You must...', 'Always...', 'Never...', 'Avoid...', 'Prefer...'
2. Add specific @file:line references to the files listed above (e.g., '@src/main.rs:42')
3. Include a '## Why' section explaining the rationale
4. Add concrete examples:
   ```rust
   // BAD: description
   bad_code_here();

   // GOOD: description
   good_code_here();
   ```
5. Remove generic phrases like 'typically', 'usually', 'best practices', 'as needed'

SUGGESTIONS:
{suggestions}

Return a JSON object with the enhanced content in the 'enhanced_body' field."##,
            content_type = content_type,
            issue = context.issue_description,
            file_context = file_context,
            name = name,
            description = description,
            body = body,
            suggestions = if context.suggestions.is_empty() {
                "- Add more specific file references\n- Use stronger directive language".to_string()
            } else {
                context.suggestions.iter().map(|s| format!("- {}", s)).collect::<Vec<_>>().join("\n")
            },
        )
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

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "enhanced_body": {"type": "string"}
            },
            "required": ["enhanced_body"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                if let Some(body) = response
                    .content
                    .get("enhanced_body")
                    .and_then(|v| v.as_str())
                {
                    let new_score = calculate_validated_quality(body, context.file_registry);

                    // Only accept if quality improves by meaningful amount
                    if new_score > old_score + 0.02 {
                        let _old_body = std::mem::replace(&mut skill.body, body.to_string());
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

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "enhanced_body": {"type": "string"}
            },
            "required": ["enhanced_body"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                if let Some(body) = response
                    .content
                    .get("enhanced_body")
                    .and_then(|v| v.as_str())
                {
                    let new_score = calculate_validated_quality(body, context.file_registry);

                    // Only accept if quality improves by meaningful amount
                    if new_score > old_score + 0.02 {
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
