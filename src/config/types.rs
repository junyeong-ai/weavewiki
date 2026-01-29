//! Value-Centric Configuration System
//!
//! Core principle: "What mistakes would AI make without this information?"
//!
//! Design:
//! - Single high-quality configuration (always thorough analysis)
//! - Value-based quality metrics (mistake_prevention, discoverability, artifact_fitness)
//! - Multi-dimensional convergence (not single quality_score)
//! - Domain-aware generation (business rules, compliance)
//! - All thresholds user-configurable

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::Severity;

// =============================================================================
// ROOT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: String,
    pub generation: GenerationConfig,
    pub value: ValueConfig,
    pub convergence: ConvergenceConfig,
    pub domain: DomainConfig,
    pub llm: LlmConfig,
    pub analysis: AnalysisConfig,
    pub insight: InsightConfig,
    pub budget: BudgetConfig,
    pub performance: PerformanceConfig,
    pub project: ProjectConfig,
    pub output: OutputConfig,
    pub validation: ValidationConfig,
    pub circuit_breaker: CircuitBreakerConfig,
    pub refinement: RefinementConfig,
    pub learning: LearningConfig,
    pub semantic_validation: SemanticValidationConfig,
    pub multi_agent: MultiAgentConfig,
    pub usability: UsabilityConfig,
    pub few_shot: FewShotConfig,
    pub quality: QualityConfig,
    pub deep_review: DeepReviewConfig,
    pub cross_validation: CrossValidationConfig,
    pub quality_loop: QualityLoopConfig,
    pub deep_analysis: DeepAnalysisConfig,
    pub structural_validation: StructuralValidationConfig,
    pub cross_artifact: CrossArtifactConfig,
    pub timeout: TimeoutConfig,
    pub distributed_analysis: DistributedAnalysisConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: "4.0".into(),
            generation: GenerationConfig::default(),
            value: ValueConfig::default(),
            convergence: ConvergenceConfig::default(),
            domain: DomainConfig::default(),
            llm: LlmConfig::default(),
            analysis: AnalysisConfig::default(),
            insight: InsightConfig::default(),
            budget: BudgetConfig::default(),
            performance: PerformanceConfig::default(),
            project: ProjectConfig::default(),
            output: OutputConfig::default(),
            validation: ValidationConfig::default(),
            circuit_breaker: CircuitBreakerConfig::default(),
            refinement: RefinementConfig::default(),
            learning: LearningConfig::default(),
            semantic_validation: SemanticValidationConfig::default(),
            multi_agent: MultiAgentConfig::default(),
            usability: UsabilityConfig::default(),
            few_shot: FewShotConfig::default(),
            quality: QualityConfig::default(),
            deep_review: DeepReviewConfig::default(),
            cross_validation: CrossValidationConfig::default(),
            quality_loop: QualityLoopConfig::default(),
            deep_analysis: DeepAnalysisConfig::default(),
            structural_validation: StructuralValidationConfig::default(),
            cross_artifact: CrossArtifactConfig::default(),
            timeout: TimeoutConfig::default(),
            distributed_analysis: DistributedAnalysisConfig::default(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> crate::types::Result<()> {
        use crate::types::ClaudegenError;

        // Value thresholds must be in valid range
        if !(0.0..=1.0).contains(&self.value.min_overall) {
            return Err(ClaudegenError::Config(
                "value.min_overall must be between 0.0 and 1.0".into(),
            ));
        }

        // Convergence validation
        if self.convergence.max_iterations == 0 {
            return Err(ClaudegenError::Config(
                "convergence.max_iterations must be > 0".into(),
            ));
        }
        if self.convergence.consecutive_passes == 0 {
            return Err(ClaudegenError::Config(
                "convergence.consecutive_passes must be > 0".into(),
            ));
        }

        // Budget validation
        if self.budget.total_tokens == 0 {
            return Err(ClaudegenError::Config(
                "budget.total_tokens must be > 0".into(),
            ));
        }

        // Performance validation
        if self.performance.parallel_workers == 0 {
            return Err(ClaudegenError::Config(
                "performance.parallel_workers must be > 0".into(),
            ));
        }

        // =========================================================================
        // Cross-field validation (interdependency checks)
        // Order matters: validate base configs before derived configs
        // =========================================================================

        // 1. Convergence consistency (base config - must validate first)
        //    This must come before deep_review validation because deep_review.required_passes
        //    is derived from convergence.consecutive_passes
        if self.convergence.consecutive_passes > self.convergence.max_iterations {
            return Err(ClaudegenError::Config(format!(
                "convergence.consecutive_passes ({}) must be <= max_iterations ({})",
                self.convergence.consecutive_passes, self.convergence.max_iterations
            )));
        }

        // 2. Deep review: max_attempts must allow convergence (derived config)
        let deep_review = self.deep_review();
        if deep_review.max_attempts < deep_review.required_passes {
            return Err(ClaudegenError::Config(format!(
                "deep_review.max_attempts ({}) must be >= required_passes ({})",
                deep_review.max_attempts, deep_review.required_passes
            )));
        }

        // 4. Quality thresholds ordering (warning only)
        if self.convergence.early_exit_threshold < self.value.min_overall {
            tracing::warn!(
                early_exit = self.convergence.early_exit_threshold,
                min_overall = self.value.min_overall,
                "early_exit_threshold < min_overall: may exit before quality target reached"
            );
        }

        self.value.validate()?;
        self.convergence.validate()?;

        Ok(())
    }
}

// =============================================================================
// GENERATION CONFIG
// =============================================================================

/// What to generate and how
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GenerationConfig {
    pub strategy: GenerationStrategy,
    pub artifacts: Vec<ArtifactType>,
    pub limits: ArtifactLimits,
    /// Minimum value score threshold for generating rules.
    ///
    /// Rules with scores below this threshold are skipped during generation.
    /// This is an ADVISORY gate - rules with lower scores may still have value
    /// in specific contexts (e.g., critical security constraints).
    ///
    /// Set to 0.0 to disable filtering and let LLM decide all rule values.
    /// Default: 0.3
    pub min_rule_value_score: f32,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            strategy: GenerationStrategy::ValueDriven,
            artifacts: vec![
                ArtifactType::ClaudeMd,
                ArtifactType::Rules,
                ArtifactType::Skills,
                ArtifactType::Agents,
            ],
            limits: ArtifactLimits::default(),
            min_rule_value_score: 0.3,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStrategy {
    /// Focus on high-value, mistake-preventing content
    #[default]
    ValueDriven,
    /// Maximize coverage of codebase
    CoverageDriven,
    /// Minimal output, only critical constraints
    Minimal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    ClaudeMd,
    Rules,
    Skills,
    Agents,
}

impl ArtifactType {
    pub fn all() -> Vec<Self> {
        vec![Self::ClaudeMd, Self::Rules, Self::Skills, Self::Agents]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeMd => "claude_md",
            Self::Rules => "rules",
            Self::Skills => "skills",
            Self::Agents => "agents",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtifactLimits {
    pub max_rules: usize,
    pub max_skills: usize,
    pub max_agents: usize,
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            max_rules: 20,
            max_skills: 15,
            max_agents: 10,
        }
    }
}

// =============================================================================
// VALUE CONFIG - Core of the new system
// =============================================================================

/// Value-based quality configuration
/// Core question: "Would AI make mistakes without this information?"
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValueConfig {
    /// Minimum overall value score (weighted average of dimensions)
    pub min_overall: f32,
    /// Per-dimension thresholds
    pub dimensions: ValueDimensions,
    /// Weights for calculating overall score
    pub weights: ValueWeights,
}

impl Default for ValueConfig {
    fn default() -> Self {
        Self {
            min_overall: 0.6,
            dimensions: ValueDimensions::default(),
            weights: ValueWeights::default(),
        }
    }
}

impl ValueConfig {
    pub fn validate(&self) -> crate::types::Result<()> {
        use crate::types::ClaudegenError;

        // Check dimension thresholds
        let dims = &self.dimensions;
        for (name, val) in [
            ("mistake_prevention", dims.mistake_prevention),
            ("discoverability", dims.discoverability),
            ("artifact_fitness", dims.artifact_fitness),
        ] {
            if !(0.0..=1.0).contains(&val) {
                return Err(ClaudegenError::Config(format!(
                    "value.dimensions.{name} must be between 0.0 and 1.0"
                )));
            }
        }

        // Weights should sum to ~1.0 (allow small tolerance)
        let w = &self.weights;
        let sum = w.mistake_prevention + w.discoverability + w.artifact_fitness;
        if (sum - 1.0).abs() > 0.01 {
            return Err(ClaudegenError::Config(format!(
                "value.weights should sum to 1.0 (current: {sum:.2})"
            )));
        }

        Ok(())
    }

    /// Calculate overall value score from dimension scores
    pub fn calculate_overall(&self, scores: &ValueScores) -> f32 {
        let w = &self.weights;
        w.mistake_prevention * scores.mistake_prevention
            + w.discoverability * scores.discoverability
            + w.artifact_fitness * scores.artifact_fitness
    }
}

/// Minimum thresholds per value dimension
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValueDimensions {
    /// How well does this prevent AI mistakes? (0.0 = no prevention, 1.0 = prevents critical mistakes)
    pub mistake_prevention: f32,
    /// How hard is this to discover from code alone? (0.0 = obvious, 1.0 = requires experience)
    pub discoverability: f32,
    /// How well does this fit the artifact type? (0.0 = wrong place, 1.0 = perfect fit)
    pub artifact_fitness: f32,
}

impl Default for ValueDimensions {
    fn default() -> Self {
        Self {
            mistake_prevention: 0.5,
            discoverability: 0.4,
            artifact_fitness: 0.6,
        }
    }
}

/// Weights for calculating overall value score
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValueWeights {
    pub mistake_prevention: f32,
    pub discoverability: f32,
    pub artifact_fitness: f32,
}

impl Default for ValueWeights {
    fn default() -> Self {
        Self {
            mistake_prevention: 0.4,
            discoverability: 0.3,
            artifact_fitness: 0.3,
        }
    }
}

/// Calculated value scores for content
#[derive(Debug, Clone, Default)]
pub struct ValueScores {
    pub mistake_prevention: f32,
    pub discoverability: f32,
    pub artifact_fitness: f32,
}

impl ValueScores {
    pub fn new(mistake_prevention: f32, discoverability: f32, artifact_fitness: f32) -> Self {
        Self {
            mistake_prevention,
            discoverability,
            artifact_fitness,
        }
    }

    pub fn meets_thresholds(&self, dims: &ValueDimensions) -> bool {
        self.mistake_prevention >= dims.mistake_prevention
            && self.discoverability >= dims.discoverability
            && self.artifact_fitness >= dims.artifact_fitness
    }
}

// =============================================================================
// CONVERGENCE CONFIG
// =============================================================================

/// Multi-dimensional convergence criteria
/// NOT just "quality >= threshold" - requires formal + value + stability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConvergenceConfig {
    /// Maximum iterations before forced termination
    pub max_iterations: usize,
    /// Consecutive passes required for stability
    pub consecutive_passes: usize,
    /// Maximum score oscillations before forcing termination
    pub max_oscillations: usize,
    /// Early exit if this score reached
    pub early_exit_threshold: f32,
    /// Iterations without improvement before escalation
    pub stagnation_patience: usize,
    /// Minimum improvement per iteration
    pub min_improvement: f32,
    /// Formal validation must pass
    pub require_formal_pass: bool,
    /// Cross-artifact validation must pass
    pub require_cross_artifact_pass: bool,
    /// Minimum acceptable quality floor (can converge without full dimension pass)
    pub quality_floor: f32,
    /// Target quality to aim for
    pub target_quality: f32,
    /// Whether early_exit bypasses dimension requirements
    pub early_exit_bypasses_dimensions: bool,
}

