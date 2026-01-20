//! Value Assessor (Layer 3)
//!
//! Assesses the value of generated content using LLM + few-shot examples.
//! Core question: "Would AI make mistakes without this information?"
//!
//! Dimensions:
//! - Mistake Prevention: How likely would AI make mistakes without this?
//! - Discoverability: How hard is this to discover from code alone?
//! - Tier Classification: tier1/tier2/tier3

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, warn};

use crate::ai::{with_timeout, LlmProvider};
use crate::config::ValueAssessmentConfig;

/// Default timeout for LLM value assessment calls (30 seconds)
const LLM_ASSESSMENT_TIMEOUT_SECS: u64 = 30;

use crate::types::Result;

use super::few_shot_examples::{FewShotExamples, TierLevel, ValueDimensions};
use super::layers::{IssueCode, IssueSeverity, LayerResult, ValidationIssue, ValidationLayer};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentAssessment {
    pub content_id: String,
    pub content_type: String,
    pub dimensions: ValueDimensions,
    pub confidence: f32,
    pub reasoning: String,
}

#[derive(Debug, Clone, Default)]
pub struct ValueAssessmentResult {
    pub passed: bool,
    pub total_items: usize,
    pub tier1_count: usize,
    pub tier2_count: usize,
    pub tier3_count: usize,
    pub assessments: Vec<ContentAssessment>,
    pub issues: Vec<ValidationIssue>,
    pub average_value: f32,
}

pub struct ValueAssessor {
    provider: Arc<dyn LlmProvider>,
    config: ValueAssessmentConfig,
    few_shot: FewShotExamples,
}

impl ValueAssessor {
    pub fn new(provider: Arc<dyn LlmProvider>, config: ValueAssessmentConfig) -> Self {
        Self {
            provider,
            config,
            few_shot: FewShotExamples::with_defaults(),
        }
    }

    pub async fn assess(&self, items: &[(String, String, String)]) -> Result<LayerResult> {
        if !self.config.enabled {
            return Ok(LayerResult::pass(ValidationLayer::ValueAssessment));
        }

        let mut result = ValueAssessmentResult::default();

        for (id, content_type, content) in items {
            match self.assess_single(id, content_type, content).await {
                Ok(assessment) => {
                    result.total_items += 1;

                    match assessment.dimensions.tier {
                        TierLevel::Tier1Generic => {
                            result.tier1_count += 1;
                            if self.config.reject_tier1 {
                                result.issues.push(
                                    ValidationIssue::error(
                                        ValidationLayer::ValueAssessment,
                                        id,
                                        IssueCode::Tier1Content,
                                        format!(
                                            "Content classified as Tier 1 (generic): {}",
                                            assessment.reasoning
                                        ),
                                    )
                                    .with_suggestion(
                                        "Add project-specific constraints, file references, or hidden knowledge",
                                    ),
                                );
                            }
                        }
                        TierLevel::Tier2Convention => {
                            result.tier2_count += 1;
                        }
                        TierLevel::Tier3Constraint => {
                            result.tier3_count += 1;
                        }
                    }

                    if assessment.dimensions.mistake_prevention < self.config.min_mistake_prevention
                    {
                        result.issues.push(
                            ValidationIssue::warning(
                                ValidationLayer::ValueAssessment,
                                id,
                                IssueCode::LowMistakePrevention,
                                format!(
                                    "Low mistake prevention score: {:.2}",
                                    assessment.dimensions.mistake_prevention
                                ),
                            )
                            .with_suggestion(
                                "Add information about common mistakes this prevents",
                            ),
                        );
                    }

                    if assessment.dimensions.discoverability < self.config.min_discoverability {
                        result.issues.push(
                            ValidationIssue::warning(
                                ValidationLayer::ValueAssessment,
                                id,
                                IssueCode::LowDiscoverability,
                                format!(
                                    "Low discoverability score: {:.2}",
                                    assessment.dimensions.discoverability
                                ),
                            )
                            .with_suggestion(
                                "Focus on information that's hard to find in code alone",
                            ),
                        );
                    }

                    result.assessments.push(assessment);
                }
                Err(e) => {
                    warn!(id = %id, error = %e, "Failed to assess content");
                    result.issues.push(
                        ValidationIssue::warning(
                            ValidationLayer::ValueAssessment,
                            id,
                            IssueCode::LlmValidationFailed,
                            format!("Failed to assess value: {}", e),
                        ),
                    );
                }
            }
        }

        result.average_value = if result.total_items > 0 {
            result
                .assessments
                .iter()
                .map(|a| a.dimensions.overall_value())
                .sum::<f32>()
                / result.total_items as f32
        } else {
            0.0
        };

        // Pass if: (no tier1 content OR tier1 rejection disabled) AND no error-level issues
        let tier1_ok = result.tier1_count == 0 || !self.config.reject_tier1;
        let no_errors = result.issues.iter().all(|i| i.severity != IssueSeverity::Error);
        result.passed = tier1_ok && no_errors;

        let score = if result.total_items > 0 {
            let tier_score =
                (result.tier2_count + result.tier3_count * 2) as f32 / (result.total_items * 2) as f32;
            (tier_score + result.average_value) / 2.0
        } else {
            1.0
        };

        if result.issues.is_empty() {
            Ok(LayerResult::pass(ValidationLayer::ValueAssessment)
                .with_score(score)
                .with_metadata("tier1_count", result.tier1_count.to_string())
                .with_metadata("tier2_count", result.tier2_count.to_string())
                .with_metadata("tier3_count", result.tier3_count.to_string()))
        } else {
            Ok(LayerResult::fail(ValidationLayer::ValueAssessment, result.issues)
                .with_score(score)
                .with_metadata("tier1_count", result.tier1_count.to_string())
                .with_metadata("tier2_count", result.tier2_count.to_string())
                .with_metadata("tier3_count", result.tier3_count.to_string()))
        }
    }

