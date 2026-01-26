//! AI Integration Layer
//!
//! Provides LLM integration for intelligent plugin generation.

pub mod budget;
pub mod metrics;
pub mod model_capabilities;
pub mod prompt;
pub mod provider;
pub mod timeout;
pub mod validation;

pub use budget::{BudgetStats, GlobalTokenBudget, SharedBudget, create_shared_budget};
pub use metrics::{
    MetricsCollector, MetricsSummary, PhaseMetrics, SharedMetrics, create_shared_metrics,
};
pub use model_capabilities::{
    AuthMode, ContextConfig, ModelCapabilities, ModelFamily, ModelRegistry,
};
pub use prompt::PromptBuilder;
pub use provider::{
    ChainConfig, ChainedProvider, CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats,
    CircuitState, ErrorCategory, ErrorClassifier, LlmError, LlmProvider, LlmResponse, ModelTier,
    ProviderChain, ProviderChainBuilder, ProviderConfig, ProviderSet, ResponseMetadata,
    ResponseTiming, TokenUsage, TrackedProvider, create_provider_for_model, create_provider_set,
    phase_id,
};

#[cfg(feature = "claude-agent")]
pub use provider::ClaudeAgentProvider;
pub use timeout::{with_timeout, with_timeout_map};
pub use validation::{ProcessedResponse, ValidationPipeline, parse_structured_output};
