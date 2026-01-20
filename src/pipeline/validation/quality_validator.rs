//! Unified Quality Validator
//!
//! Single AI-based validator evaluating from AI coding assistant perspective.
//! Combines self-review questions with quality dimensions in one LLM call.

use std::sync::Arc;

use crate::ai::LlmProvider;
use crate::config::{QualityConfig, SemanticValidationConfig};
use crate::types::{Agent, ProjectMemory, Result, Rule, Skill};

use super::semantic_validator::{
    IssueCategory, IssueSeverity, SemanticIssue, SemanticQualityResult, SemanticScore,
};

#[derive(Debug, Clone)]
pub struct QualityThresholds {
    pub min_overall: f32,
    pub min_actionability: f32,
    pub min_specificity: f32,
    pub min_evidence: f32,
    pub min_depth: f32,
    pub max_redundancy: f32,
}

impl QualityThresholds {
    /// Create from full quality config (preferred method)
    pub fn from_quality_config(config: &QualityConfig) -> Self {
        Self {
            min_overall: config.min_overall_score,
            min_actionability: config.semantic.min_actionability,
            min_specificity: config.semantic.min_specificity,
            min_evidence: config.semantic.min_evidence_quality,
            min_depth: config.semantic.min_depth,
            max_redundancy: config.semantic.max_redundancy,
        }
    }
}

impl From<&SemanticValidationConfig> for QualityThresholds {
    fn from(config: &SemanticValidationConfig) -> Self {
        Self {
            min_overall: QualityConfig::default().min_overall_score,
            min_actionability: config.min_actionability,
            min_specificity: config.min_specificity,
            min_evidence: config.min_evidence_quality,
            min_depth: config.min_depth,
            max_redundancy: config.max_redundancy,
        }
    }
}

impl Default for QualityThresholds {
    fn default() -> Self {
        let config = QualityConfig::default();
        Self {
            min_overall: config.min_overall_score,
            min_actionability: config.semantic.min_actionability,
            min_specificity: config.semantic.min_specificity,
            min_evidence: config.semantic.min_evidence_quality,
            min_depth: config.semantic.min_depth,
            max_redundancy: config.semantic.max_redundancy,
        }
    }
}

pub struct QualityValidator {
    provider: Arc<dyn LlmProvider>,
    thresholds: QualityThresholds,
}

impl QualityValidator {
    pub fn new(provider: Arc<dyn LlmProvider>, config: &SemanticValidationConfig) -> Self {
        Self {
            provider,
            thresholds: QualityThresholds::from(config),
        }
    }

