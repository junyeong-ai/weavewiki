//! LLM-based Classification System
//!
//! Provides LLM-powered classification for insights with:
//! - Tier classification (0-3) based on semantic understanding
//! - Artifact routing (CLAUDE.md, Rules, Skills, Agents)
//! - Confidence scoring for hybrid decision making
//! - Batch processing for efficiency
//! - Caching layer to reduce API costs

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, trace, warn};

use crate::ai::LlmProvider;
use crate::config::Config;
use crate::types::Result;

use super::types::{ArtifactClassification, Insight, TierClassification};

// =============================================================================
// Core Types
// =============================================================================

/// Classification result with confidence score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationResult {
    /// Tier classification (0-3)
    pub tier: TierClassification,
    /// Target artifact type
    pub artifact: ArtifactClassification,
    /// Confidence score for tier classification (0.0-1.0)
    pub tier_confidence: f32,
    /// Confidence score for artifact classification (0.0-1.0)
    pub artifact_confidence: f32,
    /// Reasoning provided by LLM
    pub reasoning: Option<String>,
}

impl Default for ClassificationResult {
    fn default() -> Self {
        Self {
            tier: TierClassification::Tier2Convention,
            artifact: ArtifactClassification::ClaudeMd,
            tier_confidence: 0.5,
            artifact_confidence: 0.5,
            reasoning: None,
        }
    }
}

/// Batch classification request
#[derive(Debug, Clone)]
pub struct BatchClassificationRequest {
    pub insights: Vec<InsightSummary>,
    pub project_context: Option<String>,
}

/// Lightweight insight summary for batch processing
#[derive(Debug, Clone, Serialize)]
pub struct InsightSummary {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub has_evidence: bool,
    pub has_prevention: bool,
    pub severity: Option<String>,
}

impl From<&Insight> for InsightSummary {
    fn from(insight: &Insight) -> Self {
        Self {
            id: insight.id.clone(),
            title: insight.title.clone(),
            description: insight.description.clone(),
            category: format!("{:?}", insight.category),
            has_evidence: !insight.evidence.is_empty(),
            has_prevention: insight.prevention_info.is_some(),
            severity: insight.severity.clone(),
        }
    }
}

/// Batch classification response from LLM
#[derive(Debug, Clone, Deserialize)]
struct LlmBatchResponse {
    classifications: Vec<LlmClassificationItem>,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmClassificationItem {
    id: String,
    tier: u8,
    artifact: String,
    tier_confidence: f32,
    artifact_confidence: f32,
    reasoning: Option<String>,
}

// =============================================================================
// LlmClassifier Trait
// =============================================================================

/// Trait for LLM-based classification
#[async_trait]
pub trait LlmClassifier: Send + Sync {
    /// Classify a single insight
    async fn classify(&self, insight: &Insight) -> Result<ClassificationResult>;

    /// Classify multiple insights in a single batch
    async fn classify_batch(
        &self,
        insights: &[Insight],
        project_context: Option<&str>,
    ) -> Result<Vec<ClassificationResult>>;

    /// Check if classification should use LLM (vs structural rules)
    fn should_use_llm(&self, insight: &Insight) -> bool;

    /// Get classifier name
    fn name(&self) -> &str;
}

// =============================================================================
// Cache Implementation
// =============================================================================

/// Cache key for classification results
#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheKey {
    content_hash: u64,
}

impl CacheKey {
    fn from_insight(insight: &Insight) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        insight.title.hash(&mut hasher);
        insight.description.hash(&mut hasher);
        format!("{:?}", insight.category).hash(&mut hasher);
        Self {
            content_hash: hasher.finish(),
        }
    }
}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.content_hash.hash(state);
    }
}

/// Cached classification entry
#[derive(Debug, Clone)]
struct CacheEntry {
    result: ClassificationResult,
    created_at: Instant,
}

/// LRU cache for classification results
pub struct ClassificationCache {
    entries: RwLock<HashMap<CacheKey, CacheEntry>>,
    ttl: Duration,
    max_entries: usize,
}

