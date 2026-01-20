//! Insight-Driven Generation Module
//!
//! LLM-powered generation based on actual project insights rather than templates.
//! Key principles:
//! - No generic content (Tier 1) - only project-specific insights
//! - Every claim must be backed by file references
//! - Self-review loop for quality assurance
//! - Skip generation if no unique value can be provided

use std::sync::Arc;

use crate::ai::LlmProvider;
use crate::config::InsightDrivenGenConfig;
use crate::pipeline::analysis::SynthesizedAnalysis;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
use crate::pipeline::phases::convention_inference::InferredConventions;
use crate::types::{Result, Skill};

/// Generator that creates content based on LLM-driven project insights
pub struct InsightDrivenGenerator {
    provider: Arc<dyn LlmProvider>,
    file_registry: VerifiedFileRegistry,
    config: InsightDrivenGenConfig,
}

/// Result of value assessment before generation
#[derive(Debug, Clone)]
pub struct ValueAssessment {
    /// Overall value score (0.0 - 1.0)
    pub score: f32,
    /// Whether the content passes minimum value threshold
    pub passes_threshold: bool,
    /// Unique patterns found specific to this project
    pub unique_patterns: Vec<String>,
    /// Hidden dependencies that must be documented
    pub hidden_dependencies: Vec<String>,
    /// Gotchas that are worth warning about
    pub gotchas: Vec<String>,
    /// Valid file references available for evidence
    pub valid_references: Vec<String>,
    /// Reason for low value (if any)
    pub low_value_reason: Option<String>,
}

/// Context for generating a specific artifact
#[derive(Debug, Clone)]
pub struct GenerationContext<'a> {
    pub name: &'a str,
    pub module_path: Option<&'a str>,
    pub conventions: &'a InferredConventions,
    pub constraints: &'a ExtractedConstraints,
    pub synthesis: Option<&'a SynthesizedAnalysis>,
}

struct ReviewResult {
    score: f32,
    improvements: Vec<String>,
}

