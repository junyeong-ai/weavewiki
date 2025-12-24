//! Unified Error Type System
//!
//! Centralized error types for the entire application.
//! Provides intelligent error classification for retry and fallback decisions.
//!
//! ## Error Categories
//!
//! - **Transient**: Temporary issues that may resolve (retry)
//! - **RateLimit**: API rate limiting (wait and retry)
//! - **TokenLimit**: Context too large (reduce or fallback)
//! - **Auth**: Authentication failures (fail fast)
//! - **Network**: Connectivity issues (retry with backoff)
//! - **Unavailable**: Provider unavailable (fallback to next)
//!
//! ## Design Principles
//!
//! - Single unified error type (WeaveError) for the entire application
//! - Structured error variants with context for better debugging
//! - Category-based routing for retry and fallback decisions
//! - No panic/unwrap - all errors are recoverable

use std::time::Duration;
use thiserror::Error;

use crate::ai::budget::BudgetError;

// =============================================================================
// Error Categories
// =============================================================================

/// Unified error categories for intelligent routing and retry decisions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Rate limited - wait then retry same provider
    RateLimit,
    /// Context/token limit exceeded - reduce or fallback
    TokenLimit,
    /// Authentication failed - fail fast, don't retry
    Auth,
    /// Network/connectivity issues - retry with backoff
    Network,
    /// Provider unavailable - fallback to next
    Unavailable,
    /// Invalid request - don't retry, fix request
    BadRequest,
    /// Parsing LLM response failed - may retry with different prompt
    ParseError,
    /// Temporary server issues - retry same provider
    Transient,
    /// Unknown error - conservative retry
    Unknown,
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimit => write!(f, "RATE_LIMIT"),
            Self::TokenLimit => write!(f, "TOKEN_LIMIT"),
            Self::Auth => write!(f, "AUTH"),
            Self::Network => write!(f, "NETWORK"),
            Self::Unavailable => write!(f, "UNAVAILABLE"),
            Self::BadRequest => write!(f, "BAD_REQUEST"),
            Self::ParseError => write!(f, "PARSE_ERROR"),
            Self::Transient => write!(f, "TRANSIENT"),
            Self::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

impl ErrorCategory {
    /// Check if this category is retryable on the same provider
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimit | Self::Network | Self::Transient | Self::ParseError
        )
    }

    /// Check if this category should trigger fallback to next provider
    pub fn should_fallback(&self) -> bool {
        matches!(self, Self::TokenLimit | Self::Unavailable)
    }

    /// Get recommended retry delay for this category
    pub fn recommended_delay(&self) -> Duration {
        match self {
            Self::RateLimit => Duration::from_secs(30),
            Self::Network => Duration::from_secs(5),
            Self::Transient => Duration::from_secs(2),
            Self::ParseError => Duration::from_secs(1),
            _ => Duration::from_millis(500),
        }
    }
}

// =============================================================================
// LLM Error
// =============================================================================

/// Unified LLM error with category, context, and retry hints
#[derive(Debug, Clone)]
pub struct LlmError {
    /// Error category for routing decisions
    pub category: ErrorCategory,
    /// Detailed error message
    pub message: String,
    /// Provider that produced the error
    pub provider: Option<String>,
    /// Suggested wait time before retry (if applicable)
    pub retry_after: Option<Duration>,
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(provider) = &self.provider {
            write!(f, "[{}:{}] {}", provider, self.category, self.message)
        } else {
            write!(f, "[{}] {}", self.category, self.message)
        }
    }
}

impl std::error::Error for LlmError {}

impl LlmError {
    /// Create a new LLM error
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            provider: None,
            retry_after: None,
        }
    }

    /// Create error with provider context
    pub fn with_provider(
        category: ErrorCategory,
        message: impl Into<String>,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            category,
            message: message.into(),
            provider: Some(provider.into()),
            retry_after: None,
        }
    }

    /// Add provider context to existing error
    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Add suggested retry delay
    pub fn retry_after(mut self, duration: Duration) -> Self {
        self.retry_after = Some(duration);
        self
    }

    /// Create from simple message (defaults to Unknown category)
    pub fn from_message(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Unknown, message)
    }

    /// Check if error is retryable on the same provider
    pub fn is_retryable(&self) -> bool {
        self.category.is_retryable()
    }

    /// Check if error should trigger fallback to next provider
    pub fn should_fallback(&self) -> bool {
        self.category.should_fallback()
    }

    /// Get recommended retry delay
    pub fn recommended_delay(&self) -> Duration {
        self.retry_after
            .unwrap_or_else(|| self.category.recommended_delay())
    }
}

