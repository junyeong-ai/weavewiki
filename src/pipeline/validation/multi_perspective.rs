//! Multi-Perspective LLM Validation
//!
//! Validates generated artifacts from multiple independent perspectives:
//! 1. Quality Assessment - Is the content well-structured and useful?
//! 2. Hallucination Detection - Are claims verifiable or suspicious?
//! 3. Completeness Check - Is anything important missing?
//!
//! Requires all perspectives to pass for final approval.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ai::LlmProvider;
use crate::types::{Agent, ProjectMemory, Result, Rule, Skill};

/// Result from multi-perspective validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiPerspectiveResult {
    pub passed: bool,
    pub quality: PerspectiveResult,
    pub hallucination: HallucinationResult,
    pub completeness: CompletenessResult,
    pub combined_score: f32,
    pub blocking_issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerspectiveResult {
    pub passed: bool,
    pub score: f32,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HallucinationResult {
    pub passed: bool,
    pub suspicious_claims: Vec<SuspiciousClaim>,
    pub confidence_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspiciousClaim {
    pub artifact: String,
    pub claim: String,
    pub reason: String,
    pub severity: ClaimSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaimSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletenessResult {
    pub passed: bool,
    pub missing_topics: Vec<String>,
    pub coverage_score: f32,
}

pub struct MultiPerspectiveValidator {
    provider: Arc<dyn LlmProvider>,
    min_quality_score: f32,
    max_suspicious_claims: usize,
    min_coverage_score: f32,
}

impl MultiPerspectiveValidator {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            min_quality_score: 0.7,
            max_suspicious_claims: 0,
            min_coverage_score: 0.8,
        }
    }

    pub fn with_thresholds(
        mut self,
        min_quality: f32,
        max_suspicious: usize,
        min_coverage: f32,
    ) -> Self {
        self.min_quality_score = min_quality;
        self.max_suspicious_claims = max_suspicious;
        self.min_coverage_score = min_coverage;
        self
    }

    pub async fn validate(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
        project_context: &str,
    ) -> Result<MultiPerspectiveResult> {
        let content = self.collect_content(skills, agents, rules, claude_md);

        // Run all perspectives in parallel for efficiency
        let (quality, hallucination, completeness) = tokio::join!(
            self.assess_quality(&content, project_context),
            self.detect_hallucinations(&content, project_context),
            self.check_completeness(&content, project_context)
        );

        let quality = quality.unwrap_or_else(|_| PerspectiveResult {
            passed: false,
            score: 0.0,
            issues: vec!["Quality assessment failed".into()],
        });

        let hallucination = hallucination.unwrap_or_else(|_| HallucinationResult {
            passed: false,
            suspicious_claims: vec![],
            confidence_score: 0.0,
        });

        let completeness = completeness.unwrap_or_else(|_| CompletenessResult {
            passed: false,
            missing_topics: vec!["Completeness check failed".into()],
            coverage_score: 0.0,
        });

        let mut blocking_issues = Vec::new();
        if !quality.passed {
            blocking_issues.push(format!("Quality score {:.2} below threshold", quality.score));
        }
        if !hallucination.passed {
            blocking_issues.push(format!(
                "{} suspicious claims detected",
                hallucination.suspicious_claims.len()
            ));
        }
        if !completeness.passed {
            blocking_issues.push(format!(
                "Coverage {:.2} below threshold, missing: {}",
                completeness.coverage_score,
                completeness.missing_topics.join(", ")
            ));
        }

        let combined_score =
            (quality.score + hallucination.confidence_score + completeness.coverage_score) / 3.0;

        let passed = quality.passed && hallucination.passed && completeness.passed;

        Ok(MultiPerspectiveResult {
            passed,
            quality,
            hallucination,
            completeness,
            combined_score,
            blocking_issues,
        })
    }

    fn collect_content(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> String {
        let mut parts = vec![format!("=== CLAUDE.md ===\n{}", claude_md.to_markdown())];

        for skill in skills {
            parts.push(format!(
                "=== SKILL: {} ===\n{}\n{}",
                skill.name, skill.description, skill.body
            ));
        }

        for agent in agents {
            parts.push(format!(
                "=== AGENT: {} ===\n{}\n{}",
                agent.name, agent.description, agent.prompt
            ));
        }

        for rule in rules {
            let paths = rule
                .paths
                .as_ref()
                .map(|p| p.join(", "))
                .unwrap_or_default();
            parts.push(format!(
                "=== RULE: {} ===\nPaths: {}\n{}",
                rule.name,
                paths,
                rule.content.join("\n")
            ));
        }

        parts.join("\n\n")
    }

    async fn assess_quality(
        &self,
        content: &str,
        project_context: &str,
    ) -> Result<PerspectiveResult> {
        let prompt = format!(
            r#"Evaluate this AI coding assistant documentation for quality.

PROJECT: {project_context}

CONTENT:
{content}

EVALUATE (0.0-1.0):
1. Clarity: Are instructions unambiguous?
2. Actionability: Can an AI follow these to write code?
3. Specificity: Project-specific, not generic advice?
4. Structure: Well-organized with clear sections?

Return JSON:
{{
  "score": 0.0-1.0,
  "passed": true/false (score >= {threshold:.2}),
  "issues": ["issue1", "issue2"]
}}"#,
            project_context = project_context,
            content = content,
            threshold = self.min_quality_score,
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "score": {"type": "number"},
                "passed": {"type": "boolean"},
                "issues": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["score", "passed", "issues"]
        });

        let response = self.provider.generate(&prompt, &schema).await?;
        let score = response
            .content
            .get("score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        let passed = score >= self.min_quality_score;
        let issues = response
            .content
            .get("issues")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(PerspectiveResult {
            passed,
            score,
            issues,
        })
    }

    async fn detect_hallucinations(
        &self,
        content: &str,
        project_context: &str,
    ) -> Result<HallucinationResult> {
        let prompt = format!(
            r#"Analyze this documentation for potential hallucinations (unverifiable claims).

PROJECT: {project_context}

CONTENT:
{content}

IDENTIFY SUSPICIOUS CLAIMS:
Look for statements that:
- Reference files/functions that may not exist
- Make specific claims without @file:line evidence
- Describe behaviors that seem assumed rather than verified
- Use confident language about uncertain details

For each suspicious claim:
- artifact: Which document contains it
- claim: The specific suspicious statement
- reason: Why it's suspicious
- severity: critical/high/medium/low

Return JSON:
{{
  "confidence_score": 0.0-1.0 (how confident the content is verifiable),
  "suspicious_claims": [
    {{"artifact": "...", "claim": "...", "reason": "...", "severity": "..."}}
  ]
}}"#,
            project_context = project_context,
            content = content,
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "confidence_score": {"type": "number"},
                "suspicious_claims": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "artifact": {"type": "string"},
                            "claim": {"type": "string"},
                            "reason": {"type": "string"},
                            "severity": {"type": "string"}
                        },
                        "required": ["artifact", "claim", "reason", "severity"]
                    }
                }
            },
            "required": ["confidence_score", "suspicious_claims"]
        });

        let response = self.provider.generate(&prompt, &schema).await?;
        let confidence_score = response
            .content
            .get("confidence_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;

        let suspicious_claims: Vec<SuspiciousClaim> = response
            .content
            .get("suspicious_claims")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        Some(SuspiciousClaim {
                            artifact: item.get("artifact")?.as_str()?.to_string(),
                            claim: item.get("claim")?.as_str()?.to_string(),
                            reason: item.get("reason")?.as_str()?.to_string(),
                            severity: match item.get("severity")?.as_str()? {
                                "critical" => ClaimSeverity::Critical,
                                "high" => ClaimSeverity::High,
                                "medium" => ClaimSeverity::Medium,
                                _ => ClaimSeverity::Low,
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Filter to only critical/high severity for pass/fail
        let blocking_claims: Vec<_> = suspicious_claims
            .iter()
            .filter(|c| matches!(c.severity, ClaimSeverity::Critical | ClaimSeverity::High))
            .collect();

        let passed = blocking_claims.len() <= self.max_suspicious_claims;

        Ok(HallucinationResult {
            passed,
            suspicious_claims,
            confidence_score,
        })
    }

    async fn check_completeness(
        &self,
        content: &str,
        project_context: &str,
    ) -> Result<CompletenessResult> {
        let prompt = format!(
            r#"Check if this AI coding documentation covers essential topics.

PROJECT: {project_context}

CONTENT:
{content}

ESSENTIAL TOPICS for AI coding assistants:
1. Project purpose and architecture overview
2. Key directories and file organization
3. Build/run/test commands
4. Important patterns and conventions
5. Common pitfalls and gotchas
6. Key files to understand first

Identify what's MISSING that an AI would need to code effectively.

Return JSON:
{{
  "coverage_score": 0.0-1.0,
  "missing_topics": ["topic1", "topic2"]
}}"#,
            project_context = project_context,
            content = content,
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "coverage_score": {"type": "number"},
                "missing_topics": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["coverage_score", "missing_topics"]
        });

        let response = self.provider.generate(&prompt, &schema).await?;
        let coverage_score = response
            .content
            .get("coverage_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;

        let missing_topics = response
            .content
            .get("missing_topics")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let passed = coverage_score >= self.min_coverage_score;

        Ok(CompletenessResult {
            passed,
            missing_topics,
            coverage_score,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claim_severity() {
        assert!(matches!(ClaimSeverity::Critical, ClaimSeverity::Critical));
        assert!(matches!(ClaimSeverity::High, ClaimSeverity::High));
    }

    #[test]
    fn test_multi_perspective_result_default() {
        let result = MultiPerspectiveResult {
            passed: false,
            quality: PerspectiveResult {
                passed: false,
                score: 0.5,
                issues: vec![],
            },
            hallucination: HallucinationResult {
                passed: true,
                suspicious_claims: vec![],
                confidence_score: 0.9,
            },
            completeness: CompletenessResult {
                passed: true,
                missing_topics: vec![],
                coverage_score: 0.85,
            },
            combined_score: 0.75,
            blocking_issues: vec!["Quality score 0.50 below threshold".into()],
        };

        assert!(!result.passed);
        assert_eq!(result.blocking_issues.len(), 1);
    }
}
