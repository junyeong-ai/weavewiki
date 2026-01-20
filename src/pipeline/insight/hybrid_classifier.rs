//! Hybrid Classifier
//!
//! Combines structural rules and LLM-based classification for optimal accuracy.
//! Strategy:
//! 1. Fast structural checks for clear-cut cases
//! 2. LLM classification for ambiguous cases
//! 3. Caching to reduce repeated LLM calls
//!
//! This approach balances speed, cost, and accuracy.

use std::sync::Arc;

use tracing::{debug, trace};

use crate::ai::LlmProvider;
use crate::config::Config;

use super::knowledge_classifier::{KnowledgeClassifier, TierClassification};
use super::llm_classifier::{ClassificationResult, DefaultLlmClassifier, LlmClassifier};
use super::types::Insight;
use super::value_scorer::ValueScore;
use super::ExtractedInsight;

// =============================================================================
// Hybrid Classification Strategy
// =============================================================================

/// Decision about which classification method to use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationStrategy {
    /// Use structural rules only (fast, deterministic)
    Structural,
    /// Use LLM classification (accurate, slower)
    Llm,
    /// Use structural as primary, LLM for verification
    Hybrid,
}

/// Hybrid classifier combining structural and LLM approaches
pub struct HybridClassifier {
    /// Structural keyword-based classifier
    structural: KnowledgeClassifier,
    /// LLM-based classifier
    llm: Option<DefaultLlmClassifier>,
    /// Configuration
    config: Arc<Config>,
    /// Strategy to use
    strategy: ClassificationStrategy,
}