impl Default for ConvergenceConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100, // High quality: allow thorough iteration
            consecutive_passes: 2,
            max_oscillations: 5,        // More patience for complex projects
            early_exit_threshold: 0.95, // Only early exit at very high quality
            stagnation_patience: 10,    // More patience before giving up
            min_improvement: 0.005,     // Detect smaller improvements
            require_formal_pass: true,
            require_cross_artifact_pass: true,
            quality_floor: 0.75,  // High quality floor
            target_quality: 0.90, // High quality target
            early_exit_bypasses_dimensions: false,
        }
    }
}

impl ConvergenceConfig {
    pub fn validate(&self) -> crate::types::Result<()> {
        use crate::types::ClaudegenError;

        if self.consecutive_passes > self.max_iterations {
            return Err(ClaudegenError::Config(
                "consecutive_passes cannot exceed max_iterations".into(),
            ));
        }

        if !(0.0..=1.0).contains(&self.early_exit_threshold) {
            return Err(ClaudegenError::Config(
                "early_exit_threshold must be between 0.0 and 1.0".into(),
            ));
        }

        Ok(())
    }
}

// =============================================================================
// DOMAIN CONFIG
// =============================================================================

/// Domain-specific configuration for business rule extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DomainConfig {
    /// Domain type for specialized extraction
    pub domain_type: DomainType,
    /// Domain-specific terminology
    pub terminology: Vec<TermDefinition>,
    /// Compliance/regulatory requirements
    pub compliance: Vec<String>,
    /// Custom business rule patterns to look for
    pub business_patterns: Vec<String>,
}

impl Default for DomainConfig {
    fn default() -> Self {
        Self {
            domain_type: DomainType::Generic,
            terminology: Vec::new(),
            compliance: Vec::new(),
            business_patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DomainType {
    ECommerce,
    FinTech,
    Healthcare,
    SaaS,
    /// Gaming/Entertainment domain
    Gaming,
    /// Education/EdTech domain
    Education,
    /// DevTools/Infrastructure domain
    DevTools,
    /// Insurance/InsurTech domain
    Insurance,
    /// Legal/LegalTech domain
    Legal,
    /// IoT/Embedded systems domain
    IoT,
    #[default]
    Generic,
    /// Custom domain discovered by LLM
    #[serde(other)]
    Other,
}

impl DomainType {
    /// Returns common compliance frameworks for the domain.
    /// For Other/custom domains, LLM should determine applicable compliance.
    pub fn default_compliance(&self) -> Vec<&'static str> {
        match self {
            Self::ECommerce => vec!["PCI-DSS", "GDPR"],
            Self::FinTech => vec!["PCI-DSS", "AML", "KYC", "SOX"],
            Self::Healthcare => vec!["HIPAA", "GDPR"],
            Self::SaaS => vec!["SOC2", "GDPR"],
            Self::Gaming => vec!["COPPA", "GDPR"],
            Self::Education => vec!["FERPA", "COPPA", "GDPR"],
            Self::Insurance => vec!["HIPAA", "SOX", "GDPR"],
            Self::Legal => vec!["GDPR", "attorney-client-privilege"],
            Self::IoT => vec!["IoT-security-baseline", "GDPR"],
            Self::DevTools | Self::Generic | Self::Other => vec![],
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ECommerce => "ecommerce",
            Self::FinTech => "fintech",
            Self::Healthcare => "healthcare",
            Self::SaaS => "saas",
            Self::Gaming => "gaming",
            Self::Education => "education",
            Self::DevTools => "devtools",
            Self::Insurance => "insurance",
            Self::Legal => "legal",
            Self::IoT => "iot",
            Self::Generic => "generic",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermDefinition {
    pub term: String,
    pub definition: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

// =============================================================================
// LLM CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Balanced model for standard operations (Sonnet-class)
    /// This is the workhorse model for generation, refinement, review
    pub default_model: String,

    /// Performance model for critical high-intelligence tasks (Opus-class)
    /// Used for: constraint extraction, mistake discovery, deep analysis
    /// Industry terminology: highest capability tier
    pub performance_model: Option<String>,

    /// Fast model for quick, simple tasks (Haiku-class)
    /// Used for: classification, detection, validation
    /// Industry terminology: speed-optimized tier
    pub fast_model: Option<String>,

    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Temperature (0.0 = deterministic, 1.0 = creative)
    pub temperature: f32,
    /// Maximum tokens to generate per request
    pub max_tokens: usize,
    /// Provider (claude-agent, openai)
    pub provider: String,
    /// Context configuration
    pub context: ContextWindowConfig,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            default_model: "claude-sonnet-4-5-20250929".into(),
            performance_model: Some("claude-opus-4-5-20251101".into()), // Opus for complex tasks
            fast_model: Some("claude-haiku-4-5-20251001".into()),       // Haiku for simple tasks
            timeout_secs: 600, // Generous timeout for complex responses
            temperature: 0.0,
            max_tokens: 8192, // Allow longer responses
            provider: "claude-agent".into(),
            context: ContextWindowConfig::default(),
        }
    }
}

/// Context window configuration for managing token limits
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextWindowConfig {
    /// Authentication mode (api_key or oauth)
    /// OAuth (Claude Code CLI) doesn't support extended context
    pub auth_mode: AuthModeConfig,
    /// Whether to use extended context (1M) if available
    /// Only works with API key authentication
    pub use_extended_context: bool,
    /// Override context window size (tokens)
    /// If set, ignores model defaults and auth mode restrictions
    pub override_window_size: Option<u64>,
    /// Ratio of context window for input vs output (0.0-1.0)
    pub input_ratio: f32,
    /// Safety margin tokens reserved for overhead
    pub safety_margin_tokens: u64,
    /// Maximum tokens per batch (if unset, calculated from window size)
    pub max_batch_tokens: Option<u64>,
}

impl Default for ContextWindowConfig {
    fn default() -> Self {
        Self {
            auth_mode: AuthModeConfig::OAuth, // Default to OAuth (Claude Code CLI)
            use_extended_context: false,      // Disabled by default
            override_window_size: None,
            input_ratio: 0.90,
            safety_margin_tokens: 10_000,
            max_batch_tokens: None, // Calculated dynamically
        }
    }
}

impl ContextWindowConfig {
    /// Get effective context window size for a model
    pub fn effective_window_size(&self, model_id: &str) -> u64 {
        if let Some(override_size) = self.override_window_size {
            return override_size;
        }

        use crate::ai::model_capabilities::{AuthMode, ModelRegistry};

        let registry = ModelRegistry::global();
        let caps = registry.get_or_default(model_id);
        let auth_mode = match self.auth_mode {
            AuthModeConfig::ApiKey => AuthMode::ApiKey,
            AuthModeConfig::OAuth => AuthMode::OAuth,
        };

        caps.effective_context_window(auth_mode, self.use_extended_context)
    }

    /// Get effective maximum batch tokens
    pub fn effective_batch_tokens(&self, model_id: &str) -> u64 {
        if let Some(batch_tokens) = self.max_batch_tokens {
            return batch_tokens;
        }

        let window = self.effective_window_size(model_id);
        let available = (window as f32 * self.input_ratio) as u64 - self.safety_margin_tokens;

        // Batch ratio based on window size
        let batch_ratio = if window >= 500_000 {
            0.10
        } else if window >= 200_000 {
            0.25
        } else {
            0.30
        };

        (available as f32 * batch_ratio) as u64
    }

    /// Get available tokens for content
    pub fn available_for_content(&self, model_id: &str) -> u64 {
        let window = self.effective_window_size(model_id);
        let input_budget = (window as f32 * self.input_ratio) as u64;
        input_budget.saturating_sub(self.safety_margin_tokens)
    }
}

/// Authentication mode configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthModeConfig {
    /// API key authentication - supports all features
    ApiKey,
    /// OAuth authentication (Claude Code CLI) - limited features
    #[default]
    OAuth,
}

impl LlmConfig {
    pub fn performance_model(&self) -> &str {
        self.performance_model
            .as_deref()
            .unwrap_or(&self.default_model)
    }

    pub fn fast_model(&self) -> &str {
        self.fast_model.as_deref().unwrap_or(&self.default_model)
    }

    // Note: Phase-based model routing is handled by ProviderSet::provider_for_phase()
    // in src/ai/provider/mod.rs. LlmConfig provides model names, ProviderSet handles routing.
}

// =============================================================================
// ANALYSIS CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    pub depth: AnalysisDepth,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Maximum file size in bytes (default: 5MB)
    pub max_file_size: usize,
    pub max_file_samples: usize,
    /// Maximum key paths to include in agent prompts
    pub max_key_paths: usize,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            depth: AnalysisDepth::Complete, // Always complete analysis
            include: vec!["**/*".into()],
            exclude: vec![
                ".git/**".into(),
                "target/**".into(),
                "dist/**".into(),
                "build/**".into(),
                "node_modules/**".into(),
                "vendor/**".into(),
                "__pycache__/**".into(),
                ".venv/**".into(),
                ".claudegen/**".into(),
            ],
            max_file_size: 10 * 1024 * 1024, // 10MB for larger files
            max_file_samples: 200,           // More samples for thorough analysis
            max_key_paths: 20,               // More key paths
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisDepth {
    Fast,
    #[default]
    Standard,
    Complete,
}

impl AnalysisDepth {
    pub fn max_files(&self) -> usize {
        match self {
            Self::Fast => 20,
            Self::Standard => 100,
            Self::Complete => usize::MAX,
        }
    }

    pub fn max_llm_calls(&self) -> usize {
        match self {
            Self::Fast => 15,
            Self::Standard => 50,
            Self::Complete => usize::MAX,
        }
    }

    pub fn enable_deep_analysis(&self) -> bool {
        matches!(self, Self::Standard | Self::Complete)
    }
}

// =============================================================================
// INSIGHT EXTRACTION CONFIG
// =============================================================================

/// Configuration for insight extraction components
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct InsightConfig {
    /// Mistake finder configuration
    pub mistakes: MistakeFinderConfig,
    /// Constraint detection configuration
    pub constraints: ConstraintDetectionConfig,
    /// Domain analysis configuration
    pub domain: DomainAnalysisConfig,
    /// Classification configuration
    pub classification: ClassificationConfig,
    /// Scoring configuration
    pub scoring: ScoringConfig,
}

/// Mistake finder configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MistakeFinderConfig {
    /// Minimum likelihood threshold to retain a mistake (0.0-1.0)
    pub min_likelihood: f32,
    /// Enable concurrency mistake detection
    pub detect_concurrency: bool,
    /// Enable initialization order detection
    pub detect_init_order: bool,
    /// Enable error handling mistake detection
    pub detect_error_handling: bool,
    /// Enable resource management mistake detection
    pub detect_resource_management: bool,
}

impl Default for MistakeFinderConfig {
    fn default() -> Self {
        Self {
            min_likelihood: 0.3,
            detect_concurrency: true,
            detect_init_order: true,
            detect_error_handling: true,
            detect_resource_management: true,
        }
    }
}

/// Constraint detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConstraintDetectionConfig {
    /// Enable concurrency constraint detection
    pub detect_concurrency: bool,
    /// Enable initialization order detection
    pub detect_init_order: bool,
    /// Enable security constraint detection
    pub detect_security: bool,
    /// Enable boundary constraint detection
    pub detect_boundary: bool,
    /// Enable performance constraint detection
    pub detect_performance: bool,
    /// Minimum severity to retain constraints
    pub min_severity: Severity,
}