    async fn assess_single(
        &self,
        id: &str,
        content_type: &str,
        content: &str,
    ) -> Result<ContentAssessment> {
        let few_shot_prompt = if self.config.use_few_shot {
            self.few_shot.to_prompt_format(self.config.few_shot_examples_count)
        } else {
            String::new()
        };

        let prompt = format!(
            r#"Assess the value of this Claude Code artifact content.

{few_shot}

CONTENT TO ASSESS:
Type: {content_type}
ID: {id}
---
{content}
---

Core Question: "Would an AI assistant make mistakes without this information?"

Evaluate:
1. **Mistake Prevention** (0.0-1.0): How likely would AI make mistakes without this?
   - 0.0 = AI already knows this, no prevention value
   - 0.5 = Helpful but not critical
   - 1.0 = Without this, AI would definitely make mistakes

2. **Discoverability** (0.0-1.0): How hard is this to discover from code alone?
   - 0.0 = Obvious from reading the code
   - 0.5 = Requires careful analysis
   - 1.0 = Hidden knowledge, not discoverable without experience

3. **Tier Classification**:
   - tier1_generic: Generic language/tool knowledge (REJECT)
   - tier2_convention: Project conventions (KEEP)
   - tier3_constraint: Hidden constraints, gotchas (ESSENTIAL)

Respond in JSON:
{{
  "mistake_prevention": 0.0-1.0,
  "discoverability": 0.0-1.0,
  "tier": "tier1_generic" | "tier2_convention" | "tier3_constraint",
  "confidence": 0.0-1.0,
  "reasoning": "brief explanation of classification"
}}"#,
            few_shot = few_shot_prompt,
            content_type = content_type,
            id = id,
            content = &content[..content.len().min(2000)]
        );

        let schema = json!({
            "type": "object",
            "properties": {
                "mistake_prevention": { "type": "number" },
                "discoverability": { "type": "number" },
                "tier": { "type": "string", "enum": ["tier1_generic", "tier2_convention", "tier3_constraint"] },
                "confidence": { "type": "number" },
                "reasoning": { "type": "string" }
            },
            "required": ["mistake_prevention", "discoverability", "tier", "confidence", "reasoning"]
        });

        let timeout = Duration::from_secs(LLM_ASSESSMENT_TIMEOUT_SECS);
        let response = with_timeout(
            timeout,
            self.provider.generate(&prompt, &schema),
            "value_assessment",
        )
        .await?;

        #[derive(Deserialize)]
        struct LlmResponse {
            mistake_prevention: f32,
            discoverability: f32,
            tier: String,
            confidence: f32,
            reasoning: String,
        }

        let parsed: LlmResponse = serde_json::from_value(response.content)?;

        let tier = match parsed.tier.as_str() {
            "tier1_generic" => TierLevel::Tier1Generic,
            "tier2_convention" => TierLevel::Tier2Convention,
            "tier3_constraint" => TierLevel::Tier3Constraint,
            _ => TierLevel::Tier2Convention,
        };

        debug!(
            id = %id,
            tier = %parsed.tier,
            mistake_prevention = %parsed.mistake_prevention,
            discoverability = %parsed.discoverability,
            "Content assessed"
        );

        Ok(ContentAssessment {
            content_id: id.to_string(),
            content_type: content_type.to_string(),
            dimensions: ValueDimensions {
                mistake_prevention: parsed.mistake_prevention,
                discoverability: parsed.discoverability,
                tier,
            },
            confidence: parsed.confidence,
            reasoning: parsed.reasoning,
        })
    }

