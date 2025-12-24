//! Pipeline Metrics Collection
//!
//! Centralized metrics aggregation for tracking LLM API usage, costs, and performance
//! across pipeline execution. Thread-safe for concurrent agent execution.
//!
//! ## Usage
//!
//! ```ignore
//! let metrics = MetricsCollector::new("session-123");
//! metrics.record_response(&response);
//! let summary = metrics.summary();
//! ```

use crate::ai::provider::{LlmResponse, TokenUsage};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

// =============================================================================
// Metrics Collector
// =============================================================================

/// Thread-safe metrics collector for pipeline execution.
///
/// Uses atomic operations for counters and RwLock for complex state.
/// Designed for minimal contention in concurrent agent execution.
pub struct MetricsCollector {
    /// Session identifier
    session_id: String,
    /// Pipeline start time
    start_time: Instant,
    /// Total LLM API calls
    api_calls: AtomicU32,
    /// Total input tokens
    input_tokens: AtomicU64,
    /// Total output tokens
    output_tokens: AtomicU64,
    /// Total latency in milliseconds
    total_latency_ms: AtomicU64,
    /// Total estimated cost (stored as microdollars for atomic ops)
    total_cost_micros: AtomicU64,
    /// Per-phase metrics
    phase_metrics: RwLock<Vec<PhaseMetrics>>,
    /// Current phase name
    current_phase: RwLock<String>,
}

/// Metrics for a specific pipeline phase
#[derive(Debug, Clone)]
pub struct PhaseMetrics {
    pub name: String,
    pub api_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub duration_ms: u64,
    pub cost_usd: f64,
}

/// Summary statistics for pipeline execution
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    pub session_id: String,
    pub total_duration_ms: u64,
    pub api_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub avg_latency_ms: f64,
    pub total_cost_usd: f64,
    pub phases: Vec<PhaseMetrics>,
}

impl MetricsCollector {
    /// Create new metrics collector for session
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            start_time: Instant::now(),
            api_calls: AtomicU32::new(0),
            input_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            total_latency_ms: AtomicU64::new(0),
            total_cost_micros: AtomicU64::new(0),
            phase_metrics: RwLock::new(Vec::new()),
            current_phase: RwLock::new(String::new()),
        }
    }

    /// Record metrics from an LLM response
    pub fn record_response(&self, response: &LlmResponse) {
        self.api_calls.fetch_add(1, Ordering::Relaxed);
        self.input_tokens
            .fetch_add(response.usage.input_tokens as u64, Ordering::Relaxed);
        self.output_tokens
            .fetch_add(response.usage.output_tokens as u64, Ordering::Relaxed);
        self.total_latency_ms
            .fetch_add(response.timing.total_ms, Ordering::Relaxed);

        // Use actual cost from provider response (in microdollars for atomic ops)
        let cost_micros = (response.cost_usd * 1_000_000.0) as u64;
        self.total_cost_micros
            .fetch_add(cost_micros, Ordering::Relaxed);
    }

    /// Record token usage directly with actual cost
    pub fn record_tokens(&self, usage: &TokenUsage, cost_usd: f64, latency_ms: u64) {
        self.api_calls.fetch_add(1, Ordering::Relaxed);
        self.input_tokens
            .fetch_add(usage.input_tokens as u64, Ordering::Relaxed);
        self.output_tokens
            .fetch_add(usage.output_tokens as u64, Ordering::Relaxed);
        self.total_latency_ms
            .fetch_add(latency_ms, Ordering::Relaxed);

        // Use actual cost (in microdollars for atomic ops)
        let cost_micros = (cost_usd * 1_000_000.0) as u64;
        self.total_cost_micros
            .fetch_add(cost_micros, Ordering::Relaxed);
    }

    /// Start a new phase
    pub fn start_phase(&self, name: impl Into<String>) {
        let mut current = self.current_phase.write().unwrap_or_else(|poisoned| {
            tracing::error!("Metrics current_phase RwLock poisoned, recovering");
            poisoned.into_inner()
        });
        *current = name.into();
    }

    /// Complete current phase and record metrics
    pub fn complete_phase(&self, phase_metrics: PhaseMetrics) {
        let mut phases = self.phase_metrics.write().unwrap_or_else(|poisoned| {
            tracing::error!("Metrics phase_metrics RwLock poisoned, recovering");
            poisoned.into_inner()
        });
        phases.push(phase_metrics);
    }

    /// Get current metrics snapshot
    pub fn snapshot(&self) -> MetricsSummary {
        let api_calls = self.api_calls.load(Ordering::Relaxed);
        let input_tokens = self.input_tokens.load(Ordering::Relaxed);
        let output_tokens = self.output_tokens.load(Ordering::Relaxed);
        let total_latency = self.total_latency_ms.load(Ordering::Relaxed);
        let total_cost_micros = self.total_cost_micros.load(Ordering::Relaxed);

        let avg_latency = if api_calls > 0 {
            total_latency as f64 / api_calls as f64
        } else {
            0.0
        };

        let phases = self
            .phase_metrics
            .read()
            .unwrap_or_else(|poisoned| {
                tracing::error!("Metrics phase_metrics RwLock poisoned on read, recovering");
                poisoned.into_inner()
            })
            .clone();

        MetricsSummary {
            session_id: self.session_id.clone(),
            total_duration_ms: self.start_time.elapsed().as_millis() as u64,
            api_calls,
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
            avg_latency_ms: avg_latency,
            total_cost_usd: total_cost_micros as f64 / 1_000_000.0,
            phases,
        }
    }

    /// Get final summary
    pub fn summary(&self) -> MetricsSummary {
        self.snapshot()
    }
}