impl Default for ConstraintDetectionConfig {
    fn default() -> Self {
        Self {
            detect_concurrency: true,
            detect_init_order: true,
            detect_security: true,
            detect_boundary: true,
            detect_performance: true,
            min_severity: Severity::Low,
        }
    }
}

/// Domain analysis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DomainAnalysisConfig {
    /// Minimum occurrences to consider a term domain-specific
    pub min_term_occurrences: usize,
    /// Maximum terminology entries to extract
    pub max_terminology: usize,
    /// Enable LLM enrichment for domain analysis
    pub llm_enrichment: bool,
    /// Business rule types to focus on
    pub rule_type_priorities: Vec<BusinessRuleType>,
}

impl Default for DomainAnalysisConfig {
    fn default() -> Self {
        Self {
            min_term_occurrences: 2,
            max_terminology: 50,
            llm_enrichment: true,
            rule_type_priorities: vec![
                BusinessRuleType::Validation,
                BusinessRuleType::StateTransition,
                BusinessRuleType::Authorization,
                BusinessRuleType::Calculation,
                BusinessRuleType::Policy,
            ],
        }
    }
}

/// Business rule types for domain analysis
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BusinessRuleType {
    Validation,
    StateTransition,
    Authorization,
    Calculation,
    Policy,
    Constraint,
    /// Custom rule type discovered by LLM
    #[serde(other)]
    Other,
}

/// Classification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClassificationConfig {
    /// Similarity threshold for duplicate detection (0.0-1.0)
    pub duplicate_similarity_threshold: f32,
    /// Enable tier classification
    pub enable_tier_classification: bool,
    /// Enable artifact routing
    pub enable_artifact_routing: bool,
    /// Enable LLM-based classification (hybrid approach)
    pub enable_llm_classification: bool,
    /// Model to use for LLM classification (defaults to haiku for speed/cost)
    pub llm_model: String,
    /// Batch size for LLM classification
    pub llm_batch_size: usize,
    /// Confidence threshold below which to escalate to stronger model
    pub llm_confidence_threshold: f32,
    /// Fallback model when confidence is below threshold
    pub llm_fallback_model: String,
    /// Cache TTL in hours for classification results
    pub cache_ttl_hours: u64,
    /// Maximum cache entries
    pub cache_max_entries: usize,
}

impl Default for ClassificationConfig {
    fn default() -> Self {
        Self {
            duplicate_similarity_threshold: 0.7,
            enable_tier_classification: true,
            enable_artifact_routing: true,
            enable_llm_classification: true,
            llm_model: "claude-haiku-4-5-20251001".to_string(),
            llm_batch_size: 10,
            llm_confidence_threshold: 0.8,
            llm_fallback_model: "claude-sonnet-4-5-20250929".to_string(),
            cache_ttl_hours: 24,
            cache_max_entries: 1000,
        }
    }
}

/// Value scoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoringConfig {
    /// Custom keyword bonuses for mistake prevention scoring
    pub mistake_prevention_keywords: HashMap<String, f32>,
    /// Custom keyword bonuses for discoverability scoring
    pub discoverability_keywords: HashMap<String, f32>,
    /// Severity to score bonus mapping
    pub severity_bonuses: SeverityBonuses,
    /// Evidence bonus per reference
    pub evidence_bonus_per_ref: f32,
    /// Maximum evidence bonus
    pub max_evidence_bonus: f32,
    /// Category-based scores for mistake prevention
    pub category_scores: CategoryScores,
    /// Bonus for having prevention info
    pub prevention_info_bonus: f32,
    /// Source-based scores for discoverability
    pub source_scores: SourceScores,
    /// Category adjustments for discoverability
    pub category_adjustments: CategoryAdjustments,
    /// Artifact fitness configuration
    pub artifact_fitness: ArtifactFitnessConfig,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        let mut mistake_keywords = HashMap::new();
        mistake_keywords.insert("security".into(), 0.3);
        mistake_keywords.insert("vulnerability".into(), 0.35);
        mistake_keywords.insert("critical".into(), 0.25);
        mistake_keywords.insert("must".into(), 0.2);
        mistake_keywords.insert("never".into(), 0.2);
        mistake_keywords.insert("required".into(), 0.15);

        let mut discover_keywords = HashMap::new();
        discover_keywords.insert("hidden".into(), 0.3);
        discover_keywords.insert("gotcha".into(), 0.35);
        discover_keywords.insert("pitfall".into(), 0.3);
        discover_keywords.insert("subtle".into(), 0.25);
        discover_keywords.insert("non-obvious".into(), 0.25);

        Self {
            mistake_prevention_keywords: mistake_keywords,
            discoverability_keywords: discover_keywords,
            severity_bonuses: SeverityBonuses::default(),
            evidence_bonus_per_ref: 0.03,
            max_evidence_bonus: 0.15,
            category_scores: CategoryScores::default(),
            prevention_info_bonus: 0.15,
            source_scores: SourceScores::default(),
            category_adjustments: CategoryAdjustments::default(),
            artifact_fitness: ArtifactFitnessConfig::default(),
        }
    }
}

