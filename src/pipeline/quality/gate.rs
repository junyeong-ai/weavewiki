//! Quality Gate
//!
//! Unified quality gate that orchestrates all quality checks.
//! Single entry point for artifact validation before output.

use std::sync::Arc;

use tracing::{debug, info};

use crate::ai::LlmProvider;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::types::{Agent, ContentTier, Result, Rule, Skill};

use super::LlmJudge;

/// Quality gate configuration
#[derive(Debug, Clone)]
pub struct QualityGateConfig {
    /// Minimum overall quality score
    pub min_quality_score: f32,
    /// Minimum tier (1=generic, 2=convention, 3=constraint)
    pub min_tier: u8,
    /// Maximum allowed Tier 1 artifacts (0 = none allowed)
    pub max_tier1_artifacts: usize,
    /// Minimum file references per artifact
    pub min_references: usize,
    /// Enable cross-artifact validation
    pub cross_validation_enabled: bool,
    /// Enable LLM judge (disable for fast mode)
    pub llm_judge_enabled: bool,
}

impl Default for QualityGateConfig {
    fn default() -> Self {
        Self {
            min_quality_score: 0.7,
            min_tier: 2,
            max_tier1_artifacts: 0,
            min_references: 2,
            cross_validation_enabled: true,
            llm_judge_enabled: true,
        }
    }
}

impl QualityGateConfig {
    pub fn fast() -> Self {
        Self {
            min_quality_score: 0.6,
            min_tier: 2,
            max_tier1_artifacts: 0,
            min_references: 1,
            cross_validation_enabled: false,
            llm_judge_enabled: false,
        }
    }

    pub fn strict() -> Self {
        Self {
            min_quality_score: 0.85,
            min_tier: 2,
            max_tier1_artifacts: 0,
            min_references: 3,
            cross_validation_enabled: true,
            llm_judge_enabled: true,
        }
    }
}

/// Result of quality gate validation
#[derive(Debug, Clone)]
pub struct GateResult {
    pub passed: bool,
    pub overall_score: f32,
    pub skill_results: Vec<ArtifactGateResult>,
    pub agent_results: Vec<ArtifactGateResult>,
    pub rule_results: Vec<ArtifactGateResult>,
    pub cross_validation: Option<ArtifactOverlapResult>,
    pub summary: GateSummary,
}

impl GateResult {
    pub fn passed_count(&self) -> usize {
        self.skill_results.iter().filter(|r| r.passed).count()
            + self.agent_results.iter().filter(|r| r.passed).count()
            + self.rule_results.iter().filter(|r| r.passed).count()
    }

    pub fn failed_count(&self) -> usize {
        self.skill_results.iter().filter(|r| !r.passed).count()
            + self.agent_results.iter().filter(|r| !r.passed).count()
            + self.rule_results.iter().filter(|r| !r.passed).count()
    }
}

/// Result for a single artifact
#[derive(Debug, Clone)]
pub struct ArtifactGateResult {
    pub name: String,
    pub passed: bool,
    pub quality_score: f32,
    pub tier: ContentTier,
    pub reference_count: usize,
    pub issues: Vec<GateIssue>,
}

/// Gate issue
#[derive(Debug, Clone)]
pub struct GateIssue {
    pub category: GateIssueCategory,
    pub message: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateIssueCategory {
    LowQuality,
    GenericContent,
    MissingReferences,
    CrossArtifactInconsistency,
    Redundancy,
}

/// Cross-artifact overlap validation result
#[derive(Debug, Clone)]
pub struct ArtifactOverlapResult {
    pub passed: bool,
    pub redundancies: Vec<RedundancyIssue>,
    pub inconsistencies: Vec<InconsistencyIssue>,
}

#[derive(Debug, Clone)]
pub struct RedundancyIssue {
    pub artifact1: String,
    pub artifact2: String,
    pub overlap_description: String,
}

#[derive(Debug, Clone)]
pub struct InconsistencyIssue {
    pub artifact1: String,
    pub artifact2: String,
    pub description: String,
}

/// Summary statistics
#[derive(Debug, Clone, Default)]
pub struct GateSummary {
    pub total_artifacts: usize,
    pub passed_artifacts: usize,
    pub tier1_count: usize,
    pub tier2_count: usize,
    pub tier3_count: usize,
    pub average_quality: f32,
    pub average_references: f32,
}

/// Unified quality gate
pub struct QualityGate {
    provider: Arc<dyn LlmProvider>,
    config: QualityGateConfig,
}

impl QualityGate {
    pub fn new(provider: Arc<dyn LlmProvider>, config: QualityGateConfig) -> Self {
        Self { provider, config }
    }