impl HybridClassifier {
    /// Create a new hybrid classifier
    pub fn new(provider: Arc<dyn LlmProvider>, config: Arc<Config>) -> Self {
        let structural = KnowledgeClassifier::new(Arc::clone(&config));

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
            structural,
            llm,
            config,
            strategy,
        }
    }

    /// Create a classifier that only uses structural rules (no LLM)
    pub fn structural_only(config: Arc<Config>) -> Self {
        Self {
            structural: KnowledgeClassifier::new(Arc::clone(&config)),
            llm: None,
            config,
            strategy: ClassificationStrategy::Structural,
        }
    }

    /// Classify a single insight
    pub async fn classify(&self, insight: &Insight) -> ClassificationResult {
        match self.strategy {
            ClassificationStrategy::Structural => self.classify_structural(insight),
            ClassificationStrategy::Llm => self.classify_llm(insight).await,
            ClassificationStrategy::Hybrid => self.classify_hybrid(insight).await,
        }
    }

    /// Classify multiple insights (batch)
    pub async fn classify_batch(
        &self,
        insights: &[Insight],
        project_context: Option<&str>,
    ) -> Vec<ClassificationResult> {
        match self.strategy {
            ClassificationStrategy::Structural => {
                insights.iter().map(|i| self.classify_structural(i)).collect()
            }
            ClassificationStrategy::Llm => {
                self.classify_batch_llm(insights, project_context).await
            }
            ClassificationStrategy::Hybrid => {
                self.classify_batch_hybrid(insights, project_context).await
            }
        }
    }

    /// Structural-only classification
    fn classify_structural(&self, insight: &Insight) -> ClassificationResult {
        let tier = self.structural.classify_tier(insight);
        let artifact = self.structural.classify_artifact(insight);

        ClassificationResult {
            tier,
            artifact,
            tier_confidence: self.structural_confidence(tier),
            artifact_confidence: 0.7, // Structural rules are moderately confident
            reasoning: None,
        }
    }

    /// LLM-only classification
    async fn classify_llm(&self, insight: &Insight) -> ClassificationResult {
        if let Some(ref llm) = self.llm {
            match llm.classify(insight).await {
                Ok(result) => result,
                Err(e) => {
                    debug!("LLM classification failed: {}. Falling back to structural", e);
                    self.classify_structural(insight)
                }
            }
        } else {
            self.classify_structural(insight)
        }
    }

    /// Hybrid classification: structural first, LLM for ambiguous cases
    async fn classify_hybrid(&self, insight: &Insight) -> ClassificationResult {
        // Step 1: Get structural classification
        let structural_result = self.classify_structural(insight);

        // Step 2: Determine if LLM is needed
        if !self.needs_llm_verification(&structural_result, insight) {
            trace!(
                insight_id = %insight.id,
                tier = ?structural_result.tier,
                "Using structural classification (high confidence)"
            );
            return structural_result;
        }

        // Step 3: Use LLM for ambiguous cases
        if let Some(ref llm) = self.llm {
            if llm.should_use_llm(insight) {
                match llm.classify(insight).await {
                    Ok(llm_result) => {
                        // Merge results: prefer LLM if confident
                        return self.merge_results(structural_result, llm_result);
                    }
                    Err(e) => {
                        debug!("LLM classification failed: {}. Using structural", e);
                    }
                }
            }
        }

        structural_result
    }

    /// Batch LLM classification
    async fn classify_batch_llm(
        &self,
        insights: &[Insight],
        project_context: Option<&str>,
    ) -> Vec<ClassificationResult> {
        if let Some(ref llm) = self.llm {
            match llm.classify_batch(insights, project_context).await {
                Ok(results) => results,
                Err(e) => {
                    debug!("Batch LLM classification failed: {}. Falling back to structural", e);
                    insights.iter().map(|i| self.classify_structural(i)).collect()
                }
            }
        } else {
            insights.iter().map(|i| self.classify_structural(i)).collect()
        }
    }

    /// Batch hybrid classification
    async fn classify_batch_hybrid(
        &self,
        insights: &[Insight],
        project_context: Option<&str>,
    ) -> Vec<ClassificationResult> {
        // First pass: structural classification with indices
        let structural_results: Vec<_> = insights
            .iter()
            .enumerate()
            .map(|(idx, i)| (idx, self.classify_structural(i)))
            .collect();

        // Identify indices needing LLM
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

        // Collect insights that need LLM (by cloning)
        let llm_insights: Vec<Insight> = needs_llm_indices
            .iter()
            .map(|&idx| insights[idx].clone())
            .collect();

        // Get LLM results for ambiguous cases
        let llm_results = self.classify_batch_llm(&llm_insights, project_context).await;

        // Create lookup map for LLM results by index
        let llm_map: std::collections::HashMap<usize, ClassificationResult> = needs_llm_indices
            .into_iter()
            .zip(llm_results.into_iter())
            .collect();

        // Merge results
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

    /// Determine if LLM verification is needed
    fn needs_llm_verification(&self, result: &ClassificationResult, insight: &Insight) -> bool {
        // High confidence structural results don't need LLM
        if result.tier_confidence >= 0.9 {
            return false;
        }

        // Tier 0 and Tier 3 should be verified by LLM if not confident
        if matches!(result.tier, TierClassification::Tier0 | TierClassification::Tier3) {
            return result.tier_confidence < 0.8;
        }

        // Medium-length content with Tier 1/2 classification might be misclassified
        let text_len = insight.title.len() + insight.description.len();
        if text_len > 100 && text_len < 500 && matches!(result.tier, TierClassification::Tier1) {
            return true;
        }

        // Has evidence but classified as low value - verify
        if !insight.evidence.is_empty() && matches!(result.tier, TierClassification::Tier0 | TierClassification::Tier1) {
            return true;
        }

        false
    }

    /// Merge structural and LLM results
    fn merge_results(
        &self,
        structural: ClassificationResult,
        llm: ClassificationResult,
    ) -> ClassificationResult {
        let confidence_threshold = self.config.insight.classification.llm_confidence_threshold;

        // If LLM is highly confident, use its result
        if llm.tier_confidence >= confidence_threshold {
            return llm;
        }

        // If structural is more confident, use it
        if structural.tier_confidence > llm.tier_confidence {
            return structural;
        }

        // Otherwise, prefer LLM (even with lower confidence, it's more nuanced)
        llm
    }

    /// Calculate confidence score for structural classification
    fn structural_confidence(&self, tier: TierClassification) -> f32 {
        // Structural rules are most confident about extreme cases
        match tier {
            TierClassification::Tier0 => 0.85, // Pretty sure this is generic
            TierClassification::Tier3 => 0.8,  // Keywords indicate high value
            TierClassification::Tier2 => 0.6,  // Medium confidence
            TierClassification::Tier1 => 0.5,  // Low confidence (default)
        }
    }

    /// Get duplicate similarity threshold
    pub fn duplicate_threshold(&self) -> f32 {
        self.structural.duplicate_threshold()
    }
}