/// Severity to score bonus mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SeverityBonuses {
    pub critical: f32,
    pub high: f32,
    pub medium: f32,
    pub low: f32,
}

impl Default for SeverityBonuses {
    fn default() -> Self {
        Self {
            critical: 0.3,
            high: 0.2,
            medium: 0.1,
            low: 0.05,
        }
    }
}

/// Category-based scores for mistake prevention
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoryScores {
    pub security_constraint: f32,
    pub technical_constraint: f32,
    pub compliance: f32,
    pub business_rule: f32,
    pub gotcha: f32,
    pub performance_constraint: f32,
    pub architecture_intent: f32,
    pub domain_knowledge: f32,
}

impl Default for CategoryScores {
    fn default() -> Self {
        Self {
            security_constraint: 0.4,
            technical_constraint: 0.3,
            compliance: 0.4,
            business_rule: 0.25,
            gotcha: 0.35,
            performance_constraint: 0.2,
            architecture_intent: 0.15,
            domain_knowledge: 0.1,
        }
    }
}

/// Source-based scores for discoverability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceScores {
    pub domain_analysis: f32,
    pub mistake_analysis: f32,
    pub constraint_detection: f32,
    pub pattern_mining: f32,
    pub manual_annotation: f32,
}

impl Default for SourceScores {
    fn default() -> Self {
        Self {
            domain_analysis: 0.2,
            mistake_analysis: 0.15,
            constraint_detection: 0.1,
            pattern_mining: -0.1,
            manual_annotation: 0.25,
        }
    }
}

/// Category adjustments for discoverability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoryAdjustments {
    pub gotcha: f32,
    pub domain_knowledge: f32,
    pub business_rule: f32,
    pub architecture_intent: f32,
}

impl Default for CategoryAdjustments {
    fn default() -> Self {
        Self {
            gotcha: 0.2,
            domain_knowledge: 0.15,
            business_rule: 0.1,
            architecture_intent: -0.05,
        }
    }
}

/// Artifact fitness configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtifactFitnessConfig {
    /// Bonus for having evidence
    pub evidence_bonus: f32,
    /// Bonus for having prevention info
    pub prevention_info_bonus: f32,
    /// Category-specific fitness bonuses
    pub constraint_category_bonus: f32,
    pub domain_category_bonus: f32,
    pub architecture_bonus: f32,
    pub gotcha_bonus: f32,
    /// Content length thresholds
    pub min_length_bonus_threshold: usize,
    pub extended_length_bonus_threshold: usize,
    pub length_bonus: f32,
}

impl Default for ArtifactFitnessConfig {
    fn default() -> Self {
        Self {
            evidence_bonus: 0.2,
            prevention_info_bonus: 0.15,
            constraint_category_bonus: 0.15,
            domain_category_bonus: 0.1,
            architecture_bonus: 0.1,
            gotcha_bonus: 0.12,
            min_length_bonus_threshold: 50,
            extended_length_bonus_threshold: 100,
            length_bonus: 0.05,
        }
    }
}

// =============================================================================
// BUDGET CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetConfig {
    /// Total token budget
    pub total_tokens: u64,
    /// Budget allocation per phase
    pub allocation: BudgetAllocation,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            total_tokens: 10_000_000, // Generous budget for thorough analysis
            allocation: BudgetAllocation::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetAllocation {
    /// Fraction for understanding phase
    pub understanding: f32,
    /// Fraction for insight extraction
    pub insight_extraction: f32,
    /// Fraction for artifact generation
    pub generation: f32,
    /// Fraction for validation
    pub validation: f32,
    /// Fraction for refinement
    pub refinement: f32,
}

impl Default for BudgetAllocation {
    fn default() -> Self {
        Self {
            understanding: 0.15,
            insight_extraction: 0.25,
            generation: 0.30,
            validation: 0.20,
            refinement: 0.10,
        }
    }
}

impl BudgetAllocation {
    pub fn tokens_for_phase(&self, total: u64, phase: &str) -> u64 {
        let fraction = match phase {
            "understanding" => self.understanding,
            "insight_extraction" => self.insight_extraction,
            "generation" => self.generation,
            "validation" => self.validation,
            "refinement" => self.refinement,
            _ => 0.1, // Unknown phases get 10%
        };
        (total as f64 * fraction as f64) as u64
    }
}

// =============================================================================
// PERFORMANCE CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    /// Parallel workers for analysis
    pub parallel_workers: usize,
    /// Checkpoint interval in minutes
    pub checkpoint_interval_minutes: u64,
    /// Maximum runtime in hours
    pub max_runtime_hours: u64,
    /// Resume from checkpoint on crash
    pub resume_on_crash: bool,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            parallel_workers: 4,
            checkpoint_interval_minutes: 5,
            max_runtime_hours: 168, // 1 week
            resume_on_crash: true,
        }
    }
}

// =============================================================================
// PROJECT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: Option<String>,
    pub project_type: ProjectType,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: None,
            project_type: ProjectType::Auto,
        }
    }
}

// =============================================================================
// OUTPUT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct OutputConfig {
    pub agents: OutputAgentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OutputAgentConfig {
    /// Default model for agents
    pub default_model: AgentModelType,
    /// Per-agent overrides by name
    pub overrides: std::collections::HashMap<String, AgentOverride>,
    /// Role-based model/tool mappings
    pub role_mappings: Vec<RoleMapping>,
    /// Tool sets for different roles
    pub tools: AgentToolsets,
}

/// Agent model type for configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentModelType {
    Haiku,
    #[default]
    Sonnet,
    Opus,
}

impl AgentModelType {
    pub fn to_agent_model(&self) -> crate::types::AgentModel {
        match self {
            Self::Haiku => crate::types::AgentModel::Haiku,
            Self::Sonnet => crate::types::AgentModel::Sonnet,
            Self::Opus => crate::types::AgentModel::Opus,
        }
    }
}

/// Per-agent configuration override
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentOverride {
    pub model: Option<AgentModelType>,
    pub tools: Option<Vec<String>>,
}

/// Role-based mapping for agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleMapping {
    /// Patterns to match in role name
    pub patterns: Vec<String>,
    /// Model to use for matching roles
    pub model: AgentModelType,
}

/// Tool sets for different agent roles
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentToolsets {
    pub default: Vec<String>,
    pub debug: Vec<String>,
    pub architect: Vec<String>,
    pub coordinator: Vec<String>,
}

impl Default for AgentToolsets {
    fn default() -> Self {
        Self {
            default: vec!["Read".into(), "Glob".into(), "Grep".into()],
            debug: vec!["Read".into(), "Glob".into(), "Grep".into(), "Bash".into()],
            architect: vec!["Read".into(), "Glob".into(), "Grep".into(), "Task".into()],
            coordinator: vec!["Read".into(), "Glob".into(), "Grep".into(), "Task".into()],
        }
    }
}

// =============================================================================
// PROJECT TYPE (used elsewhere in codebase)
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    Cli,
    Library,
    Backend,
    Frontend,
    Monorepo,
    Agent,
    Hybrid,
    #[default]
    Auto,
}

impl ProjectType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Library => "library",
            Self::Backend => "backend",
            Self::Frontend => "frontend",
            Self::Monorepo => "monorepo",
            Self::Agent => "agent",
            Self::Hybrid => "hybrid",
            Self::Auto => "auto",
        }
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }
}

impl std::fmt::Display for ProjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// RETRY CONFIG (for AI providers)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetryConfig {
    pub max_retries: usize,
    pub max_total_attempts: usize,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_delay_secs: u64,
    pub backoff_factor: f32,
    pub rate_limit_fallback_secs: u64,
    pub rate_limit_delay_secs: u64,
    pub network_retry_delay_secs: u64,
    pub transient_retry_delay_secs: u64,
    pub parse_error_retry_delay_secs: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            max_total_attempts: 10,
            base_delay_ms: 1000,
            max_delay_ms: 60_000,
            max_delay_secs: 60,
            backoff_factor: 2.0,
            rate_limit_fallback_secs: 30,
            rate_limit_delay_secs: 30,
            network_retry_delay_secs: 5,
            transient_retry_delay_secs: 2,
            parse_error_retry_delay_secs: 1,
        }
    }
}

impl RetryConfig {
    pub fn delay_for_attempt(&self, attempt: usize) -> std::time::Duration {
        let delay = self.base_delay_ms as f64 * (self.backoff_factor as f64).powi(attempt as i32);
        let capped = delay.min(self.max_delay_ms as f64);
        std::time::Duration::from_millis(capped as u64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: usize,
    pub recovery_timeout_secs: u64,
    pub half_open_max_calls: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout_secs: 60,
            half_open_max_calls: 3,
        }
    }
}

// =============================================================================
// REFINEMENT CONFIG
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RefinementStrategyType {
    Semantic,
    Evidence,
    Regeneration,
}