// =============================================================================
// Error Classifier
// =============================================================================

/// Error classifier for intelligent error routing
pub struct ErrorClassifier;

impl ErrorClassifier {
    /// Classify an error message from any provider
    pub fn classify(message: &str, provider: &str) -> LlmError {
        let lower = message.to_lowercase();

        // Rate limiting patterns
        if lower.contains("rate limit")
            || lower.contains("429")
            || lower.contains("too many requests")
            || lower.contains("quota exceeded")
        {
            return LlmError::with_provider(ErrorCategory::RateLimit, message, provider)
                .retry_after(Duration::from_secs(30));
        }

        // Token/context limit patterns
        if lower.contains("token")
            && (lower.contains("limit") || lower.contains("exceed") || lower.contains("maximum"))
            || lower.contains("context length")
            || lower.contains("context too long")
            || lower.contains("too large")
        {
            return LlmError::with_provider(ErrorCategory::TokenLimit, message, provider);
        }

        // Authentication patterns
        if lower.contains("auth")
            || lower.contains("401")
            || lower.contains("403")
            || lower.contains("api key")
            || lower.contains("invalid key")
            || lower.contains("unauthorized")
            || lower.contains("permission denied")
        {
            return LlmError::with_provider(ErrorCategory::Auth, message, provider);
        }

        // Network patterns
        if lower.contains("network")
            || lower.contains("connection")
            || lower.contains("dns")
            || lower.contains("timeout")
            || lower.contains("timed out")
            || lower.contains("unreachable")
        {
            return LlmError::with_provider(ErrorCategory::Network, message, provider)
                .retry_after(Duration::from_secs(5));
        }

        // Provider unavailable patterns
        if lower.contains("503")
            || lower.contains("502")
            || lower.contains("service unavailable")
            || lower.contains("server error")
            || lower.contains("500")
            || lower.contains("internal error")
            || lower.contains("not found")
            || lower.contains("not installed")
        {
            return LlmError::with_provider(ErrorCategory::Unavailable, message, provider);
        }

        // Bad request patterns
        if lower.contains("400")
            || lower.contains("bad request")
            || lower.contains("invalid")
            || lower.contains("malformed")
        {
            return LlmError::with_provider(ErrorCategory::BadRequest, message, provider);
        }

        // Parse error patterns
        if lower.contains("parse")
            || lower.contains("json")
            || lower.contains("syntax")
            || lower.contains("unexpected token")
        {
            return LlmError::with_provider(ErrorCategory::ParseError, message, provider)
                .retry_after(Duration::from_secs(1));
        }

        // Transient patterns (server-side issues that may resolve)
        if lower.contains("retry")
            || lower.contains("temporary")
            || lower.contains("overloaded")
            || lower.contains("non-zero status")
        {
            return LlmError::with_provider(ErrorCategory::Transient, message, provider)
                .retry_after(Duration::from_secs(2));
        }

        // Default: unknown error
        LlmError::with_provider(ErrorCategory::Unknown, message, provider)
    }

    /// Classify HTTP status code directly (more accurate than string matching)
    pub fn classify_http_status(status: u16, message: &str, provider: &str) -> LlmError {
        match status {
            429 => LlmError::with_provider(ErrorCategory::RateLimit, message, provider)
                .retry_after(Duration::from_secs(30)),
            401 | 403 => LlmError::with_provider(ErrorCategory::Auth, message, provider),
            400 => LlmError::with_provider(ErrorCategory::BadRequest, message, provider),
            // 500 series are transient - can retry
            500 | 502 | 503 | 504 => {
                LlmError::with_provider(ErrorCategory::Transient, message, provider)
                    .retry_after(Duration::from_secs(5))
            }
            404 => LlmError::with_provider(ErrorCategory::Unavailable, message, provider),
            _ => LlmError::with_provider(ErrorCategory::Unknown, message, provider),
        }
    }

