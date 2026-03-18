//! LLM Provider Abstraction
//!
//! Defines the LlmProvider trait for structured LLM output generation.
//! All providers return `LlmResponse` with token usage metrics for cost tracking.
//!
//! ## Modules
//!
//! - `chain`: Fallback provider chain with cascading attempts
//! - `circuit_breaker`: Circuit breaker pattern for provider resilience
//! - `claude_agent`: Direct Claude API integration (default provider)

mod chain;
mod circuit_breaker;
mod openai;
mod tracked;

#[cfg(feature = "claude-agent")]
mod claude_agent;

pub use chain::{ChainConfig, ChainedProvider, ProviderChain, ProviderChainBuilder};
pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats, CircuitState,
};
pub use openai::OpenAiProvider;
pub use tracked::TrackedProvider;

// Note: Context window constants removed - use ModelRegistry instead
#[cfg(feature = "claude-agent")]
pub use claude_agent::ClaudeAgentProvider;

// Re-export error types from centralized location
pub use crate::types::{ErrorCategory, ErrorClassifier, LlmError};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::types::Result;

// =============================================================================
// LLM Response with Usage Metrics
// =============================================================================

/// Why the LLM stopped generating
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StopReason {
    #[default]
    EndTurn,
    MaxTokens,
    StopSequence,
    Refusal,
}

/// Complete LLM response including content, usage metrics, and stop reason
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Value,
    pub usage: TokenUsage,
    pub cost_usd: f64,
    pub timing: ResponseTiming,
    pub metadata: ResponseMetadata,
    pub stop_reason: StopReason,
}

impl LlmResponse {
    pub fn new(
        content: Value,
        usage: TokenUsage,
        timing: ResponseTiming,
        metadata: ResponseMetadata,
        stop_reason: StopReason,
    ) -> Self {
        Self {
            content,
            usage,
            cost_usd: 0.0,
            timing,
            metadata,
            stop_reason,
        }
    }

    pub fn content_only(content: Value) -> Self {
        Self {
            content,
            usage: TokenUsage::default(),
            cost_usd: 0.0,
            timing: ResponseTiming::default(),
            metadata: ResponseMetadata::default(),
            stop_reason: StopReason::EndTurn,
        }
    }

    pub fn with_metrics(
        content: Value,
        usage: TokenUsage,
        cost_usd: f64,
        timing: ResponseTiming,
        metadata: ResponseMetadata,
    ) -> Self {
        Self {
            content,
            usage,
            cost_usd,
            timing,
            metadata,
            stop_reason: StopReason::EndTurn,
        }
    }

    pub fn is_truncated(&self) -> bool {
        self.stop_reason == StopReason::MaxTokens
    }
}

/// Token usage metrics for cost tracking
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input tokens (prompt)
    pub input_tokens: u32,
    /// Output tokens (response)
    pub output_tokens: u32,
    /// Cache read tokens (if applicable)
    pub cache_read_tokens: u32,
    /// Cache write tokens (if applicable)
    pub cache_write_tokens: u32,
}

impl TokenUsage {
    /// Total tokens used (input + output)
    pub fn total(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }

    /// Total tokens including cache operations
    pub fn total_with_cache(&self) -> u32 {
        self.input_tokens + self.output_tokens + self.cache_read_tokens + self.cache_write_tokens
    }

    /// Create from OpenAI-style usage response
    pub fn from_openai(prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            input_tokens: prompt_tokens,
            output_tokens: completion_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        }
    }
}

/// Response timing metrics
#[derive(Debug, Clone, Default)]
pub struct ResponseTiming {
    /// Total response time in milliseconds (wall clock)
    pub total_ms: u64,
    /// API processing time in milliseconds (from provider response)
    pub api_ms: Option<u64>,
}

impl ResponseTiming {
    pub fn from_duration(duration: std::time::Duration) -> Self {
        Self {
            total_ms: duration.as_millis() as u64,
            api_ms: None,
        }
    }