impl RefinementStrategyType {
    pub fn all() -> Vec<Self> {
        vec![Self::Semantic, Self::Evidence, Self::Regeneration]
    }
}

// =============================================================================
// ADAPTIVE ITERATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdaptiveIterationConfig {
    pub base_iterations: usize,
    pub max_extension: usize,
    pub extension_triggers: Vec<ExtensionTriggerConfig>,
    pub allow_early_exit: bool,
    pub min_iterations_for_exit: usize,
    pub quality_improving_delta: f32,
    pub high_uncertainty_threshold: f32,
}

impl Default for AdaptiveIterationConfig {
    fn default() -> Self {
        Self {
            base_iterations: 10,
            max_extension: 5,
            extension_triggers: vec![
                ExtensionTriggerConfig::QualityImproving,
                ExtensionTriggerConfig::HighUncertainty,
            ],
            allow_early_exit: true,
            min_iterations_for_exit: 3,
            quality_improving_delta: 0.02,
            high_uncertainty_threshold: 0.3,
        }
    }
}

impl AdaptiveIterationConfig {
    pub fn max_total(&self) -> usize {
        self.base_iterations + self.max_extension
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionTriggerConfig {
    QualityImproving,
    HighUncertainty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RefinementConfig {
    pub enabled_strategies: Vec<RefinementStrategyType>,
    pub max_attempts_per_strategy: usize,
    pub strategy_rotation_enabled: bool,
    pub timeout_secs: u64,
    pub adaptive_iteration: AdaptiveIterationConfig,
    /// Number of consecutive low-improvement iterations before escalation
    pub stagnation_patience: usize,
    /// Minimum improvement delta to reset stagnation counter
    pub stagnation_threshold: f32,
    pub require_all_dimensions: bool,
    /// Maximum issues to address per refinement iteration
    ///
    /// # Advisory Configuration
    ///
    /// This limits how many issues are refined in a single iteration:
    /// - Default: 5 issues per iteration
    /// - Lower values = more focused but slower convergence
    /// - Higher values = faster but may overwhelm LLM context
    ///
    /// ## When to increase:
    /// - Large codebases with many interconnected issues
    /// - High-quality LLM models with large context windows
    /// - Projects where issues are largely independent
    ///
    /// ## When to decrease:
    /// - Complex projects with deep interdependencies
    /// - When refinement quality degrades with batch sizes
    /// - Budget-constrained scenarios (fewer tokens per iteration)
    ///
    /// Set to 0 for unlimited (not recommended - may exceed context limits).
    pub issues_per_iteration: usize,
    /// How many times a strategy can fail before being skipped for a pattern
    pub strategy_retry_limit: usize,
    pub oscillation_strict_passes: usize,
    pub oscillation_lenient_passes: usize,
    pub oscillation_stability_variance: f32,
    pub oscillation_variance_window: usize,
    pub enable_rollback: bool,
    pub rollback_threshold: f32,
    pub max_rollbacks: usize,
    pub post_convergence_verification: bool,
    pub post_convergence_passes: usize,
    pub max_convergence_detections: usize,
    pub dimension_thresholds: DimensionThresholds,
    pub min_improvement_per_iteration: f32,
    pub detect_oscillation: bool,
    pub oscillation_window: usize,
    pub oscillation_min_amplitude: f32,
    /// Minimum quality improvement to accept a regeneration
    pub quality_acceptance_delta: f32,
    /// Self-critique inner loop configuration
    pub self_critique: SelfCritiqueConfig,
    /// Evidence feedback loop configuration
    pub evidence_feedback: EvidenceFeedbackConfig,
}

/// Configuration for self-critique inner loop within refinement
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SelfCritiqueConfig {
    /// Enable self-critique loop
    pub enabled: bool,
    /// Maximum critique iterations per refinement cycle
    pub max_iterations: usize,
    /// Minimum quality improvement to continue critique loop
    pub min_improvement: f32,
    /// Skip critique if quality already above this threshold
    pub quality_skip_threshold: f32,
}

impl Default for SelfCritiqueConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_iterations: 3,
            min_improvement: 0.02,
            quality_skip_threshold: 0.92,
        }
    }
}

/// Configuration for evidence feedback loop
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EvidenceFeedbackConfig {
    /// Enable evidence feedback loop
    pub enabled: bool,
    /// Maximum retry iterations with feedback
    pub max_retries: usize,
    /// Minimum references required per section
    pub min_refs_per_section: usize,
    /// Target total references to aim for
    pub target_refs: usize,
}

impl Default for EvidenceFeedbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            min_refs_per_section: 1,
            target_refs: 5,
        }
    }
}

impl Default for RefinementConfig {
    fn default() -> Self {
        Self {
            enabled_strategies: RefinementStrategyType::all(),
            max_attempts_per_strategy: 5, // More attempts for thorough refinement
            strategy_rotation_enabled: true,
            timeout_secs: 1800, // 30 minutes per refinement phase
            adaptive_iteration: AdaptiveIterationConfig::default(),
            stagnation_patience: 10,      // More patience before giving up
            stagnation_threshold: 0.005,  // Detect smaller improvements
            require_all_dimensions: true, // Require all quality dimensions
            issues_per_iteration: 10,     // Address more issues per iteration
            strategy_retry_limit: 5,      // More retries per strategy
            oscillation_strict_passes: 3,
            oscillation_lenient_passes: 2,
            oscillation_stability_variance: 0.02, // Tighter stability
            oscillation_variance_window: 5,       // Larger window for detection
            enable_rollback: true,
            rollback_threshold: 0.15, // More tolerance before rollback
            max_rollbacks: 5,         // More rollback opportunities
            post_convergence_verification: true,
            post_convergence_passes: 3, // More verification passes
            max_convergence_detections: 5,
            dimension_thresholds: DimensionThresholds::default(),
            min_improvement_per_iteration: 0.005, // Detect smaller improvements
            detect_oscillation: true,
            oscillation_window: 5,
            oscillation_min_amplitude: 0.02,
            quality_acceptance_delta: 0.03, // Tighter acceptance
            self_critique: SelfCritiqueConfig::default(),
            evidence_feedback: EvidenceFeedbackConfig::default(),
        }
    }
}

// =============================================================================
// LEARNING CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LearningConfig {
    pub enabled: bool,
    pub max_patterns: usize,
    pub pattern_threshold: f32,
    pub persist_to_disk: bool,
    pub quality_thresholds: LearningQualityThresholds,
    pub min_improvement_for_pattern: f32,
    pub recommend_success_threshold: f32,
    pub recommend_min_samples: usize,
    pub fallback_success_threshold: f32,
    pub failing_strategy_min_attempts: usize,
    pub failing_strategy_threshold: f32,
    pub escalation_threshold: f32,
    /// Maximum iterations to store per strategy
    pub max_stored_iterations: usize,
    /// Maximum outcomes to track per strategy
    pub max_outcomes_per_strategy: usize,
    /// Window size for escalation detection
    pub escalation_window: usize,
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_patterns: 100,
            pattern_threshold: 0.7,
            persist_to_disk: true,
            quality_thresholds: LearningQualityThresholds::default(),
            min_improvement_for_pattern: 0.05,
            recommend_success_threshold: 0.7,
            recommend_min_samples: 3,
            fallback_success_threshold: 0.5,
            failing_strategy_min_attempts: 5,
            failing_strategy_threshold: 0.3,
            escalation_threshold: 0.8,
            max_stored_iterations: 50,
            max_outcomes_per_strategy: 100,
            escalation_window: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LearningQualityThresholds {
    pub excellent: f32,
    pub good: f32,
    pub acceptable: f32,
    pub poor: f32,
}

impl Default for LearningQualityThresholds {
    fn default() -> Self {
        Self {
            excellent: 0.9,
            good: 0.7,
            acceptable: 0.5,
            poor: 0.3,
        }
    }
}

impl LearningQualityThresholds {
    /// Convert to array format for QualityRange
    pub fn as_array(&self) -> [f32; 4] {
        [self.poor, self.acceptable, self.good, self.excellent]
    }
}

// =============================================================================
// QUALITY CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QualityConfig {
    pub enabled: bool,
    pub min_quality: f32,
    pub target_quality: f32,
    pub min_file_refs: usize,
    pub max_tier1_ratio: f32,
    pub reference_only_mode: bool,
    pub scoring: TierScoringWeights,
    pub skill: SkillQualityConfig,
    pub agent: AgentQualityConfig,
    pub semantic: SemanticQualityThresholds,
}

impl Default for QualityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_quality: 0.7,
            target_quality: 0.9,
            min_file_refs: 3,
            max_tier1_ratio: 0.1,
            reference_only_mode: false,
            scoring: TierScoringWeights::default(),
            skill: SkillQualityConfig::default(),
            agent: AgentQualityConfig::default(),
            semantic: SemanticQualityThresholds::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticQualityThresholds {
    pub min_actionability: f32,
    pub min_specificity: f32,
    pub min_evidence_quality: f32,
    pub min_depth: f32,
    pub max_redundancy: f32,
}

impl Default for SemanticQualityThresholds {
    fn default() -> Self {
        Self {
            min_actionability: 0.5,
            min_specificity: 0.5,
            min_evidence_quality: 0.5,
            min_depth: 0.5,
            max_redundancy: 0.3,
        }
    }
}