    /// Classify a WeaveError with proper type-based routing
    pub fn classify_weave_error(err: &WeaveError, provider: &str) -> LlmError {
        match err {
            WeaveError::Config(_) => {
                LlmError::with_provider(ErrorCategory::BadRequest, err.to_string(), provider)
            }
            WeaveError::Io(_) => {
                LlmError::with_provider(ErrorCategory::Network, err.to_string(), provider)
                    .retry_after(Duration::from_secs(5))
            }
            WeaveError::Database(_) => {
                LlmError::with_provider(ErrorCategory::Unavailable, err.to_string(), provider)
            }
            WeaveError::LlmApi(msg) => Self::classify(msg, provider),
            WeaveError::Llm(llm_err) => Self::classify(&llm_err.message, provider),
            WeaveError::BudgetExceeded { .. } => {
                LlmError::with_provider(ErrorCategory::TokenLimit, err.to_string(), provider)
            }
            WeaveError::Json(_) => {
                LlmError::with_provider(ErrorCategory::ParseError, err.to_string(), provider)
            }
            _ => LlmError::with_provider(ErrorCategory::Unknown, err.to_string(), provider),
        }
    }
}

// =============================================================================
// Validation Error
// =============================================================================

/// Structured validation error with context
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// What validation failed
    pub kind: ValidationErrorKind,
    /// Field or component that failed validation
    pub field: Option<String>,
    /// Detailed message
    pub message: String,
    /// Expected value or format
    pub expected: Option<String>,
    /// Actual value received
    pub actual: Option<String>,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(field) = &self.field {
            write!(f, "Validation failed for '{}': {}", field, self.message)
        } else {
            write!(f, "Validation failed: {}", self.message)
        }
    }
}

impl std::error::Error for ValidationError {}

impl ValidationError {
    /// Create a new validation error
    pub fn new(kind: ValidationErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            field: None,
            message: message.into(),
            expected: None,
            actual: None,
        }
    }

    /// Add field context
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// Add expected/actual values
    pub fn with_comparison(
        mut self,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        self.expected = Some(expected.into());
        self.actual = Some(actual.into());
        self
    }

    /// Create from simple message
    pub fn from_message(message: impl Into<String>) -> Self {
        Self::new(ValidationErrorKind::General, message)
    }
}

/// Validation error kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationErrorKind {
    /// Schema validation failed
    Schema,
    /// Required field missing
    MissingField,
    /// Invalid format
    Format,
    /// Value out of range
    Range,
    /// Consistency check failed
    Consistency,
    /// General validation error
    General,
}

// =============================================================================
// Application Error
// =============================================================================

#[derive(Debug, Error)]
pub enum WeaveError {
    // -------------------------------------------------------------------------
    // System Errors (auto From impl)
    // -------------------------------------------------------------------------
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    // -------------------------------------------------------------------------
    // LLM Errors
    // -------------------------------------------------------------------------
    /// Structured LLM error with category and retry hints
    #[error("LLM error: {0}")]
    Llm(LlmError),

    /// Simple LLM API error (use Llm variant for structured errors)
    #[error("LLM API error: {0}")]
    LlmApi(String),

    // -------------------------------------------------------------------------
    // Pipeline Errors
    // -------------------------------------------------------------------------
    /// Pipeline phase error with recovery context
    #[error("Pipeline error in phase {phase}: {message}")]
    Pipeline {
        phase: u8,
        phase_name: String,
        message: String,
        recoverable: bool,
    },

    /// Operation timeout with context
    #[error("Timeout after {duration:?}: {operation}")]
    Timeout {
        operation: String,
        duration: Duration,
    },

    // -------------------------------------------------------------------------
    // Domain Errors
    // -------------------------------------------------------------------------
    #[error("Parse error in {path}: {message}")]
    Parse { message: String, path: String },

    #[error("{0}")]
    Validation(ValidationError),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Not initialized: run 'weavewiki init' first")]
    NotInitialized,

    #[error("Wiki generation failed for {item}: {reason}")]
    WikiGeneration { item: String, reason: String },

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Verification failed: {0}")]
    Verification(String),

