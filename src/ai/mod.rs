//! AI Integration Layer
//!
//! Provides LLM integration for intelligent documentation generation.
//! Primary provider: Claude Code CLI with --json-schema support.
//!
//! ## Modules
//!
//! - `provider`: LLM provider abstraction and implementations
//! - `validation`: Response validation, JSON repair, diagram validation
//! - `tokenizer`: Token counting and budget management
//! - `budget`: Global pipeline token budget tracking
//! - `preflight`: Pre-flight validation checks
//! - `metrics`: Pipeline metrics collection and cost tracking
//! - `timeout`: Unified timeout configuration and helpers

pub mod budget;
pub mod metrics;
pub mod preflight;
pub mod prompt;
pub mod provider;
pub mod timeout;
pub mod tokenizer;
pub mod validation;

pub use budget::{
    BudgetStats, BudgetStatus, ComplexityEstimate, ComplexityEstimator, DegradationAction,
    DynamicAllocator, GlobalTokenBudget, PhaseAllocations, PhaseEstimates, PhaseLimits, PhaseStats,
    RuntimeMonitor, SharedBudget, TaleConfig, TierBreakdown, create_shared_budget,
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
