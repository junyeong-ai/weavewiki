//! Fallback Provider Chain with Circuit Breaker
//!
//! Cascading provider attempts with intelligent routing and resilience patterns.
//!
//! ## Features
//!
//! - **Circuit Breaker**: Automatically disables failing providers
//! - **DashMap**: Lock-free concurrent access to circuit breakers
//! - **Rate Limit Aware**: Parses retry-after headers for intelligent backoff
//! - **Exponential Backoff**: With proper random jitter using `rand` crate
//! - **Error Classification**: Intelligent retry/fallback decisions
//!
//! ## Strategy
//!
//! 1. Check circuit breaker state
//! 2. Try provider if circuit is closed/half-open
//! 3. On failure, classify error and update circuit breaker
//! 4. If rate-limited, use retry-after from response
//! 5. If fallback-eligible, try next provider
//! 6. Continue until success or all providers exhausted

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dashmap::DashMap;
use rand::Rng;
use serde_json::Value;
use tokio::time::sleep;
use tracing::{debug, info, instrument, warn};

use crate::constants::chain as chain_constants;

use super::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
use super::{LlmProvider, LlmResponse, ProviderConfig, SharedProvider};
use crate::types::{ErrorCategory, ErrorClassifier, LlmError, Result, WeaveError};

/// Provider with metadata for chain routing
#[derive(Clone)]
pub struct ChainedProvider {
    /// Provider instance
    pub provider: Arc<dyn LlmProvider + Send + Sync>,
    /// Cost per 1K tokens (for optimization)
    pub cost_per_1k: f32,
    /// Priority (lower = try first)
    pub priority: u8,
    /// Maximum retries for this provider
    pub max_retries: u8,
}

impl ChainedProvider {
    pub fn new(provider: Arc<dyn LlmProvider + Send + Sync>) -> Self {
        Self {
            provider,
            cost_per_1k: 0.0,
            priority: 100,
            max_retries: chain_constants::DEFAULT_MAX_RETRIES,
        }
    }

    /// Create from a shared provider
    pub fn from_shared(provider: SharedProvider) -> Self {
        Self::new(provider)
    }

    pub fn with_cost(mut self, cost: f32) -> Self {
        self.cost_per_1k = cost;
        self
    }

    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_max_retries(mut self, retries: u8) -> Self {
        self.max_retries = retries;
        self
    }
}

/// Configuration for the provider chain
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// Maximum total attempts across all providers
    pub max_total_attempts: usize,
    /// Base delay for exponential backoff
    pub base_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Backoff multiplier
    pub backoff_factor: f32,
    /// Whether to optimize for cost
    pub cost_optimize: bool,
    /// Circuit breaker configuration
    pub circuit_breaker: CircuitBreakerConfig,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            max_total_attempts: chain_constants::MAX_TOTAL_ATTEMPTS,
            base_delay: Duration::from_millis(chain_constants::BASE_DELAY_MS),
            max_delay: Duration::from_secs(chain_constants::MAX_DELAY_SECS),
            backoff_factor: chain_constants::BACKOFF_FACTOR,
            cost_optimize: true,
            circuit_breaker: CircuitBreakerConfig::default(),
        }
    }
}

/// Result of a chain execution attempt
#[derive(Debug)]
pub struct ChainAttemptResult {
    pub provider_name: String,
    pub attempt_number: usize,
    pub success: bool,
    pub error: Option<LlmError>,
    pub duration_ms: u64,
    pub circuit_state: CircuitState,
}

/// Execution statistics for the chain
#[derive(Debug, Default)]
pub struct ChainStats {
    pub total_attempts: usize,
    pub successful_provider: Option<String>,
    pub attempts: Vec<ChainAttemptResult>,
    pub total_duration_ms: u64,
    pub providers_skipped_circuit_open: usize,
}

/// Fallback provider chain with circuit breakers and cascading attempts
///
/// Uses DashMap for lock-free concurrent access to circuit breakers.
pub struct ProviderChain {
    providers: Vec<ChainedProvider>,
    config: ChainConfig,
    /// Circuit breakers for each provider (lock-free concurrent map)
    circuit_breakers: Arc<DashMap<String, CircuitBreaker>>,
}

impl ProviderChain {
    /// Create a new provider chain
    pub fn new(config: ChainConfig) -> Self {
        Self {
            providers: Vec::new(),
            config,
            circuit_breakers: Arc::new(DashMap::new()),
        }
    }

    /// Add a provider to the chain
    pub fn add_provider(mut self, provider: ChainedProvider) -> Self {
        self.providers.push(provider);
        self
    }

