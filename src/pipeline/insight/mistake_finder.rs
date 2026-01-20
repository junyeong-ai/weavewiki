//! Mistake Finder
//!
//! Identifies potential mistakes AI could make without proper documentation.
//! Core question: "What would AI get wrong without this information?"

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::ai::LlmProvider;
use crate::config::Config;
use crate::pipeline::phases::convention_inference::AsyncStyle;
use crate::types::Result;

use super::InsightContext;

/// Severity of potential mistake
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MistakeSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl MistakeSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub fn score(&self) -> f32 {
        match self {
            Self::Low => 0.3,
            Self::Medium => 0.5,
            Self::High => 0.7,
            Self::Critical => 1.0,
        }
    }
}

/// A potential mistake AI could make
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PotentialMistake {
    pub title: String,
    pub description: String,
    pub category: MistakeCategory,
    pub severity: MistakeSeverity,
    pub prevention: String,
    pub evidence: Vec<String>,
    pub likelihood: f32,
}

/// Category of potential mistake
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MistakeCategory {
    Concurrency,
    InitOrder,
    ErrorHandling,
    Security,
    BusinessLogic,
    Architecture,
    Performance,
    ResourceManagement,
}

impl MistakeCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Concurrency => "concurrency",
            Self::InitOrder => "init_order",
            Self::ErrorHandling => "error_handling",
            Self::Security => "security",
            Self::BusinessLogic => "business_logic",
            Self::Architecture => "architecture",
            Self::Performance => "performance",
            Self::ResourceManagement => "resource_management",
        }
    }
}

/// Finds potential AI mistakes by analyzing project patterns
pub struct MistakeFinder {
    provider: Arc<dyn LlmProvider>,
    config: Arc<Config>,
}

impl MistakeFinder {
    pub fn new(provider: Arc<dyn LlmProvider>, config: Arc<Config>) -> Self {
        Self { provider, config }
    }