impl ClassificationCache {
    pub fn new(ttl_hours: u64, max_entries: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_hours * 3600),
            max_entries,
        }
    }

    pub fn get(&self, insight: &Insight) -> Option<ClassificationResult> {
        let key = CacheKey::from_insight(insight);
        let entries = self.entries.read().ok()?;

        if let Some(entry) = entries.get(&key)
            && entry.created_at.elapsed() < self.ttl
        {
            trace!(insight_id = %insight.id, "Cache hit");
            return Some(entry.result.clone());
        }
        None
    }

    pub fn put(&self, insight: &Insight, result: ClassificationResult) {
        let key = CacheKey::from_insight(insight);
        let Ok(mut entries) = self.entries.write() else {
            return;
        };

        // Evict oldest entries if at capacity
        if entries.len() >= self.max_entries {
            Self::evict_oldest_entries(&mut entries);
        }

        entries.insert(
            key,
            CacheEntry {
                result,
                created_at: Instant::now(),
            },
        );
    }

    fn evict_oldest_entries(entries: &mut HashMap<CacheKey, CacheEntry>) {
        // Remove 10% of entries (oldest first)
        let to_remove = entries.len() / 10;
        if to_remove == 0 {
            return;
        }

        let mut oldest: Vec<_> = entries
            .iter()
            .map(|(k, v)| (k.clone(), v.created_at))
            .collect();
        oldest.sort_by_key(|(_, time)| *time);

        for (key, _) in oldest.into_iter().take(to_remove) {
            entries.remove(&key);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.write() {
            entries.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().map(|e| e.is_empty()).unwrap_or(true)
    }
}

// =============================================================================
// Default LLM Classifier Implementation
// =============================================================================

/// Default LLM-based classifier using configured provider
pub struct DefaultLlmClassifier {
    provider: Arc<dyn LlmProvider>,
    cache: ClassificationCache,
    batch_size: usize,
}

impl DefaultLlmClassifier {
    pub fn new(provider: Arc<dyn LlmProvider>, config: Arc<Config>) -> Self {
        let llm_config = &config.insight.classification;
        Self {
            provider,
            cache: ClassificationCache::new(
                llm_config.cache_ttl_hours,
                llm_config.cache_max_entries,
            ),
            batch_size: llm_config.llm_batch_size,
        }
    }

    /// Build classification prompt for single insight
    fn build_single_prompt(&self, insight: &Insight) -> String {
        format!(
            r#"You are classifying insights for Claude Code plugin generation.

Insight to classify:
- Title: {}
- Description: {}
- Category: {:?}
- Has Evidence: {}
- Has Prevention Info: {}
- Severity: {}

Classification criteria:

TIER CLASSIFICATION:
- Tier 0 (REJECT): Generic knowledge AI already knows (e.g., "use best practices", "handle errors", "write tests")
- Tier 1 (LOW): Information easily discoverable from code structure alone
- Tier 2 (MEDIUM): Project-specific conventions requiring analysis to discover
- Tier 3 (HIGH): Hidden constraints, gotchas, or critical knowledge that prevents mistakes

ARTIFACT ROUTING:
- rules: Mandatory constraints, forbidden actions, security requirements
- skills: Reusable procedures, workflows, step-by-step guides
- agents: Domain expertise, specialized knowledge, business rules
- claude_md: Architecture overview, project context, general guidance

Classify this insight with confidence scores (0.0-1.0).
Higher confidence = more certain about the classification."#,
            insight.title,
            insight.description,
            insight.category,
            !insight.evidence.is_empty(),
            insight.prevention_info.is_some(),
            insight.severity.as_deref().unwrap_or("none")
        )
    }

    /// Build batch classification prompt
    fn build_batch_prompt(
        &self,
        summaries: &[InsightSummary],
        project_context: Option<&str>,
    ) -> String {
        let insights_json = serde_json::to_string_pretty(summaries).unwrap_or_default();
        let context = project_context.unwrap_or("Not provided");

        format!(
            r#"You are classifying multiple insights for Claude Code plugin generation.

Project Context: {context}

Insights to classify:
{insights_json}

Classification criteria:

TIER CLASSIFICATION:
- Tier 0 (REJECT): Generic knowledge AI already knows (e.g., "use best practices", "handle errors")
- Tier 1 (LOW): Information easily discoverable from code structure
- Tier 2 (MEDIUM): Project-specific conventions requiring analysis
- Tier 3 (HIGH): Hidden constraints, gotchas, critical mistake-prevention knowledge

ARTIFACT ROUTING:
- rules: Mandatory constraints, forbidden actions, security requirements
- skills: Reusable procedures, workflows, step-by-step guides
- agents: Domain expertise, specialized knowledge, business rules
- claude_md: Architecture overview, project context, general guidance

Classify ALL insights with confidence scores (0.0-1.0).
Return classifications in the same order as the input."#
        )
    }

    /// JSON schema for single classification response
    fn single_response_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "tier": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 3,
                    "description": "Tier classification (0=reject, 1=low, 2=medium, 3=high)"
                },
                "artifact": {
                    "type": "string",
                    "enum": ["rules", "skills", "agents", "claude_md"],
                    "description": "Target artifact type"
                },
                "tier_confidence": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 1.0,
                    "description": "Confidence in tier classification"
                },
                "artifact_confidence": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 1.0,
                    "description": "Confidence in artifact classification"
                },
                "reasoning": {
                    "type": "string",
                    "description": "Brief reasoning for the classification"
                }
            },
            "required": ["tier", "artifact", "tier_confidence", "artifact_confidence"]
        })
    }

    /// JSON schema for batch classification response
    fn batch_response_schema(count: usize) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "classifications": {
                    "type": "array",
                    "minItems": count,
                    "maxItems": count,
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "tier": { "type": "integer", "minimum": 0, "maximum": 3 },
                            "artifact": { "type": "string", "enum": ["rules", "skills", "agents", "claude_md"] },
                            "tier_confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                            "artifact_confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                            "reasoning": { "type": "string" }
                        },
                        "required": ["id", "tier", "artifact", "tier_confidence", "artifact_confidence"]
                    }
                }
            },
            "required": ["classifications"]
        })
    }

    /// Convert tier number to enum
    fn tier_from_number(n: u8) -> TierClassification {
        match n {
            0 => TierClassification::Tier0Hallucinated,
            1 => TierClassification::Tier1Generic,
            2 => TierClassification::Tier2Convention,
            3 => TierClassification::Tier3Constraint,
            _ => TierClassification::Tier2Convention, // Default fallback
        }
    }

    /// Convert artifact string to enum
    fn artifact_from_string(s: &str) -> ArtifactClassification {
        match s.to_lowercase().as_str() {
            "rules" => ArtifactClassification::Rule,
            "skills" => ArtifactClassification::Skill,
            "agents" => ArtifactClassification::Agent,
            "claude_md" | "claudemd" => ArtifactClassification::ClaudeMd,
            _ => ArtifactClassification::ClaudeMd, // Default fallback
        }
    }

    /// Check if insight can be structurally classified (no LLM needed)
    fn can_classify_structurally(&self, insight: &Insight) -> Option<ClassificationResult> {
        // Tier 0 structural checks (no LLM needed)
        let text = format!("{} {}", insight.title, insight.description).to_lowercase();

        // Very short content is likely low value
        if text.len() < 20 {
            return Some(ClassificationResult {
                tier: TierClassification::Tier0Hallucinated,
                artifact: ArtifactClassification::ClaudeMd,
                tier_confidence: 0.9,
                artifact_confidence: 0.5,
                reasoning: Some("Content too short to be valuable".to_string()),
            });
        }

        // Clear Tier 3 indicators
        const TIER3_STRONG: &[&str] = &[
            "race condition",
            "deadlock",
            "memory leak",
            "data loss",
            "security vulnerability",
            "injection attack",
        ];

        for pattern in TIER3_STRONG {
            if text.contains(pattern) {
                return Some(ClassificationResult {
                    tier: TierClassification::Tier3Constraint,
                    artifact: ArtifactClassification::Rule,
                    tier_confidence: 0.95,
                    artifact_confidence: 0.8,
                    reasoning: Some(format!("Contains critical pattern: {}", pattern)),
                });
            }
        }

        None // Need LLM classification
    }
}

