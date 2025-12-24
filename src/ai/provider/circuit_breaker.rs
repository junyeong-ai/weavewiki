//! Circuit Breaker Pattern for Provider Resilience
//!
//! Implements the circuit breaker pattern to prevent cascading failures
//! when LLM providers are experiencing issues.
//!
//! ## States
//!
//! - **Closed**: Normal operation, requests flow through
//! - **Open**: Provider is failing, requests are rejected immediately
//! - **HalfOpen**: Testing if provider has recovered
//!
//! ## Transitions
//!
//! ```text
//! Closed --[failure_threshold reached]--> Open
//! Open --[timeout elapsed]--> HalfOpen
//! HalfOpen --[success]--> Closed
//! HalfOpen --[failure]--> Open
//! ```

use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::constants::circuit_breaker as cb_constants;

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation - requests flow through
    Closed,
    /// Provider is failing - requests rejected immediately
    Open,
    /// Testing recovery - limited requests allowed
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "CLOSED"),
            Self::Open => write!(f, "OPEN"),
            Self::HalfOpen => write!(f, "HALF_OPEN"),
        }
    }
}

/// Configuration for circuit breaker behavior
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening circuit
    pub failure_threshold: u32,
    /// Number of consecutive successes in half-open to close circuit
    pub success_threshold: u32,
    /// Duration to wait before transitioning from open to half-open
    pub open_timeout: Duration,
    /// Maximum requests allowed in half-open state
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: cb_constants::FAILURE_THRESHOLD,
            success_threshold: cb_constants::SUCCESS_THRESHOLD,
            open_timeout: Duration::from_secs(cb_constants::RECOVERY_TIMEOUT_SECS),
            half_open_max_requests: cb_constants::HALF_OPEN_MAX_REQUESTS,
        }
    }
}

impl CircuitBreakerConfig {
    /// Create a strict configuration for critical providers
    pub fn strict() -> Self {
        Self {
            failure_threshold: 3,
            success_threshold: 3,
            open_timeout: Duration::from_secs(cb_constants::RECOVERY_TIMEOUT_SECS * 2),
            half_open_max_requests: 1,
        }
    }

    /// Create a lenient configuration for unstable providers
    pub fn lenient() -> Self {
        Self {
            failure_threshold: 10,
            success_threshold: 1,
            open_timeout: Duration::from_secs(15),
            half_open_max_requests: 5,
        }
    }
}

/// Unified internal state - all mutable state in single struct
/// to ensure atomicity of state transitions
#[derive(Debug)]
struct CircuitBreakerInner {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    half_open_requests: u32,
    opened_at: Option<Instant>,
    blocked_count: u64,
}

impl CircuitBreakerInner {
    fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            half_open_requests: 0,
            opened_at: None,
            blocked_count: 0,
        }
    }

    fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.half_open_requests = 0;
        self.opened_at = None;
    }
}

