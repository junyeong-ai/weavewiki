//! AI Integration Layer
//!
//! Provides LLM integration for intelligent documentation generation.

pub mod budget;
pub mod metrics;
pub mod preflight;
pub mod prompt;
pub mod provider;
pub mod timeout;
pub mod tokenizer;
pub mod validation;

pub use budget::{
    BudgetStats, ComplexityEstimate, GlobalTokenBudget, PhaseAllocations, PhaseEstimates,
    PhaseLimits, PhaseStats, SharedBudget, TaleConfig, TierBreakdown, create_shared_budget,
    estimate_complexity, estimate_complexity_simple,
};
pub use metrics::{
    MetricsCollector, MetricsSummary, PhaseMetrics, SharedMetrics, create_shared_metrics,
};
pub use preflight::{PreflightCheck, PreflightResult};
pub use prompt::{PromptBuilder, PromptSection, PromptTemplates};
pub use provider::{
    ChainConfig, ChainedProvider, CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats,
    CircuitState, ClaudeCodeProvider, ErrorCategory, ErrorClassifier, LlmError, LlmProvider,
    LlmResponse, ProviderChain, ProviderChainBuilder, ProviderConfig, ResponseMetadata,
    ResponseTiming, TokenUsage,
};
pub use timeout::{TimeoutConfig, with_timeout, with_timeout_map};
pub use tokenizer::{
    BatchStats, FileBatch, FileWithTokens, TokenBudget, TokenBudgetBatcher, TokenCounter,
    TokenEstimator,
};
pub use validation::{JsonRepairer, ProcessedResponse, ValidationPipeline};