    // -------------------------------------------------------------------------
    // Budget Errors
    // -------------------------------------------------------------------------
    #[error("Budget exceeded: consumed {consumed} of {budget} tokens")]
    BudgetExceeded { consumed: u64, budget: u64 },

    #[error("Phase budget exceeded: {phase_name} consumed {consumed}/{limit} tokens")]
    PhaseBudgetExceeded {
        phase: u8,
        phase_name: String,
        consumed: u64,
        limit: u64,
    },
}

impl From<LlmError> for WeaveError {
    fn from(err: LlmError) -> Self {
        WeaveError::Llm(err)
    }
}

impl From<ValidationError> for WeaveError {
    fn from(err: ValidationError) -> Self {
        WeaveError::Validation(err)
    }
}

impl From<BudgetError> for WeaveError {
    fn from(err: BudgetError) -> Self {
        match err {
            BudgetError::GlobalExceeded {
                consumed, budget, ..
            } => WeaveError::BudgetExceeded { consumed, budget },
            BudgetError::PhaseExceeded {
                phase,
                phase_name,
                consumed,
                limit,
                ..
            } => WeaveError::PhaseBudgetExceeded {
                phase,
                phase_name: phase_name.to_string(),
                consumed,
                limit,
            },
            BudgetError::InvalidPhase { phase } => {
                WeaveError::Config(format!("Invalid phase number: {} (valid: 0-4)", phase))
            }
            BudgetError::ReservationFailed {
                phase,
                requested,
                available,
            } => WeaveError::Config(format!(
                "Budget reservation failed for phase {}: requested {}, available {}",
                phase, requested, available
            )),
        }
    }
}

impl From<anyhow::Error> for WeaveError {
    fn from(err: anyhow::Error) -> Self {
        // Try to downcast to known error types
        if err.downcast_ref::<rusqlite::Error>().is_some() {
            return WeaveError::Storage(err.to_string());
        }
        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
            return WeaveError::Io(std::io::Error::new(io_err.kind(), io_err.to_string()));
        }

        // Default to Storage error for context-wrapped errors (most anyhow usage is in storage)
        WeaveError::Storage(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, WeaveError>;

// =============================================================================
// Helper Functions
// =============================================================================

impl WeaveError {
    /// Create a timeout error
    pub fn timeout(operation: impl Into<String>, duration: Duration) -> Self {
        Self::Timeout {
            operation: operation.into(),
            duration,
        }
    }

    /// Create a pipeline error
    pub fn pipeline(phase: u8, phase_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Pipeline {
            phase,
            phase_name: phase_name.into(),
            message: message.into(),
            recoverable: false,
        }
    }

    /// Create a recoverable pipeline error
    pub fn pipeline_recoverable(
        phase: u8,
        phase_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::Pipeline {
            phase,
            phase_name: phase_name.into(),
            message: message.into(),
            recoverable: true,
        }
    }

    /// Create an LLM error from message (convenience wrapper)
    pub fn llm(message: impl Into<String>) -> Self {
        Self::Llm(LlmError::from_message(message))
    }

    /// Create an LLM error with category
    pub fn llm_with_category(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self::Llm(LlmError::new(category, message))
    }

    /// Check if this error is recoverable (can be retried)
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::Llm(e) => e.is_retryable(),
            Self::Pipeline { recoverable, .. } => *recoverable,
            Self::Timeout { .. } => true, // Timeouts can be retried
            _ => false,
        }
    }

    /// Check if this error should trigger fallback to another provider
    pub fn should_fallback(&self) -> bool {
        match self {
            Self::Llm(e) => e.should_fallback(),
            Self::BudgetExceeded { .. } | Self::PhaseBudgetExceeded { .. } => true,
            _ => false,
        }
    }
}

/// Context extension trait for adding context to errors
pub trait ResultExt<T> {
    /// Add context to an error
    fn with_context<C: Into<String>>(self, context: C) -> Result<T>;