    pub fn with_api_time(duration: std::time::Duration, api_ms: Option<u64>) -> Self {
        Self {
            total_ms: duration.as_millis() as u64,
            api_ms,
        }
    }
}

/// Response metadata
#[derive(Debug, Clone, Default)]
pub struct ResponseMetadata {
    /// Model used
    pub model: String,
    /// Provider name
    pub provider: String,
}

/// Shared LLM provider type for concurrent access across pipeline stages.
pub type SharedProvider = Arc<dyn LlmProvider + Send + Sync>;

// =============================================================================
// Provider Set - Tiered Model Routing
// =============================================================================

/// Model tiers for different task types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    /// Fast model for quick, simple tasks (Haiku-class)
    /// Used for: classification, detection, validation
    Fast,
    /// Default model for standard operations (Sonnet-class)
    /// Used for: generation, refinement, review
    Default,
    /// Performance model for critical high-intelligence tasks (Opus-class)
    /// Used for: constraint extraction, mistake discovery, deep analysis
    Performance,
}

/// Pipeline phase identifiers for type-safe phase-based routing
///
/// Use these constants with `ProviderSet::provider_for_phase()` to ensure
/// compile-time verification of phase names.
pub mod phase_id {
    // Fast tier phases (quick classification)
    pub const PROJECT_DETECTION: &str = "project_detection";
    pub const CONVENTION_INFERENCE: &str = "convention_inference";
    pub const VALIDATION: &str = "validation";
    pub const TIER_CLASSIFICATION: &str = "tier_classification";

    // Performance tier phases (high-intelligence)
    pub const CONSTRAINT_EXTRACTION: &str = "constraint_extraction";
    pub const MISTAKE_DISCOVERY: &str = "mistake_discovery";
    pub const DEEP_ANALYSIS: &str = "deep_analysis";
    pub const SYNTHESIS: &str = "synthesis";
    pub const DEEP_REVIEW: &str = "deep_review";

    // Default tier phases
    pub const GENERATION: &str = "generation";
    pub const REFINEMENT: &str = "refinement";
    pub const REVIEW: &str = "review";
}

/// Set of providers for tiered model routing
///
/// Enables phase-based model selection where different pipeline phases
/// use different model tiers based on task complexity.
#[derive(Clone)]
pub struct ProviderSet {
    /// Fast model provider (Haiku-class)
    pub fast: Arc<dyn LlmProvider>,
    /// Default model provider (Sonnet-class)
    pub default: Arc<dyn LlmProvider>,
    /// Performance model provider (Opus-class)
    pub performance: Arc<dyn LlmProvider>,
}

