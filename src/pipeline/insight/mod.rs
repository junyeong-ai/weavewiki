//! Insight Engine Module
//!
//! Extracts high-value insights from project analysis to generate artifacts.
//! Core principle: "What mistakes would AI make without this information?"

mod constraint_detector;
mod domain_analyzer;
mod hybrid_classifier;
mod llm_classifier;
mod mistake_finder;
mod types;
mod value_scorer;

pub use constraint_detector::ConstraintDetector;
pub use domain_analyzer::{BusinessRuleExtractor, DomainAnalyzer, TerminologyExtractor};
pub use hybrid_classifier::{ClassificationStrategy, HybridClassifier};
pub use llm_classifier::{
    BatchClassificationRequest, ClassificationCache, ClassificationResult, DefaultLlmClassifier,
    InsightSummary, LlmClassifier,
};
pub use mistake_finder::{MistakeFinder, MistakeSeverity, PotentialMistake};
pub use types::{
    ArtifactClassification, BusinessRule, Constraint, ConstraintType, DomainKnowledge,
    ExtractedInsight, Insight, InsightCategory, InsightSource, Knowledge, Terminology,
    TierClassification,
};
pub use value_scorer::{ValueScore, ValueScorer};

use std::collections::HashSet;
use std::sync::Arc;

use tracing::{debug, instrument, trace};

use crate::ai::LlmProvider;
use crate::config::Config;
use crate::pipeline::analysis::SynthesizedAnalysis;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
use crate::pipeline::phases::convention_inference::InferredConventions;
use crate::types::Result;

/// Unified insight extraction engine
pub struct InsightEngine {
    mistake_finder: MistakeFinder,
    constraint_detector: ConstraintDetector,
    domain_analyzer: DomainAnalyzer,
    classifier: HybridClassifier,
    value_scorer: ValueScorer,
}

/// Context for insight extraction
#[derive(Debug)]
pub struct InsightContext<'a> {
    pub conventions: &'a InferredConventions,
    pub constraints: &'a ExtractedConstraints,
    pub synthesis: Option<&'a SynthesizedAnalysis>,
    pub file_registry: &'a VerifiedFileRegistry,
}

/// Result of insight extraction
#[derive(Debug, Clone)]
pub struct InsightExtractionResult {
    pub insights: Vec<ExtractedInsight>,
    pub by_artifact: ArtifactInsights,
    pub high_value: Vec<ExtractedInsight>,
    pub total_value: f32,
    pub stats: ExtractionStats,
}

/// Insights organized by target artifact
#[derive(Debug, Clone, Default)]
pub struct ArtifactInsights {
    pub claude_md: Vec<ExtractedInsight>,
    pub rules: Vec<ExtractedInsight>,
    pub skills: Vec<ExtractedInsight>,
    pub agents: Vec<ExtractedInsight>,
}

/// Statistics about extraction process
#[derive(Debug, Clone, Default)]
pub struct ExtractionStats {
    pub total_extracted: usize,
    pub tier0_rejected: usize,
    pub tier1_low_value: usize,
    pub tier2_medium: usize,
    pub tier3_high_value: usize,
    pub duplicates_removed: usize,
}

impl InsightEngine {
    pub fn new(provider: Arc<dyn LlmProvider>, config: Arc<Config>) -> Self {
        Self {
            mistake_finder: MistakeFinder::new(Arc::clone(&provider), Arc::clone(&config)),
            constraint_detector: ConstraintDetector::new(Arc::clone(&config)),
            domain_analyzer: DomainAnalyzer::new(Arc::clone(&provider), Arc::clone(&config)),
            classifier: HybridClassifier::new(Arc::clone(&provider), Arc::clone(&config)),
            value_scorer: ValueScorer::new(config),
        }
    }

