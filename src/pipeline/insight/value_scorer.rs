//! Value Scorer
//!
//! Scores insights based on three dimensions:
//! - Mistake Prevention: Would AI make mistakes without this?
//! - Discoverability: How hard is this to find in code?
//! - Artifact Fitness: How well does this fit the target artifact?

use std::sync::Arc;

use tracing::trace;

use crate::config::Config;

use super::types::{Insight, InsightCategory, InsightSource};

/// Value score for an insight
#[derive(Debug, Clone, Default)]
pub struct ValueScore {
    /// How well this prevents AI mistakes (0.0 - 1.0)
    pub mistake_prevention: f32,
    /// How hard this is to discover from code (0.0 - 1.0)
    pub discoverability: f32,
    /// How well this fits the artifact type (0.0 - 1.0)
    pub artifact_fitness: f32,
    /// Overall weighted score
    pub overall: f32,
}

impl ValueScore {
    pub fn new(mistake_prevention: f32, discoverability: f32, artifact_fitness: f32) -> Self {
        Self {
            mistake_prevention,
            discoverability,
            artifact_fitness,
            overall: 0.0,
        }
    }

    pub fn with_overall(mut self, weights: &crate::config::ValueWeights) -> Self {
        self.overall = weights.mistake_prevention * self.mistake_prevention
            + weights.discoverability * self.discoverability
            + weights.artifact_fitness * self.artifact_fitness;
        self
    }

    pub fn meets_thresholds(&self, dims: &crate::config::ValueDimensions) -> bool {
        self.mistake_prevention >= dims.mistake_prevention
            && self.discoverability >= dims.discoverability
            && self.artifact_fitness >= dims.artifact_fitness
    }
}

/// Scores insights by value dimensions
pub struct ValueScorer {
    config: Arc<Config>,
}

impl ValueScorer {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    /// Score an insight based on value dimensions
    pub fn score(&self, insight: &Insight) -> ValueScore {
        let mistake_prevention = self.score_mistake_prevention(insight);
        let discoverability = self.score_discoverability(insight);
        let artifact_fitness = self.score_artifact_fitness(insight);

        let score = ValueScore::new(mistake_prevention, discoverability, artifact_fitness)
            .with_overall(&self.config.value.weights);

        trace!(
            insight_id = %insight.id,
            mistake_prevention,
            discoverability,
            artifact_fitness,
            overall = score.overall,
            "Scored insight"
        );

        score
    }

    /// Score how well this insight prevents AI mistakes
    fn score_mistake_prevention(&self, insight: &Insight) -> f32 {
        let mut score = 0.0f32;
        let scoring_config = &self.config.insight.scoring;

        score += self.category_mistake_score(insight);
        score += self.severity_bonus(insight);
        score += self.prevention_and_evidence_bonus(insight);
        score += self.keyword_score_mistake_prevention(insight, scoring_config);

        score.min(1.0)
    }

    fn category_mistake_score(&self, insight: &Insight) -> f32 {
        let cat_scores = &self.config.insight.scoring.category_scores;
        match insight.category {
            InsightCategory::SecurityConstraint => cat_scores.security_constraint,
            InsightCategory::TechnicalConstraint => cat_scores.technical_constraint,
            InsightCategory::Compliance => cat_scores.compliance,
            InsightCategory::BusinessRule => cat_scores.business_rule,
            InsightCategory::Gotcha => cat_scores.gotcha,
            InsightCategory::PerformanceConstraint => cat_scores.performance_constraint,
            InsightCategory::ArchitectureIntent => cat_scores.architecture_intent,
            InsightCategory::DomainKnowledge => cat_scores.domain_knowledge,
        }
    }

    fn severity_bonus(&self, insight: &Insight) -> f32 {
        let Some(ref severity) = insight.severity else {
            return 0.0;
        };
        let bonuses = &self.config.insight.scoring.severity_bonuses;
        match severity.to_lowercase().as_str() {
            "critical" => bonuses.critical,
            "high" => bonuses.high,
            "medium" => bonuses.medium,
            _ => bonuses.low,
        }
    }

    fn prevention_and_evidence_bonus(&self, insight: &Insight) -> f32 {
        let mut score = 0.0;
        let scoring_config = &self.config.insight.scoring;

        if insight.prevention_info.is_some() {
            score += scoring_config.prevention_info_bonus;
        }
        if !insight.evidence.is_empty() {
            let evidence_bonus =
                (insight.evidence.len() as f32) * scoring_config.evidence_bonus_per_ref;
            score += evidence_bonus.min(scoring_config.max_evidence_bonus);
        }
        score
    }