// =============================================================================
// DEEP REVIEW CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DeepReviewConfig {
    pub enabled: bool,
    pub max_attempts: u32,
    pub required_passes: u32,
    pub review_timeout_secs: u64,
    pub check_regression: bool,
    pub reject_tier1: bool,
    /// Minimum ratio of valid file references to total references.
    ///
    /// Rationale: Generated content references files that should exist in the codebase.
    /// A ratio of 0.5 (50%) means at least half of @file:line references must point to
    /// actual files. Lower values tolerate more hallucination or future/planned paths.
    /// Higher values (0.8+) are stricter, suitable for mature codebases.
    pub min_evidence_ratio: f32,
    /// Maximum characters of CLAUDE.md to include in semantic quality review
    pub claude_md_preview_chars: usize,
    /// Maximum characters per skill body to include in review
    pub skill_preview_chars: usize,
    /// Maximum characters per agent body to include in review
    pub agent_preview_chars: usize,
    /// Maximum number of skills to include in review (0 = unlimited)
    pub max_skills_in_review: usize,
    /// Maximum number of agents to include in review (0 = unlimited)
    pub max_agents_in_review: usize,
}

impl Default for DeepReviewConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 3,
            required_passes: 2,
            review_timeout_secs: 300,
            check_regression: true,
            reject_tier1: false, // LLM determines tier quality in SemanticQuality check
            min_evidence_ratio: 0.5,
            // Preview sizes - configurable to balance thoroughness vs token limits
            claude_md_preview_chars: 4000, // Increased from hardcoded 2000
            skill_preview_chars: 1000,     // Increased from hardcoded 500
            agent_preview_chars: 1000,     // Increased from hardcoded 500
            max_skills_in_review: 0,       // 0 = include all (was hardcoded 3)
            max_agents_in_review: 0,       // 0 = include all (was hardcoded 2)
        }
    }
}

// =============================================================================
// VALIDATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ValidationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Threshold in seconds for considering a file "recently modified"
    /// Files modified within this threshold trigger info-level notifications
    #[serde(default = "default_stale_threshold")]
    pub stale_file_threshold_secs: u64,
}

fn default_true() -> bool {
    true
}
fn default_stale_threshold() -> u64 {
    60
}

// =============================================================================
// SEMANTIC VALIDATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticValidationConfig {
    pub enabled: bool,
    pub use_ai_validation: bool,
    pub dimension_weights: SemanticDimensionWeights,
    pub min_overall_score: f32,
    pub weights: SemanticWeights,
    pub thresholds: SemanticThresholds,
    pub min_actionability: f32,
    pub min_specificity: f32,
    pub min_evidence_quality: f32,
    pub max_redundancy: f32,
    pub min_depth: f32,
    pub reject_generic_content: bool,
    pub require_file_line_refs: bool,
    pub min_actionable_items: usize,
}

impl Default for SemanticValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            use_ai_validation: true, // LLM-as-judge instead of regex patterns
            dimension_weights: SemanticDimensionWeights::default(),
            min_overall_score: 0.6,
            weights: SemanticWeights::default(),
            thresholds: SemanticThresholds::default(),
            min_actionability: 0.5,
            min_specificity: 0.5,
            min_evidence_quality: 0.5,
            max_redundancy: 0.3,
            min_depth: 0.5,
            reject_generic_content: true,
            require_file_line_refs: false,
            min_actionable_items: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticWeights {
    pub actionability: f32,
    pub specificity: f32,
    pub evidence: f32,
    pub redundancy: f32,
    pub depth: f32,
}

impl Default for SemanticWeights {
    fn default() -> Self {
        Self {
            actionability: 0.25,
            specificity: 0.25,
            evidence: 0.20,
            redundancy: 0.15,
            depth: 0.15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticThresholds {
    pub low_actionability_multiplier: f32,
    pub medium_specificity_threshold: f32,
    pub high_specificity_threshold: f32,
    pub evidence_base_score: f32,
    pub quantity_score_multiplier: f32,
    pub line_ref_bonus: f32,
    pub high_redundancy_threshold: f32,
    pub max_redundancy_ratio: f32,
    pub complexity_weight: f32,
    pub section_weight: f32,
    pub reference_weight: f32,
    pub depth_min_score: f32,
    pub depth_complexity_bonus: f32,
    // Fields for semantic_validator.rs
    pub min_line_length_actionability: usize,
    pub min_line_length_specificity: usize,
    pub max_generic_penalty: f32,
    pub file_line_bonus: f32,
    pub validity_score_weight: f32,
    pub quantity_score_weight: f32,
    pub overlap_threshold: f32,
    pub min_phrase_line_length: usize,
    pub max_phrase_words: usize,
    pub min_substantive_line_length: usize,
    pub min_substantive_lines_deep: usize,
    pub min_depth_indicators: usize,
    pub min_total_lines_shallow_check: usize,
    pub default_depth_score: f32,
    pub low_actionability_suggestion_threshold: usize,
    pub too_generic_suggestion_threshold: usize,
    pub redundant_suggestion_threshold: usize,
    pub shallow_suggestion_threshold: usize,
}

impl Default for SemanticThresholds {
    fn default() -> Self {
        Self {
            low_actionability_multiplier: 0.8,
            medium_specificity_threshold: 0.5,
            high_specificity_threshold: 0.8,
            evidence_base_score: 0.3,
            quantity_score_multiplier: 2.0,
            line_ref_bonus: 0.2,
            high_redundancy_threshold: 0.5,
            max_redundancy_ratio: 0.3,
            complexity_weight: 0.3,
            section_weight: 0.3,
            reference_weight: 0.4,
            depth_min_score: 0.5,
            depth_complexity_bonus: 0.1,
            // Defaults for semantic_validator.rs
            min_line_length_actionability: 10,
            min_line_length_specificity: 10,
            max_generic_penalty: 0.3,
            file_line_bonus: 0.2,
            validity_score_weight: 0.4,
            quantity_score_weight: 0.3,
            overlap_threshold: 0.5,
            min_phrase_line_length: 15,
            max_phrase_words: 5,
            min_substantive_line_length: 20,
            min_substantive_lines_deep: 3,
            min_depth_indicators: 2,
            min_total_lines_shallow_check: 5,
            default_depth_score: 0.5,
            low_actionability_suggestion_threshold: 2,
            too_generic_suggestion_threshold: 2,
            redundant_suggestion_threshold: 1,
            shallow_suggestion_threshold: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticDimensionWeights {
    pub specificity: f32,
    pub actionability: f32,
    pub evidence: f32,
    pub uniqueness: f32,
    pub redundancy: f32,
    pub depth: f32,
    pub min_substantive_lines_deep: usize,
    pub min_line_length_actionability: usize,
}

impl Default for SemanticDimensionWeights {
    fn default() -> Self {
        Self {
            specificity: 0.3,
            actionability: 0.3,
            evidence: 0.25,
            uniqueness: 0.15,
            redundancy: 0.1,
            depth: 0.2,
            min_substantive_lines_deep: 3,
            min_line_length_actionability: 20,
        }
    }
}

// =============================================================================
// ANALYSIS SPECIALTY
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisSpecialty {
    Architecture,
    Security,
    Performance,
    Testing,
    Documentation,
    Domain,
    Structure,
    Pattern,
    Constraint,
}

impl AnalysisSpecialty {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Architecture,
            Self::Security,
            Self::Performance,
            Self::Testing,
            Self::Documentation,
            Self::Domain,
            Self::Structure,
            Self::Pattern,
            Self::Constraint,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Architecture => "architecture",
            Self::Security => "security",
            Self::Performance => "performance",
            Self::Testing => "testing",
            Self::Documentation => "documentation",
            Self::Domain => "domain",
            Self::Structure => "structure",
            Self::Pattern => "pattern",
            Self::Constraint => "constraint",
        }
    }
}

// =============================================================================
// MULTI-AGENT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiAgentConfig {
    pub enabled: bool,
    pub specialists: Vec<AnalysisSpecialty>,
    pub enabled_specialists: Vec<AnalysisSpecialty>,
    pub parallel_execution: bool,
    pub conflict_resolution: ConflictResolution,
    pub cross_validate_specialists: bool,
    pub synthesis_retries: usize,
    pub specialist_timeout_secs: u64,
    pub total_timeout_secs: u64,
    /// Maximum files to include in pattern analysis prompts
    pub max_files_for_patterns: usize,
    /// Maximum lines per file for pattern analysis
    pub max_lines_per_file_patterns: usize,
    /// Maximum files to include in constraint analysis prompts
    pub max_files_for_constraints: usize,
    /// Maximum lines per file for constraint analysis
    pub max_lines_per_file_constraints: usize,
    /// Minimum detected modules to trigger multi-agent orchestration
    pub min_modules: usize,
    /// Generate module_map.json
    pub generate_module_map: bool,
    /// Minimum modules to trigger hierarchical grouping with sub-orchestrators
    pub min_modules_for_grouping: usize,
}

impl Default for MultiAgentConfig {
    fn default() -> Self {
        let default_specialists = vec![
            AnalysisSpecialty::Architecture,
            AnalysisSpecialty::Security,
            AnalysisSpecialty::Domain,
        ];
        Self {
            enabled: true,
            specialists: default_specialists.clone(),
            enabled_specialists: default_specialists,
            parallel_execution: true,
            conflict_resolution: ConflictResolution::WeightedVote,
            cross_validate_specialists: true,
            synthesis_retries: 3,
            specialist_timeout_secs: 120,
            total_timeout_secs: 600,
            // Code preview limits - configurable to balance context vs token limits
            max_files_for_patterns: 15,      // Increased from hardcoded 10
            max_lines_per_file_patterns: 80, // Increased from hardcoded 50
            max_files_for_constraints: 20,
            max_lines_per_file_constraints: 150,
            min_modules: 2,
            generate_module_map: true,
            min_modules_for_grouping: 6,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    #[default]
    WeightedVote,
    FirstWins,
    Merge,
}

// =============================================================================
// CROSS VALIDATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossValidationConfig {
    pub enabled: bool,
    pub check_consistency: bool,
    pub check_coverage: bool,
    pub min_overlap_score: f32,
    pub plan_output_consistency: bool,
    pub evidence_traceability: bool,
    pub artifact_consistency: bool,
}

impl Default for CrossValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_consistency: true,
            check_coverage: true,
            min_overlap_score: 0.5,
            plan_output_consistency: true,
            evidence_traceability: true,
            artifact_consistency: true,
        }
    }
}

// =============================================================================
// USABILITY CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UsabilityConfig {
    pub enabled: bool,
    pub check_readability: bool,
    pub check_completeness: bool,
    pub max_complexity_score: f32,
    pub min_usability_score: f32,
    pub max_context_tokens: usize,
    pub thresholds: UsabilityThresholds,
}