impl ProviderSet {
    /// Create a new ProviderSet with the same provider for all tiers.
    ///
    /// **Warning**: This does NOT wrap providers in ProviderChain, so there is:
    /// - No circuit breaker protection
    /// - No retry on transient failures
    /// - No fallback to alternative providers
    ///
    /// For production use with resilience, use `create_provider_set()` instead.
    pub fn single(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            fast: Arc::clone(&provider),
            default: Arc::clone(&provider),
            performance: provider,
        }
    }

    /// Enable budget and metrics tracking for all providers
    pub fn with_tracking(
        self,
        budget: crate::ai::budget::SharedBudget,
        metrics: crate::ai::metrics::SharedMetrics,
    ) -> Self {
        Self {
            fast: TrackedProvider::wrap_with_tracking(self.fast, budget.clone(), metrics.clone()),
            default: TrackedProvider::wrap_with_tracking(
                self.default,
                budget.clone(),
                metrics.clone(),
            ),
            performance: TrackedProvider::wrap_with_tracking(self.performance, budget, metrics),
        }
    }

    /// Get provider for a specific tier
    pub fn provider_for_tier(&self, tier: ModelTier) -> &Arc<dyn LlmProvider> {
        match tier {
            ModelTier::Fast => &self.fast,
            ModelTier::Default => &self.default,
            ModelTier::Performance => &self.performance,
        }
    }

    /// Get provider for a specific pipeline phase
    ///
    /// Phase-to-tier mapping:
    /// - Fast: project_detection, convention_inference, validation
    /// - Default: generation, refinement, review
    /// - Performance: constraint_extraction, mistake_discovery, deep_analysis
    ///
    /// Use `phase_id::*` constants for type-safe phase names.
    pub fn provider_for_phase(&self, phase: &str) -> &Arc<dyn LlmProvider> {
        // Fast tier - quick classification and detection
        if matches!(
            phase,
            phase_id::PROJECT_DETECTION
                | phase_id::CONVENTION_INFERENCE
                | phase_id::VALIDATION
                | phase_id::TIER_CLASSIFICATION
        ) {
            return &self.fast;
        }
        // Performance tier - high-intelligence tasks
        if matches!(
            phase,
            phase_id::CONSTRAINT_EXTRACTION
                | phase_id::MISTAKE_DISCOVERY
                | phase_id::DEEP_ANALYSIS
                | phase_id::SYNTHESIS
                | phase_id::DEEP_REVIEW
        ) {
            return &self.performance;
        }
        // Default tier - everything else
        &self.default
    }

    /// Get the default provider (backward compatibility)
    pub fn default_provider(&self) -> &Arc<dyn LlmProvider> {
        &self.default
    }
}

impl std::fmt::Debug for ProviderSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSet")
            .field("fast", &self.fast.model())
            .field("default", &self.default.model())
            .field("performance", &self.performance.model())
            .finish()
    }
}

// =============================================================================
// Provider Configuration
// =============================================================================

/// Configuration for LLM providers
#[derive(Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider type: "claude-agent", "openai"
    pub provider: String,
    /// Model name (provider-specific)
    pub model: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Temperature for LLM generation (0.0 = deterministic, 1.0 = creative)
    pub temperature: f32,
    /// API key - never serialized for security
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    /// API base URL (for custom endpoints)
    #[serde(default)]
    pub api_base: Option<String>,
    /// Maximum tokens to generate
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    /// Enable 1M extended context (only claude-sonnet-4-5-20250929)
    #[serde(default)]
    pub extended_context: bool,
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("timeout_secs", &self.timeout_secs)
            .field("temperature", &self.temperature)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("api_base", &self.api_base)
            .field("max_tokens", &self.max_tokens)
            .field("extended_context", &self.extended_context)
            .finish()
    }
}

fn default_max_tokens() -> usize {
    4096
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider: "claude-agent".to_string(),
            model: None,
            timeout_secs: 300,
            temperature: 0.0,
            api_key: None,
            api_base: None,
            max_tokens: 4096,
            extended_context: false,
        }
    }
}

// =============================================================================
// LLM Provider Trait
// =============================================================================

/// LLM Provider trait for structured output generation with usage metrics
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate structured output with JSON Schema validation
    ///
    /// Returns `LlmResponse` containing both the content and usage metrics.
    /// All providers must populate usage metrics for cost tracking.
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse>;

    /// Provider name for logging
    fn name(&self) -> &str;

    /// Model name currently in use
    fn model(&self) -> &str;

    /// Check if the provider is available
    async fn health_check(&self) -> Result<bool>;
}

/// Create a shared provider from configuration (async)
///
/// Default provider is "claude-agent" which uses direct Claude API.
/// Fallback to "openai" for OpenAI-compatible endpoints.
#[cfg(feature = "claude-agent")]
pub async fn create_provider(config: &ProviderConfig) -> Result<SharedProvider> {
    match config.provider.as_str() {
        "claude-agent" => Ok(Arc::new(ClaudeAgentProvider::from_config(config).await?)),
        "openai" => Ok(Arc::new(OpenAiProvider::new(config.clone())?)),
        _ => Err(crate::types::ClaudegenError::Config(format!(
            "Unknown provider: {}. Supported: claude-agent, openai",
            config.provider
        ))),
    }
}

