//! Tracked Provider - Budget and Metrics Integration
//!
//! Wrapper that adds budget enforcement and metrics collection to any LlmProvider.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::{LlmProvider, LlmResponse, SharedProvider};
use crate::ai::budget::SharedBudget;
use crate::ai::metrics::SharedMetrics;
use crate::types::{ClaudegenError, Result};

pub struct TrackedProvider {
    inner: SharedProvider,
    budget: Option<SharedBudget>,
    metrics: Option<SharedMetrics>,
}

impl TrackedProvider {
    pub fn wrap_with_tracking(
        inner: SharedProvider,
        budget: SharedBudget,
        metrics: SharedMetrics,
    ) -> Arc<dyn LlmProvider> {
        Arc::new(Self {
            inner,
            budget: Some(budget),
            metrics: Some(metrics),
        })
    }
}

#[async_trait]
impl LlmProvider for TrackedProvider {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse> {
        // Pre-check: estimate if we have budget for this request
        if let Some(ref budget) = self.budget {
            let estimated_input = (prompt.len() / 4) as u64;
            let estimated_output = 2000_u64; // Conservative estimate
            let estimated_total = estimated_input + estimated_output;

            if !budget.can_consume(estimated_total) {
                let stats = budget.stats();
                return Err(ClaudegenError::Budget {
                    consumed: stats.consumed,
                    budget: stats.total_budget,
                    requested: estimated_total,
                });
            }
        }

        // Execute the actual LLM call
        let response = self.inner.generate(prompt, schema).await?;

        // Post: consume actual tokens from budget
        if let Some(ref budget) = self.budget {
            let actual_tokens = response.usage.total() as u64;
            if let Err(e) = budget.consume(actual_tokens) {
                tracing::warn!(
                    tokens = actual_tokens,
                    error = %e,
                    "Budget exceeded after response (continuing)"
                );
            }
        }

        // Post: record metrics
        if let Some(ref metrics) = self.metrics {
            metrics.record_response(&response);
        }

        Ok(response)
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn model(&self) -> &str {
        self.inner.model()
    }

    async fn health_check(&self) -> Result<bool> {
        self.inner.health_check().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::budget::create_shared_budget;
    use crate::ai::metrics::create_shared_metrics;
    use crate::ai::provider::{ResponseMetadata, ResponseTiming, TokenUsage};

    struct MockProvider;

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _schema: &Value) -> Result<LlmResponse> {
            Ok(LlmResponse::with_metrics(
                serde_json::json!({"result": "ok"}),
                TokenUsage {
                    input_tokens: 100,
                    output_tokens: 50,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
                0.01,
                ResponseTiming::default(),
                ResponseMetadata::default(),
            ))
        }

        fn name(&self) -> &str {
            "mock"
        }

        fn model(&self) -> &str {
            "mock-model"
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_tracked_provider_records_metrics() {
        let budget = create_shared_budget(100_000);
        let metrics = create_shared_metrics("test-session");
        let inner: SharedProvider = Arc::new(MockProvider);

        let tracked = TrackedProvider::wrap_with_tracking(inner, budget.clone(), metrics.clone());

        tracked
            .generate("test prompt", &serde_json::json!({}))
            .await
            .unwrap();

        let summary = metrics.summary();
        assert_eq!(summary.api_calls, 1);
        assert_eq!(summary.input_tokens, 100);
        assert_eq!(summary.output_tokens, 50);
    }

    #[tokio::test]
    async fn test_tracked_provider_consumes_budget() {
        let budget = create_shared_budget(10_000);
        let metrics = create_shared_metrics("test-session");
        let inner: SharedProvider = Arc::new(MockProvider);

        let tracked = TrackedProvider::wrap_with_tracking(inner, budget.clone(), metrics);

        tracked
            .generate("test", &serde_json::json!({}))
            .await
            .unwrap();

        let stats = budget.stats();
        assert_eq!(stats.consumed, 150); // 100 input + 50 output
    }

    #[tokio::test]
    async fn test_tracked_provider_rejects_over_budget() {
        let budget = create_shared_budget(100); // Very small budget
        let metrics = create_shared_metrics("test-session");
        let inner: SharedProvider = Arc::new(MockProvider);

        let tracked = TrackedProvider::wrap_with_tracking(inner, budget, metrics);

        // Long prompt that exceeds budget estimate
        let long_prompt = "x".repeat(10_000);
        let result = tracked.generate(&long_prompt, &serde_json::json!({})).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ClaudegenError::Budget { .. }));
    }
}
