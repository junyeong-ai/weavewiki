//! Evidence Strategy
//!
//! Enhances artifacts with file references based on source insights.
//! Uses total reference count as a universal metric - LLM decides
//! which parts of content need evidence grounding.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::ai::LlmProvider;
use crate::ai::response::generate_schema;
use crate::ai::validation::deserialize_llm_response;
use crate::config::EvidenceFeedbackConfig;
use crate::pipeline::file_reference;
use crate::types::{Agent, Result, Skill};

use super::{
    IssueKind, RefinementStrategy, StrategyContext, StrategyResult, calculate_validated_quality,
};

struct EnhancementRequest<'a> {
    content_type: &'a str,
    name: &'a str,
    current_body: &'a str,
    current_refs: usize,
    target_refs: usize,
    retry: usize,
}

/// Result of evidence feedback loop
#[derive(Debug, Clone)]
pub enum EvidenceResult {
    /// References meet or exceed target
    Sufficient { total_refs: usize },
    /// Some references added but below target
    Partial {
        added: usize,
        total: usize,
        target: usize,
    },
    /// No references could be added
    NoImprovement { reason: String },
    /// Feedback loop disabled
    Disabled,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct EnhancedContentOutput {
    #[serde(default)]
    enhanced_content: String,
}

pub struct EvidenceStrategy {
    provider: Arc<dyn LlmProvider>,
    config: EvidenceFeedbackConfig,
}

impl EvidenceStrategy {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            config: EvidenceFeedbackConfig::default(),
        }
    }

    pub fn feedback_config(mut self, config: EvidenceFeedbackConfig) -> Self {
        self.config = config;
        self
    }

    /// Evidence feedback loop: iteratively improve references until target met or max retries
    /// Uses total reference count - LLM decides which parts need enhancement
    pub async fn evidence_feedback_loop(
        &self,
        skill: &mut Skill,
        context: &StrategyContext<'_>,
    ) -> Result<EvidenceResult> {
        if !self.config.enabled {
            return Ok(EvidenceResult::Disabled);
        }

        let initial_refs = self.count_references(&skill.body, context);
        let target = self.config.target_refs;

        if initial_refs >= target {
            return Ok(EvidenceResult::Sufficient {
                total_refs: initial_refs,
            });
        }

        let mut current_refs = initial_refs;
        let mut retry = 0;

        while retry < self.config.max_retries && current_refs < target {
            retry += 1;

            let request = EnhancementRequest {
                content_type: "skill",
                name: &skill.name,
                current_body: &skill.body,
                current_refs,
                target_refs: target,
                retry,
            };
            let feedback_prompt = self.build_enhancement_prompt(&request, context);

            let schema = generate_schema::<EnhancedContentOutput>();

            match self.provider.generate(&feedback_prompt, &schema).await {
                Ok(response) => {
                    let output: EnhancedContentOutput =
                        deserialize_llm_response(&response.content, "evidence_feedback")?;

                    if !output.enhanced_content.trim().is_empty() {
                        let new_refs = self.count_references(&output.enhanced_content, context);
                        let old_quality =
                            calculate_validated_quality(&skill.body, context.file_registry);
                        let new_quality = calculate_validated_quality(
                            &output.enhanced_content,
                            context.file_registry,
                        );

                        // Accept if refs improved without significant quality loss
                        if new_refs > current_refs && new_quality >= old_quality * 0.85 {
                            skill.body = output.enhanced_content;
                            current_refs = new_refs;

                            tracing::debug!(
                                skill = %skill.name,
                                retry,
                                refs = current_refs,
                                target,
                                "Evidence feedback loop: refs improved"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        skill = %skill.name,
                        retry,
                        error = %e,
                        "Evidence feedback loop: LLM call failed"
                    );
                }
            }
        }

        let added = current_refs - initial_refs;
        if current_refs >= target {
            Ok(EvidenceResult::Sufficient {
                total_refs: current_refs,
            })
        } else if added > 0 {
            Ok(EvidenceResult::Partial {
                added,
                total: current_refs,
                target,
            })
        } else {
            Ok(EvidenceResult::NoImprovement {
                reason: format!("Could not add references after {} retries", retry),
            })
        }
    }

    fn build_enhancement_prompt(
        &self,
        request: &EnhancementRequest<'_>,
        context: &StrategyContext<'_>,
    ) -> String {
        let file_context = context.file_registry.to_prompt_context(50);
        let code_samples = context.file_registry.get_code_samples(3);

        format!(
            r#"[RETRY {retry}/{max_retries}] Add @file:line references to {content_type} "{name}".

CURRENT STATUS:
- References found: {current_refs}
- Target references: {target_refs}
- Gap: {gap} more needed

CURRENT CONTENT:
{current_body}

AVAILABLE FILES:
{file_context}

CODE SAMPLES:
{code_samples}

REQUIREMENTS:
1. Add @file:line references (e.g., @src/main.rs:42) where claims need evidence
2. Reference actual files from AVAILABLE FILES list
3. Preserve the content structure and meaning
4. Focus on adding evidence to claims that need grounding

Return JSON with enhanced_content field containing the full enhanced content."#,
            retry = request.retry,
            max_retries = self.config.max_retries,
            content_type = request.content_type,
            name = request.name,
            current_refs = request.current_refs,
            target_refs = request.target_refs,
            gap = request.target_refs.saturating_sub(request.current_refs),
            current_body = request.current_body,
            file_context = file_context,
            code_samples = code_samples,
        )
    }

    /// Count valid references in content
    fn count_references(&self, content: &str, context: &StrategyContext<'_>) -> usize {
        let refs = file_reference::extract_references(content);
        let valid = refs
            .iter()
            .filter(|r| context.file_registry.contains(&r.path))
            .count();

        if valid < refs.len() {
            tracing::debug!(
                total = refs.len(),
                valid = valid,
                "Some references not found in registry"
            );
        }

        valid
    }

    /// Single-pass evidence enhancement for content
    async fn enhance_content(
        &self,
        content_type: &str,
        name: &str,
        body: &str,
        context: &StrategyContext<'_>,
    ) -> Result<Option<String>> {
        let old_refs = self.count_references(body, context);
        let old_quality = calculate_validated_quality(body, context.file_registry);

        let request = EnhancementRequest {
            content_type,
            name,
            current_body: body,
            current_refs: old_refs,
            target_refs: self.config.target_refs,
            retry: 1,
        };
        let prompt = self.build_enhancement_prompt(&request, context);

        let schema = generate_schema::<EnhancedContentOutput>();

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let output: EnhancedContentOutput =
                    deserialize_llm_response(&response.content, "evidence_enhance")?;

                if !output.enhanced_content.trim().is_empty() {
                    let new_refs = self.count_references(&output.enhanced_content, context);
                    let new_quality = calculate_validated_quality(
                        &output.enhanced_content,
                        context.file_registry,
                    );

                    if new_refs > old_refs && new_quality >= old_quality * 0.85 {
                        return Ok(Some(output.enhanced_content));
                    }
                }
                Ok(None)
            }
            Err(e) => {
                tracing::warn!(name = name, error = %e, "Evidence enhancement failed");
                Ok(None)
            }
        }
    }
}