impl Default for UsabilityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_readability: true,
            check_completeness: true,
            max_complexity_score: 0.7,
            min_usability_score: 0.7,
            // 0 = auto-compute from ContextConfig based on model
            max_context_tokens: 0,
            thresholds: UsabilityThresholds::default(),
        }
    }
}

impl UsabilityConfig {
    /// Get effective max context tokens (auto-compute if 0)
    pub fn effective_max_context_tokens(
        &self,
        model_id: &str,
        context_config: &ContextWindowConfig,
    ) -> usize {
        if self.max_context_tokens > 0 {
            self.max_context_tokens
        } else {
            // Use ~25% of available context for artifact content validation
            (context_config.available_for_content(model_id) as f32 * 0.25) as usize
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UsabilityThresholds {
    pub progressive_disclosure_weight: f32,
    pub context_efficiency_weight: f32,
    pub task_relevance_weight: f32,
    pub min_entry_point_length: usize,
    pub max_entry_point_length: usize,
    pub min_unique_content_ratio: f32,
    pub min_valuable_rules_ratio: f32,
    pub min_word_length_for_uniqueness: usize,
    pub min_actionable_ratio: f32,
    pub min_description_length: usize,
    pub min_clear_scope_ratio: f32,
    pub tokens_per_word_estimate: f32,
    pub token_score_weight: f32,
    pub redundancy_score_weight: f32,
    pub essential_ratio_weight: f32,
}

impl Default for UsabilityThresholds {
    fn default() -> Self {
        Self {
            progressive_disclosure_weight: 0.4,
            context_efficiency_weight: 0.3,
            task_relevance_weight: 0.3,
            min_entry_point_length: 200,
            max_entry_point_length: 10000,
            min_unique_content_ratio: 0.3,
            min_valuable_rules_ratio: 0.5,
            min_word_length_for_uniqueness: 4,
            min_actionable_ratio: 0.5,
            min_description_length: 20,
            min_clear_scope_ratio: 0.5,
            tokens_per_word_estimate: 1.3,
            token_score_weight: 0.4,
            redundancy_score_weight: 0.3,
            essential_ratio_weight: 0.3,
        }
    }
}

// =============================================================================
// QUALITY LOOP CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QualityLoopConfig {
    pub enabled: bool,
    pub max_iterations: usize,
    pub max_outer_iterations: usize,
    pub target_quality: f32,
    pub min_improvement: f32,
    pub analysis_confidence_threshold: f32,
    pub synthesis_confidence_threshold: f32,
    pub max_invalid_reference_ratio: f32,
    pub reanalysis_gap_threshold: f32,
}

impl Default for QualityLoopConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_iterations: 50,
            max_outer_iterations: 20,
            target_quality: 0.90,
            min_improvement: 0.005,
            analysis_confidence_threshold: 0.75,
            synthesis_confidence_threshold: 0.85,
            max_invalid_reference_ratio: 0.1,
            reanalysis_gap_threshold: 0.2,
        }
    }
}

// =============================================================================
// AGENT GENERATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentGenerationConfig {
    pub max_agents: usize,
    pub min_evidence_refs: usize,
    pub require_domain_expertise: bool,
    pub default_tools: Vec<String>,
}

impl Default for AgentGenerationConfig {
    fn default() -> Self {
        Self {
            max_agents: 10,
            min_evidence_refs: 3,
            require_domain_expertise: true,
            default_tools: vec!["Read".into(), "Glob".into(), "Grep".into()],
        }
    }
}

// =============================================================================
// EVIDENCE CONFIG
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceDepth {
    Minimal,
    #[default]
    Standard,
    Comprehensive,
    FileOnly,
    FileAndLine,
    FileLineContext,
}

impl EvidenceDepth {
    pub fn min_refs(&self) -> usize {
        match self {
            Self::Minimal | Self::FileOnly => 1,
            Self::Standard | Self::FileAndLine => 2,
            Self::Comprehensive | Self::FileLineContext => 5,
        }
    }
}

// =============================================================================
// FEW SHOT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FewShotConfig {
    pub enabled: bool,
    pub max_examples: usize,
    pub example_selection: ExampleSelection,
}

impl Default for FewShotConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_examples: 3,
            example_selection: ExampleSelection::MostRelevant,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExampleSelection {
    #[default]
    MostRelevant,
    Random,
    Recent,
}

// =============================================================================
// DEEP ANALYSIS CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DeepAnalysisConfig {
    pub enabled: bool,
    pub max_depth: usize,
    pub max_iterations: usize,
    pub follow_imports: bool,
    pub analyze_dependencies: bool,
    pub min_confidence: f32,
    pub max_code_context_chars: usize,
    pub targeted_reanalysis: bool,
    // multi_agent moved to root Config
}

impl Default for DeepAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_depth: 5,       // Deeper analysis for complex projects
            max_iterations: 30, // More iterations for thorough analysis
            follow_imports: true,
            analyze_dependencies: true,
            min_confidence: 0.75,            // Higher confidence threshold
            max_code_context_chars: 100_000, // More context for better understanding
            targeted_reanalysis: true,
        }
    }
}

// =============================================================================
// STRUCTURAL VALIDATION CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StructuralValidationConfig {
    pub enabled: bool,
    pub check_file_refs: bool,
    pub check_syntax: bool,
    pub check_completeness: bool,
    pub min_module_coverage: f32,
    pub core_module_threshold: f32,
    pub required_modules: Vec<String>,
}

impl Default for StructuralValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_file_refs: true,
            check_syntax: true,
            check_completeness: true,
            min_module_coverage: 0.7,
            core_module_threshold: 0.5,
            required_modules: Vec::new(),
        }
    }
}

// =============================================================================
// CONFIG ACCESSORS (direct field references)
// =============================================================================

impl Config {
    pub fn circuit_breaker(&self) -> &CircuitBreakerConfig {
        &self.circuit_breaker
    }

    pub fn refinement(&self) -> &RefinementConfig {
        &self.refinement
    }

    pub fn learning(&self) -> &LearningConfig {
        &self.learning
    }

    pub fn quality(&self) -> &QualityConfig {
        &self.quality
    }

    pub fn deep_review(&self) -> &DeepReviewConfig {
        &self.deep_review
    }

    pub fn semantic_validation(&self) -> &SemanticValidationConfig {
        &self.semantic_validation
    }

    pub fn multi_agent(&self) -> &MultiAgentConfig {
        &self.multi_agent
    }

    pub fn cross_validation(&self) -> &CrossValidationConfig {
        &self.cross_validation
    }

    pub fn usability(&self) -> &UsabilityConfig {
        &self.usability
    }

    pub fn cross_artifact(&self) -> &CrossArtifactConfig {
        &self.cross_artifact
    }

    pub fn quality_loop(&self) -> &QualityLoopConfig {
        &self.quality_loop
    }

    pub fn few_shot(&self) -> &FewShotConfig {
        &self.few_shot
    }

    pub fn deep_analysis(&self) -> &DeepAnalysisConfig {
        &self.deep_analysis
    }

    pub fn structural_validation(&self) -> &StructuralValidationConfig {
        &self.structural_validation
    }

    pub fn timeout(&self) -> &TimeoutConfig {
        &self.timeout
    }

    pub fn evidence_depth(&self) -> EvidenceDepth {
        match self.analysis.depth {
            AnalysisDepth::Fast => EvidenceDepth::Minimal,
            AnalysisDepth::Standard => EvidenceDepth::Standard,
            AnalysisDepth::Complete => EvidenceDepth::Comprehensive,
        }
    }
}