impl InsightDrivenGenerator {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        file_registry: VerifiedFileRegistry,
        config: InsightDrivenGenConfig,
    ) -> Self {
        Self {
            provider,
            file_registry,
            config,
        }
    }

    pub fn assess_skill_value(&self, context: &GenerationContext<'_>) -> ValueAssessment {
        let mut score = 0.0f32;
        let mut unique_patterns = Vec::new();
        let mut hidden_dependencies = Vec::new();
        let mut gotchas = Vec::new();
        let mut valid_references = Vec::new();
        let mut low_value_reason = None;

        if let Some(synthesis) = context.synthesis {
            if let Some(module) = context.module_path.and_then(|path| {
                synthesis.modules.iter().find(|m| m.path == path)
            }) {
                if !module.responsibility.is_empty() {
                    unique_patterns.push(format!("Responsibility: {}", module.responsibility));
                    score += 0.2;
                }

                if !module.public_items.is_empty() {
                    for item in module.public_items.iter().take(5) {
                        unique_patterns.push(format!("Public API: {}", item));
                    }
                    score += 0.1;
                }

                if !module.patterns.is_empty() {
                    for pattern in module.patterns.iter().take(3) {
                        unique_patterns.push(format!("Pattern: {}", pattern));
                    }
                    score += 0.15;
                }
            }

            for pattern in &synthesis.deep.patterns {
                if context.module_path.is_some_and(|p| {
                    pattern.locations.iter().any(|loc| loc.file.starts_with(p))
                }) {
                    unique_patterns.push(format!("{}: {}", pattern.name, pattern.description));
                    score += 0.15;
                }
            }
        }

        if let Some(path) = context.module_path {
            for dep in &context.constraints.hidden_dependencies {
                if dep.source.contains(path) || dep.target.contains(path) {
                    hidden_dependencies.push(format!(
                        "{} → {}: {}",
                        dep.source, dep.target, dep.description
                    ));
                    score += 0.2;
                }
            }
        }

        for gotcha in &context.constraints.gotchas {
            let module_path = context.module_path.unwrap_or("");
            if gotcha.related_files.iter().any(|f| f.contains(module_path))
                || gotcha.description.contains(module_path)
            {
                gotchas.push(format!("{}: {}", gotcha.title, gotcha.description));
                score += 0.15;
            }
        }

        if let Some(path) = context.module_path {
            let module_files = self.file_registry.files_in_directory(path);
            for file in module_files.into_iter().take(10) {
                valid_references.push(file);
            }
            if !valid_references.is_empty() {
                score += 0.1;
            }
        }

        for anti in &context.constraints.anti_patterns {
            if let Some(path) = context.module_path
                && anti.evidence.iter().any(|e| e.file.contains(path)) {
                    unique_patterns.push(format!("Anti-pattern: {}", anti.name));
                    score += 0.1;
                }
        }

        score = score.min(1.0);
        let passes_threshold = score >= self.config.min_value_score;

        if !passes_threshold {
            if unique_patterns.is_empty() && hidden_dependencies.is_empty() && gotchas.is_empty() {
                low_value_reason = Some("No unique patterns, hidden dependencies, or gotchas found".to_string());
            } else {
                low_value_reason = Some(format!(
                    "Value score {:.2} below threshold {:.2}",
                    score, self.config.min_value_score
                ));
            }
        }

        ValueAssessment {
            score,
            passes_threshold,
            unique_patterns,
            hidden_dependencies,
            gotchas,
            valid_references,
            low_value_reason,
        }
    }

    pub async fn generate_skill(
        &self,
        context: &GenerationContext<'_>,
    ) -> Result<Option<Skill>> {
        let assessment = self.assess_skill_value(context);

        if !assessment.passes_threshold {
            tracing::debug!(
                name = context.name,
                score = assessment.score,
                reason = ?assessment.low_value_reason,
                "Skipping skill generation due to low value"
            );
            return Ok(None);
        }

        let top_insights = if self.config.use_llm_decisions {
            self.extract_top_insights(context, &assessment).await?
        } else {
            assessment.unique_patterns.iter().take(3).cloned().collect()
        };

        let prompt = self.build_skill_prompt(context, &assessment, &top_insights);

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "body": { "type": "string" }
            },
            "required": ["name", "description", "body"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let name = response.content.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(context.name);
                let description = response.content.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let body = response.content.get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let final_body = if self.config.self_review_enabled {
                    self.self_review_loop(body, &assessment).await?
                } else {
                    body.to_string()
                };

                let skill = Skill::new(name, description, final_body)
                    .with_user_invocable(true);

                Ok(Some(skill))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to generate insight-driven skill");
                Ok(None)
            }
        }
    }

    async fn extract_top_insights(
        &self,
        context: &GenerationContext<'_>,
        assessment: &ValueAssessment,
    ) -> Result<Vec<String>> {
        let mut all_insights = Vec::new();
        all_insights.extend(assessment.unique_patterns.clone());
        all_insights.extend(assessment.hidden_dependencies.clone());
        all_insights.extend(assessment.gotchas.clone());

        if all_insights.len() <= 3 {
            return Ok(all_insights);
        }

        let prompt = format!(
            "Given these insights about {} module:\n{}\n\n\
            Select TOP 3 most valuable for an AI coding assistant.\n\
            Criteria: unique to this project, not generic, actionable.\n\n\
            Return JSON: {{\"top_insights\": [\"insight1\", \"insight2\", \"insight3\"]}}",
            context.module_path.unwrap_or("project"),
            all_insights.iter()
                .enumerate()
                .map(|(i, s)| format!("{}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": { "top_insights": { "type": "array", "items": { "type": "string" } } },
            "required": ["top_insights"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let insights = response.content.get("top_insights")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_else(|| all_insights.into_iter().take(3).collect());
                Ok(insights)
            }
            Err(_) => Ok(all_insights.into_iter().take(3).collect()),
        }
    }

    async fn self_review_loop(
        &self,
        initial_body: &str,
        assessment: &ValueAssessment,
    ) -> Result<String> {
        let mut current = initial_body.to_string();
        let mut last_score = 0.0f32;
        let mut consecutive_no_change = 0;

        for iteration in 0..self.config.max_review_iterations {
            let review = self.review_content(&current, assessment).await?;
            last_score = review.score;

            if review.score >= self.config.review_acceptance_threshold {
                tracing::debug!(
                    iteration = iteration,
                    score = review.score,
                    "Content passed self-review"
                );
                return Ok(current);
            }

            if review.improvements.is_empty() {
                tracing::warn!(
                    iteration = iteration,
                    score = review.score,
                    threshold = self.config.review_acceptance_threshold,
                    "Self-review stuck: no improvements suggested but quality below threshold"
                );
                break;
            }

            let improved = self.apply_improvements(&current, &review.improvements).await?;

            if improved == current {
                consecutive_no_change += 1;
                tracing::warn!(
                    iteration = iteration,
                    consecutive = consecutive_no_change,
                    "Improvement application produced no change"
                );
                if consecutive_no_change >= 2 {
                    tracing::warn!("Breaking review loop: consecutive no-change iterations");
                    break;
                }
            } else {
                consecutive_no_change = 0;
            }

            current = improved;
        }

        if last_score < self.config.review_acceptance_threshold {
            tracing::warn!(
                score = last_score,
                threshold = self.config.review_acceptance_threshold,
                "Self-review failed to meet quality threshold after all iterations"
            );
        }

        Ok(current)
    }

    async fn review_content(
        &self,
        body: &str,
        assessment: &ValueAssessment,
    ) -> Result<ReviewResult> {
        let available_refs = assessment.valid_references.iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");

        let prompt = format!(
            "Review this skill content for quality:\n\n{}\n\n\
            Available files for reference: {}\n\n\
            Score (0-1) based on:\n\
            1. Has @file:line references (not inline code blocks)\n\
            2. Project-specific (not generic)\n\
            3. Actionable instructions (must/should/avoid/use)\n\n\
            Return JSON: {{\"score\": 0.8, \"improvements\": [\"Add reference to @src/x.rs:10\"]}}",
            body, available_refs
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "score": { "type": "number" },
                "improvements": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["score", "improvements"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                // Try direct field access first
                let score = response.content.get("score")
                    .and_then(|v| v.as_f64())
                    .or_else(|| {
                        // Try parsing as string if wrapped
                        if let Some(s) = response.content.as_str()
                            && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
                                return parsed.get("score").and_then(|v| v.as_f64());
                            }
                        None
                    })
                    .unwrap_or_else(|| {
                        // Fallback: estimate based on content quality
                        let has_refs = body.contains("@src/") || body.contains("@lib/");
                        let has_actionable = body.to_lowercase().contains("must")
                            || body.to_lowercase().contains("should")
                            || body.to_lowercase().contains("avoid");
                        let base = if has_refs { 0.4 } else { 0.2 };
                        let bonus = if has_actionable { 0.2 } else { 0.0 };
                        tracing::debug!(
                            has_refs = has_refs,
                            has_actionable = has_actionable,
                            estimated_score = base + bonus,
                            "Using estimated score due to parse failure"
                        );
                        base + bonus
                    }) as f32;

                let improvements = response.content.get("improvements")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_else(|| {
                        // Generate default improvements based on content analysis
                        let mut default_improvements = Vec::new();
                        if !body.contains("@src/") && !body.contains("@lib/") {
                            default_improvements.push(format!(
                                "Add @file:line references. Available: {}",
                                available_refs.chars().take(100).collect::<String>()
                            ));
                        }
                        if !body.to_lowercase().contains("must")
                            && !body.to_lowercase().contains("should")
                        {
                            default_improvements.push(
                                "Add actionable directives (must/should/avoid)".to_string()
                            );
                        }
                        default_improvements
                    });

                tracing::debug!(
                    score = score,
                    improvements_count = improvements.len(),
                    "Review content completed"
                );
                Ok(ReviewResult { score, improvements })
            }
            Err(e) => {
                tracing::warn!(error = %e, "Review LLM call failed");
                // Provide actionable improvements even on failure
                let mut improvements = vec![];
                if !body.contains("@src/") {
                    improvements.push(format!(
                        "Add file references from: {}",
                        available_refs.chars().take(100).collect::<String>()
                    ));
                }
                if !body.to_lowercase().contains("must") {
                    improvements.push("Add actionable directives (must/should/avoid)".to_string());
                }
                Ok(ReviewResult { score: 0.3, improvements })
            }
        }
    }

    async fn apply_improvements(&self, body: &str, improvements: &[String]) -> Result<String> {
        // Extract available file references from improvements
        let file_hint = improvements.iter()
            .find(|s| s.contains("Available:") || s.contains("@src/"))
            .map(|s| s.as_str())
            .unwrap_or("");

        let prompt = format!(
            "Improve this skill documentation:\n\n{}\n\n\
            REQUIRED IMPROVEMENTS:\n{}\n\n\
            CRITICAL REQUIREMENTS:\n\
            1. Add @file:line references (e.g., @src/main.rs:42) instead of inline code\n\
            2. Use actionable language: must, should, avoid, use, prefer\n\
            3. Be project-specific, not generic\n\
            4. Max {} lines of inline code\n\n\
            {}\n\n\
            Return the improved content as JSON: {{\"improved_body\": \"## Title\\n\\nImproved content...\"}}",
            body,
            improvements.iter().enumerate()
                .map(|(i, s)| format!("{}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n"),
            self.config.max_inline_code_lines,
            if !file_hint.is_empty() { format!("FILE HINT: {}", file_hint) } else { String::new() }
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": { "improved_body": { "type": "string" } },
            "required": ["improved_body"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                // Try multiple extraction strategies
                let improved = response.content.get("improved_body")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| {
                        // Try parsing string response as JSON
                        if let Some(s) = response.content.as_str() {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
                                return parsed.get("improved_body")
                                    .and_then(|v| v.as_str())
                                    .map(String::from);
                            }
                            // If it's plain text that looks like content, use it
                            if s.contains("##") || s.contains("@src/") {
                                return Some(s.to_string());
                            }
                        }
                        None
                    })
                    .or_else(|| {
                        // Try extracting from nested content field
                        response.content.get("content")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    });

                match improved {
                    Some(new_body) if !new_body.is_empty() && new_body != body => {
                        tracing::debug!(
                            old_len = body.len(),
                            new_len = new_body.len(),
                            "Applied improvements successfully"
                        );
                        Ok(new_body)
                    }
                    _ => {
                        tracing::debug!("Improvement produced no change, trying manual enhancement");
                        // Manual enhancement: add basic improvements if LLM failed
                        let mut enhanced = body.to_string();
                        if !enhanced.contains("@src/") && !file_hint.is_empty() {
                            // Try to extract a file path from hint
                            if let Some(path) = file_hint.split_whitespace()
                                .find(|s| s.starts_with("src/") || s.starts_with("@src/"))
                            {
                                enhanced = format!("{}\n\nSee {} for implementation details.", enhanced, path);
                            }
                        }
                        if !enhanced.to_lowercase().contains("must") && !enhanced.to_lowercase().contains("should") {
                            enhanced = enhanced.replace("Use ", "You should use ");
                        }
                        Ok(enhanced)
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Apply improvements LLM call failed");
                // Manual enhancement on error
                let mut enhanced = body.to_string();
                if !enhanced.to_lowercase().contains("must") {
                    enhanced = enhanced.replace("Use ", "You must use ");
                }
                Ok(enhanced)
            }
        }
    }

    fn build_skill_prompt(
        &self,
        context: &GenerationContext<'_>,
        assessment: &ValueAssessment,
        top_insights: &[String],
    ) -> String {
        let mut prompt = format!(
            "Generate a skill for \"{}\" that provides UNIQUE VALUE to AI coding assistants.\n\n\
CONTEXT:\n\
Module: {}\n\
Architecture: {}\n\n",
            context.name,
            context.module_path.unwrap_or("(project-level)"),
            context.conventions.architecture.pattern_name,
        );

        if !top_insights.is_empty() {
            prompt.push_str("TOP INSIGHTS TO FOCUS ON:\n");
            for insight in top_insights {
                prompt.push_str(&format!("- {}\n", insight));
            }
        }

        if !assessment.hidden_dependencies.is_empty() {
            prompt.push_str("\nHIDDEN DEPENDENCIES (MUST document):\n");
            for dep in &assessment.hidden_dependencies {
                prompt.push_str(&format!("- {}\n", dep));
            }
        }

        if !assessment.gotchas.is_empty() {
            prompt.push_str("\nGOTCHAS (MUST warn about):\n");
            for gotcha in &assessment.gotchas {
                prompt.push_str(&format!("- {}\n", gotcha));
            }
        }

        if !assessment.valid_references.is_empty() {
            prompt.push_str("\nAVAILABLE FILES (MUST use @file:line format):\n");
            for file in assessment.valid_references.iter().take(15) {
                prompt.push_str(&format!("- @{}\n", file));
            }
        }

        let reference_instruction = if self.config.reference_only_mode {
            "FORBIDDEN: Do NOT include inline code blocks. Use ONLY @file:line references."
        } else {
            format!("Use @file:line references. Max {} lines inline code.", self.config.max_inline_code_lines).leak()
        };

        prompt.push_str(&format!(
            "\n\
CRITICAL REQUIREMENTS:\n\
1. NO generic build/test commands (Claude already knows them)\n\
2. NO basic language idioms or patterns\n\
3. {}\n\
4. Focus on THIS project's specific constraints and gotchas\n\
5. At least {} @file:line references required\n\
6. At least {} actionable statements (must/should/avoid/never)\n\
\n\
MANDATORY QUALITY (ALL must be satisfied):\n\
✓ Minimum {} @file:line references (e.g., @src/pipeline/mod.rs:42)\n\
✓ Minimum {} actionable directives (You must/should/avoid/never)\n\
✓ Project-specific constraints only (NOT generic advice)\n\
✓ Include \"## Gotchas\" or \"## Constraints\" section\n\
\n\
EXAMPLE of CORRECT format:\n\
## Pipeline Execution\n\
\n\
You must follow the phase order in @src/pipeline/adaptive.rs:45.\n\
Always verify preconditions via @src/pipeline/validation/mod.rs:20.\n\
\n\
### Gotchas\n\
- Never skip project detection - it sets critical context\n\
- Avoid direct LLM calls; use strategy layer @src/pipeline/strategy/mod.rs:10\n\
\n\
OUTPUT FORMAT (JSON):\n\
{{\"name\": \"skill-name\", \"description\": \"one line\", \"body\": \"## Title\\n\\nContent...\"}}\n",
            reference_instruction,
            self.config.min_file_refs,
            self.config.min_actionable_statements,
            self.config.min_file_refs,
            self.config.min_actionable_statements,
        ));

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InsightDrivenGenConfig;
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization,
        NamingConventions, TestingConvention,
    };

    fn create_test_conventions() -> InferredConventions {
        InferredConventions {
            architecture: ArchitectureConvention {
                pattern_name: "Hexagonal".to_string(),
                description: "Port and adapter pattern".to_string(),
                layers: Vec::new(),
                data_flow: String::new(),
                confidence: 0.8,
            },
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        }
    }

    fn create_test_constraints() -> ExtractedConstraints {
        ExtractedConstraints {
            anti_patterns: Vec::new(),
            hidden_dependencies: Vec::new(),
            complex_workflows: Vec::new(),
            implicit_rules: Vec::new(),
            gotchas: Vec::new(),
        }
    }

    #[test]
    fn test_value_assessment_empty_context() {
        let registry = VerifiedFileRegistry::empty();

        let generator = InsightDrivenGenerator::new(
            std::sync::Arc::new(MockProvider),
            registry,
            InsightDrivenGenConfig::default(),
        );

        let conventions = create_test_conventions();
        let constraints = create_test_constraints();

        let context = GenerationContext {
            name: "test-skill",
            module_path: None,
            conventions: &conventions,
            constraints: &constraints,
            synthesis: None,
        };

        let assessment = generator.assess_skill_value(&context);

        assert!(!assessment.passes_threshold);
        assert!(assessment.low_value_reason.is_some());
    }

    #[test]
    fn test_value_assessment_with_dependencies() {
        use crate::pipeline::phases::constraint_extraction::{HiddenDependency, HiddenDepType};

        let registry = VerifiedFileRegistry::empty();

        let generator = InsightDrivenGenerator::new(
            std::sync::Arc::new(MockProvider),
            registry,
            InsightDrivenGenConfig::default(),
        );

        let conventions = create_test_conventions();
        let mut constraints = create_test_constraints();

        // Add hidden dependencies
        constraints.hidden_dependencies.push(HiddenDependency {
            source: "src/api/".to_string(),
            target: "src/domain/".to_string(),
            dependency_type: HiddenDepType::ImplicitOrdering,
            description: "API must validate before domain call".to_string(),
            impact: "Validation failures at domain level".to_string(),
            evidence: Vec::new(),
        });

        let context = GenerationContext {
            name: "api-validation",
            module_path: Some("src/api/"),
            conventions: &conventions,
            constraints: &constraints,
            synthesis: None,
        };

        let assessment = generator.assess_skill_value(&context);

        // With hidden dependency, should have higher value
        assert!(!assessment.hidden_dependencies.is_empty());
        assert!(assessment.score > 0.0);
    }

    // Mock provider for tests
    struct MockProvider;

    #[async_trait::async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(
            &self,
            _prompt: &str,
            _schema: &serde_json::Value,
        ) -> crate::types::Result<crate::ai::LlmResponse> {
            Ok(crate::ai::LlmResponse {
                content: serde_json::json!({
                    "name": "test-skill",
                    "description": "Test skill",
                    "body": "## Test\n\nBody text @src/main.rs:10"
                }),
                usage: crate::ai::TokenUsage::default(),
                cost_usd: 0.0,
                timing: crate::ai::ResponseTiming::default(),
                metadata: crate::ai::ResponseMetadata::default(),
            })
        }

        fn name(&self) -> &str {
            "mock"
        }

        fn model(&self) -> &str {
            "mock-model"
        }

        async fn health_check(&self) -> crate::types::Result<bool> {
            Ok(true)
        }
    }
}