#[async_trait]
impl RefinementStrategy for EvidenceStrategy {
    fn name(&self) -> &str {
        "evidence"
    }

    fn applicable_to(&self, issue: &IssueKind) -> bool {
        matches!(
            issue,
            IssueKind::WeakEvidence | IssueKind::MissingReferences
        )
    }

    fn priority(&self) -> u8 {
        70
    }

    async fn refine_skill(
        &self,
        skill: &mut Skill,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult> {
        let old_refs = self.count_references(&skill.body, context);
        let old_quality = calculate_validated_quality(&skill.body, context.file_registry);

        // Use feedback loop if enabled
        if self.config.enabled {
            let result = self.evidence_feedback_loop(skill, context).await?;
            let new_quality = calculate_validated_quality(&skill.body, context.file_registry);

            return match result {
                EvidenceResult::Sufficient { total_refs } => Ok(StrategyResult {
                    success: true,
                    quality_delta: new_quality - old_quality,
                    changes_made: vec![format!(
                        "Evidence sufficient: {} refs in skill '{}' (quality: {:.0}%)",
                        total_refs,
                        skill.name,
                        new_quality * 100.0
                    )],
                }),
                EvidenceResult::Partial {
                    added,
                    total,
                    target,
                } => Ok(StrategyResult {
                    success: added > 0,
                    quality_delta: new_quality - old_quality,
                    changes_made: vec![format!(
                        "Added {} refs to skill '{}' (total: {}, target: {}, quality: {:.0}% -> {:.0}%)",
                        added,
                        skill.name,
                        total,
                        target,
                        old_quality * 100.0,
                        new_quality * 100.0
                    )],
                }),
                EvidenceResult::NoImprovement { reason } => {
                    tracing::debug!(skill = %skill.name, reason = %reason, "No improvement");
                    Ok(StrategyResult::default())
                }
                EvidenceResult::Disabled => unreachable!("feedback loop is enabled"),
            };
        }

        // Single-pass when feedback loop disabled
        if let Some(enhanced) = self
            .enhance_content("skill", &skill.name, &skill.body, context)
            .await?
        {
            let new_refs = self.count_references(&enhanced, context);
            let new_quality = calculate_validated_quality(&enhanced, context.file_registry);
            skill.body = enhanced;
            Ok(StrategyResult {
                success: true,
                quality_delta: new_quality - old_quality,
                changes_made: vec![format!(
                    "Added {} refs to skill '{}' (quality: {:.0}% -> {:.0}%)",
                    new_refs - old_refs,
                    skill.name,
                    old_quality * 100.0,
                    new_quality * 100.0
                )],
            })
        } else {
            Ok(StrategyResult::default())
        }
    }

    async fn refine_agent(
        &self,
        agent: &mut Agent,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult> {
        let old_refs = self.count_references(&agent.prompt, context);
        let old_quality = calculate_validated_quality(&agent.prompt, context.file_registry);

        if let Some(enhanced) = self
            .enhance_content("agent", &agent.name, &agent.prompt, context)
            .await?
        {
            let new_refs = self.count_references(&enhanced, context);
            let new_quality = calculate_validated_quality(&enhanced, context.file_registry);
            agent.prompt = enhanced;
            Ok(StrategyResult {
                success: true,
                quality_delta: new_quality - old_quality,
                changes_made: vec![format!(
                    "Added {} refs to agent '{}' (quality: {:.0}% -> {:.0}%)",
                    new_refs - old_refs,
                    agent.name,
                    old_quality * 100.0,
                    new_quality * 100.0
                )],
            })
        } else {
            Ok(StrategyResult::default())
        }
    }
}