/// Create a provider for a specific model (for task-based model selection)
///
/// This allows creating providers for different models on demand,
/// useful for routing high-intelligence tasks to Opus while keeping
/// standard tasks on Sonnet.
#[cfg(feature = "claude-agent")]
pub async fn create_provider_for_model(
    base_config: &ProviderConfig,
    model: &str,
) -> Result<SharedProvider> {
    let mut config = base_config.clone();
    config.model = Some(model.to_string());
    create_provider(&config).await
}

/// Create a shared provider (async for API consistency when claude-agent feature disabled)
#[cfg(not(feature = "claude-agent"))]
pub async fn create_provider(config: &ProviderConfig) -> Result<SharedProvider> {
    match config.provider.as_str() {
        "openai" => Ok(Arc::new(OpenAiProvider::new(config.clone())?)),
        _ => Err(crate::types::ClaudegenError::Config(format!(
            "Unknown provider: {}. Enable claude-agent feature for Claude API support.",
            config.provider
        ))),
    }
}

/// Create a provider for a specific model (non-claude-agent version)
#[cfg(not(feature = "claude-agent"))]
pub async fn create_provider_for_model(
    base_config: &ProviderConfig,
    model: &str,
) -> Result<SharedProvider> {
    let mut config = base_config.clone();
    config.model = Some(model.to_string());
    create_provider(&config).await
}

/// Create a tiered provider set from LlmConfig with resilience
///
/// Creates separate providers for fast, default, and performance tiers
/// based on the configured models. Each provider is wrapped in a ProviderChain
/// for circuit breaker protection and retry logic.
///
/// The circuit_breaker config from Config is converted and applied to each provider chain.
pub async fn create_provider_set(
    base_config: &ProviderConfig,
    llm_config: &crate::config::LlmConfig,
    circuit_breaker_config: &crate::config::CircuitBreakerConfig,
) -> Result<ProviderSet> {
    // Convert config type to provider circuit breaker type
    let cb_config = CircuitBreakerConfig {
        failure_threshold: circuit_breaker_config.failure_threshold as u32,
        success_threshold: 2, // Default: 2 successes to close
        open_timeout: std::time::Duration::from_secs(circuit_breaker_config.recovery_timeout_secs),
        half_open_max_requests: circuit_breaker_config.half_open_max_calls as u32,
    };

    // Create default provider first
    let default_raw = create_provider(base_config).await?;
    let default = wrap_with_resilience(default_raw, &cb_config);

    // Create fast provider (falls back to default if not configured)
    let fast = if let Some(fast_model) = &llm_config.fast_model {
        let raw = create_provider_for_model(base_config, fast_model).await?;
        wrap_with_resilience(raw, &cb_config)
    } else {
        Arc::clone(&default)
    };

    // Create performance provider (falls back to default if not configured)
    let performance = if let Some(perf_model) = &llm_config.performance_model {
        let raw = create_provider_for_model(base_config, perf_model).await?;
        wrap_with_resilience(raw, &cb_config)
    } else {
        Arc::clone(&default)
    };

    tracing::info!(
        fast_model = fast.model(),
        default_model = default.model(),
        performance_model = performance.model(),
        "ProviderSet created with tiered models and resilience"
    );

    Ok(ProviderSet {
        fast,
        default,
        performance,
    })
}

/// Wrap a provider in ProviderChain for circuit breaker and retry support
fn wrap_with_resilience(
    provider: SharedProvider,
    circuit_breaker_config: &CircuitBreakerConfig,
) -> Arc<dyn LlmProvider> {
    let chain_config = ChainConfig {
        circuit_breaker: circuit_breaker_config.clone(),
        ..ChainConfig::default()
    };
    let chain = ProviderChainBuilder::new()
        .add_shared(provider)
        .config(chain_config)
        .build();
    Arc::new(chain)
}