    /// Build chain from provider configs
    pub fn from_configs(configs: &[ProviderConfig], chain_config: ChainConfig) -> Result<Self> {
        let mut chain = Self::new(chain_config);

        for (idx, config) in configs.iter().enumerate() {
            let provider = super::create_provider(config)?;
            let chained = ChainedProvider::from_shared(provider)
                .with_priority(idx as u8)
                .with_cost(estimate_cost(&config.provider));

            chain.providers.push(chained);
        }

        Ok(chain)
    }

    /// Sort providers by cost (cheapest first)
    pub fn optimize_for_cost(&mut self) {
        self.providers.sort_by(|a, b| {
            a.cost_per_1k
                .partial_cmp(&b.cost_per_1k)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Sort providers by priority
    pub fn optimize_for_priority(&mut self) {
        self.providers.sort_by_key(|p| p.priority);
    }

    /// Execute with fallback chain and circuit breakers
    #[instrument(skip(self, prompt, schema), fields(providers = self.providers.len()))]
    pub async fn execute(&self, prompt: &str, schema: &Value) -> Result<(LlmResponse, ChainStats)> {
        let mut stats = ChainStats::default();
        let start_time = std::time::Instant::now();

        if self.providers.is_empty() {
            return Err(WeaveError::Config(
                "No providers configured in chain".to_string(),
            ));
        }

        // Pre-initialize circuit breakers for all providers (lock-free)
        for provider_entry in &self.providers {
            let name = provider_entry.provider.name();
            self.circuit_breakers
                .entry(name.to_string())
                .or_insert_with(|| CircuitBreaker::new(name, self.config.circuit_breaker.clone()));
        }

        let mut last_error: Option<WeaveError> = None;
        let mut current_delay = self.config.base_delay;

        for provider_entry in &self.providers {
            let provider = &provider_entry.provider;
            let provider_name = provider.name().to_string();

            // Check circuit breaker state (lock-free read)
            let circuit_state = self
                .circuit_breakers
                .get(&provider_name)
                .map(|cb| cb.state())
                .unwrap_or(CircuitState::Closed);

            if circuit_state == CircuitState::Open {
                debug!(provider = %provider_name, "Skipping provider (circuit OPEN)");
                stats.providers_skipped_circuit_open += 1;
                continue;
            }

            for attempt in 1..=provider_entry.max_retries {
                if stats.total_attempts >= self.config.max_total_attempts {
                    break;
                }

                // Check circuit breaker before each attempt
                let allow = self
                    .circuit_breakers
                    .get(&provider_name)
                    .map(|cb| cb.allow_request())
                    .unwrap_or(true);

                if !allow {
                    debug!(provider = %provider_name, "Circuit breaker blocked request");
                    break;
                }

                stats.total_attempts += 1;
                let attempt_start = std::time::Instant::now();

                debug!(
                    total_attempt = stats.total_attempts,
                    max_attempts = self.config.max_total_attempts,
                    provider = %provider_name,
                    attempt = attempt,
                    max_retries = provider_entry.max_retries,
                    ?circuit_state,
                    "Chain attempt"
                );

                match provider.generate(prompt, schema).await {
                    Ok(response) => {
                        let duration_ms = attempt_start.elapsed().as_millis() as u64;

                        // Record success in circuit breaker (lock-free)
                        if let Some(cb) = self.circuit_breakers.get(&provider_name) {
                            cb.record_success();
                        }

                        let current_state = self
                            .circuit_breakers
                            .get(&provider_name)
                            .map(|cb| cb.state())
                            .unwrap_or(CircuitState::Closed);

                        stats.attempts.push(ChainAttemptResult {
                            provider_name: provider_name.clone(),
                            attempt_number: attempt as usize,
                            success: true,
                            error: None,
                            duration_ms,
                            circuit_state: current_state,
                        });
                        stats.successful_provider = Some(provider_name);
                        stats.total_duration_ms = start_time.elapsed().as_millis() as u64;

                        info!(
                            provider = %stats.successful_provider.as_deref().unwrap_or("unknown"),
                            attempts = stats.total_attempts,
                            "Chain succeeded"
                        );

                        return Ok((response, stats));
                    }
                    Err(err) => {
                        let classified =
                            ErrorClassifier::classify(&err.to_string(), &provider_name);
                        let duration_ms = attempt_start.elapsed().as_millis() as u64;

                        // Record failure in circuit breaker (lock-free)
                        if let Some(cb) = self.circuit_breakers.get(&provider_name) {
                            cb.record_failure();
                        }

                        let current_state = self
                            .circuit_breakers
                            .get(&provider_name)
                            .map(|cb| cb.state())
                            .unwrap_or(CircuitState::Closed);

                        stats.attempts.push(ChainAttemptResult {
                            provider_name: provider_name.clone(),
                            attempt_number: attempt as usize,
                            success: false,
                            error: Some(classified.clone()),
                            duration_ms,
                            circuit_state: current_state,
                        });

                        warn!(
                            provider = %provider_name,
                            attempt = attempt,
                            ?current_state,
                            error = %err,
                            category = %classified.category,
                            "Provider failed"
                        );

                        last_error = Some(err);

                        // If circuit opened, skip to next provider
                        if current_state == CircuitState::Open {
                            info!(provider = %provider_name, "Circuit opened, moving to next provider");
                            break;
                        }

                        // Decide whether to retry same provider or move to next
                        match classified.category {
                            ErrorCategory::Auth => {
                                info!(provider = %provider_name, "Auth error, trying next provider");
                                break;
                            }
                            ErrorCategory::TokenLimit => {
                                info!(provider = %provider_name, "Token limit, trying next provider");
                                break;
                            }
                            ErrorCategory::BadRequest => {
                                warn!("Bad request error, stopping chain");
                                stats.total_duration_ms = start_time.elapsed().as_millis() as u64;
                                return Err(last_error.unwrap_or_else(|| {
                                    WeaveError::LlmApi("Bad request with unknown error".to_string())
                                }));
                            }
                            ErrorCategory::RateLimit => {
                                // Use retry_after from error if available, otherwise use default
                                let wait = classified.retry_after.unwrap_or_else(|| {
                                    parse_rate_limit_delay(&classified.message)
                                        .unwrap_or(Duration::from_secs(30))
                                });
                                info!(
                                    wait_secs = wait.as_secs(),
                                    "Rate limited, waiting before retry"
                                );
                                sleep(wait).await;
                            }
                            ErrorCategory::Network | ErrorCategory::Transient => {
                                if attempt < provider_entry.max_retries {
                                    let jitter = random_jitter(current_delay);
                                    let delay = current_delay + jitter;
                                    debug!(delay_ms = delay.as_millis(), "Retrying after backoff");
                                    sleep(delay).await;
                                    current_delay = calculate_backoff(
                                        current_delay,
                                        self.config.backoff_factor,
                                        self.config.max_delay,
                                    );
                                }
                            }
                            ErrorCategory::ParseError => {
                                // Parse errors may succeed on retry with same prompt
                                if attempt < provider_entry.max_retries {
                                    let wait = classified.recommended_delay();
                                    debug!(wait_ms = wait.as_millis(), "Parse error, retrying");
                                    sleep(wait).await;
                                }
                            }
                            ErrorCategory::Unavailable => {
                                info!(provider = %provider_name, "Provider unavailable, trying next");
                                break;
                            }
                            ErrorCategory::Unknown => {
                                if attempt < provider_entry.max_retries {
                                    sleep(current_delay).await;
                                }
                            }
                        }
                    }
                }
            }

            // Reset delay for next provider
            current_delay = self.config.base_delay;
        }

        stats.total_duration_ms = start_time.elapsed().as_millis() as u64;

        Err(last_error
            .unwrap_or_else(|| WeaveError::LlmApi("All providers in chain failed".to_string())))
    }

    /// Get circuit breaker stats for all providers
    pub fn circuit_breaker_stats(&self) -> Vec<super::circuit_breaker::CircuitBreakerStats> {
        self.circuit_breakers
            .iter()
            .map(|entry| entry.value().stats())
            .collect()
    }

    /// Reset all circuit breakers
    pub fn reset_circuit_breakers(&self) {
        for entry in self.circuit_breakers.iter() {
            entry.value().reset();
        }
    }

    /// Get current state of a specific circuit breaker
    pub fn circuit_state(&self, provider_name: &str) -> Option<CircuitState> {
        self.circuit_breakers
            .get(provider_name)
            .map(|cb| cb.state())
    }
}

impl Clone for ProviderChain {
    fn clone(&self) -> Self {
        Self {
            providers: self.providers.clone(),
            config: self.config.clone(),
            // Share circuit breakers across clones to maintain state consistency
            circuit_breakers: Arc::clone(&self.circuit_breakers),
        }
    }
}

#[async_trait]
impl LlmProvider for ProviderChain {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse> {
        let (response, _stats) = self.execute(prompt, schema).await?;
        Ok(response)
    }

    fn name(&self) -> &str {
        "provider-chain"
    }

    fn model(&self) -> &str {
        self.providers
            .first()
            .map(|p| p.provider.model())
            .unwrap_or("unknown")
    }

    async fn health_check(&self) -> Result<bool> {
        for provider in &self.providers {
            if provider.provider.health_check().await.unwrap_or(false) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Parse rate limit delay from error message
///
/// Extracts retry-after seconds from common rate limit error formats.
fn parse_rate_limit_delay(message: &str) -> Option<Duration> {
    let lower = message.to_lowercase();

    // Pattern: "retry after N seconds" or "retry-after: N"
    if let Some(idx) = lower.find("retry") {
        let after_retry = &lower[idx..];
        for word in after_retry.split_whitespace() {
            if let Ok(secs) = word.parse::<u64>() {
                return Some(Duration::from_secs(secs.min(300))); // Cap at 5 minutes
            }
        }
    }

    // Pattern: "wait N seconds" or "in N seconds"
    for pattern in &["wait ", "in "] {
        if let Some(idx) = lower.find(pattern) {
            let after_pattern = &lower[idx + pattern.len()..];
            for word in after_pattern.split_whitespace() {
                if let Ok(secs) = word.parse::<u64>() {
                    return Some(Duration::from_secs(secs.min(300)));
                }
            }
        }
    }

    None
}

/// Provider cost hint for chain ordering (not actual cost)
fn estimate_cost(_provider_type: &str) -> f32 {
    // All providers report actual cost in response.cost_usd
    // This is only used for initial chain ordering preference
    0.0
}

/// Generate random jitter using thread-local RNG for efficiency
fn random_jitter(base_delay: Duration) -> Duration {
    let max_jitter_ms = (base_delay.as_millis() as u64) / 4;
    if max_jitter_ms == 0 {
        return Duration::ZERO;
    }
    // Use thread-local RNG (rand 0.9+ API)
    let jitter_ms = rand::rng().random_range(0..max_jitter_ms);
    Duration::from_millis(jitter_ms)
}

/// Calculate exponential backoff with cap
fn calculate_backoff(current: Duration, factor: f32, max: Duration) -> Duration {
    let next = Duration::from_secs_f32(current.as_secs_f32() * factor);
    std::cmp::min(next, max)
}

/// Builder for creating provider chains
pub struct ProviderChainBuilder {
    providers: Vec<ChainedProvider>,
    config: ChainConfig,
}

impl ProviderChainBuilder {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            config: ChainConfig::default(),
        }
    }

    /// Add a provider with automatic settings
    pub fn add_provider(mut self, provider: impl LlmProvider + 'static) -> Self {
        let name = provider.name().to_string();
        let chained = ChainedProvider::new(Arc::new(provider))
            .with_cost(estimate_cost(&name))
            .with_priority(self.providers.len() as u8);
        self.providers.push(chained);
        self
    }

    /// Add a shared provider with automatic settings
    pub fn add_shared(mut self, provider: SharedProvider) -> Self {
        let name = provider.name().to_string();
        let chained = ChainedProvider::from_shared(provider)
            .with_cost(estimate_cost(&name))
            .with_priority(self.providers.len() as u8);
        self.providers.push(chained);
        self
    }

    /// Add a provider with custom configuration
    pub fn add_with_config(mut self, provider: ChainedProvider) -> Self {
        self.providers.push(provider);
        self
    }

    /// Set chain configuration
    pub fn with_config(mut self, config: ChainConfig) -> Self {
        self.config = config;
        self
    }

    /// Set circuit breaker configuration
    pub fn with_circuit_breaker(mut self, config: CircuitBreakerConfig) -> Self {
        self.config.circuit_breaker = config;
        self
    }

    /// Optimize ordering for cost
    pub fn optimize_cost(mut self) -> Self {
        self.config.cost_optimize = true;
        self
    }

    /// Build the chain
    pub fn build(mut self) -> ProviderChain {
        if self.config.cost_optimize {
            self.providers.sort_by(|a, b| {
                a.cost_per_1k
                    .partial_cmp(&b.cost_per_1k)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        ProviderChain {
            providers: self.providers,
            config: self.config,
            circuit_breakers: Arc::new(DashMap::new()),
        }
    }
}

impl Default for ProviderChainBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider {
        name: String,
        should_fail: bool,
        fail_count: std::sync::atomic::AtomicU32,
        max_failures: u32,
    }

    impl MockProvider {
        fn new(name: &str, should_fail: bool) -> Self {
            Self {
                name: name.to_string(),
                should_fail,
                fail_count: std::sync::atomic::AtomicU32::new(0),
                max_failures: 2,
            }
        }

        fn failing_then_success(name: &str, failures: u32) -> Self {
            Self {
                name: name.to_string(),
                should_fail: true,
                fail_count: std::sync::atomic::AtomicU32::new(0),
                max_failures: failures,
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _schema: &Value) -> Result<LlmResponse> {
            if self.should_fail {
                let count = self
                    .fail_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if count < self.max_failures {
                    return Err(WeaveError::LlmApi(format!("{} transient error", self.name)));
                }
            }
            Ok(LlmResponse::content_only(
                serde_json::json!({"result": "success", "provider": self.name}),
            ))
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn model(&self) -> &str {
            "mock-model"
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(!self.should_fail)
        }
    }

    #[tokio::test]
    async fn test_chain_success_first_provider() {
        let chain = ProviderChainBuilder::new()
            .add_provider(MockProvider::new("primary", false))
            .add_provider(MockProvider::new("fallback", false))
            .build();

        let result = chain.generate("test", &serde_json::json!({})).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().content["provider"], "primary");
    }

    #[tokio::test]
    async fn test_chain_fallback_on_failure() {
        let always_fail = MockProvider {
            name: "primary".to_string(),
            should_fail: true,
            fail_count: std::sync::atomic::AtomicU32::new(0),
            max_failures: 100,
        };

        let chain = ProviderChainBuilder::new()
            .add_provider(always_fail)
            .add_provider(MockProvider::new("fallback", false))
            .with_config(ChainConfig {
                max_total_attempts: 10,
                circuit_breaker: CircuitBreakerConfig {
                    failure_threshold: 5,
                    ..Default::default()
                },
                ..Default::default()
            })
            .build();

        let (response, stats) = chain.execute("test", &serde_json::json!({})).await.unwrap();

        assert_eq!(response.content["provider"], "fallback");
        assert!(stats.total_attempts > 1);
    }

    #[tokio::test]
    async fn test_chain_retry_then_success() {
        let chain = ProviderChainBuilder::new()
            .add_provider(MockProvider::failing_then_success("flaky", 2))
            .build();

        let (response, stats) = chain.execute("test", &serde_json::json!({})).await.unwrap();

        assert_eq!(response.content["provider"], "flaky");
        assert_eq!(stats.total_attempts, 3);
    }

    #[test]
    fn test_chain_builder() {
        let chain = ProviderChainBuilder::new()
            .add_provider(MockProvider::new("a", false))
            .add_provider(MockProvider::new("b", false))
            .optimize_cost()
            .build();

        assert_eq!(chain.providers.len(), 2);
    }

    #[test]
    fn test_estimate_cost() {
        // All providers return 0.0 - actual cost comes from response.cost_usd
        assert_eq!(estimate_cost("claude-code"), 0.0);
        assert_eq!(estimate_cost("openai"), 0.0);
        assert_eq!(estimate_cost("unknown"), 0.0);
    }

    #[test]
    fn test_random_jitter() {
        let base = Duration::from_millis(1000);
        let jitter = random_jitter(base);
        assert!(jitter <= Duration::from_millis(250));
    }

    #[test]
    fn test_calculate_backoff() {
        let current = Duration::from_millis(500);
        let next = calculate_backoff(current, 1.5, Duration::from_secs(30));
        assert_eq!(next, Duration::from_millis(750));

        // Test cap
        let large = Duration::from_secs(25);
        let capped = calculate_backoff(large, 1.5, Duration::from_secs(30));
        assert_eq!(capped, Duration::from_secs(30));
    }

    #[test]
    fn test_parse_rate_limit_delay() {
        // "retry after N seconds" pattern
        let msg1 = "Rate limit exceeded. Please retry after 30 seconds.";
        assert_eq!(parse_rate_limit_delay(msg1), Some(Duration::from_secs(30)));

        // "wait N seconds" pattern
        let msg2 = "Too many requests. Please wait 60 seconds before trying again.";
        assert_eq!(parse_rate_limit_delay(msg2), Some(Duration::from_secs(60)));

        // Cap at 5 minutes
        let msg3 = "Retry after 1000 seconds";
        assert_eq!(parse_rate_limit_delay(msg3), Some(Duration::from_secs(300)));

        // No delay found
        let msg4 = "Rate limit exceeded";
        assert_eq!(parse_rate_limit_delay(msg4), None);
    }

    #[test]
    fn test_circuit_breaker_stats() {
        let chain = ProviderChainBuilder::new()
            .add_provider(MockProvider::new("test", false))
            .build();

        // Pre-initialize the circuit breaker via entry API
        chain
            .circuit_breakers
            .entry("test".to_string())
            .or_insert_with(|| CircuitBreaker::new("test", CircuitBreakerConfig::default()));

        let stats = chain.circuit_breaker_stats();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].provider_name, "test");
    }
}
