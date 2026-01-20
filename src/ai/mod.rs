//! AI Integration Layer
//!
//! Provides LLM integration for intelligent plugin generation.

pub mod budget;
pub mod metrics;
pub mod model_capabilities;
pub mod preflight;
pub mod prompt;
pub mod provider;
pub mod timeout;
pub mod tokenizer;
pub mod validation;

pub use budget::{BudgetStats, GlobalTokenBudget, SharedBudget, create_shared_budget};
pub use metrics::{
    MetricsCollector, MetricsSummary, PhaseMetrics, SharedMetrics, create_shared_metrics,
};
pub use model_capabilities::{
    AuthMode, ContextConfig, ModelCapabilities, ModelFamily, ModelRegistry,
};
pub use preflight::{PreflightCheck, PreflightResult};
pub use prompt::PromptBuilder;
pub use provider::{
    ChainConfig, ChainedProvider, CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats,
    CircuitState, ErrorCategory, ErrorClassifier, LlmError, LlmProvider, LlmResponse,
    ProviderChain, ProviderChainBuilder, ProviderConfig, ResponseMetadata,
    ResponseTiming, TokenUsage, create_provider_for_model,
};

#[cfg(feature = "claude-agent")]
pub use provider::ClaudeAgentProvider;
pub use timeout::{TimeoutConfig, with_timeout, with_timeout_map};
pub use tokenizer::{
    BatchStats, FileBatch, FileWithTokens, TokenBudget, TokenBudgetBatcher, TokenCounter,
    TokenEstimator,
};
pub use validation::{JsonRepairer, ProcessedResponse, ValidationPipeline};
