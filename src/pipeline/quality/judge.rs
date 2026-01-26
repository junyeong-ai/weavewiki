use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::Result;
use crate::ai::LlmProvider;
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

pub struct LlmJudge {
    provider: Arc<dyn LlmProvider>,
    config: JudgeConfig,
}

impl LlmJudge {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            config: JudgeConfig::default(),
        }
    }

    pub fn with_config(mut self, config: JudgeConfig) -> Self {
        self.config = config;
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
        self.parse_response(&response.content.to_string())
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
        json!({
            "type": "object",
            "properties": {
                "tier": { "type": "integer", "minimum": 1, "maximum": 3 },
                "quality_score": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                "issues": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "code": { "type": "string" },
                            "message": { "type": "string" },
                            "severity": { "type": "string", "enum": ["critical", "major", "minor"] },
                            "evidence": { "type": "array", "items": { "type": "string" } }
                        }
                    }
                },
                "suggestions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "action": { "type": "string" },
                            "rationale": { "type": "string" },
                            "priority": { "type": "string", "enum": ["low", "medium", "high"] }
                        }
                    }
                }
            },
            "required": ["tier", "quality_score", "issues", "suggestions"]
        })
    }

    fn parse_response(&self, content: &str) -> Result<JudgmentResult> {
        let parsed: serde_json::Value = serde_json::from_str(content).unwrap_or_else(|e| {
            tracing::warn!(
                error = %e,
                content_preview = %content.chars().take(200).collect::<String>(),
                "Failed to parse LLM response as JSON, using defaults"
            );
            json!({
                "tier": 1,
                "quality_score": 0.0,
                "issues": [],
                "suggestions": []
            })
        });

        let tier_num = parsed.get("tier").and_then(|v| v.as_i64()).unwrap_or(1) as u8;
        let tier = match tier_num {
            1 => ContentTier::Tier1Generic,
            2 => ContentTier::Tier2Convention,
            _ => ContentTier::Tier3Constraint,
        };

        let quality_score = parsed
            .get("quality_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;

        let issues: Vec<QualityIssue> = parsed
            .get("issues")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| {
                        Some(QualityIssue {
                            code: i.get("code")?.as_str()?.to_string(),
                            message: i.get("message")?.as_str()?.to_string(),
                            severity: match i.get("severity")?.as_str()? {
                                "critical" => IssueSeverity::Critical,
                                "major" => IssueSeverity::Major,
                                _ => IssueSeverity::Minor,
                            },
                            evidence: i
                                .get("evidence")
                                .and_then(|e| e.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let suggestions: Vec<Suggestion> = parsed
            .get("suggestions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| {
                        Some(Suggestion {
                            action: i.get("action")?.as_str()?.to_string(),
                            rationale: i.get("rationale")?.as_str()?.to_string(),
                            priority: match i.get("priority")?.as_str()? {
                                "high" => Severity::High,
                                "medium" => Severity::Medium,
                                _ => Severity::Low,
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(JudgmentResult {
            overall_score: quality_score,
            tier,
            issues,
            suggestions,
        })
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

        let schema = json!({
            "type": "object",
            "properties": {
                "tier": { "type": "integer", "minimum": 1, "maximum": 3 }
            },
            "required": ["tier"]
        });

        let response = self.provider.generate(&prompt, &schema).await?;
        let tier_num = response
            .content
            .get("tier")
            .and_then(|v| v.as_i64())
            .unwrap_or(2) as u8;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgmentResult {
    pub overall_score: f32,
    pub tier: ContentTier,
    pub issues: Vec<QualityIssue>,
    pub suggestions: Vec<Suggestion>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityIssue {
    pub code: String,
    pub message: String,
    pub severity: IssueSeverity,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Critical,
    Major,
    Minor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub action: String,
    pub rationale: String,
    pub priority: Severity,
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
        };
        assert!(result.is_acceptable(0.7));
        assert!(!result.is_acceptable(0.9));
    }

    #[test]
    fn test_tier1_uses_standard_threshold() {
        // Tier1 is now treated the same as other tiers - LLM judgment is authoritative
        // High quality Tier1 is acceptable
        let high_quality = JudgmentResult {
            overall_score: 0.95,
            tier: ContentTier::Tier1Generic,
            issues: vec![],
            suggestions: vec![],
        };
        assert!(high_quality.is_acceptable(0.5));

        // Tier1 above min_score is acceptable (no special 0.9 threshold)
        let above_threshold = JudgmentResult {
            overall_score: 0.85,
            tier: ContentTier::Tier1Generic,
            issues: vec![],
            suggestions: vec![],
        };
        assert!(above_threshold.is_acceptable(0.5)); // Now passes - LLM determined quality
        assert!(!above_threshold.is_acceptable(0.9)); // Below 0.9 threshold

        // Tier0 (hallucinated) is always rejected
        let hallucinated = JudgmentResult {
            overall_score: 0.99,
            tier: ContentTier::Tier0Hallucinated,
            issues: vec![],
            suggestions: vec![],
        };
        assert!(!hallucinated.is_acceptable(0.5)); // Always rejected
    }
}