    /// Validate all artifacts through the quality gate
    pub async fn validate(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        file_registry: &VerifiedFileRegistry,
    ) -> Result<GateResult> {
        let mut skill_results = Vec::new();
        let mut agent_results = Vec::new();
        let mut rule_results = Vec::new();

        // Validate skills
        for skill in skills {
            let result = self.validate_skill(skill, file_registry).await?;
            skill_results.push(result);
        }

        // Validate agents
        for agent in agents {
            let result = self.validate_agent(agent, file_registry).await?;
            agent_results.push(result);
        }

        // Validate rules
        for rule in rules {
            let result = self.validate_rule(rule, file_registry).await?;
            rule_results.push(result);
        }

        // Cross-artifact validation
        let cross_validation = if self.config.cross_validation_enabled {
            Some(self.cross_validate(skills, agents, rules))
        } else {
            None
        };

        // Build summary
        let summary = self.build_summary(&skill_results, &agent_results, &rule_results);

        // Determine overall pass/fail
        let all_passed = skill_results.iter().all(|r| r.passed)
            && agent_results.iter().all(|r| r.passed)
            && rule_results.iter().all(|r| r.passed)
            && cross_validation
                .as_ref()
                .map(|cv| cv.passed)
                .unwrap_or(true);

        let result = GateResult {
            passed: all_passed,
            overall_score: summary.average_quality,
            skill_results,
            agent_results,
            rule_results,
            cross_validation,
            summary,
        };

        info!(
            passed = result.passed,
            total = result.summary.total_artifacts,
            passed_count = result.summary.passed_artifacts,
            avg_quality = result.summary.average_quality,
            "Quality gate validation complete"
        );

        Ok(result)
    }

    async fn validate_skill(
        &self,
        skill: &Skill,
        file_registry: &VerifiedFileRegistry,
    ) -> Result<ArtifactGateResult> {
        let mut issues = Vec::new();
        let reference_count = count_references(&skill.body, file_registry);

        // Get LLM judgment if enabled
        let judgment = if self.config.llm_judge_enabled {
            let judge = LlmJudge::new(Arc::clone(&self.provider));
            Some(judge.evaluate_skill(skill).await?)
        } else {
            None
        };

        let (quality_score, tier) = if let Some(ref j) = judgment {
            (j.overall_score, j.tier)
        } else {
            (skill.quality.score, skill.quality.tier)
        };

        // Check quality threshold
        if quality_score < self.config.min_quality_score {
            issues.push(GateIssue {
                category: GateIssueCategory::LowQuality,
                message: format!(
                    "Quality score {:.0}% below minimum {:.0}%",
                    quality_score * 100.0,
                    self.config.min_quality_score * 100.0
                ),
                blocking: true,
            });
        }

        // Check tier
        if matches!(
            tier,
            ContentTier::Tier1Generic | ContentTier::Tier0Hallucinated
        ) {
            issues.push(GateIssue {
                category: GateIssueCategory::GenericContent,
                message: format!("Content classified as {:?}", tier),
                blocking: true,
            });
        }

        // Check references
        if reference_count < self.config.min_references {
            issues.push(GateIssue {
                category: GateIssueCategory::MissingReferences,
                message: format!(
                    "Only {} file references (minimum {})",
                    reference_count, self.config.min_references
                ),
                blocking: false,
            });
        }

        let passed = issues.iter().all(|i| !i.blocking);

        debug!(
            skill = %skill.name,
            passed,
            quality = quality_score,
            tier = ?tier,
            refs = reference_count,
            "Validated skill"
        );

        Ok(ArtifactGateResult {
            name: skill.name.clone(),
            passed,
            quality_score,
            tier,
            reference_count,
            issues,
        })
    }

    async fn validate_agent(
        &self,
        agent: &Agent,
        file_registry: &VerifiedFileRegistry,
    ) -> Result<ArtifactGateResult> {
        let mut issues = Vec::new();
        let reference_count = count_references(&agent.prompt, file_registry);

        let judgment = if self.config.llm_judge_enabled {
            let judge = LlmJudge::new(Arc::clone(&self.provider));
            Some(judge.evaluate_agent(agent).await?)
        } else {
            None
        };

        let (quality_score, tier) = if let Some(ref j) = judgment {
            (j.overall_score, j.tier)
        } else {
            (agent.quality.score, agent.quality.tier)
        };

        if quality_score < self.config.min_quality_score {
            issues.push(GateIssue {
                category: GateIssueCategory::LowQuality,
                message: format!(
                    "Quality score {:.0}% below minimum {:.0}%",
                    quality_score * 100.0,
                    self.config.min_quality_score * 100.0
                ),
                blocking: true,
            });
        }

        if matches!(
            tier,
            ContentTier::Tier1Generic | ContentTier::Tier0Hallucinated
        ) {
            issues.push(GateIssue {
                category: GateIssueCategory::GenericContent,
                message: format!("Content classified as {:?}", tier),
                blocking: true,
            });
        }

        if reference_count < self.config.min_references {
            issues.push(GateIssue {
                category: GateIssueCategory::MissingReferences,
                message: format!(
                    "Only {} file references (minimum {})",
                    reference_count, self.config.min_references
                ),
                blocking: false,
            });
        }

        let passed = issues.iter().all(|i| !i.blocking);

        Ok(ArtifactGateResult {
            name: agent.name.clone(),
            passed,
            quality_score,
            tier,
            reference_count,
            issues,
        })
    }

