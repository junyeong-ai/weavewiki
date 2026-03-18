use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::ai::LlmProvider;
use crate::ai::response::generate_schema;
use crate::ai::validation::deserialize_llm_response;
use crate::types::{Agent, ContentTier, Rule, Severity, Skill};

#[derive(Debug, Clone)]
pub struct JudgeConfig {
    pub min_quality_score: f32,
    pub min_reference_count: usize,
    pub tier_validation_enabled: bool,
    /// Maximum characters to include in content preview for LLM validation
    pub max_content_preview_chars: usize,
}

impl Default for JudgeConfig {
    fn default() -> Self {
        Self {
            min_quality_score: 0.7,
            min_reference_count: 0, // LLM determines reference sufficiency, not fixed count
            tier_validation_enabled: true,
            max_content_preview_chars: 4000, // Increased from hardcoded 2000
        }
    }
}

/// Tier validation result from LLM classification
#[derive(Debug, Clone)]
pub struct TierValidation {
    pub tier: ContentTier,
    pub confidence: f32,
}

/// Project context for enriched quality evaluation
#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    pub project_name: String,
    pub primary_language: Option<String>,
    pub module_names: Vec<String>,
}

pub struct LlmJudge {
    provider: Arc<dyn LlmProvider>,
    config: JudgeConfig,
    file_registry: Option<crate::pipeline::context::VerifiedFileRegistry>,
    project_context: Option<ProjectContext>,
}