// =============================================================================
// Extended ExtractedInsight Creation
// =============================================================================

impl HybridClassifier {
    /// Create an ExtractedInsight with classification
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

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{LlmProvider, LlmResponse, ResponseMetadata, ResponseTiming, TokenUsage};
    use crate::pipeline::insight::knowledge_classifier::ArtifactClassification;
    use crate::pipeline::insight::types::{InsightCategory, InsightSource};
    use crate::types::Result;
    use async_trait::async_trait;

    struct MockProvider;

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _schema: &serde_json::Value) -> Result<LlmResponse> {
            Ok(LlmResponse {
                content: serde_json::json!({
                    "tier": 2,
                    "artifact": "rules",
                    "tier_confidence": 0.85,
                    "artifact_confidence": 0.8,
                    "reasoning": "Test reasoning"
                }),
                usage: TokenUsage::default(),
                cost_usd: 0.0,
                timing: ResponseTiming::default(),
                metadata: ResponseMetadata::default(),
            })
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(true)
        }

        fn name(&self) -> &str {
            "mock"
        }

        fn model(&self) -> &str {
            "mock-model"
        }
    }

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

        assert!(classifier.structural_confidence(TierClassification::Tier0) > 0.8);
        assert!(classifier.structural_confidence(TierClassification::Tier3) > 0.7);
        assert!(classifier.structural_confidence(TierClassification::Tier1) < 0.6);
    }

    #[tokio::test]
    async fn test_structural_classification() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        let insight = create_test_insight(
            "Follow best practices",
            "Use best practices when writing code",
        );

        let result = classifier.classify(&insight).await;
        // Should be rejected as Tier 0 (generic advice)
        assert_eq!(result.tier, TierClassification::Tier0);
    }

    #[tokio::test]
    async fn test_hybrid_classification_with_evidence() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::new(Arc::new(MockProvider), config);

        let mut insight = create_test_insight(
            "Database Connection Rule",
            "Must use connection pool for all database operations",
        );
        insight.evidence = vec!["src/db.rs:42".to_string()];

        let result = classifier.classify(&insight).await;
        // Should have reasonable confidence
        assert!(result.tier_confidence > 0.5);
    }

    #[test]
    fn test_needs_llm_verification() {
        let config = Arc::new(Config::default());
        let classifier = HybridClassifier::structural_only(config);

        // High confidence result doesn't need LLM
        let high_conf = ClassificationResult {
            tier: TierClassification::Tier2,
            artifact: ArtifactClassification::ClaudeMd,
            tier_confidence: 0.95,
            artifact_confidence: 0.9,
            reasoning: None,
        };
        let insight = create_test_insight("Test", "Description");
        assert!(!classifier.needs_llm_verification(&high_conf, &insight));

        // Low confidence Tier 3 needs verification
        let low_conf_tier3 = ClassificationResult {
            tier: TierClassification::Tier3,
            artifact: ArtifactClassification::Rules,
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
            tier: TierClassification::Tier1,
            artifact: ArtifactClassification::ClaudeMd,
            tier_confidence: 0.5,
            artifact_confidence: 0.5,
            reasoning: None,
        };

        let llm = ClassificationResult {
            tier: TierClassification::Tier3,
            artifact: ArtifactClassification::Rules,
            tier_confidence: 0.9,
            artifact_confidence: 0.85,
            reasoning: Some("Critical constraint".to_string()),
        };

        let merged = classifier.merge_results(structural, llm);
        assert_eq!(merged.tier, TierClassification::Tier3);
        assert_eq!(merged.artifact, ArtifactClassification::Rules);
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