    pub fn assess_programmatic(&self, content: &str) -> ValueDimensions {
        let lower = content.to_lowercase();

        let tier1_patterns = [
            "cargo build",
            "npm install",
            "go build",
            "pip install",
            "best practices",
            "clean code",
            "write tests",
            "handle errors",
            "use async/await",
        ];

        let tier3_patterns = [
            "must",
            "never",
            "critical",
            "race condition",
            "deadlock",
            "order matters",
            "sequence",
            "do not",
            "gotcha",
            "pitfall",
            "hidden",
        ];

        let has_file_refs = content.contains("@") && content.contains(":");
        let has_code_examples = content.contains("```");

        let tier1_matches = tier1_patterns.iter().filter(|p| lower.contains(*p)).count();
        let tier3_matches = tier3_patterns.iter().filter(|p| lower.contains(*p)).count();

        if tier1_matches >= 2 && tier3_matches == 0 && !has_file_refs {
            return ValueDimensions::tier1();
        }

        if tier3_matches >= 2 || (tier3_matches >= 1 && has_file_refs) {
            let mp = (0.5 + tier3_matches as f32 * 0.15).min(1.0);
            let disc = if has_file_refs { 0.7 } else { 0.5 };
            return ValueDimensions::tier3(mp, disc);
        }

        let mp = if has_file_refs { 0.5 } else { 0.3 };
        let disc = if has_code_examples { 0.4 } else { 0.3 };
        ValueDimensions::tier2(mp, disc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    struct MockProvider;

    #[async_trait::async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(
            &self,
            _prompt: &str,
            _schema: &Value,
        ) -> crate::types::Result<crate::ai::LlmResponse> {
            unimplemented!()
        }
        fn name(&self) -> &str {
            "mock"
        }
        fn model(&self) -> &str {
            "mock"
        }
        async fn health_check(&self) -> crate::types::Result<bool> {
            Ok(true)
        }
    }

    #[test]
    fn test_programmatic_tier1_detection() {
        let assessor = ValueAssessor::new(
            Arc::new(MockProvider),
            ValueAssessmentConfig::default(),
        );

        let tier1_content = "Use cargo build to compile. Follow best practices.";
        let result = assessor.assess_programmatic(tier1_content);
        assert_eq!(result.tier, TierLevel::Tier1Generic);
    }

    #[test]
    fn test_programmatic_tier3_detection() {
        let assessor = ValueAssessor::new(
            Arc::new(MockProvider),
            ValueAssessmentConfig::default(),
        );

        let tier3_content = "MUST use Arc::clone - never create new instances. See @src/ai/mod.rs:42";
        let result = assessor.assess_programmatic(tier3_content);
        assert_eq!(result.tier, TierLevel::Tier3Constraint);
    }

    #[test]
    fn test_programmatic_tier2_default() {
        let assessor = ValueAssessor::new(
            Arc::new(MockProvider),
            ValueAssessmentConfig::default(),
        );

        let tier2_content = "Controllers are in src/adapter/inbound/web directory";
        let result = assessor.assess_programmatic(tier2_content);
        assert_eq!(result.tier, TierLevel::Tier2Convention);
    }
}