    pub async fn find_potential_mistakes(
        &self,
        ctx: &InsightContext<'_>,
    ) -> Result<Vec<PotentialMistake>> {
        let mut mistakes = Vec::new();
        let mistake_config = &self.config.insight.mistakes;

        // Run enabled detectors
        if mistake_config.detect_concurrency {
            mistakes.extend(self.detect_concurrency_mistakes(ctx));
        }
        if mistake_config.detect_init_order {
            mistakes.extend(self.detect_init_order_mistakes(ctx));
        }
        if mistake_config.detect_error_handling {
            mistakes.extend(self.detect_error_handling_mistakes(ctx));
        }
        if mistake_config.detect_resource_management {
            mistakes.extend(self.detect_resource_mistakes(ctx));
        }

        if self.should_use_llm_discovery() {
            let llm_mistakes = self.discover_mistakes_with_llm(ctx).await?;
            mistakes.extend(llm_mistakes);
        }

        // Filter by configurable minimum likelihood
        let min_likelihood = mistake_config.min_likelihood;
        mistakes.retain(|m| m.likelihood >= min_likelihood);

        mistakes.sort_by(|a, b| {
            let score_a = a.severity.score() * a.likelihood;
            let score_b = b.severity.score() * b.likelihood;
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        debug!(count = mistakes.len(), min_likelihood, "Found potential mistakes");

        Ok(mistakes)
    }

    fn should_use_llm_discovery(&self) -> bool {
        matches!(
            self.config.analysis.depth,
            crate::config::AnalysisDepth::Complete | crate::config::AnalysisDepth::Standard
        )
    }

    fn detect_concurrency_mistakes(&self, ctx: &InsightContext<'_>) -> Vec<PotentialMistake> {
        let mut mistakes = Vec::new();

        for dep in &ctx.constraints.hidden_dependencies {
            if dep.description.to_lowercase().contains("shared")
                || dep.description.to_lowercase().contains("arc")
                || dep.description.to_lowercase().contains("mutex")
                || dep.description.to_lowercase().contains("rwlock")
            {
                mistakes.push(PotentialMistake {
                    title: format!("Shared state between {} and {}", dep.source, dep.target),
                    description: format!(
                        "Shared state detected: {}. Improper handling can cause race conditions.",
                        dep.description
                    ),
                    category: MistakeCategory::Concurrency,
                    severity: MistakeSeverity::High,
                    prevention: format!(
                        "Always use proper synchronization when accessing shared state. {}",
                        dep.impact
                    ),
                    evidence: dep.evidence.iter().map(|e| e.file.clone()).collect(),
                    likelihood: 0.7,
                });
            }
        }

        // Check async style
        if ctx.conventions.async_pattern.style != AsyncStyle::Synchronous {
            let runtime_info = ctx
                .conventions
                .async_pattern
                .runtime
                .as_deref()
                .unwrap_or("unknown");

            mistakes.push(PotentialMistake {
                title: "Async code requires careful handling".to_string(),
                description: format!(
                    "Project uses {:?} async style with {} runtime. Common mistakes include blocking in async context.",
                    ctx.conventions.async_pattern.style, runtime_info
                ),
                category: MistakeCategory::Concurrency,
                severity: MistakeSeverity::Medium,
                prevention: format!(
                    "Follow the project's async conventions with {:?} style. Avoid blocking operations.",
                    ctx.conventions.async_pattern.style
                ),
                evidence: Vec::new(),
                likelihood: 0.5,
            });
        }

        mistakes
    }

    fn detect_init_order_mistakes(&self, ctx: &InsightContext<'_>) -> Vec<PotentialMistake> {
        let mut mistakes = Vec::new();

        for dep in &ctx.constraints.hidden_dependencies {
            let desc_lower = dep.description.to_lowercase();
            if desc_lower.contains("init")
                || desc_lower.contains("before")
                || desc_lower.contains("order")
                || desc_lower.contains("depends on")
            {
                mistakes.push(PotentialMistake {
                    title: format!("Initialization order: {} before {}", dep.source, dep.target),
                    description: dep.description.clone(),
                    category: MistakeCategory::InitOrder,
                    severity: MistakeSeverity::High,
                    prevention: format!(
                        "Ensure {} is initialized/called before {}. {}",
                        dep.source, dep.target, dep.impact
                    ),
                    evidence: dep.evidence.iter().map(|e| e.file.clone()).collect(),
                    likelihood: 0.8,
                });
            }
        }

        mistakes
    }

    fn detect_error_handling_mistakes(&self, ctx: &InsightContext<'_>) -> Vec<PotentialMistake> {
        let mut mistakes = Vec::new();

        let error_pattern = &ctx.conventions.error_handling;
        let style_name = format!("{:?}", error_pattern.style);

        mistakes.push(PotentialMistake {
            title: format!("Follow {} error pattern", style_name),
            description: format!(
                "Project uses {:?} error handling. Inconsistent error handling breaks the pattern.",
                error_pattern.style
            ),
            category: MistakeCategory::ErrorHandling,
            severity: MistakeSeverity::Medium,
            prevention: format!(
                "Use {:?} style for error handling. Propagation: {}",
                error_pattern.style, error_pattern.propagation_pattern
            ),
            evidence: Vec::new(),
            likelihood: 0.6,
        });

        for gotcha in &ctx.constraints.gotchas {
            if gotcha.title.to_lowercase().contains("error")
                || gotcha.description.to_lowercase().contains("exception")
                || gotcha.description.to_lowercase().contains("panic")
            {
                mistakes.push(PotentialMistake {
                    title: gotcha.title.clone(),
                    description: gotcha.description.clone(),
                    category: MistakeCategory::ErrorHandling,
                    severity: MistakeSeverity::High,
                    prevention: gotcha.solution.clone(),
                    evidence: gotcha.related_files.clone(),
                    likelihood: 0.7,
                });
            }
        }

        mistakes
    }

    fn detect_resource_mistakes(&self, ctx: &InsightContext<'_>) -> Vec<PotentialMistake> {
        let mut mistakes = Vec::new();

        for gotcha in &ctx.constraints.gotchas {
            if gotcha.title.to_lowercase().contains("resource")
                || gotcha.title.to_lowercase().contains("connection")
                || gotcha.title.to_lowercase().contains("pool")
                || gotcha.title.to_lowercase().contains("leak")
                || gotcha.description.to_lowercase().contains("cleanup")
                || gotcha.description.to_lowercase().contains("close")
            {
                mistakes.push(PotentialMistake {
                    title: gotcha.title.clone(),
                    description: gotcha.description.clone(),
                    category: MistakeCategory::ResourceManagement,
                    severity: MistakeSeverity::High,
                    prevention: gotcha.solution.clone(),
                    evidence: gotcha.related_files.clone(),
                    likelihood: 0.75,
                });
            }
        }

        for rule in &ctx.constraints.implicit_rules {
            if rule.description.to_lowercase().contains("resource")
                || rule.description.to_lowercase().contains("pool")
                || rule.description.to_lowercase().contains("connection")
            {
                mistakes.push(PotentialMistake {
                    title: format!("Resource rule: {}", rule.name),
                    description: rule.description.clone(),
                    category: MistakeCategory::ResourceManagement,
                    severity: MistakeSeverity::Medium,
                    prevention: rule.description.clone(),
                    evidence: rule.evidence.iter().map(|e| e.file.clone()).collect(),
                    likelihood: 0.6,
                });
            }
        }

        mistakes
    }

    async fn discover_mistakes_with_llm(
        &self,
        ctx: &InsightContext<'_>,
    ) -> Result<Vec<PotentialMistake>> {
        let context_summary = self.build_context_summary(ctx);

        let prompt = format!(
            r#"Analyze this project context and identify potential mistakes an AI coding assistant could make.

PROJECT CONTEXT:
{}

QUESTION: What mistakes would an AI make when working on this codebase if it didn't have proper documentation?

Focus on:
1. Non-obvious initialization orders or dependencies
2. Hidden business rules that aren't in the code
3. Security gotchas specific to this project
4. Performance traps
5. Architecture patterns that must be followed

For each mistake, provide:
- A clear title
- Description of what could go wrong
- Severity (critical/high/medium/low)
- How to prevent it
- Which category (concurrency/init_order/error_handling/security/business_logic/architecture/performance/resource_management)

Return JSON array:
[{{"title": "...", "description": "...", "severity": "...", "prevention": "...", "category": "...", "likelihood": 0.8}}]"#,
            context_summary
        );

        let schema = serde_json::json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "severity": { "type": "string", "enum": ["critical", "high", "medium", "low"] },
                    "prevention": { "type": "string" },
                    "category": { "type": "string" },
                    "likelihood": { "type": "number" }
                },
                "required": ["title", "description", "severity", "prevention", "category"]
            }
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let items = response.content.as_array().cloned().unwrap_or_default();
                let mistakes = items
                    .iter()
                    .filter_map(|item| self.parse_llm_mistake(item))
                    .collect();
                Ok(mistakes)
            }
            Err(e) => {
                warn!(error = %e, "LLM mistake discovery failed, using pattern-based only");
                Ok(Vec::new())
            }
        }
    }

    fn build_context_summary(&self, ctx: &InsightContext<'_>) -> String {
        let mut summary = String::new();

        summary.push_str(&format!(
            "Architecture: {} - {}\n",
            ctx.conventions.architecture.pattern_name, ctx.conventions.architecture.description
        ));

        summary.push_str(&format!(
            "Error Handling: {:?} ({})\n",
            ctx.conventions.error_handling.style, ctx.conventions.error_handling.propagation_pattern
        ));

        if ctx.conventions.async_pattern.style != AsyncStyle::Synchronous {
            summary.push_str(&format!(
                "Async: {:?}\n",
                ctx.conventions.async_pattern.style
            ));
        }

        if !ctx.constraints.hidden_dependencies.is_empty() {
            summary.push_str("\nHidden Dependencies:\n");
            for dep in ctx.constraints.hidden_dependencies.iter().take(5) {
                summary.push_str(&format!(
                    "- {} -> {}: {}\n",
                    dep.source, dep.target, dep.description
                ));
            }
        }

        if !ctx.constraints.gotchas.is_empty() {
            summary.push_str("\nKnown Gotchas:\n");
            for gotcha in ctx.constraints.gotchas.iter().take(5) {
                summary.push_str(&format!("- {}: {}\n", gotcha.title, gotcha.description));
            }
        }

        if !ctx.constraints.anti_patterns.is_empty() {
            summary.push_str("\nAnti-patterns:\n");
            for pattern in ctx.constraints.anti_patterns.iter().take(3) {
                summary.push_str(&format!("- {}: {}\n", pattern.name, pattern.description));
            }
        }

        summary
    }

    fn parse_llm_mistake(&self, item: &serde_json::Value) -> Option<PotentialMistake> {
        let title = item.get("title")?.as_str()?.to_string();
        let description = item.get("description")?.as_str()?.to_string();
        let severity_str = item.get("severity")?.as_str()?;
        let prevention = item.get("prevention")?.as_str()?.to_string();
        let category_str = item.get("category")?.as_str()?;
        let likelihood = item.get("likelihood").and_then(|v| v.as_f64()).unwrap_or(0.6) as f32;

        let severity = match severity_str {
            "critical" => MistakeSeverity::Critical,
            "high" => MistakeSeverity::High,
            "medium" => MistakeSeverity::Medium,
            _ => MistakeSeverity::Low,
        };

        let category = match category_str {
            "concurrency" => MistakeCategory::Concurrency,
            "init_order" => MistakeCategory::InitOrder,
            "error_handling" => MistakeCategory::ErrorHandling,
            "security" => MistakeCategory::Security,
            "business_logic" => MistakeCategory::BusinessLogic,
            "architecture" => MistakeCategory::Architecture,
            "performance" => MistakeCategory::Performance,
            "resource_management" => MistakeCategory::ResourceManagement,
            _ => MistakeCategory::Architecture,
        };

        Some(PotentialMistake {
            title,
            description,
            category,
            severity,
            prevention,
            evidence: Vec::new(),
            likelihood,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{LlmResponse, ResponseMetadata, ResponseTiming, TokenUsage};
    use crate::pipeline::context::VerifiedFileRegistry;
    use crate::pipeline::phases::constraint_extraction::{
        ExtractedConstraints, Gotcha, HiddenDependency, HiddenDepType, ImplicitRule,
    };
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, AsyncStyle, ErrorHandlingPattern, ErrorStyle,
        FileOrganization, InferredConventions, NamingConventions, TestingConvention,
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
                timing: ResponseTiming {
                    total_ms: 100,
                    api_ms: None,
                },
                metadata: ResponseMetadata {
                    model: "mock-model".to_string(),
                    provider: "mock".to_string(),
                },
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

    fn create_test_finder() -> MistakeFinder {
        let provider = Arc::new(MockProvider);
        let config = Arc::new(Config::default());
        MistakeFinder::new(provider, config)
    }

    fn create_test_context<'a>(
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        registry: &'a VerifiedFileRegistry,
    ) -> InsightContext<'a> {
        InsightContext {
            conventions,
            constraints,
            synthesis: None,
            file_registry: registry,
        }
    }

    fn default_conventions() -> InferredConventions {
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

    #[test]
    fn test_mistake_severity_as_str() {
        assert_eq!(MistakeSeverity::Low.as_str(), "low");
        assert_eq!(MistakeSeverity::Medium.as_str(), "medium");
        assert_eq!(MistakeSeverity::High.as_str(), "high");
        assert_eq!(MistakeSeverity::Critical.as_str(), "critical");
    }

    #[test]
    fn test_mistake_severity_score() {
        assert!((MistakeSeverity::Low.score() - 0.3).abs() < f32::EPSILON);
        assert!((MistakeSeverity::Medium.score() - 0.5).abs() < f32::EPSILON);
        assert!((MistakeSeverity::High.score() - 0.7).abs() < f32::EPSILON);
        assert!((MistakeSeverity::Critical.score() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_mistake_category_as_str() {
        assert_eq!(MistakeCategory::Concurrency.as_str(), "concurrency");
        assert_eq!(MistakeCategory::InitOrder.as_str(), "init_order");
        assert_eq!(MistakeCategory::ErrorHandling.as_str(), "error_handling");
        assert_eq!(MistakeCategory::Security.as_str(), "security");
        assert_eq!(MistakeCategory::BusinessLogic.as_str(), "business_logic");
        assert_eq!(MistakeCategory::Architecture.as_str(), "architecture");
        assert_eq!(MistakeCategory::Performance.as_str(), "performance");
        assert_eq!(
            MistakeCategory::ResourceManagement.as_str(),
            "resource_management"
        );
    }

    #[test]
    fn test_detect_concurrency_mistakes_from_shared_state() {
        let finder = create_test_finder();
        let conventions = default_conventions();
        let mut constraints = ExtractedConstraints::default();

        constraints.hidden_dependencies.push(HiddenDependency {
            source: "service_a".to_string(),
            target: "service_b".to_string(),
            dependency_type: HiddenDepType::SharedState,
            description: "Shared Arc<Mutex<State>> between services".to_string(),
            impact: "Race condition if not properly locked".to_string(),
            evidence: Vec::new(),
        });

        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let mistakes = finder.detect_concurrency_mistakes(&ctx);
        assert!(!mistakes.is_empty());
        assert_eq!(mistakes[0].category, MistakeCategory::Concurrency);
        assert!(mistakes[0].title.contains("service_a"));
    }

    #[test]
    fn test_detect_concurrency_mistakes_async_style() {
        let finder = create_test_finder();
        let mut conventions = default_conventions();
        conventions.async_pattern.style = AsyncStyle::AsyncAwait;
        conventions.async_pattern.runtime = Some("tokio".to_string());

        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let mistakes = finder.detect_concurrency_mistakes(&ctx);
        assert!(!mistakes.is_empty());
        assert!(mistakes[0].title.contains("Async code"));
    }

    #[test]
    fn test_detect_init_order_mistakes() {
        let finder = create_test_finder();
        let conventions = default_conventions();
        let mut constraints = ExtractedConstraints::default();

        constraints.hidden_dependencies.push(HiddenDependency {
            source: "database".to_string(),
            target: "cache".to_string(),
            dependency_type: HiddenDepType::ImplicitOrdering,
            description: "Database must be initialized before cache".to_string(),
            impact: "Cache will fail without database connection".to_string(),
            evidence: Vec::new(),
        });

        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let mistakes = finder.detect_init_order_mistakes(&ctx);
        assert!(!mistakes.is_empty());
        assert_eq!(mistakes[0].category, MistakeCategory::InitOrder);
        assert!(mistakes[0].title.contains("database"));
    }

    #[test]
    fn test_detect_error_handling_mistakes() {
        let finder = create_test_finder();
        let mut conventions = default_conventions();
        conventions.error_handling.style = ErrorStyle::ResultType;
        conventions.error_handling.propagation_pattern = "Use ? operator".to_string();

        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let mistakes = finder.detect_error_handling_mistakes(&ctx);
        assert!(!mistakes.is_empty());
        assert_eq!(mistakes[0].category, MistakeCategory::ErrorHandling);
    }

    #[test]
    fn test_detect_error_handling_from_gotcha() {
        let finder = create_test_finder();
        let conventions = default_conventions();
        let mut constraints = ExtractedConstraints::default();

        constraints.gotchas.push(Gotcha {
            title: "Error handling in API layer".to_string(),
            description: "API exceptions must be caught and converted".to_string(),
            when: "When handling API calls".to_string(),
            solution: "Use try-catch with proper error mapping".to_string(),
            related_files: vec!["src/api/handler.rs".to_string()],
        });

        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let mistakes = finder.detect_error_handling_mistakes(&ctx);
        assert!(mistakes.len() >= 2); // One from pattern + one from gotcha
        let gotcha_mistake = mistakes.iter().find(|m| m.title.contains("API layer"));
        assert!(gotcha_mistake.is_some());
    }

    #[test]
    fn test_detect_resource_mistakes_from_gotcha() {
        let finder = create_test_finder();
        let conventions = default_conventions();
        let mut constraints = ExtractedConstraints::default();

        constraints.gotchas.push(Gotcha {
            title: "Connection pool exhaustion".to_string(),
            description: "Connection pool can be exhausted if connections are not returned".to_string(),
            when: "Under heavy load".to_string(),
            solution: "Always use try-finally or RAII for connections".to_string(),
            related_files: vec!["src/db/pool.rs".to_string()],
        });

        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let mistakes = finder.detect_resource_mistakes(&ctx);
        assert!(!mistakes.is_empty());
        assert_eq!(mistakes[0].category, MistakeCategory::ResourceManagement);
    }

    #[test]
    fn test_detect_resource_mistakes_from_implicit_rule() {
        let finder = create_test_finder();
        let conventions = default_conventions();
        let mut constraints = ExtractedConstraints::default();

        constraints.implicit_rules.push(ImplicitRule {
            name: "Resource cleanup".to_string(),
            description: "All database connections must be returned to pool".to_string(),
            applies_to: vec!["src/db/".to_string()],
            enforcement: crate::pipeline::phases::constraint_extraction::RuleEnforcement::Convention,
            evidence: Vec::new(),
        });

        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let mistakes = finder.detect_resource_mistakes(&ctx);
        assert!(!mistakes.is_empty());
        assert!(mistakes[0].title.contains("Resource rule"));
    }

    #[test]
    fn test_parse_llm_mistake_valid() {
        let finder = create_test_finder();
        let item = serde_json::json!({
            "title": "Memory leak in cache",
            "description": "Cache entries are never evicted",
            "severity": "high",
            "prevention": "Implement TTL for cache entries",
            "category": "resource_management",
            "likelihood": 0.8
        });

        let mistake = finder.parse_llm_mistake(&item);
        assert!(mistake.is_some());
        let m = mistake.unwrap();
        assert_eq!(m.title, "Memory leak in cache");
        assert_eq!(m.severity, MistakeSeverity::High);
        assert_eq!(m.category, MistakeCategory::ResourceManagement);
        assert!((m.likelihood - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_llm_mistake_missing_field() {
        let finder = create_test_finder();
        let item = serde_json::json!({
            "title": "Incomplete mistake",
            "description": "Missing required fields"
        });

        let mistake = finder.parse_llm_mistake(&item);
        assert!(mistake.is_none());
    }

    #[test]
    fn test_parse_llm_mistake_unknown_severity() {
        let finder = create_test_finder();
        let item = serde_json::json!({
            "title": "Unknown severity",
            "description": "Uses unknown severity value",
            "severity": "extreme",
            "prevention": "Handle unknown values",
            "category": "security"
        });

        let mistake = finder.parse_llm_mistake(&item);
        assert!(mistake.is_some());
        assert_eq!(mistake.unwrap().severity, MistakeSeverity::Low);
    }

    #[test]
    fn test_parse_llm_mistake_unknown_category() {
        let finder = create_test_finder();
        let item = serde_json::json!({
            "title": "Unknown category",
            "description": "Uses unknown category value",
            "severity": "medium",
            "prevention": "Handle unknown values",
            "category": "unknown_category"
        });

        let mistake = finder.parse_llm_mistake(&item);
        assert!(mistake.is_some());
        assert_eq!(mistake.unwrap().category, MistakeCategory::Architecture);
    }
}