impl LlmJudge {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            config: JudgeConfig::default(),
            file_registry: None,
            project_context: None,
        }
    }

    pub fn config(mut self, config: JudgeConfig) -> Self {
        self.config = config;
        self
    }

    pub fn file_registry(mut self, registry: crate::pipeline::context::VerifiedFileRegistry) -> Self {
        self.file_registry = Some(registry);
        self
    }

    pub fn project_context(mut self, ctx: ProjectContext) -> Self {
        self.project_context = Some(ctx);
        self
    }

    pub async fn evaluate_skill(&self, skill: &Skill) -> Result<JudgmentResult> {
        let content = format!(
            "Name: {}\nDescription: {}\nBody:\n{}",
            skill.name, skill.description, skill.body
        );
        self.evaluate(&skill.name, "Skill", &content).await
    }

    pub async fn evaluate_agent(&self, agent: &Agent) -> Result<JudgmentResult> {
        let content = format!(
            "Name: {}\nDescription: {}\nPrompt:\n{}",
            agent.name, agent.description, agent.prompt
        );
        self.evaluate(&agent.name, "Agent", &content).await
    }

    pub async fn evaluate_rule(&self, rule: &Rule) -> Result<JudgmentResult> {
        let content = format!("Name: {}\nContent:\n{}", rule.name, rule.content.join("\n"));
        self.evaluate(&rule.name, "Rule", &content).await
    }

    pub async fn evaluate(
        &self,
        name: &str,
        artifact_type: &str,
        content: &str,
    ) -> Result<JudgmentResult> {
        let prompt = self.build_prompt(name, artifact_type, content);
        let schema = self.schema();

        let response = self.provider.generate(&prompt, &schema).await?;
        self.parse_response_value(&response.content)
    }

    /// Evaluate each artifact individually, returning per-artifact results.
    pub async fn evaluate_all(&self, artifacts: &Artifacts) -> Result<Vec<JudgmentResult>> {
        let mut results = Vec::with_capacity(
            artifacts.skills.len() + artifacts.agents.len() + artifacts.rules.len(),
        );

        for skill in &artifacts.skills {
            results.push(self.evaluate_skill(skill).await?);
        }
        for agent in &artifacts.agents {
            results.push(self.evaluate_agent(agent).await?);
        }
        for rule in &artifacts.rules {
            results.push(self.evaluate_rule(rule).await?);
        }

        Ok(results)
    }

    pub async fn evaluate_artifacts(&self, artifacts: &Artifacts) -> Result<JudgmentResult> {
        let mut all_issues = Vec::new();
        let mut all_suggestions = Vec::new();
        let mut scores = Vec::new();
        let mut tiers = Vec::new();

        for skill in &artifacts.skills {
            let result = self.evaluate_skill(skill).await?;
            scores.push(result.overall_score);
            tiers.push(result.tier);
            all_issues.extend(result.issues);
            all_suggestions.extend(result.suggestions);
        }

        for agent in &artifacts.agents {
            let result = self.evaluate_agent(agent).await?;
            scores.push(result.overall_score);
            tiers.push(result.tier);
            all_issues.extend(result.issues);
            all_suggestions.extend(result.suggestions);
        }

        for rule in &artifacts.rules {
            let result = self.evaluate_rule(rule).await?;
            scores.push(result.overall_score);
            tiers.push(result.tier);
            all_issues.extend(result.issues);
            all_suggestions.extend(result.suggestions);
        }

        let overall_score = if scores.is_empty() {
            0.0
        } else {
            scores.iter().sum::<f32>() / scores.len() as f32
        };

        let tier = tiers
            .iter()
            .min_by_key(|t| match t {
                ContentTier::Tier0Hallucinated => 0,
                ContentTier::Tier1Generic => 1,
                ContentTier::Tier2Convention => 2,
                ContentTier::Tier3Constraint => 3,
            })
            .copied()
            .unwrap_or(ContentTier::Tier1Generic);

        Ok(JudgmentResult {
            overall_score,
            tier,
            issues: all_issues,
            suggestions: all_suggestions,
            value_assessment: None,
        })
    }

    fn build_prompt(&self, name: &str, artifact_type: &str, content: &str) -> String {
        format!(
            r#"## Quality Judgment

### Artifact
**Type**: {artifact_type}
**Name**: {name}

```
{content}
```

### Evaluation Criteria

1. **Tier Classification**:
   - Tier 1: Generic knowledge - "Use async/await", "Handle errors"
   - Tier 2: Project conventions - "Controllers in adapter/inbound/web"
   - Tier 3: Hidden constraints - Non-obvious gotchas, race conditions, initialization order

2. **Actionability**: Clear specific actions with project-specific context?

3. **Evidence**: Valid @file:line references? Minimum {min_refs} expected.

### Output JSON
```json
{{
  "tier": 1-3,
  "quality_score": 0.0-1.0,
  "issues": [{{"code": "...", "message": "...", "severity": "critical|major|minor", "evidence": ["..."]}}],
  "suggestions": [{{"action": "...", "rationale": "...", "priority": "high|medium|low"}}]
}}
```"#,
            min_refs = self.config.min_reference_count
        )
    }

    fn schema(&self) -> serde_json::Value {
        generate_schema::<JudgmentOutput>()
    }

    fn parse_response_value(&self, value: &serde_json::Value) -> Result<JudgmentResult> {
        let output: JudgmentOutput = deserialize_llm_response(value, "judge")?;

        let tier = match output.tier {
            1 => ContentTier::Tier1Generic,
            2 => ContentTier::Tier2Convention,
            _ => ContentTier::Tier3Constraint,
        };

        let issues = output
            .issues
            .into_iter()
            .filter(|i| !i.code.is_empty())
            .map(|i| QualityIssue {
                code: i.code,
                message: i.message,
                severity: match i.severity.as_str() {
                    "critical" => IssueSeverity::Critical,
                    "major" => IssueSeverity::Major,
                    _ => IssueSeverity::Minor,
                },
                evidence: i.evidence,
            })
            .collect();

        let suggestions = output
            .suggestions
            .into_iter()
            .filter(|s| !s.action.is_empty())
            .map(|s| Suggestion {
                action: s.action,
                rationale: s.rationale,
                priority: match s.priority.as_str() {
                    "high" => Severity::High,
                    "medium" => Severity::Medium,
                    _ => Severity::Low,
                },
            })
            .collect();

        Ok(JudgmentResult {
            overall_score: output.quality_score,
            tier,
            issues,
            suggestions,
            value_assessment: None,
        })
    }

    /// Aggregate per-artifact judgment results into a single summary.
    pub fn aggregate_results(results: &[JudgmentResult]) -> JudgmentResult {
        if results.is_empty() {
            return JudgmentResult {
                overall_score: 0.0,
                tier: ContentTier::Tier1Generic,
                issues: Vec::new(),
                suggestions: Vec::new(),
                value_assessment: None,
            };
        }

        let scores: Vec<f32> = results.iter().map(|r| r.overall_score).collect();
        let overall_score = scores.iter().sum::<f32>() / scores.len() as f32;

        let tier = results
            .iter()
            .map(|r| r.tier)
            .min_by_key(|t| match t {
                ContentTier::Tier0Hallucinated => 0,
                ContentTier::Tier1Generic => 1,
                ContentTier::Tier2Convention => 2,
                ContentTier::Tier3Constraint => 3,
            })
            .unwrap_or(ContentTier::Tier1Generic);

        let issues: Vec<QualityIssue> =
            results.iter().flat_map(|r| r.issues.clone()).collect();
        let suggestions: Vec<Suggestion> =
            results.iter().flat_map(|r| r.suggestions.clone()).collect();

        JudgmentResult {
            overall_score,
            tier,
            issues,
            suggestions,
            value_assessment: None,
        }
    }

    /// LLM-based tier validation
    pub async fn validate_tier(&self, content: &str) -> Result<TierValidation> {
        if !self.config.tier_validation_enabled {
            return Ok(TierValidation {
                tier: ContentTier::Tier2Convention,
                confidence: 0.5,
            });
        }

        let prompt = format!(
            r#"Classify this content into exactly ONE tier based on its value:

CONTENT:
```
{content}
```

TIER DEFINITIONS:
- Tier 1 (Generic): Universal programming knowledge anyone could look up
  Examples: "Use async/await", "Handle errors gracefully", "Follow naming conventions"

- Tier 2 (Convention): Project-specific patterns, file organization, naming conventions
  Examples: "Controllers in adapter/inbound/web", "Use repository pattern for data access"

- Tier 3 (Constraint): Hidden knowledge that causes bugs or issues if missed
  Examples: Specific race conditions, initialization order dependencies, non-obvious side effects

OUTPUT: Single integer 1, 2, or 3"#,
            content = content
                .chars()
                .take(self.config.max_content_preview_chars)
                .collect::<String>()
        );

        let schema = generate_schema::<TierOutput>();

        let response = self.provider.generate(&prompt, &schema).await?;
        let output: TierOutput = deserialize_llm_response(&response.content, "tier_validation")?;
        let tier_num = output.tier;

        let tier = match tier_num {
            1 => ContentTier::Tier1Generic,
            2 => ContentTier::Tier2Convention,
            _ => ContentTier::Tier3Constraint,
        };

        Ok(TierValidation {
            tier,
            confidence: 0.85,
        })
    }
}