    fn keyword_score_mistake_prevention(
        &self,
        insight: &Insight,
        scoring_config: &crate::config::ScoringConfig,
    ) -> f32 {
        let text = format!("{} {}", insight.title, insight.description).to_lowercase();
        let mut score = 0.0;

        // Config keywords
        for (keyword, bonus) in &scoring_config.mistake_prevention_keywords {
            if text.contains(&keyword.to_lowercase()) {
                score += bonus;
            }
        }

        // Built-in keywords (only if not overridden by config)
        const BUILTIN: &[(&str, f32)] = &[
            ("must", 0.1),
            ("never", 0.1),
            ("always", 0.08),
            ("fail", 0.08),
            ("error", 0.05),
            ("bug", 0.08),
            ("crash", 0.1),
            ("race condition", 0.12),
            ("deadlock", 0.1),
            ("memory leak", 0.1),
            ("data loss", 0.12),
        ];

        for (keyword, bonus) in BUILTIN {
            if text.contains(*keyword)
                && !scoring_config.mistake_prevention_keywords.contains_key(*keyword)
            {
                score += bonus;
            }
        }
        score
    }

    /// Score how hard this is to discover from code alone
    fn score_discoverability(&self, insight: &Insight) -> f32 {
        let mut score = 0.5f32; // Base: moderate difficulty
        let scoring_config = &self.config.insight.scoring;

        score += self.source_discoverability_score(insight);
        score += self.category_discoverability_adjustment(insight);
        score += self.keyword_score_discoverability(insight, scoring_config);

        score.clamp(0.0, 1.0)
    }

    fn source_discoverability_score(&self, insight: &Insight) -> f32 {
        let source_scores = &self.config.insight.scoring.source_scores;
        match insight.source {
            InsightSource::DomainAnalysis => source_scores.domain_analysis,
            InsightSource::MistakeAnalysis => source_scores.mistake_analysis,
            InsightSource::ConstraintDetection => source_scores.constraint_detection,
            InsightSource::PatternMining => source_scores.pattern_mining,
            InsightSource::ManualAnnotation => source_scores.manual_annotation,
        }
    }

    fn category_discoverability_adjustment(&self, insight: &Insight) -> f32 {
        let cat_adj = &self.config.insight.scoring.category_adjustments;
        match insight.category {
            InsightCategory::Gotcha => cat_adj.gotcha,
            InsightCategory::DomainKnowledge => cat_adj.domain_knowledge,
            InsightCategory::BusinessRule => cat_adj.business_rule,
            InsightCategory::ArchitectureIntent => cat_adj.architecture_intent,
            _ => 0.0,
        }
    }

    fn keyword_score_discoverability(
        &self,
        insight: &Insight,
        scoring_config: &crate::config::ScoringConfig,
    ) -> f32 {
        let text = format!("{} {}", insight.title, insight.description).to_lowercase();
        let mut score = 0.0;

        // Config keywords
        for (keyword, bonus) in &scoring_config.discoverability_keywords {
            if text.contains(&keyword.to_lowercase()) {
                score += bonus;
            }
        }

        // Built-in hidden knowledge indicators
        const BUILTIN_HIDDEN: &[(&str, f32)] = &[
            ("implicit", 0.1),
            ("undocumented", 0.2),
            ("unwritten", 0.15),
            ("tacit", 0.2),
            ("tribal", 0.2),
            ("experience", 0.1),
            ("history", 0.1),
            ("legacy", 0.1),
            ("trap", 0.15),
            ("surprise", 0.1),
        ];

        for (keyword, bonus) in BUILTIN_HIDDEN {
            if text.contains(*keyword)
                && !scoring_config.discoverability_keywords.contains_key(*keyword)
            {
                score += bonus;
            }
        }

        // Obvious knowledge indicators (decrease score)
        const OBVIOUS: &[(&str, f32)] = &[
            ("readme", -0.15),
            ("documentation", -0.1),
            ("comment", -0.1),
            ("obvious", -0.2),
            ("standard", -0.1),
            ("common", -0.1),
            ("well-known", -0.15),
        ];

        for (keyword, adjustment) in OBVIOUS {
            if text.contains(*keyword) {
                score += adjustment;
            }
        }
        score
    }

    /// Score how well this fits as artifact content
    fn score_artifact_fitness(&self, insight: &Insight) -> f32 {
        let mut score = 0.5f32; // Base: acceptable fit
        let fitness_config = &self.config.insight.scoring.artifact_fitness;

        score += self.fitness_evidence_bonus(insight, fitness_config);
        score += self.fitness_category_bonus(insight, fitness_config);
        score += self.fitness_content_quality(insight, fitness_config);

        score.clamp(0.0, 1.0)
    }