// =============================================================================
// COMPATIBILITY TYPES (for existing pipeline code)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TierScoringWeights {
    pub specificity: f32,
    pub actionability: f32,
    pub evidence: f32,
    pub uniqueness: f32,
    pub tier_bonus: f32,
    // Additional fields for tier_filter.rs
    pub base_score: f32,
    pub file_ref_weight: f32,
    pub file_ref_max_count: usize,
    pub code_example_weight: f32,
    pub code_example_max_count: usize,
    pub tier3_indicator_weight: f32,
    pub tier3_indicator_max_count: usize,
    pub tier1_penalty: f32,
    pub tool_presence_weight: f32,
    pub section_weight: f32,
    pub generic_phrase_penalty: f32,
    pub example_indicator_weight: f32,
    pub path_scoped_weight: f32,
    pub generic_rule_penalty: f32,
    pub claude_md_base_score: f32,
    pub min_skill_body_lines: usize,
    pub min_claude_md_lines: usize,
    pub tier3_threshold: f32,
    pub tier2_threshold: f32,
    pub primary_content_ratio: f32,
}

impl Default for TierScoringWeights {
    fn default() -> Self {
        Self {
            specificity: 0.25,
            actionability: 0.25,
            evidence: 0.25,
            uniqueness: 0.15,
            tier_bonus: 0.10,
            // Tier filter scoring defaults
            base_score: 0.4,
            file_ref_weight: 0.1,
            file_ref_max_count: 5,
            code_example_weight: 0.05,
            code_example_max_count: 3,
            tier3_indicator_weight: 0.1,
            tier3_indicator_max_count: 3,
            tier1_penalty: 0.15,
            tool_presence_weight: 0.1,
            section_weight: 0.05,
            generic_phrase_penalty: 0.1,
            example_indicator_weight: 0.1,
            path_scoped_weight: 0.1,
            generic_rule_penalty: 0.15,
            claude_md_base_score: 0.5,
            min_skill_body_lines: 5,
            min_claude_md_lines: 20,
            tier3_threshold: 0.7,
            tier2_threshold: 0.4,
            primary_content_ratio: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillQualityConfig {
    pub min_file_refs: usize,
    pub min_score: f32,
    pub min_chars: usize,
    pub min_steps: usize,
    pub target_file_refs: usize,
}

impl Default for SkillQualityConfig {
    fn default() -> Self {
        Self {
            min_file_refs: 2,
            min_score: 0.6,
            min_chars: 500,
            min_steps: 3,
            target_file_refs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentQualityConfig {
    pub min_evidence_refs: usize,
    pub min_score: f32,
    pub require_domain_expertise: bool,
    pub min_file_refs: usize,
    pub min_chars: usize,
    pub min_sections: usize,
    pub target_file_refs: usize,
}

impl Default for AgentQualityConfig {
    fn default() -> Self {
        Self {
            min_evidence_refs: 2,
            min_score: 0.6,
            require_domain_expertise: true,
            min_file_refs: 2,
            min_chars: 500,
            min_sections: 3,
            target_file_refs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DimensionThresholds {
    pub specificity: f32,
    pub actionability: f32,
    pub evidence: f32,
    pub uniqueness: f32,
    pub semantic: f32,
    pub surface: f32,
    pub cross_artifact: f32,
    pub usability: f32,
}

impl Default for DimensionThresholds {
    fn default() -> Self {
        Self {
            specificity: 0.5,
            actionability: 0.5,
            evidence: 0.5,
            uniqueness: 0.3,
            semantic: 0.6,
            surface: 0.5,
            cross_artifact: 0.5,
            usability: 0.6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossArtifactConfig {
    pub enabled: bool,
    pub check_consistency: bool,
    pub check_coverage: bool,
    pub max_inconsistencies: usize,
    pub min_coherence_score: f32,
    pub max_overlap_ratio: f32,
    pub reference_weight: f32,
    pub coverage_weight: f32,
    pub role_clarity_weight: f32,
}

impl Default for CrossArtifactConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_consistency: true,
            check_coverage: true,
            max_inconsistencies: 5,
            min_coherence_score: 0.5,
            max_overlap_ratio: 0.3,
            reference_weight: 0.4,
            coverage_weight: 0.3,
            role_clarity_weight: 0.3,
        }
    }
}

// =============================================================================
// TIMEOUT CONFIG
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeoutConfig {
    pub session_timeout_secs: u64,
    pub quality_loop_timeout_secs: u64,
    pub analysis_phase_timeout_secs: u64,
    pub generation_phase_timeout_secs: u64,
    pub specialist_timeout_secs: u64,
    pub llm_call_timeout_secs: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            session_timeout_secs: 7200,        // 2 hours - generous for large projects
            quality_loop_timeout_secs: 3600,   // 1 hour - allow thorough quality iteration
            analysis_phase_timeout_secs: 1800, // 30 minutes - large monorepos need time
            generation_phase_timeout_secs: 900, // 15 minutes - complex generation
            specialist_timeout_secs: 300,      // 5 minutes - specialist tasks
            llm_call_timeout_secs: 600,        // 10 minutes - long LLM responses
        }
    }
}

impl TimeoutConfig {
    pub fn effective_checkpoint_interval_secs(&self) -> u64 {
        (self.quality_loop_timeout_secs / 4).max(60)
    }
}

// =============================================================================
// DISTRIBUTED ANALYSIS CONFIG
// =============================================================================

/// Configuration for distributed parallel analysis (100% coverage)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DistributedAnalysisConfig {
    /// Enable distributed analysis
    pub enabled: bool,
    /// Maximum parallel analysis agents
    pub max_parallel_agents: usize,
    /// Maximum tokens per analysis chunk
    pub max_tokens_per_chunk: usize,
    /// Overlap lines between chunks for context continuity
    pub chunk_overlap_lines: usize,
    /// Minimum files to trigger distributed analysis
    pub min_files_for_distributed: usize,
    /// Maximum characters to include per file in LLM prompts
    pub max_file_content_chars: usize,
    /// Maximum common import patterns to track in aggregation
    pub max_common_import_patterns: usize,
    /// Maximum dependency edges to display in prompts
    pub max_dependency_display: usize,
    /// Timeout for file read operations in seconds
    pub file_read_timeout_secs: u64,
}

impl Default for DistributedAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_parallel_agents: 8, // More parallelism for large projects
            max_tokens_per_chunk: 80_000, // Larger chunks for better context
            chunk_overlap_lines: 100, // More overlap for continuity
            min_files_for_distributed: 30, // Start distributed earlier
            max_file_content_chars: 20_000, // More content per file
            max_common_import_patterns: 20, // Track more patterns
            max_dependency_display: 50, // Show more dependencies
            file_read_timeout_secs: 60, // More time for large files
        }
    }
}

impl Config {
    pub fn distributed_analysis(&self) -> &DistributedAnalysisConfig {
        &self.distributed_analysis
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_validates() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_value_config_validation() {
        let mut config = Config::default();
        assert!(config.validate().is_ok());

        // Invalid dimension
        config.value.dimensions.mistake_prevention = 1.5;
        assert!(config.validate().is_err());

        // Fix and test weight validation
        config.value.dimensions.mistake_prevention = 0.5;
        config.value.weights.mistake_prevention = 0.5;
        config.value.weights.discoverability = 0.5;
        config.value.weights.artifact_fitness = 0.5; // Sum = 1.5, invalid
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_value_score_calculation() {
        let value_config = ValueConfig::default();
        let scores = ValueScores::new(0.8, 0.6, 0.7);

        let overall = value_config.calculate_overall(&scores);
        // 0.4 * 0.8 + 0.3 * 0.6 + 0.3 * 0.7 = 0.32 + 0.18 + 0.21 = 0.71
        assert!((overall - 0.71).abs() < 0.01);
    }

    #[test]
    fn test_analysis_depth() {
        assert_eq!(AnalysisDepth::Fast.max_files(), 20);
        assert_eq!(AnalysisDepth::Standard.max_llm_calls(), 50);
        assert!(AnalysisDepth::Complete.enable_deep_analysis());
    }

    #[test]
    fn test_budget_allocation() {
        let allocation = BudgetAllocation::default();
        let total = 1_000_000u64;

        assert_eq!(allocation.tokens_for_phase(total, "understanding"), 150_000);
        assert_eq!(allocation.tokens_for_phase(total, "generation"), 300_000);
    }

    #[test]
    fn test_domain_type() {
        assert_eq!(DomainType::FinTech.as_str(), "fintech");
        assert!(
            DomainType::FinTech
                .default_compliance()
                .contains(&"PCI-DSS")
        );
        assert!(
            DomainType::Healthcare
                .default_compliance()
                .contains(&"HIPAA")
        );
    }

    #[test]
    fn test_artifact_types() {
        let all = ArtifactType::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&ArtifactType::ClaudeMd));
        assert!(all.contains(&ArtifactType::Rules));
    }

    // =========================================================================
    // Cross-field validation tests
    // =========================================================================

    #[test]
    fn test_convergence_passes_vs_iterations() {
        let mut config = Config::default();

        // Valid: consecutive_passes <= max_iterations
        config.convergence.consecutive_passes = 2;
        config.convergence.max_iterations = 10;
        assert!(config.validate().is_ok());

        // Invalid: consecutive_passes > max_iterations
        config.convergence.consecutive_passes = 15;
        config.convergence.max_iterations = 10;
        let result = config.validate();
        assert!(result.is_err());
        // Check that the error mentions the constraint violation
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("consecutive_passes") || err_msg.contains("max_iterations"),
            "Error should mention convergence constraint: {}",
            err_msg
        );
    }
}