#[derive(Default)]
pub struct Artifacts {
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub rules: Vec<Rule>,
}

impl From<&crate::types::artifacts::GeneratedArtifacts> for Artifacts {
    fn from(ga: &crate::types::artifacts::GeneratedArtifacts) -> Self {
        Self {
            skills: ga.skills.clone(),
            agents: ga.agents.clone(),
            rules: ga.rules.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentResult {
    pub overall_score: f32,
    pub tier: ContentTier,
    pub issues: Vec<QualityIssue>,
    pub suggestions: Vec<Suggestion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_assessment: Option<ValueAssessment>,
}

/// Per-dimension value scores from LLM judgment.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ValueAssessment {
    pub actionability: f32,
    pub domain_specificity: f32,
    pub information_density: f32,
}

impl JudgmentResult {
    /// Check if content is acceptable based on tier and quality score.
    /// Tier classification is informational - LLM judgment on quality is authoritative.
    /// Only Tier0 (hallucinated/invalid) content is rejected outright.
    pub fn is_acceptable(&self, min_score: f32) -> bool {
        match self.tier {
            ContentTier::Tier0Hallucinated => false, // Invalid content always rejected
            // Tier1/2/3 all use same threshold - LLM determines appropriateness
            _ => self.overall_score >= min_score,
        }
    }

    pub fn critical_issues(&self) -> Vec<&QualityIssue> {
        self.issues
            .iter()
            .filter(|i| matches!(i.severity, IssueSeverity::Critical))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualityIssue {
    pub code: String,
    pub message: String,
    pub severity: IssueSeverity,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Critical,
    Major,
    Minor,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Suggestion {
    pub action: String,
    pub rationale: String,
    pub priority: Severity,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct JudgmentOutput {
    #[serde(default)]
    tier: u8,
    #[serde(default)]
    quality_score: f32,
    #[serde(default)]
    issues: Vec<IssueOutput>,
    #[serde(default)]
    suggestions: Vec<SuggestionOutput>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct IssueOutput {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct SuggestionOutput {
    #[serde(default)]
    action: String,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    priority: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct TierOutput {
    #[serde(default)]
    tier: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_judgment_result_acceptable() {
        let result = JudgmentResult {
            overall_score: 0.8,
            tier: ContentTier::Tier2Convention,
            issues: vec![],
            suggestions: vec![],
            value_assessment: None,
        };
        assert!(result.is_acceptable(0.7));
        assert!(!result.is_acceptable(0.9));
    }

    #[test]
    fn test_tier1_uses_standard_threshold() {
        let high_quality = JudgmentResult {
            overall_score: 0.95,
            tier: ContentTier::Tier1Generic,
            issues: vec![],
            suggestions: vec![],
            value_assessment: None,
        };
        assert!(high_quality.is_acceptable(0.5));

        let above_threshold = JudgmentResult {
            overall_score: 0.85,
            tier: ContentTier::Tier1Generic,
            issues: vec![],
            suggestions: vec![],
            value_assessment: None,
        };
        assert!(above_threshold.is_acceptable(0.5));
        assert!(!above_threshold.is_acceptable(0.9));

        let hallucinated = JudgmentResult {
            overall_score: 0.99,
            tier: ContentTier::Tier0Hallucinated,
            issues: vec![],
            suggestions: vec![],
            value_assessment: None,
        };
        assert!(!hallucinated.is_acceptable(0.5));
    }
}