    pub async fn validate(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
        project_context: &str,
    ) -> Result<SemanticQualityResult> {
        let content = self.collect_content(skills, agents, rules, claude_md);
        let prompt = self.build_prompt(&content, project_context);

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "overall_score": {"type": "number"},
                "actionability": {"type": "number"},
                "specificity": {"type": "number"},
                "evidence": {"type": "number"},
                "depth": {"type": "number"},
                "redundancy": {"type": "number"},
                "issues": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "target": {"type": "string"},
                            "category": {"type": "string"},
                            "description": {"type": "string"},
                            "severity": {"type": "string"}
                        },
                        "required": ["target", "category", "description", "severity"]
                    }
                },
                "suggestions": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["overall_score", "actionability", "specificity", "evidence", "depth", "redundancy", "issues", "suggestions"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => self.parse_response(&response.content),
            Err(e) => {
                tracing::warn!(error = %e, "Quality validation failed");
                Ok(self.default_result())
            }
        }
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
            parts.push(format!("=== SKILL: {} ===\n{}\n{}", skill.name, skill.description, skill.body));
        }

        for agent in agents {
            parts.push(format!("=== AGENT: {} ===\n{}\n{}", agent.name, agent.description, agent.prompt));
        }

        for rule in rules {
            let paths = rule.paths.as_ref().map(|p| p.join(", ")).unwrap_or_default();
            parts.push(format!("=== RULE: {} ===\nPaths: {}\n{}", rule.name, paths, rule.content.join("\n")));
        }

        parts.join("\n\n")
    }

    fn build_prompt(&self, content: &str, project_context: &str) -> String {
        let t = &self.thresholds;
        format!(
            r##"You are an AI coding assistant evaluating documentation written FOR AI coding assistants.

PROJECT CONTEXT:
{project_context}

CONTENT:
{content}

EVALUATE AS IF YOU WILL USE THIS TO CODE:

SELF-REVIEW (critical questions):
1. CLAUDE.md: Would I know what this project does in 30 seconds? Architecture clear? Commands listed?
2. SKILLS: Are these tasks I'd ACTUALLY do (add feature, fix bug)? Or internal workflows (NOT useful)?
3. AGENTS: Project-specific knowledge? Or generic roles any LLM knows (NOT useful)?
4. RULES: Add value beyond CLAUDE.md? Project-specific, not generic advice?

QUALITY DIMENSIONS (0.0-1.0):
- actionability (min {actionability:.2}): Clear "do this" / "avoid that" directives
- specificity (min {specificity:.2}): Project-specific, not generic advice
- evidence (min {evidence:.2}): @file:line references to actual code
- depth (min {depth:.2}): WHY explained, not just WHAT
- redundancy (max {redundancy:.2}): Lower is better, no duplication

SCORING (be STRICT):
- 0.0-0.3: Useless - would confuse me
- 0.4-0.5: Marginal - mostly generic
- 0.6-0.7: Adequate - some useful info
- 0.8-0.9: Good - can work effectively
- 0.9-1.0: Excellent - know exactly what to do

ISSUES: For each problem found:
- target: Which artifact (CLAUDE.md, skill name, agent name, rule name)
- category: low_actionability|too_generic|weak_evidence|redundant|shallow|missing_reference
- description: What's wrong
- severity: critical|high|medium|low

Return JSON."##,
            project_context = project_context,
            content = content,
            actionability = t.min_actionability,
            specificity = t.min_specificity,
            evidence = t.min_evidence,
            depth = t.min_depth,
            redundancy = t.max_redundancy,
        )
    }

    fn parse_response(&self, content: &serde_json::Value) -> Result<SemanticQualityResult> {
        let overall = content.get("overall_score").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let actionability = content.get("actionability").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let specificity = content.get("specificity").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let evidence = content.get("evidence").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let depth = content.get("depth").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let redundancy = content.get("redundancy").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;

        let t = &self.thresholds;

        let issues = self.parse_issues(content.get("issues"));
        let suggestions = content
            .get("suggestions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        Ok(SemanticQualityResult {
            passed: overall >= t.min_overall,
            overall_score: overall,
            actionability: SemanticScore {
                score: actionability,
                passed: actionability >= t.min_actionability,
                details: format!("Score: {:.2}", actionability),
            },
            specificity: SemanticScore {
                score: specificity,
                passed: specificity >= t.min_specificity,
                details: format!("Score: {:.2}", specificity),
            },
            evidence_quality: SemanticScore {
                score: evidence,
                passed: evidence >= t.min_evidence,
                details: format!("Score: {:.2}", evidence),
            },
            redundancy: SemanticScore {
                score: redundancy,
                passed: redundancy <= t.max_redundancy,
                details: format!("Score: {:.2}", redundancy),
            },
            depth: SemanticScore {
                score: depth,
                passed: depth >= t.min_depth,
                details: format!("Score: {:.2}", depth),
            },
            issues,
            suggestions,
        })
    }

    fn parse_issues(&self, value: Option<&serde_json::Value>) -> Vec<SemanticIssue> {
        value
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        Some(SemanticIssue {
                            target: item.get("target")?.as_str()?.to_string(),
                            category: self.parse_category(item.get("category")?.as_str()?),
                            description: item.get("description")?.as_str()?.to_string(),
                            severity: self.parse_severity(item.get("severity")?.as_str()?),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn parse_category(&self, s: &str) -> IssueCategory {
        match s.to_lowercase().replace('-', "_").as_str() {
            "low_actionability" => IssueCategory::LowActionability,
            "too_generic" => IssueCategory::TooGeneric,
            "weak_evidence" | "missing_reference" => IssueCategory::WeakEvidence,
            "redundant" => IssueCategory::Redundant,
            "shallow" => IssueCategory::Shallow,
            _ => IssueCategory::LowActionability,
        }
    }

    fn parse_severity(&self, s: &str) -> IssueSeverity {
        match s.to_lowercase().as_str() {
            "critical" => IssueSeverity::Critical,
            "high" => IssueSeverity::High,
            "medium" => IssueSeverity::Medium,
            _ => IssueSeverity::Low,
        }
    }

    fn default_result(&self) -> SemanticQualityResult {
        SemanticQualityResult {
            passed: false,
            overall_score: 0.0,
            actionability: SemanticScore { score: 0.0, passed: false, details: "Validation failed".into() },
            specificity: SemanticScore { score: 0.0, passed: false, details: "Validation failed".into() },
            evidence_quality: SemanticScore { score: 0.0, passed: false, details: "Validation failed".into() },
            redundancy: SemanticScore { score: 0.5, passed: false, details: "Validation failed".into() },
            depth: SemanticScore { score: 0.0, passed: false, details: "Validation failed".into() },
            issues: vec![SemanticIssue {
                target: "validation".to_string(),
                category: IssueCategory::LowActionability,
                description: "Quality validation unavailable".to_string(),
                severity: IssueSeverity::High,
            }],
            suggestions: Vec::new(),
        }
    }
}