#[async_trait]
impl LlmClassifier for DefaultLlmClassifier {
    async fn classify(&self, insight: &Insight) -> Result<ClassificationResult> {
        // Check cache first
        if let Some(cached) = self.cache.get(insight) {
            return Ok(cached);
        }

        // Try structural classification
        if let Some(structural) = self.can_classify_structurally(insight) {
            self.cache.put(insight, structural.clone());
            return Ok(structural);
        }

        // LLM classification
        let prompt = self.build_single_prompt(insight);
        let schema = Self::single_response_schema();

        let response = self.provider.generate(&prompt, &schema).await?;

        #[derive(Deserialize)]
        struct SingleResponse {
            tier: u8,
            artifact: String,
            tier_confidence: f32,
            artifact_confidence: f32,
            reasoning: Option<String>,
        }

        let parsed: SingleResponse = serde_json::from_value(response.content)?;

        let result = ClassificationResult {
            tier: Self::tier_from_number(parsed.tier),
            artifact: Self::artifact_from_string(&parsed.artifact),
            tier_confidence: parsed.tier_confidence,
            artifact_confidence: parsed.artifact_confidence,
            reasoning: parsed.reasoning,
        };

        self.cache.put(insight, result.clone());
        Ok(result)
    }

    async fn classify_batch(
        &self,
        insights: &[Insight],
        project_context: Option<&str>,
    ) -> Result<Vec<ClassificationResult>> {
        if insights.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = vec![ClassificationResult::default(); insights.len()];
        let mut need_llm: Vec<(usize, &Insight)> = Vec::new();

        // First pass: check cache and structural classification
        for (idx, insight) in insights.iter().enumerate() {
            if let Some(cached) = self.cache.get(insight) {
                results[idx] = cached;
                continue;
            }

            if let Some(structural) = self.can_classify_structurally(insight) {
                self.cache.put(insight, structural.clone());
                results[idx] = structural;
                continue;
            }

            need_llm.push((idx, insight));
        }

        if need_llm.is_empty() {
            debug!("All insights classified from cache/structural rules");
            return Ok(results);
        }

        debug!(
            llm_needed = need_llm.len(),
            cached = insights.len() - need_llm.len(),
            "Classifying insights"
        );

        // Process in batches
        for chunk in need_llm.chunks(self.batch_size) {
            let summaries: Vec<InsightSummary> = chunk
                .iter()
                .map(|(_, i)| InsightSummary::from(*i))
                .collect();

            let prompt = self.build_batch_prompt(&summaries, project_context);
            let schema = Self::batch_response_schema(summaries.len());

            match self.provider.generate(&prompt, &schema).await {
                Ok(response) => {
                    let batch_result: LlmBatchResponse = serde_json::from_value(response.content)
                        .unwrap_or_else(|e| {
                            warn!("Failed to parse batch response: {}", e);
                            LlmBatchResponse {
                                classifications: Vec::new(),
                            }
                        });

                    // Map results back by ID
                    let id_map: HashMap<&str, &LlmClassificationItem> = batch_result
                        .classifications
                        .iter()
                        .map(|c| (c.id.as_str(), c))
                        .collect();

                    for (idx, insight) in chunk {
                        if let Some(item) = id_map.get(insight.id.as_str()) {
                            let result = ClassificationResult {
                                tier: Self::tier_from_number(item.tier),
                                artifact: Self::artifact_from_string(&item.artifact),
                                tier_confidence: item.tier_confidence,
                                artifact_confidence: item.artifact_confidence,
                                reasoning: item.reasoning.clone(),
                            };
                            self.cache.put(insight, result.clone());
                            results[*idx] = result;
                        }
                    }
                }
                Err(e) => {
                    warn!("Batch classification failed: {}. Using defaults.", e);
                    // Keep default results for failed batch
                }
            }
        }

        Ok(results)
    }