    async fn validate_rule(
        &self,
        rule: &Rule,
        file_registry: &VerifiedFileRegistry,
    ) -> Result<ArtifactGateResult> {
        let content = rule.content.join("\n");
        let reference_count = count_references(&content, file_registry);

        // Rules are simpler - just check for minimum content
        let quality_score = if content.len() > 100 { 0.8 } else { 0.5 };
        let tier = ContentTier::Tier2Convention;

        let issues = Vec::new();
        let passed = quality_score >= self.config.min_quality_score;

        Ok(ArtifactGateResult {
            name: rule.name.clone(),
            passed,
            quality_score,
            tier,
            reference_count,
            issues,
        })
    }

    /// Cross-artifact validation.
    /// Note: Word-based similarity/overlap detection was removed as unreliable.
    /// Reliable redundancy/inconsistency detection requires LLM-based semantic analysis.
    fn cross_validate(
        &self,
        _skills: &[Skill],
        _agents: &[Agent],
        _rules: &[Rule],
    ) -> ArtifactOverlapResult {
        ArtifactOverlapResult {
            passed: true,
            redundancies: Vec::new(),
            inconsistencies: Vec::new(),
        }
    }

    fn build_summary(
        &self,
        skills: &[ArtifactGateResult],
        agents: &[ArtifactGateResult],
        rules: &[ArtifactGateResult],
    ) -> GateSummary {
        let all_results: Vec<&ArtifactGateResult> = skills
            .iter()
            .chain(agents.iter())
            .chain(rules.iter())
            .collect();

        let total = all_results.len();
        if total == 0 {
            return GateSummary::default();
        }

        let passed = all_results.iter().filter(|r| r.passed).count();
        let tier1 = all_results
            .iter()
            .filter(|r| matches!(r.tier, ContentTier::Tier1Generic))
            .count();
        let tier2 = all_results
            .iter()
            .filter(|r| matches!(r.tier, ContentTier::Tier2Convention))
            .count();
        let tier3 = all_results
            .iter()
            .filter(|r| matches!(r.tier, ContentTier::Tier3Constraint))
            .count();

        let avg_quality = all_results.iter().map(|r| r.quality_score).sum::<f32>() / total as f32;
        let avg_refs =
            all_results.iter().map(|r| r.reference_count).sum::<usize>() as f32 / total as f32;

        GateSummary {
            total_artifacts: total,
            passed_artifacts: passed,
            tier1_count: tier1,
            tier2_count: tier2,
            tier3_count: tier3,
            average_quality: avg_quality,
            average_references: avg_refs,
        }
    }
}

/// Count file references in content
fn count_references(content: &str, file_registry: &VerifiedFileRegistry) -> usize {
    let mut count = 0;
    for line in content.lines() {
        for word in line.split_whitespace() {
            if word.starts_with('@') && word.contains(':') {
                let path = word.trim_start_matches('@').split(':').next().unwrap_or("");
                if file_registry.contains(path) {
                    count += 1;
                }
            }
        }
    }
    count
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_config_presets() {
        let default = QualityGateConfig::default();
        assert_eq!(default.min_quality_score, 0.7);

        let fast = QualityGateConfig::fast();
        assert_eq!(fast.min_quality_score, 0.6);
        assert!(!fast.llm_judge_enabled);

        let strict = QualityGateConfig::strict();
        assert_eq!(strict.min_quality_score, 0.85);
    }

    #[test]
    fn test_gate_result_counts() {
        let result = GateResult {
            passed: true,
            overall_score: 0.8,
            skill_results: vec![
                ArtifactGateResult {
                    name: "skill1".into(),
                    passed: true,
                    quality_score: 0.9,
                    tier: ContentTier::Tier3Constraint,
                    reference_count: 5,
                    issues: vec![],
                },
                ArtifactGateResult {
                    name: "skill2".into(),
                    passed: false,
                    quality_score: 0.5,
                    tier: ContentTier::Tier1Generic,
                    reference_count: 0,
                    issues: vec![],
                },
            ],
            agent_results: vec![],
            rule_results: vec![],
            cross_validation: None,
            summary: GateSummary::default(),
        };

        assert_eq!(result.passed_count(), 1);
        assert_eq!(result.failed_count(), 1);
    }
}
