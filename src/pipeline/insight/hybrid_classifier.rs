//! Hybrid Classifier
//!
//! Combines structural rules and LLM-based classification.
//! Strategy:
//! 1. Fast structural checks for clear-cut cases
//! 2. LLM classification for ambiguous cases
//! 3. Caching to reduce repeated LLM calls

use std::sync::Arc;

use tracing::{debug, trace};

use crate::ai::LlmProvider;
use crate::config::Config;

use super::llm_classifier::{ClassificationResult, DefaultLlmClassifier, LlmClassifier};
use super::types::{
    ArtifactClassification, ExtractedInsight, Insight, InsightCategory, TierClassification,
    text_contains_any,
};
use super::value_scorer::ValueScore;

/// Classification strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationStrategy {
    Structural,
    Llm,
    Hybrid,
}

/// Hybrid classifier combining structural rules and LLM
pub struct HybridClassifier {
    llm: Option<DefaultLlmClassifier>,
    config: Arc<Config>,
    strategy: ClassificationStrategy,
}

impl HybridClassifier {
    pub fn new(provider: Arc<dyn LlmProvider>, config: Arc<Config>) -> Self {
        let llm = if config.insight.classification.enable_llm_classification {
            Some(DefaultLlmClassifier::new(
                Arc::clone(&provider),
                Arc::clone(&config),
            ))
        } else {
            None
        };

        let strategy = if config.insight.classification.enable_llm_classification {
            ClassificationStrategy::Hybrid
        } else {
            ClassificationStrategy::Structural
        };

        Self {
            llm,
            config,
            strategy,
        }
    }

    pub fn structural_only(config: Arc<Config>) -> Self {
        Self {
            llm: None,
            config,
            strategy: ClassificationStrategy::Structural,
        }
    }

    pub async fn classify(&self, insight: &Insight) -> ClassificationResult {
        match self.strategy {
            ClassificationStrategy::Structural => self.classify_structural(insight),
            ClassificationStrategy::Llm => self.classify_llm(insight).await,
            ClassificationStrategy::Hybrid => self.classify_hybrid(insight).await,
        }
    }

    pub async fn classify_batch(
        &self,
        insights: &[Insight],
        project_context: Option<&str>,
    ) -> Vec<ClassificationResult> {
        match self.strategy {
            ClassificationStrategy::Structural => insights
                .iter()
                .map(|i| self.classify_structural(i))
                .collect(),
            ClassificationStrategy::Llm => self.classify_batch_llm(insights, project_context).await,
            ClassificationStrategy::Hybrid => {
                self.classify_batch_hybrid(insights, project_context).await
            }
        }
    }

    fn classify_structural(&self, insight: &Insight) -> ClassificationResult {
        let tier = self.classify_tier(insight);
        let artifact = self.classify_artifact(insight);

        ClassificationResult {
            tier,
            artifact,
            tier_confidence: self.structural_confidence(tier),
            artifact_confidence: 0.7,
            reasoning: None,
        }
    }

    async fn classify_llm(&self, insight: &Insight) -> ClassificationResult {
        if let Some(ref llm) = self.llm {
            match llm.classify(insight).await {
                Ok(result) => result,
                Err(e) => {
                    debug!(
                        "LLM classification failed: {}. Falling back to structural",
                        e
                    );
                    self.classify_structural(insight)
                }
            }
        } else {
            self.classify_structural(insight)
        }
    }

    async fn classify_hybrid(&self, insight: &Insight) -> ClassificationResult {
        let structural_result = self.classify_structural(insight);

        if !self.needs_llm_verification(&structural_result, insight) {
            trace!(
                insight_id = %insight.id,
                tier = ?structural_result.tier,
                "Using structural classification (high confidence)"
            );
            return structural_result;
        }

        if let Some(ref llm) = self.llm
            && llm.should_use_llm(insight)
        {
            match llm.classify(insight).await {
                Ok(llm_result) => {
                    return self.merge_results(structural_result, llm_result);
                }
                Err(e) => {
                    debug!("LLM classification failed: {}. Using structural", e);
                }
            }
        }

        structural_result
    }

    async fn classify_batch_llm(
        &self,
        insights: &[Insight],
        project_context: Option<&str>,
    ) -> Vec<ClassificationResult> {
        if let Some(ref llm) = self.llm {
            match llm.classify_batch(insights, project_context).await {
                Ok(results) => results,
                Err(e) => {
                    debug!(
                        "Batch LLM classification failed: {}. Falling back to structural",
                        e
                    );
                    insights
                        .iter()
                        .map(|i| self.classify_structural(i))
                        .collect()
                }
            }
        } else {
            insights
                .iter()
                .map(|i| self.classify_structural(i))
                .collect()
        }
    }