/// Thread-safe circuit breaker with unified state management.
///
/// All state is protected by a single RwLock to ensure consistency
/// between failure counts and state transitions.
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    provider_name: String,
    inner: RwLock<CircuitBreakerInner>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker for a provider
    pub fn new(provider_name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            provider_name: provider_name.into(),
            inner: RwLock::new(CircuitBreakerInner::new()),
        }
    }

    /// Create with default configuration
    pub fn with_defaults(provider_name: impl Into<String>) -> Self {
        Self::new(provider_name, CircuitBreakerConfig::default())
    }

    /// Get current circuit state (checking for timeout transitions)
    pub fn state(&self) -> CircuitState {
        // Check for open -> half-open transition
        self.check_state_transition();

        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .state
    }

    /// Check if request should be allowed
    ///
    /// Returns `true` if the request can proceed, `false` if circuit is open.
    pub fn allow_request(&self) -> bool {
        self.check_state_transition();

        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match inner.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                inner.blocked_count += 1;
                tracing::debug!(
                    "Circuit breaker [{}]: Request blocked (circuit OPEN)",
                    self.provider_name
                );
                false
            }
            CircuitState::HalfOpen => {
                if inner.half_open_requests < self.config.half_open_max_requests {
                    inner.half_open_requests += 1;
                    tracing::debug!(
                        "Circuit breaker [{}]: Allowing test request ({}/{})",
                        self.provider_name,
                        inner.half_open_requests,
                        self.config.half_open_max_requests
                    );
                    true
                } else {
                    inner.blocked_count += 1;
                    tracing::debug!(
                        "Circuit breaker [{}]: Half-open request limit reached",
                        self.provider_name
                    );
                    false
                }
            }
        }
    }

    /// Record a successful request
    pub fn record_success(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Reset failure count on any success
        inner.failure_count = 0;

        if inner.state == CircuitState::HalfOpen {
            inner.success_count += 1;

            if inner.success_count >= self.config.success_threshold {
                // Transition to closed
                inner.state = CircuitState::Closed;
                inner.success_count = 0;
                inner.half_open_requests = 0;
                inner.opened_at = None;

                tracing::info!(
                    "Circuit breaker [{}]: Closed (provider recovered)",
                    self.provider_name
                );
            }
        }
    }

    /// Record a failed request
    pub fn record_failure(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Reset success count on any failure
        inner.success_count = 0;

        match inner.state {
            CircuitState::Closed => {
                inner.failure_count += 1;

                if inner.failure_count >= self.config.failure_threshold {
                    // Transition to open
                    inner.state = CircuitState::Open;
                    inner.opened_at = Some(Instant::now());
                    inner.half_open_requests = 0;

                    tracing::warn!(
                        "Circuit breaker [{}]: Opened after {} failures (timeout: {:?})",
                        self.provider_name,
                        self.config.failure_threshold,
                        self.config.open_timeout
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open immediately opens the circuit
                inner.state = CircuitState::Open;
                inner.opened_at = Some(Instant::now());
                inner.half_open_requests = 0;
                inner.failure_count = 0;

                tracing::warn!(
                    "Circuit breaker [{}]: Re-opened after failure in half-open state",
                    self.provider_name
                );
            }
            CircuitState::Open => {
                // Already open, nothing to do
            }
        }
    }

    /// Get statistics for monitoring
    pub fn stats(&self) -> CircuitBreakerStats {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        CircuitBreakerStats {
            provider_name: self.provider_name.clone(),
            state: inner.state,
            failure_count: inner.failure_count,
            success_count: inner.success_count,
            blocked_count: inner.blocked_count,
            time_in_state: inner.opened_at.map(|t| t.elapsed()),
        }
    }

    /// Force reset to closed state (for manual intervention)
    pub fn reset(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        inner.reset();

        tracing::info!(
            "Circuit breaker [{}]: Manually reset to CLOSED",
            self.provider_name
        );
    }

    /// Check if state transition is needed (open -> half-open)
    fn check_state_transition(&self) {
        let should_transition = {
            let inner = self
                .inner
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            if inner.state == CircuitState::Open {
                if let Some(opened_at) = inner.opened_at {
                    opened_at.elapsed() >= self.config.open_timeout
                } else {
                    false
                }
            } else {
                false
            }
        };

        if should_transition {
            let mut inner = self
                .inner
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            // Double-check state (may have changed between read and write)
            if inner.state == CircuitState::Open {
                inner.state = CircuitState::HalfOpen;
                inner.half_open_requests = 0;
                inner.success_count = 0;

                tracing::info!(
                    "Circuit breaker [{}]: Transitioning to HALF_OPEN (testing recovery)",
                    self.provider_name
                );
            }
        }
    }
}

/// Statistics for monitoring circuit breaker state
#[derive(Debug, Clone)]
pub struct CircuitBreakerStats {
    pub provider_name: String,
    pub state: CircuitState,
    pub failure_count: u32,
    pub success_count: u32,
    pub blocked_count: u64,
    pub time_in_state: Option<Duration>,
}

impl CircuitBreakerStats {
    /// Format as human-readable summary
    pub fn summary(&self) -> String {
        let time_str = self
            .time_in_state
            .map(|d| format!(" for {:.1}s", d.as_secs_f64()))
            .unwrap_or_default();

        format!(
            "[{}] {} | failures={} successes={} blocked={}{}",
            self.provider_name,
            self.state,
            self.failure_count,
            self.success_count,
            self.blocked_count,
            time_str
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_closed() {
        let cb = CircuitBreaker::with_defaults("test");
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_opens_after_threshold_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // First two failures don't open
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        // Third failure opens
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_success_resets_failure_count() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        cb.record_failure();
        cb.record_failure();
        cb.record_success(); // Resets count

        cb.record_failure();
        cb.record_failure();
        // Still closed because success reset the count
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_closes_on_success() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            open_timeout: Duration::from_millis(1),
            half_open_max_requests: 5,
        };
        let cb = CircuitBreaker::new("test", config);

        // Open the circuit
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(10));

        // Should be half-open now
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        assert!(cb.allow_request());

        // Successes close it
        cb.record_success();
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_opens_on_failure() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            open_timeout: Duration::from_millis(1),
            half_open_max_requests: 5,
        };
        let cb = CircuitBreaker::new("test", config);

        // Open the circuit
        cb.record_failure();

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(10));

        // Should be half-open
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Failure reopens
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_blocked_count() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        cb.record_failure();
        assert!(!cb.allow_request());
        assert!(!cb.allow_request());
        assert!(!cb.allow_request());

        let stats = cb.stats();
        assert_eq!(stats.blocked_count, 3);
    }

    #[test]
    fn test_manual_reset() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_unified_state_consistency() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 1,
            open_timeout: Duration::from_millis(1),
            half_open_max_requests: 2,
        };
        let cb = CircuitBreaker::new("test", config);

        // Verify state is consistent through transitions
        cb.record_failure();
        let stats = cb.stats();
        assert_eq!(stats.failure_count, 1);
        assert_eq!(stats.state, CircuitState::Closed);

        cb.record_failure();
        let stats = cb.stats();
        assert_eq!(stats.failure_count, 2);
        assert_eq!(stats.state, CircuitState::Open);

        std::thread::sleep(Duration::from_millis(10));

        // Transition to half-open
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Success in half-open
        cb.record_success();
        let stats = cb.stats();
        assert_eq!(stats.state, CircuitState::Closed);
        assert_eq!(stats.failure_count, 0);
        assert_eq!(stats.success_count, 0);
    }
}