    /// Add context using a closure (lazy evaluation)
    fn with_context_fn<F, C>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> C,
        C: Into<String>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> ResultExt<T> for std::result::Result<T, E> {
    fn with_context<C: Into<String>>(self, context: C) -> Result<T> {
        self.map_err(|e| WeaveError::Storage(format!("{}: {}", context.into(), e)))
    }

    fn with_context_fn<F, C>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> C,
        C: Into<String>,
    {
        self.map_err(|e| WeaveError::Storage(format!("{}: {}", f().into(), e)))
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_category_display() {
        assert_eq!(ErrorCategory::RateLimit.to_string(), "RATE_LIMIT");
        assert_eq!(ErrorCategory::TokenLimit.to_string(), "TOKEN_LIMIT");
        assert_eq!(ErrorCategory::Auth.to_string(), "AUTH");
    }

    #[test]
    fn test_error_category_retryable() {
        assert!(ErrorCategory::RateLimit.is_retryable());
        assert!(ErrorCategory::Network.is_retryable());
        assert!(ErrorCategory::Transient.is_retryable());
        assert!(ErrorCategory::ParseError.is_retryable());
        assert!(!ErrorCategory::Auth.is_retryable());
        assert!(!ErrorCategory::BadRequest.is_retryable());
    }

    #[test]
    fn test_error_category_fallback() {
        assert!(ErrorCategory::TokenLimit.should_fallback());
        assert!(ErrorCategory::Unavailable.should_fallback());
        assert!(!ErrorCategory::RateLimit.should_fallback());
        assert!(!ErrorCategory::Auth.should_fallback());
    }

    #[test]
    fn test_classify_rate_limit() {
        let err = ErrorClassifier::classify("Rate limit exceeded, please retry", "openai");
        assert_eq!(err.category, ErrorCategory::RateLimit);
        assert!(err.is_retryable());
        assert!(!err.should_fallback());
    }

    #[test]
    fn test_classify_token_limit() {
        let err = ErrorClassifier::classify("Token limit exceeded: 150000 > 128000", "claude");
        assert_eq!(err.category, ErrorCategory::TokenLimit);
        assert!(!err.is_retryable());
        assert!(err.should_fallback());
    }

    #[test]
    fn test_classify_auth() {
        let err = ErrorClassifier::classify("Invalid API key provided", "openai");
        assert_eq!(err.category, ErrorCategory::Auth);
        assert!(!err.is_retryable());
        assert!(!err.should_fallback());
    }

    #[test]
    fn test_classify_network() {
        let err = ErrorClassifier::classify("Connection timed out after 30s", "ollama");
        assert_eq!(err.category, ErrorCategory::Network);
        assert!(err.is_retryable());
    }

    #[test]
    fn test_classify_unavailable() {
        let err = ErrorClassifier::classify("Service unavailable (503)", "openai");
        assert_eq!(err.category, ErrorCategory::Unavailable);
        assert!(err.should_fallback());
    }

    #[test]
    fn test_classify_unknown() {
        let err = ErrorClassifier::classify("Something weird happened", "test");
        assert_eq!(err.category, ErrorCategory::Unknown);
    }

    #[test]
    fn test_classify_http_status() {
        let rate_limit = ErrorClassifier::classify_http_status(429, "Rate limited", "test");
        assert_eq!(rate_limit.category, ErrorCategory::RateLimit);

        let auth = ErrorClassifier::classify_http_status(401, "Unauthorized", "test");
        assert_eq!(auth.category, ErrorCategory::Auth);

        let server_error = ErrorClassifier::classify_http_status(500, "Server error", "test");
        assert_eq!(server_error.category, ErrorCategory::Transient);
    }

    #[test]
    fn test_recommended_delay() {
        let rate_limit = LlmError::new(ErrorCategory::RateLimit, "test");
        assert!(rate_limit.recommended_delay() >= Duration::from_secs(30));

        let network = LlmError::new(ErrorCategory::Network, "test");
        assert!(network.recommended_delay() >= Duration::from_secs(5));

        let custom =
            LlmError::new(ErrorCategory::Unknown, "test").retry_after(Duration::from_secs(100));
        assert_eq!(custom.recommended_delay(), Duration::from_secs(100));
    }

    #[test]
    fn test_llm_error_display() {
        let err = LlmError::with_provider(ErrorCategory::RateLimit, "Too many requests", "openai");
        assert_eq!(err.to_string(), "[openai:RATE_LIMIT] Too many requests");

        let err_no_provider = LlmError::new(ErrorCategory::Network, "Connection failed");
        assert_eq!(err_no_provider.to_string(), "[NETWORK] Connection failed");
    }
}