    async fn classify_batch_hybrid(
        &self,
        insights: &[Insight],
        project_context: Option<&str>,
    ) -> Vec<ClassificationResult> {
        let structural_results: Vec<_> = insights
            .iter()
            .enumerate()
            .map(|(idx, i)| (idx, self.classify_structural(i)))
            .collect();

        let needs_llm_indices: Vec<_> = structural_results
            .iter()
            .filter(|(idx, result)| self.needs_llm_verification(result, &insights[*idx]))
            .map(|(idx, _)| *idx)
            .collect();

        if needs_llm_indices.is_empty() {
            return structural_results.into_iter().map(|(_, r)| r).collect();
        }

        debug!(
            needs_llm = needs_llm_indices.len(),
            structural_only = insights.len() - needs_llm_indices.len(),
            "Hybrid batch classification"
        );

        let llm_insights: Vec<Insight> = needs_llm_indices
            .iter()
            .map(|&idx| insights[idx].clone())
            .collect();

        let llm_results = self
            .classify_batch_llm(&llm_insights, project_context)
            .await;

        let llm_map: std::collections::HashMap<usize, ClassificationResult> = needs_llm_indices
            .into_iter()
            .zip(llm_results.into_iter())
            .collect();

        structural_results
            .into_iter()
            .map(|(idx, structural)| {
                if let Some(llm_result) = llm_map.get(&idx) {
                    self.merge_results(structural, llm_result.clone())
                } else {
                    structural
                }
            })
            .collect()
    }

    fn classify_tier(&self, insight: &Insight) -> TierClassification {
        if !self
            .config
            .insight
            .classification
            .enable_tier_classification
        {
            trace!(insight_id = %insight.id, "Tier classification disabled, defaulting to Tier2");
            return TierClassification::Tier2Convention;
        }

        let title_lower = insight.title.to_lowercase();
        let desc_lower = insight.description.to_lowercase();
        let combined = format!("{} {}", title_lower, desc_lower);

        if self.matches_tier0(&combined) {
            trace!(insight_id = %insight.id, "Classified as Tier0");
            return TierClassification::Tier0Hallucinated;
        }

        if self.matches_tier3(&combined, insight) {
            trace!(insight_id = %insight.id, "Classified as Tier3");
            return TierClassification::Tier3Constraint;
        }

        if self.matches_tier2(&combined, insight) {
            trace!(insight_id = %insight.id, "Classified as Tier2");
            return TierClassification::Tier2Convention;
        }

        trace!(insight_id = %insight.id, "Classified as Tier1 (default)");
        TierClassification::Tier1Generic
    }

    fn classify_artifact(&self, insight: &Insight) -> ArtifactClassification {
        if !self.config.insight.classification.enable_artifact_routing {
            trace!(insight_id = %insight.id, "Artifact routing disabled, defaulting to ClaudeMd");
            return ArtifactClassification::ClaudeMd;
        }

        if self.is_rule_material(insight) {
            trace!(insight_id = %insight.id, "Classified as Rules");
            return ArtifactClassification::Rule;
        }

        if self.is_skill_material(insight) {
            trace!(insight_id = %insight.id, "Classified as Skills");
            return ArtifactClassification::Skill;
        }

        if self.is_agent_material(insight) {
            trace!(insight_id = %insight.id, "Classified as Agents");
            return ArtifactClassification::Agent;
        }

        trace!(insight_id = %insight.id, "Classified as ClaudeMd (default)");
        ArtifactClassification::ClaudeMd
    }

    fn matches_tier0(&self, text: &str) -> bool {
        const BUILTIN_TIER0: &[&str] = &[
            "use best practices",
            "follow conventions",
            "write clean code",
            "write tests",
            "handle errors",
            "npm install",
            "cargo build",
            "git commit",
            "docker run",
            "pip install",
            "use typescript",
            "prefer async/await",
            "use descriptive names",
            "add comments",
            "follow dry principle",
            "avoid magic numbers",
        ];

        text_contains_any(text, BUILTIN_TIER0)
    }

    fn matches_tier3(&self, text: &str, insight: &Insight) -> bool {
        match insight.category {
            InsightCategory::SecurityConstraint | InsightCategory::Compliance => return true,
            InsightCategory::TechnicalConstraint
                if insight.severity == Some("critical".to_string()) =>
            {
                return true;
            }
            InsightCategory::BusinessRule if insight.prevention_info.is_some() => return true,
            _ => {}
        }

        const BUILTIN_TIER3: &[&str] = &[
            "must",
            "never",
            "always",
            "critical",
            "required",
            "forbidden",
            "mandatory",
            "breaks if",
            "fails when",
            "race condition",
            "security",
            "compliance",
            "regulation",
            "sla",
            "production",
            "data loss",
            "injection",
            "authentication",
            "authorization",
            "deadlock",
            "memory leak",
        ];

        if text_contains_any(text, BUILTIN_TIER3) {
            return true;
        }

        !insight.evidence.is_empty() && insight.prevention_info.is_some()
    }