    fn should_use_llm(&self, insight: &Insight) -> bool {
        // Check cache first - if cached, no need for LLM
        if self.cache.get(insight).is_some() {
            return false;
        }

        // If can classify structurally, no LLM needed
        if self.can_classify_structurally(insight).is_some() {
            return false;
        }

        true
    }

    fn name(&self) -> &str {
        "default_llm_classifier"
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{LlmResponse, ResponseMetadata, ResponseTiming, TokenUsage};
    use crate::pipeline::insight::types::{InsightCategory, InsightSource};

    struct MockProvider {
        response: serde_json::Value,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(
            &self,
            _prompt: &str,
            _schema: &serde_json::Value,
        ) -> Result<LlmResponse> {
            Ok(LlmResponse {
                content: self.response.clone(),
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
    fn test_cache_key_consistency() {
        let insight1 = create_test_insight("Test", "Description");
        let insight2 = create_test_insight("Test", "Description");

        let key1 = CacheKey::from_insight(&insight1);
        let key2 = CacheKey::from_insight(&insight2);

        assert_eq!(key1.content_hash, key2.content_hash);
    }

    #[test]
    fn test_cache_put_get() {
        let cache = ClassificationCache::new(24, 100);
        let insight = create_test_insight("Test", "Description");
        let result = ClassificationResult {
            tier: TierClassification::Tier3Constraint,
            artifact: ArtifactClassification::Rule,
            tier_confidence: 0.9,
            artifact_confidence: 0.8,
            reasoning: Some("Test".to_string()),
        };

        cache.put(&insight, result.clone());
        let cached = cache.get(&insight).unwrap();

        assert_eq!(cached.tier, TierClassification::Tier3Constraint);
        assert_eq!(cached.artifact, ArtifactClassification::Rule);
    }

    #[test]
    fn test_structural_classification_short_content() {
        let provider = MockProvider {
            response: json!({}),
        };
        let config = Arc::new(Config::default());
        let classifier = DefaultLlmClassifier::new(Arc::new(provider), config);

        let insight = create_test_insight("X", "Y");
        let result = classifier.can_classify_structurally(&insight);

        assert!(result.is_some());
        assert_eq!(result.unwrap().tier, TierClassification::Tier0Hallucinated);
    }

    #[test]
    fn test_structural_classification_critical_pattern() {
        let provider = MockProvider {
            response: json!({}),
        };
        let config = Arc::new(Config::default());
        let classifier = DefaultLlmClassifier::new(Arc::new(provider), config);

        let insight = create_test_insight(
            "Race Condition in User Service",
            "There is a race condition when updating user data",
        );
        let result = classifier.can_classify_structurally(&insight);

        assert!(result.is_some());
        assert_eq!(result.unwrap().tier, TierClassification::Tier3Constraint);
    }

    #[tokio::test]
    async fn test_classify_uses_cache() {
        let provider = MockProvider {
            response: json!({
                "tier": 2,
                "artifact": "rules",
                "tier_confidence": 0.8,
                "artifact_confidence": 0.7,
                "reasoning": "Test"
            }),
        };
        let config = Arc::new(Config::default());
        let classifier = DefaultLlmClassifier::new(Arc::new(provider), config);

        let insight = create_test_insight("Test Insight", "This is a test description for caching");

        // First call
        let result1 = classifier.classify(&insight).await.unwrap();
        assert_eq!(result1.tier, TierClassification::Tier2Convention);

        // Second call should hit cache
        let result2 = classifier.classify(&insight).await.unwrap();
        assert_eq!(result1.tier, result2.tier);
    }

    #[test]
    fn test_tier_from_number() {
        assert_eq!(
            DefaultLlmClassifier::tier_from_number(0),
            TierClassification::Tier0Hallucinated
        );
        assert_eq!(
            DefaultLlmClassifier::tier_from_number(3),
            TierClassification::Tier3Constraint
        );
        assert_eq!(
            DefaultLlmClassifier::tier_from_number(99),
            TierClassification::Tier2Convention
        ); // fallback
    }

    #[test]
    fn test_artifact_from_string() {
        assert_eq!(
            DefaultLlmClassifier::artifact_from_string("rules"),
            ArtifactClassification::Rule
        );
        assert_eq!(
            DefaultLlmClassifier::artifact_from_string("SKILLS"),
            ArtifactClassification::Skill
        );
        assert_eq!(
            DefaultLlmClassifier::artifact_from_string("unknown"),
            ArtifactClassification::ClaudeMd
        ); // fallback
    }

    #[test]
    fn test_insight_summary_from_insight() {
        let mut insight = create_test_insight("Title", "Description");
        insight.evidence = vec!["file.rs".to_string()];
        insight.prevention_info = Some("Prevention".to_string());

        let summary = InsightSummary::from(&insight);

        assert_eq!(summary.title, "Title");
        assert!(summary.has_evidence);
        assert!(summary.has_prevention);
    }
}
