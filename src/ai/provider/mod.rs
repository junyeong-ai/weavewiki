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

#[cfg(feature = "claude-agent")]
mod claude_agent;

pub use chain::{ChainConfig, ChainedProvider, ProviderChain, ProviderChainBuilder};
pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats, CircuitState,
};
pub use openai::OpenAiProvider;

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

/// Complete LLM response including content, usage metrics, and actual cost
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// Generated content (structured JSON)
    pub content: Value,
    /// Token usage metrics
    pub usage: TokenUsage,
    /// Actual cost in USD (from provider API response)
    pub cost_usd: f64,
    /// Response timing
    pub timing: ResponseTiming,
    /// Provider and model info
    pub metadata: ResponseMetadata,
}

impl LlmResponse {
    /// Create response with content only (usage/cost unknown)
    pub fn content_only(content: Value) -> Self {
        Self {
            content,
            usage: TokenUsage::default(),
            cost_usd: 0.0,
            timing: ResponseTiming::default(),
            metadata: ResponseMetadata::default(),
        }
    }

    /// Create full response with all metrics including actual cost
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
        }
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