    fn fitness_evidence_bonus(
        &self,
        insight: &Insight,
        config: &crate::config::ArtifactFitnessConfig,
    ) -> f32 {
        let mut score = 0.0;
        if !insight.evidence.is_empty() {
            score += config.evidence_bonus;
        }
        if insight.prevention_info.is_some() {
            score += config.prevention_info_bonus;
        }
        score
    }

    fn fitness_category_bonus(
        &self,
        insight: &Insight,
        config: &crate::config::ArtifactFitnessConfig,
    ) -> f32 {
        match insight.category {
            InsightCategory::TechnicalConstraint
            | InsightCategory::SecurityConstraint
            | InsightCategory::PerformanceConstraint
            | InsightCategory::Compliance => config.constraint_category_bonus,
            InsightCategory::BusinessRule | InsightCategory::DomainKnowledge => {
                config.domain_category_bonus
            }
            InsightCategory::ArchitectureIntent => config.architecture_bonus,
            InsightCategory::Gotcha => config.gotcha_bonus,
        }
    }

    fn fitness_content_quality(
        &self,
        insight: &Insight,
        config: &crate::config::ArtifactFitnessConfig,
    ) -> f32 {
        let text = format!("{} {}", insight.title, insight.description);
        let text_lower = text.to_lowercase();
        let mut score = 0.0;

        // Length bonuses
        if text.len() > config.min_length_bonus_threshold {
            score += config.length_bonus;
        }
        if text.len() > config.extended_length_bonus_threshold {
            score += config.length_bonus;
        }

        // Actionable language bonus
        const ACTIONABLE: &[&str] = &["must", "should", "avoid", "use", "ensure", "verify", "check"];
        for keyword in ACTIONABLE {
            if text_lower.contains(*keyword) {
                score += 0.03;
            }
        }

        // Generic language penalty
        const GENERIC: &[&str] = &["general", "usually", "sometimes", "might", "could", "possibly"];
        for keyword in GENERIC {
            if text_lower.contains(*keyword) {
                score -= 0.05;
            }
        }

        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn create_test_scorer() -> ValueScorer {
        ValueScorer::new(Arc::new(Config::default()))
    }

    #[test]
    fn test_security_insight_high_mistake_prevention() {
        let scorer = create_test_scorer();

        let insight = Insight {
            id: "test".to_string(),
            category: InsightCategory::SecurityConstraint,
            title: "SQL Injection Vulnerability".to_string(),
            description: "Must sanitize all user input to prevent SQL injection attacks".to_string(),
            prevention_info: Some("Use parameterized queries".to_string()),
            evidence: vec!["src/db.rs".to_string()],
            source: InsightSource::MistakeAnalysis,
            severity: Some("critical".to_string()),
        };

        let score = scorer.score(&insight);

        assert!(score.mistake_prevention >= 0.7);
    }

    #[test]
    fn test_hidden_gotcha_high_discoverability() {
        let scorer = create_test_scorer();

        let insight = Insight {
            id: "test".to_string(),
            category: InsightCategory::Gotcha,
            title: "Hidden Dependency".to_string(),
            description: "Implicit ordering requirement that is undocumented".to_string(),
            prevention_info: None,
            evidence: Vec::new(),
            source: InsightSource::DomainAnalysis,
            severity: None,
        };

        let score = scorer.score(&insight);

        assert!(score.discoverability >= 0.6);
    }

    #[test]
    fn test_evidence_improves_fitness() {
        let scorer = create_test_scorer();

        let insight_with_evidence = Insight {
            id: "test1".to_string(),
            category: InsightCategory::TechnicalConstraint,
            title: "Constraint".to_string(),
            description: "Must follow this pattern".to_string(),
            prevention_info: Some("Use correct approach".to_string()),
            evidence: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
            source: InsightSource::ConstraintDetection,
            severity: None,
        };

        let insight_without_evidence = Insight {
            id: "test2".to_string(),
            category: InsightCategory::TechnicalConstraint,
            title: "Constraint".to_string(),
            description: "Must follow this pattern".to_string(),
            prevention_info: None,
            evidence: Vec::new(),
            source: InsightSource::ConstraintDetection,
            severity: None,
        };

        let score_with = scorer.score(&insight_with_evidence);
        let score_without = scorer.score(&insight_without_evidence);

        assert!(score_with.artifact_fitness > score_without.artifact_fitness);
    }
}