    fn matches_tier2(&self, text: &str, insight: &Insight) -> bool {
        match insight.category {
            InsightCategory::ArchitectureIntent | InsightCategory::DomainKnowledge => return true,
            InsightCategory::TechnicalConstraint if !insight.evidence.is_empty() => return true,
            _ => {}
        }

        const BUILTIN_TIER2: &[&str] = &[
            "should",
            "prefer",
            "recommend",
            "architecture",
            "design",
            "workflow",
        ];

        if text_contains_any(text, BUILTIN_TIER2) {
            return true;
        }

        !insight.evidence.is_empty()
    }

    fn is_rule_material(&self, insight: &Insight) -> bool {
        const RULE_KEYWORDS: &[&str] = &[
            "must",
            "never",
            "always",
            "forbidden",
            "required",
            "constraint",
            "rule",
            "policy",
            "enforce",
            "mandate",
            "prohibit",
        ];

        match insight.category {
            InsightCategory::TechnicalConstraint
            | InsightCategory::SecurityConstraint
            | InsightCategory::PerformanceConstraint
            | InsightCategory::Compliance => return true,
            _ => {}
        }

        let text = format!("{} {}", insight.title, insight.description).to_lowercase();
        text_contains_any(&text, RULE_KEYWORDS)
    }

    fn is_skill_material(&self, insight: &Insight) -> bool {
        const SKILL_KEYWORDS: &[&str] = &[
            "how to",
            "step",
            "procedure",
            "checklist",
            "workflow",
            "guide",
            "tutorial",
            "when to",
            "add new",
            "create",
            "implement",
            "extend",
            "modify",
        ];

        if matches!(
            insight.category,
            InsightCategory::DomainKnowledge | InsightCategory::BusinessRule
        ) {
            return false;
        }

        let text = format!("{} {}", insight.title, insight.description).to_lowercase();
        text_contains_any(&text, SKILL_KEYWORDS)
    }

    fn is_agent_material(&self, insight: &Insight) -> bool {
        const AGENT_KEYWORDS: &[&str] = &[
            "expert",
            "specialist",
            "domain",
            "business",
            "industry",
            "regulatory",
            "compliance",
            "finance",
            "medical",
            "legal",
        ];

        if matches!(
            insight.category,
            InsightCategory::DomainKnowledge | InsightCategory::BusinessRule
        ) {
            return true;
        }

        let text = format!("{} {}", insight.title, insight.description).to_lowercase();
        text_contains_any(&text, AGENT_KEYWORDS)
    }

    fn needs_llm_verification(&self, result: &ClassificationResult, insight: &Insight) -> bool {
        if result.tier_confidence >= 0.9 {
            return false;
        }

        if matches!(
            result.tier,
            TierClassification::Tier0Hallucinated | TierClassification::Tier3Constraint
        ) {
            return result.tier_confidence < 0.8;
        }

        let text_len = insight.title.len() + insight.description.len();
        if text_len > 100
            && text_len < 500
            && matches!(result.tier, TierClassification::Tier1Generic)
        {
            return true;
        }

        if !insight.evidence.is_empty()
            && matches!(
                result.tier,
                TierClassification::Tier0Hallucinated | TierClassification::Tier1Generic
            )
        {
            return true;
        }

        false
    }

    fn merge_results(
        &self,
        structural: ClassificationResult,
        llm: ClassificationResult,
    ) -> ClassificationResult {
        let confidence_threshold = self.config.insight.classification.llm_confidence_threshold;

        if llm.tier_confidence >= confidence_threshold {
            return llm;
        }

        if structural.tier_confidence > llm.tier_confidence {
            return structural;
        }

        llm
    }

    fn structural_confidence(&self, tier: TierClassification) -> f32 {
        match tier {
            TierClassification::Tier0Hallucinated => 0.85,
            TierClassification::Tier3Constraint => 0.8,
            TierClassification::Tier2Convention => 0.6,
            TierClassification::Tier1Generic => 0.5,
        }
    }

    pub fn duplicate_threshold(&self) -> f32 {
        self.config
            .insight
            .classification
            .duplicate_similarity_threshold
    }

