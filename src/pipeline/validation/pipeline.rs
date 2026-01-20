//! Validation Pipeline Orchestrator
//!
//! Coordinates the 5-layer validation pipeline:
//! - Layer 0: Format (programmatic)
//! - Layer 1: Evidence (programmatic + file I/O)
//! - Layer 2: Semantic Context (LLM + file reading)
//! - Layer 3: Value Assessment (LLM + few-shot)
//! - Layer 4: Cross-Artifact (LLM)

use std::path::Path;
use std::sync::Arc;

use tracing::{debug, info};

use crate::ai::LlmProvider;
use crate::config::{ProjectType, QualityConfig, ValidationConfig};
use crate::pipeline::context::VerifiedFileRegistry;
use crate::types::{Agent, Plugin, ProjectMemory, Result, Rule, Skill};

use super::clean_pass::{CleanPassStatus, CleanPassTracker};
use super::evidence::{validate_evidence, EnhancedEvidenceResult};
use super::layers::{
    IssueCode, IssueSeverity, LayerResult, ValidationIssue, ValidationLayer, ValidationResults,
};
use super::semantic_context::SemanticContextValidator;
use super::value_assessor::ValueAssessor;

pub struct ValidationPipeline {
    provider: Arc<dyn LlmProvider>,
    config: ValidationConfig,
    quality_config: QualityConfig,
    project_type: ProjectType,
    file_registry: VerifiedFileRegistry,
    project_root: std::path::PathBuf,
    clean_pass_tracker: CleanPassTracker,
}