impl MetricsSummary {
    /// Format summary for display
    pub fn display(&self) -> String {
        format!(
            "Session: {}\n\
             Duration: {:.1}s\n\
             API Calls: {}\n\
             Tokens: {} (input: {}, output: {})\n\
             Avg Latency: {:.0}ms\n\
             Estimated Cost: ${:.4}",
            self.session_id,
            self.total_duration_ms as f64 / 1000.0,
            self.api_calls,
            self.total_tokens,
            self.input_tokens,
            self.output_tokens,
            self.avg_latency_ms,
            self.total_cost_usd
        )
    }
}

// =============================================================================
// Shared Type
// =============================================================================

use std::sync::Arc;

/// Shared metrics collector for pipeline stages
pub type SharedMetrics = Arc<MetricsCollector>;

/// Create shared metrics collector
pub fn create_shared_metrics(session_id: impl Into<String>) -> SharedMetrics {
    Arc::new(MetricsCollector::new(session_id))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::{ResponseMetadata, ResponseTiming};

    #[test]
    fn test_record_response() {
        let metrics = MetricsCollector::new("test-session");

        let response = LlmResponse::with_metrics(
            serde_json::json!({}),
            TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            0.0125, // Actual cost from provider
            ResponseTiming {
                total_ms: 500,
                api_ms: None,
            },
            ResponseMetadata {
                model: "claude-3-sonnet".to_string(),
                provider: "claude-code".to_string(),
            },
        );

        metrics.record_response(&response);

        let summary = metrics.summary();
        assert_eq!(summary.api_calls, 1);
        assert_eq!(summary.input_tokens, 100);
        assert_eq!(summary.output_tokens, 50);
        assert_eq!(summary.total_tokens, 150);
        assert!((summary.total_cost_usd - 0.0125).abs() < 0.0001);
    }

    #[test]
    fn test_concurrent_recording() {
        use std::thread;

        let metrics = Arc::new(MetricsCollector::new("concurrent-test"));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let m = Arc::clone(&metrics);
                thread::spawn(move || {
                    for _ in 0..100 {
                        m.record_tokens(
                            &TokenUsage {
                                input_tokens: 10,
                                output_tokens: 5,
                                cache_read_tokens: 0,
                                cache_write_tokens: 0,
                            },
                            0.001, // Cost per call
                            50,
                        );
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let summary = metrics.summary();
        assert_eq!(summary.api_calls, 1000);
        assert_eq!(summary.input_tokens, 10000);
        assert_eq!(summary.output_tokens, 5000);
        assert!((summary.total_cost_usd - 1.0).abs() < 0.001); // 1000 * 0.001 = 1.0
    }

    #[test]
    fn test_summary_display() {
        let metrics = MetricsCollector::new("display-test");

        metrics.record_tokens(
            &TokenUsage {
                input_tokens: 1000,
                output_tokens: 500,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            0.05, // Actual cost
            1000,
        );

        let summary = metrics.summary();
        let display = summary.display();

        assert!(display.contains("display-test"));
        assert!(display.contains("1500")); // total tokens
        assert!(display.contains("$")); // cost
    }
}