    pub async fn create_extracted_insight(
        &self,
        insight: Insight,
        value: ValueScore,
    ) -> ExtractedInsight {
        let result = self.classify(&insight).await;

        ExtractedInsight {
            insight,
            tier: result.tier,
            artifact: result.artifact,
            value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::insight::types::InsightSource;

    fn create_test_insight(title: &str, description: &str) -> Insight {
        Insight {
            id: format!("test-{}", uuid::Uuid::new_v4().as_simple()),
            category: InsightCategory::TechnicalConstraint,
            title: title.to_string(),
            description: description.to_string(),
            prevention_info: None,
            evidence: Vec::new(),
            source: InsightSource::MistakeAnalysis,
            severity: None,
        }
    }

    #[test]
    fn test_structural_only_classifier() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);
        assert_eq!(classifier.strategy, ClassificationStrategy::Structural);
    }

    #[test]
    fn test_structural_confidence_levels() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        assert!(classifier.structural_confidence(TierClassification::Tier0Hallucinated) > 0.8);
        assert!(classifier.structural_confidence(TierClassification::Tier3Constraint) > 0.7);
        assert!(classifier.structural_confidence(TierClassification::Tier1Generic) < 0.6);
    }

    #[tokio::test]
    async fn test_tier0_classification() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        let insight = create_test_insight(
            "Follow best practices",
            "Use best practices when writing code",
        );

        let result = classifier.classify(&insight).await;
        assert_eq!(result.tier, TierClassification::Tier0Hallucinated);
    }

    #[tokio::test]
    async fn test_tier3_classification() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        let mut insight = create_test_insight(
            "Authentication Required",
            "Must validate JWT tokens on all protected endpoints",
        );
        insight.category = InsightCategory::SecurityConstraint;
        insight.prevention_info = Some("Use auth middleware".to_string());
        insight.evidence = vec!["src/auth.rs".to_string()];

        let result = classifier.classify(&insight).await;
        assert_eq!(result.tier, TierClassification::Tier3Constraint);
    }

    #[tokio::test]
    async fn test_rule_classification() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        let insight = create_test_insight(
            "Database Connection",
            "Must use connection pool, never create direct connections",
        );

        let result = classifier.classify(&insight).await;
        assert_eq!(result.artifact, ArtifactClassification::Rule);
    }

    #[tokio::test]
    async fn test_skill_classification() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        let mut insight = create_test_insight(
            "Adding New Endpoint",
            "How to add a new API endpoint: step 1, step 2...",
        );
        insight.category = InsightCategory::ArchitectureIntent;

        let result = classifier.classify(&insight).await;
        assert_eq!(result.artifact, ArtifactClassification::Skill);
    }

    #[tokio::test]
    async fn test_agent_classification() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        let mut insight = create_test_insight(
            "Payment Processing Expert",
            "Domain expert knowledge about payment processing and compliance",
        );
        insight.category = InsightCategory::DomainKnowledge;

        let result = classifier.classify(&insight).await;
        assert_eq!(result.artifact, ArtifactClassification::Agent);
    }

    #[test]
    fn test_needs_llm_verification() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        let high_conf = ClassificationResult {
            tier: TierClassification::Tier2Convention,
            artifact: ArtifactClassification::ClaudeMd,
            tier_confidence: 0.95,
            artifact_confidence: 0.9,
            reasoning: None,
        };
        let insight = create_test_insight("Test", "Description");
        assert!(!classifier.needs_llm_verification(&high_conf, &insight));

        let low_conf_tier3 = ClassificationResult {
            tier: TierClassification::Tier3Constraint,
            artifact: ArtifactClassification::Rule,
            tier_confidence: 0.6,
            artifact_confidence: 0.7,
            reasoning: None,
        };
        assert!(classifier.needs_llm_verification(&low_conf_tier3, &insight));
    }

    #[test]
    fn test_merge_results_prefers_confident_llm() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        let structural = ClassificationResult {
            tier: TierClassification::Tier1Generic,
            artifact: ArtifactClassification::ClaudeMd,
            tier_confidence: 0.5,
            artifact_confidence: 0.5,
            reasoning: None,
        };

        let llm = ClassificationResult {
            tier: TierClassification::Tier3Constraint,
            artifact: ArtifactClassification::Rule,
            tier_confidence: 0.9,
            artifact_confidence: 0.85,
            reasoning: Some("Critical constraint".to_string()),
        };

        let merged = classifier.merge_results(structural, llm);
        assert_eq!(merged.tier, TierClassification::Tier3Constraint);
        assert_eq!(merged.artifact, ArtifactClassification::Rule);
    }

    #[tokio::test]
    async fn test_batch_classification() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        let insights = vec![
            create_test_insight("Rule 1", "Description 1"),
            create_test_insight("Rule 2", "Description 2"),
            create_test_insight("Rule 3", "Description 3"),
        ];

        let results = classifier.classify_batch(&insights, None).await;
        assert_eq!(results.len(), 3);
    }
}