    #[instrument(skip(self, ctx))]
    pub async fn extract(&self, ctx: &InsightContext<'_>) -> Result<InsightExtractionResult> {
        let mut all_insights = Vec::new();
        let mut stats = ExtractionStats::default();

        // Phase 1: Collect raw insights from all sources
        debug!("Collecting insights from all sources");
        self.collect_mistakes(&mut all_insights, ctx).await?;
        self.collect_constraints(&mut all_insights, ctx);
        self.collect_domain_knowledge(&mut all_insights, ctx).await?;

        stats.total_extracted = all_insights.len();
        debug!(total = stats.total_extracted, "Raw insights collected");

        // Phase 2: Batch classify all insights
        debug!("Classifying insights");
        let classifications = self
            .classifier
            .classify_batch(&all_insights, None)
            .await;

        // Phase 3: Score and filter
        debug!("Scoring and filtering");
        let mut classified_insights = Vec::with_capacity(all_insights.len());

        for (insight, result) in all_insights.into_iter().zip(classifications.into_iter()) {
            if result.tier == TierClassification::Tier0 {
                stats.tier0_rejected += 1;
                trace!(id = %insight.id, "Rejected: Tier0");
                continue;
            }

            match result.tier {
                TierClassification::Tier1 => stats.tier1_low_value += 1,
                TierClassification::Tier2 => stats.tier2_medium += 1,
                TierClassification::Tier3 => stats.tier3_high_value += 1,
                _ => {}
            }

            let value = self.value_scorer.score(&insight);
            let mut extracted = ExtractedInsight {
                insight,
                tier: result.tier,
                artifact: result.artifact,
                value,
            };

            self.enrich_evidence(&mut extracted, ctx);
            classified_insights.push(extracted);
        }

        debug!(
            rejected = stats.tier0_rejected,
            tier1 = stats.tier1_low_value,
            tier2 = stats.tier2_medium,
            tier3 = stats.tier3_high_value,
            "Classification complete"
        );

        // Phase 4: Deduplicate
        let (unique, dup_count) = self.deduplicate(classified_insights);
        stats.duplicates_removed = dup_count;
        debug!(removed = dup_count, remaining = unique.len(), "Deduplication complete");

        // Organize results
        let by_artifact = self.organize_by_artifact(&unique);
        let high_value: Vec<_> = unique
            .iter()
            .filter(|i| i.tier == TierClassification::Tier3)
            .cloned()
            .collect();
        let total_value = unique
            .iter()
            .map(|i| i.value.overall)
            .sum::<f32>()
            / unique.len().max(1) as f32;

        debug!(
            high_value = high_value.len(),
            total_value,
            total = unique.len(),
            "Extraction complete"
        );

        Ok(InsightExtractionResult {
            insights: unique,
            by_artifact,
            high_value,
            total_value,
            stats,
        })
    }

    async fn collect_mistakes(
        &self,
        insights: &mut Vec<Insight>,
        ctx: &InsightContext<'_>,
    ) -> Result<()> {
        let mistakes = self.mistake_finder.find_potential_mistakes(ctx).await?;
        debug!(count = mistakes.len(), "Mistakes found");

        for m in mistakes {
            insights.push(Insight {
                id: format!("mistake-{}", uuid::Uuid::new_v4().as_simple()),
                category: InsightCategory::TechnicalConstraint,
                title: m.title,
                description: m.description,
                prevention_info: Some(m.prevention),
                evidence: m.evidence,
                source: InsightSource::MistakeAnalysis,
                severity: Some(m.severity.as_str().to_string()),
            });
        }
        Ok(())
    }

    fn collect_constraints(&self, insights: &mut Vec<Insight>, ctx: &InsightContext<'_>) {
        let constraints = self.constraint_detector.detect_all(ctx);
        debug!(count = constraints.len(), "Constraints detected");

        for c in constraints {
            insights.push(Insight {
                id: format!("constraint-{}", uuid::Uuid::new_v4().as_simple()),
                category: match c.constraint_type {
                    ConstraintType::Security => InsightCategory::SecurityConstraint,
                    ConstraintType::Performance => InsightCategory::PerformanceConstraint,
                    _ => InsightCategory::TechnicalConstraint,
                },
                title: c.name,
                description: c.description,
                prevention_info: c.prevention,
                evidence: c.evidence,
                source: InsightSource::ConstraintDetection,
                severity: Some(c.severity),
            });
        }
    }

    async fn collect_domain_knowledge(
        &self,
        insights: &mut Vec<Insight>,
        ctx: &InsightContext<'_>,
    ) -> Result<()> {
        let domain = self.domain_analyzer.analyze(ctx).await?;
        debug!(
            rules = domain.business_rules.len(),
            terms = domain.terminology.len(),
            "Domain knowledge extracted"
        );

        for rule in domain.business_rules {
            insights.push(Insight {
                id: format!("rule-{}", uuid::Uuid::new_v4().as_simple()),
                category: InsightCategory::BusinessRule,
                title: rule.name,
                description: rule.description,
                prevention_info: rule.consequence,
                evidence: rule.evidence,
                source: InsightSource::DomainAnalysis,
                severity: None,
            });
        }

        for term in domain.terminology {
            insights.push(Insight {
                id: format!("term-{}", uuid::Uuid::new_v4().as_simple()),
                category: InsightCategory::DomainKnowledge,
                title: term.term,
                description: term.definition,
                prevention_info: term.usage_context,
                evidence: term.occurrences,
                source: InsightSource::DomainAnalysis,
                severity: None,
            });
        }
        Ok(())
    }

    fn enrich_evidence(&self, insight: &mut ExtractedInsight, ctx: &InsightContext<'_>) {
        if !insight.insight.evidence.is_empty() {
            return;
        }

        let keywords: Vec<&str> = insight
            .insight
            .title
            .split_whitespace()
            .chain(insight.insight.description.split_whitespace())
            .filter(|w| w.len() > 3)
            .take(5)
            .collect();

        for kw in keywords {
            for file in ctx.file_registry.files_matching(kw).iter().take(3) {
                insight.insight.evidence.push(file.to_string());
            }
        }
    }