impl ValidationPipeline {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        config: ValidationConfig,
        quality_config: QualityConfig,
        project_type: ProjectType,
        file_registry: VerifiedFileRegistry,
        project_root: impl AsRef<Path>,
    ) -> Self {
        let clean_pass_tracker = CleanPassTracker::new(config.clean_pass.consecutive_passes)
            .with_max_attempts(config.clean_pass.max_attempts);

        Self {
            provider,
            config,
            quality_config,
            project_type,
            file_registry,
            project_root: project_root.as_ref().to_path_buf(),
            clean_pass_tracker,
        }
    }

    pub async fn validate(
        &mut self,
        plugin: &Plugin,
        memory: &ProjectMemory,
    ) -> Result<ValidationResults> {
        if !self.config.enabled {
            info!("Validation pipeline disabled, skipping");
            return Ok(ValidationResults::new());
        }

        let mut results = ValidationResults::new();

        if self.config.layers.format_enabled {
            let format_result = self.validate_format(plugin, memory);
            let has_critical = format_result.critical_count() > 0;
            results.add_layer_result(format_result);

            if has_critical && self.config.layers.early_exit_on_critical {
                debug!("Critical format issues found, early exit");
                return Ok(results);
            }
        }

        if self.config.layers.evidence_enabled {
            let evidence_result = self.validate_evidence(plugin, memory).await?;
            let has_critical = evidence_result.critical_count() > 0;
            results.add_layer_result(evidence_result);

            if has_critical && self.config.layers.early_exit_on_critical {
                debug!("Critical evidence issues found, early exit");
                return Ok(results);
            }
        }

        if self.config.layers.semantic_context_enabled {
            let semantic_result = self.validate_semantic_context(plugin, memory).await?;
            results.add_layer_result(semantic_result);
        }

        if self.config.layers.value_assessment_enabled {
            let value_result = self.validate_value(plugin).await?;
            results.add_layer_result(value_result);
        }

        if self.config.layers.cross_artifact_enabled {
            let cross_result = self.validate_cross_artifact(plugin).await?;
            results.add_layer_result(cross_result);
        }

        Ok(results)
    }

    pub fn record_validation_attempt(
        &mut self,
        results: &ValidationResults,
    ) -> CleanPassStatus {
        self.clean_pass_tracker.record_attempt(results)
    }

    pub fn clean_pass_status(&self) -> CleanPassStatus {
        if self.clean_pass_tracker.current_streak() >= self.clean_pass_tracker.required_passes() {
            CleanPassStatus::Converged {
                passes: self.clean_pass_tracker.current_streak(),
            }
        } else {
            CleanPassStatus::InProgress {
                streak: self.clean_pass_tracker.current_streak(),
                required: self.clean_pass_tracker.required_passes(),
            }
        }
    }

    pub fn reset_clean_pass(&mut self) {
        self.clean_pass_tracker.reset();
    }

    fn validate_format(&self, plugin: &Plugin, memory: &ProjectMemory) -> LayerResult {
        let mut issues = Vec::new();

        for skill in &plugin.skills {
            issues.extend(self.validate_skill_format(skill));
        }

        for agent in &plugin.agents {
            issues.extend(self.validate_agent_format(agent));
        }

        for rule in &plugin.rules {
            issues.extend(self.validate_rule_format(rule));
        }

        if memory.overview.is_empty() {
            issues.push(ValidationIssue::error(
                ValidationLayer::Format,
                "CLAUDE.md",
                IssueCode::InvalidStructure,
                "CLAUDE.md has no content",
            ));
        }

        if issues.is_empty() {
            LayerResult::pass(ValidationLayer::Format)
        } else {
            LayerResult::fail(ValidationLayer::Format, issues)
        }
    }

    fn validate_skill_format(&self, skill: &Skill) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if skill.name.is_empty() {
            issues.push(ValidationIssue::critical(
                ValidationLayer::Format,
                "skill:unknown",
                IssueCode::MissingRequiredField,
                "Skill missing required 'name' field",
            ));
        }

        if skill.description.is_empty() {
            issues.push(ValidationIssue::error(
                ValidationLayer::Format,
                format!("skill:{}", skill.name),
                IssueCode::MissingRequiredField,
                "Skill missing required 'description' field",
            ));
        }

        if skill.body.lines().count() < 3 {
            issues.push(
                ValidationIssue::warning(
                    ValidationLayer::Format,
                    format!("skill:{}", skill.name),
                    IssueCode::InvalidStructure,
                    "Skill body is too short (< 3 lines)",
                )
                .with_suggestion("Add detailed steps and evidence references"),
            );
        }

        issues
    }

    fn validate_agent_format(&self, agent: &Agent) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if agent.name.is_empty() {
            issues.push(ValidationIssue::critical(
                ValidationLayer::Format,
                "agent:unknown",
                IssueCode::MissingRequiredField,
                "Agent missing required 'name' field",
            ));
        }

        if agent.description.is_empty() {
            issues.push(ValidationIssue::error(
                ValidationLayer::Format,
                format!("agent:{}", agent.name),
                IssueCode::MissingRequiredField,
                "Agent missing required 'description' field",
            ));
        }

        let generic_patterns = ["help with", "assist with", "general purpose"];
        let desc_lower = agent.description.to_lowercase();
        for pattern in generic_patterns {
            if desc_lower.contains(pattern) {
                issues.push(
                    ValidationIssue::error(
                        ValidationLayer::Format,
                        format!("agent:{}", agent.name),
                        IssueCode::GenericContent,
                        format!("Agent description contains generic phrase: '{}'", pattern),
                    )
                    .with_suggestion("Use specific domain language describing expertise"),
                );
            }
        }

        issues
    }

    fn validate_rule_format(&self, rule: &Rule) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if rule.name.is_empty() {
            issues.push(ValidationIssue::critical(
                ValidationLayer::Format,
                "rule:unknown",
                IssueCode::MissingRequiredField,
                "Rule missing required 'name' field",
            ));
        }

        if rule.content.is_empty() {
            issues.push(ValidationIssue::error(
                ValidationLayer::Format,
                format!("rule:{}", rule.name),
                IssueCode::MissingRequiredField,
                "Rule has no content",
            ));
        }

        issues
    }

    async fn validate_evidence(
        &self,
        plugin: &Plugin,
        memory: &ProjectMemory,
    ) -> Result<LayerResult> {
        let evidence_result: EnhancedEvidenceResult = validate_evidence(
            self.project_type,
            &self.quality_config,
            self.file_registry.clone(),
            &self.project_root,
            &plugin.skills,
            &plugin.agents,
            &plugin.rules,
            memory,
        )
        .await;

        let issues: Vec<ValidationIssue> = evidence_result
            .issues
            .iter()
            .map(|ei| {
                let severity = match ei.severity {
                    super::evidence::IssueSeverity::Critical => IssueSeverity::Critical,
                    super::evidence::IssueSeverity::High => IssueSeverity::Error,
                    super::evidence::IssueSeverity::Medium => IssueSeverity::Warning,
                    super::evidence::IssueSeverity::Low => IssueSeverity::Info,
                };

                let code = match ei.category {
                    super::evidence::IssueCategory::InsufficientReferences => {
                        IssueCode::InsufficientReferences
                    }
                    super::evidence::IssueCategory::HallucinatedReference => {
                        IssueCode::FileNotFound
                    }
                    super::evidence::IssueCategory::InsufficientDepth => {
                        IssueCode::InsufficientReferences
                    }
                    super::evidence::IssueCategory::MissingLineNumber => {
                        IssueCode::InvalidReference
                    }
                    super::evidence::IssueCategory::MissingContext => {
                        IssueCode::InvalidReference
                    }
                };

                ValidationIssue::new(ValidationLayer::Evidence, severity, &ei.artifact, code, &ei.description)
                    .with_suggestion(&ei.suggestion)
            })
            .collect();

        if issues.is_empty() {
            Ok(LayerResult::pass(ValidationLayer::Evidence)
                .with_score(evidence_result.overall_score)
                .with_metadata(
                    "valid_references",
                    evidence_result.summary.valid_references.to_string(),
                )
                .with_metadata(
                    "hallucinated",
                    evidence_result.summary.hallucinated_references.to_string(),
                ))
        } else {
            Ok(LayerResult::fail(ValidationLayer::Evidence, issues)
                .with_score(evidence_result.overall_score))
        }
    }

    async fn validate_semantic_context(
        &self,
        plugin: &Plugin,
        memory: &ProjectMemory,
    ) -> Result<LayerResult> {
        let mut validator = SemanticContextValidator::new(
            Arc::clone(&self.provider),
            self.config.semantic_context.clone(),
            self.file_registry.clone(),
            &self.project_root,
        );

        let mut artifacts: Vec<(String, String)> = Vec::new();

        for skill in &plugin.skills {
            artifacts.push((format!("skill:{}", skill.name), skill.body.clone()));
        }

        for agent in &plugin.agents {
            artifacts.push((format!("agent:{}", agent.name), agent.prompt.clone()));
        }

        for rule in &plugin.rules {
            artifacts.push((
                format!("rule:{}", rule.name),
                rule.content.join("\n"),
            ));
        }

        artifacts.push(("CLAUDE.md".to_string(), memory.to_markdown()));

        validator.validate(&artifacts).await
    }

    async fn validate_value(&self, plugin: &Plugin) -> Result<LayerResult> {
        let assessor = ValueAssessor::new(
            Arc::clone(&self.provider),
            self.config.value_assessment.clone(),
        );

        let mut items: Vec<(String, String, String)> = Vec::new();

        for skill in &plugin.skills {
            items.push((
                format!("skill:{}", skill.name),
                "skill".to_string(),
                format!("{}\n\n{}", skill.description, skill.body),
            ));
        }

        for agent in &plugin.agents {
            items.push((
                format!("agent:{}", agent.name),
                "agent".to_string(),
                format!("{}\n\n{}", agent.description, agent.prompt),
            ));
        }

        for rule in &plugin.rules {
            items.push((
                format!("rule:{}", rule.name),
                "rule".to_string(),
                rule.content.join("\n"),
            ));
        }

        assessor.assess(&items).await
    }

    async fn validate_cross_artifact(&self, plugin: &Plugin) -> Result<LayerResult> {
        let mut issues = Vec::new();

        let mut reference_map: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();

        let ref_pattern = regex::Regex::new(r"@([a-zA-Z0-9_./\-]+:\d+)").expect("Invalid regex");

        for skill in &plugin.skills {
            for cap in ref_pattern.captures_iter(&skill.body) {
                if let Some(m) = cap.get(1) {
                    reference_map
                        .entry(m.as_str().to_string())
                        .or_default()
                        .push((format!("skill:{}", skill.name), skill.description.clone()));
                }
            }
        }

        for agent in &plugin.agents {
            for cap in ref_pattern.captures_iter(&agent.prompt) {
                if let Some(m) = cap.get(1) {
                    reference_map
                        .entry(m.as_str().to_string())
                        .or_default()
                        .push((format!("agent:{}", agent.name), agent.description.clone()));
                }
            }
        }

        for (reference, artifacts) in &reference_map {
            if artifacts.len() > 1 {
                let descriptions: Vec<_> = artifacts.iter().map(|(_, desc)| desc.as_str()).collect();

                if descriptions.windows(2).any(|w| !w[0].is_empty() && !w[1].is_empty() && w[0] != w[1]) {
                    let artifact_names: Vec<_> =
                        artifacts.iter().map(|(name, _)| name.as_str()).collect();
                    issues.push(
                        ValidationIssue::warning(
                            ValidationLayer::CrossArtifact,
                            artifact_names.join(", "),
                            IssueCode::InconsistentDescription,
                            format!(
                                "Reference {} used in multiple artifacts with different contexts",
                                reference
                            ),
                        )
                        .with_suggestion("Ensure consistent descriptions for shared references"),
                    );
                }
            }
        }

        for skill in &plugin.skills {
            for other_skill in &plugin.skills {
                if skill.name != other_skill.name {
                    let similarity = self.content_similarity(&skill.body, &other_skill.body);
                    if similarity > 0.8 {
                        issues.push(
                            ValidationIssue::warning(
                                ValidationLayer::CrossArtifact,
                                format!("skill:{}, skill:{}", skill.name, other_skill.name),
                                IssueCode::DuplicateContent,
                                format!(
                                    "High content similarity ({:.0}%) between skills",
                                    similarity * 100.0
                                ),
                            )
                            .with_suggestion("Consider merging or differentiating these skills"),
                        );
                    }
                }
            }
        }

        if issues.is_empty() {
            Ok(LayerResult::pass(ValidationLayer::CrossArtifact))
        } else {
            Ok(LayerResult::fail(ValidationLayer::CrossArtifact, issues))
        }
    }

    /// Calculate content similarity with stopword filtering and bigram overlap
    ///
    /// Uses a hybrid approach:
    /// 1. Filter common stopwords to reduce false positives
    /// 2. Combine unigram Jaccard similarity (60%) with bigram overlap (40%)
    /// 3. This accounts for both vocabulary overlap and word order
    fn content_similarity(&self, a: &str, b: &str) -> f32 {
        const STOPWORDS: &[&str] = &[
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for",
            "of", "with", "by", "from", "is", "are", "was", "were", "be", "been",
            "being", "have", "has", "had", "do", "does", "did", "will", "would",
            "could", "should", "may", "might", "must", "shall", "can", "it", "its",
            "this", "that", "these", "those", "as", "if", "when", "where", "which",
            "who", "whom", "what", "how", "than", "then", "so", "no", "not", "only",
            "just", "more", "most", "other", "some", "any", "all", "each", "every",
            "both", "few", "many", "much", "such", "own", "same", "into", "through",
            "during", "before", "after", "above", "below", "between", "under", "over",
        ];

        let normalize = |s: &str| -> Vec<String> {
            s.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() > 2 && !STOPWORDS.contains(w))
                .map(|s| s.to_string())
                .collect()
        };

        let a_words = normalize(a);
        let b_words = normalize(b);

        if a_words.is_empty() || b_words.is_empty() {
            return 0.0;
        }

        // Unigram Jaccard similarity (vocabulary overlap)
        let a_set: std::collections::HashSet<_> = a_words.iter().collect();
        let b_set: std::collections::HashSet<_> = b_words.iter().collect();
        let intersection = a_set.intersection(&b_set).count();
        let union = a_set.union(&b_set).count();
        let unigram_sim = intersection as f32 / union as f32;

        // Bigram overlap (word order awareness)
        let bigrams = |words: &[String]| -> std::collections::HashSet<String> {
            words
                .windows(2)
                .map(|w| format!("{} {}", w[0], w[1]))
                .collect()
        };

        let a_bigrams = bigrams(&a_words);
        let b_bigrams = bigrams(&b_words);

        let bigram_sim = if a_bigrams.is_empty() || b_bigrams.is_empty() {
            0.0
        } else {
            let bi_intersection = a_bigrams.intersection(&b_bigrams).count();
            let bi_union = a_bigrams.union(&b_bigrams).count();
            bi_intersection as f32 / bi_union as f32
        };

        // Weighted combination: 60% unigram + 40% bigram
        unigram_sim * 0.6 + bigram_sim * 0.4
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Skill;
    use serde_json::json;

    fn create_test_pipeline() -> ValidationPipeline {
        struct MockProvider;

        #[async_trait::async_trait]
        impl LlmProvider for MockProvider {
            async fn generate(
                &self,
                _prompt: &str,
                _schema: &serde_json::Value,
            ) -> crate::types::Result<crate::ai::LlmResponse> {
                Ok(crate::ai::LlmResponse::content_only(json!({})))
            }
            fn name(&self) -> &str {
                "mock"
            }
            fn model(&self) -> &str {
                "mock"
            }
            async fn health_check(&self) -> crate::types::Result<bool> {
                Ok(true)
            }
        }

        ValidationPipeline::new(
            Arc::new(MockProvider),
            ValidationConfig::default(),
            QualityConfig::default(),
            ProjectType::Auto,
            VerifiedFileRegistry::empty(),
            "/tmp",
        )
    }

    #[test]
    fn test_format_validation_empty_skill() {
        let pipeline = create_test_pipeline();
        let skill = Skill::new("", "desc", "body");
        let issues = pipeline.validate_skill_format(&skill);

        assert!(!issues.is_empty());
        assert!(issues
            .iter()
            .any(|i| i.code == IssueCode::MissingRequiredField));
    }

    #[test]
    fn test_format_validation_valid_skill() {
        let pipeline = create_test_pipeline();
        let skill = Skill::new(
            "test-skill",
            "A specific skill description",
            "Step 1\nStep 2\nStep 3\nStep 4",
        );
        let issues = pipeline.validate_skill_format(&skill);

        assert!(issues.is_empty());
    }

    #[test]
    fn test_generic_agent_detection() {
        let pipeline = create_test_pipeline();
        let agent = Agent::new("helper", "Help with any task", "prompt");
        let issues = pipeline.validate_agent_format(&agent);

        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| i.code == IssueCode::GenericContent));
    }

    #[test]
    fn test_content_similarity() {
        let pipeline = create_test_pipeline();

        let a = "the quick brown fox jumps over the lazy dog";
        let b = "the quick brown fox jumps over the lazy dog";
        assert!(pipeline.content_similarity(a, b) > 0.99);

        let c = "completely different content here";
        assert!(pipeline.content_similarity(a, c) < 0.3);
    }

    #[test]
    fn test_clean_pass_tracking() {
        let mut pipeline = create_test_pipeline();

        let clean = ValidationResults::new();
        let status = pipeline.record_validation_attempt(&clean);
        assert!(matches!(status, CleanPassStatus::InProgress { streak: 1, .. }));

        let status = pipeline.record_validation_attempt(&clean);
        assert!(matches!(status, CleanPassStatus::Converged { passes: 2 }));
    }
}