    fn deduplicate(&self, insights: Vec<ExtractedInsight>) -> (Vec<ExtractedInsight>, usize) {
        let threshold = self.classifier.duplicate_threshold();
        let mut unique: Vec<(ExtractedInsight, HashSet<String>)> = Vec::new();
        let mut seen_titles: HashSet<String> = HashSet::new();
        let mut duplicates = 0;

        for insight in insights {
            let title_lower = insight.insight.title.to_lowercase();
            if seen_titles.contains(&title_lower) {
                duplicates += 1;
                continue;
            }

            let words: HashSet<String> = insight
                .insight
                .description
                .to_lowercase()
                .split_whitespace()
                .map(String::from)
                .collect();

            let is_similar = unique
                .iter()
                .any(|(_, existing)| jaccard_similarity(&words, existing) > threshold);

            if is_similar {
                duplicates += 1;
                continue;
            }

            seen_titles.insert(title_lower);
            unique.push((insight, words));
        }

        (unique.into_iter().map(|(i, _)| i).collect(), duplicates)
    }

    fn organize_by_artifact(&self, insights: &[ExtractedInsight]) -> ArtifactInsights {
        let mut result = ArtifactInsights::default();
        for i in insights {
            match i.artifact {
                ArtifactClassification::ClaudeMd => result.claude_md.push(i.clone()),
                ArtifactClassification::Rules => result.rules.push(i.clone()),
                ArtifactClassification::Skills => result.skills.push(i.clone()),
                ArtifactClassification::Agents => result.agents.push(i.clone()),
            }
        }
        result
    }
}

fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.len() + b.len() - intersection;
    if union == 0 {
        return 0.0;
    }
    intersection as f32 / union as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{LlmResponse, ResponseMetadata, ResponseTiming, TokenUsage};
    use crate::pipeline::context::VerifiedFileRegistry;
    use crate::pipeline::phases::constraint_extraction::{
        ExtractedConstraints, Gotcha, ImplicitRule, RuleEnforcement,
    };
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization,
        InferredConventions, NamingConventions, TestingConvention,
    };
    use async_trait::async_trait;
    use serde_json::Value;

    struct MockProvider;

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _schema: &Value) -> Result<LlmResponse> {
            Ok(LlmResponse {
                content: serde_json::json!([]),
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

    fn create_test_conventions() -> InferredConventions {
        InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        }
    }

    fn create_test_constraints() -> ExtractedConstraints {
        let mut c = ExtractedConstraints::default();
        c.gotchas.push(Gotcha {
            title: "Critical Security Issue".to_string(),
            description: "Must validate all input".to_string(),
            when: "When processing user input".to_string(),
            solution: "Use sanitization".to_string(),
            related_files: vec!["src/auth.rs".to_string()],
        });
        c.implicit_rules.push(ImplicitRule {
            name: "Database Rule".to_string(),
            description: "Must use connection pool".to_string(),
            applies_to: vec!["src/db".to_string()],
            enforcement: RuleEnforcement::Convention,
            evidence: Vec::new(),
        });
        c
    }

    #[test]
    fn test_jaccard_similarity() {
        let a: HashSet<String> = ["hello", "world"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["hello", "world"].iter().map(|s| s.to_string()).collect();
        assert!((jaccard_similarity(&a, &b) - 1.0).abs() < 0.01);

        let c: HashSet<String> = ["foo", "bar"].iter().map(|s| s.to_string()).collect();
        assert!((jaccard_similarity(&a, &c) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_organize_by_artifact() {
        let engine = InsightEngine::new(Arc::new(MockProvider), Arc::new(Config::default()));

        let insights = vec![
            ExtractedInsight {
                insight: Insight {
                    id: "1".to_string(),
                    category: InsightCategory::SecurityConstraint,
                    title: "Security".to_string(),
                    description: "Validate input".to_string(),
                    prevention_info: None,
                    evidence: Vec::new(),
                    source: InsightSource::ConstraintDetection,
                    severity: None,
                },
                tier: TierClassification::Tier3,
                artifact: ArtifactClassification::Rules,
                value: ValueScore::default(),
            },
            ExtractedInsight {
                insight: Insight {
                    id: "2".to_string(),
                    category: InsightCategory::ArchitectureIntent,
                    title: "Architecture".to_string(),
                    description: "Use hexagonal".to_string(),
                    prevention_info: None,
                    evidence: Vec::new(),
                    source: InsightSource::DomainAnalysis,
                    severity: None,
                },
                tier: TierClassification::Tier2,
                artifact: ArtifactClassification::ClaudeMd,
                value: ValueScore::default(),
            },
        ];

        let organized = engine.organize_by_artifact(&insights);
        assert_eq!(organized.rules.len(), 1);
        assert_eq!(organized.claude_md.len(), 1);
    }

    #[tokio::test]
    async fn test_extract_basic() {
        let engine = InsightEngine::new(Arc::new(MockProvider), Arc::new(Config::default()));

        let conventions = create_test_conventions();
        let constraints = create_test_constraints();
        let registry = VerifiedFileRegistry::empty();

        let ctx = InsightContext {
            conventions: &conventions,
            constraints: &constraints,
            synthesis: None,
            file_registry: &registry,
        };

        let result = engine.extract(&ctx).await.unwrap();
        assert!(result.stats.total_extracted > 0);
    }
}
