//! Claude Code CLI Provider
//!
//! Primary LLM provider using local Claude Code CLI.
//! Returns LlmResponse with token usage metrics for cost tracking.
//!
//! Note: Retry logic is handled at the ProviderChain level.
//! This provider performs single-shot execution only.

use async_trait::async_trait;
use serde_json::Value;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, info};

use super::{
    LlmProvider, LlmResponse, ProviderConfig, ResponseMetadata, ResponseTiming, TokenUsage,
};
use crate::types::{Result, WeaveError};

/// Claude Code CLI Provider
///
/// Executes LLM requests via the Claude Code CLI tool.
/// Retry and fallback logic is delegated to ProviderChain.
pub struct ClaudeCodeProvider {
    model: String,
    timeout_secs: u64,
    temperature: f32,
}

impl ClaudeCodeProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            model: config
                .model
                .unwrap_or_else(|| "claude-sonnet-4-20250514".to_string()),
            timeout_secs: config.timeout_secs,
            temperature: config.temperature,
        }
    }

    /// Execute a single Claude Code CLI call
    async fn execute(&self, prompt: &str, schema: &Value) -> Result<LlmResponse> {
        let schema_str = serde_json::to_string(schema)?;
        let start_time = Instant::now();

        debug!(
            "Executing Claude Code CLI (model={}, temperature={})",
            self.model, self.temperature
        );

        let mut cmd = Command::new("claude");
        cmd.arg("-p")
            .arg(prompt)
            .arg("--output-format")
            .arg("json")
            .arg("--model")
            .arg(&self.model)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if !schema.is_null() {
            cmd.arg("--json-schema").arg(&schema_str);
        }

        cmd.env("CLAUDE_CODE_TEMPERATURE", self.temperature.to_string());

        let child = cmd.spawn().map_err(|e| {
            WeaveError::LlmApi(format!(
                "Failed to spawn Claude Code CLI: {}. Is it installed?",
                e
            ))
        })?;

        let output = timeout(
            Duration::from_secs(self.timeout_secs),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            WeaveError::LlmApi(format!(
                "Claude Code timed out after {}s",
                self.timeout_secs
            ))
        })?
        .map_err(|e| WeaveError::LlmApi(format!("Claude Code execution failed: {}", e)))?;

        let elapsed = start_time.elapsed();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Check for API error in stdout
            if let Ok(response) = serde_json::from_str::<Value>(&stdout)
                && response
                    .get("is_error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            {
                let error_msg = response
                    .get("result")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown API error");
                return Err(WeaveError::LlmApi(format!(
                    "Claude Code API error: {}",
                    error_msg
                )));
            }

            let error_msg = if stderr.trim().is_empty() {
                "Process exited with non-zero status"
            } else {
                stderr.as_ref()
            };
            return Err(WeaveError::LlmApi(format!(
                "Claude Code failed: {}",
                error_msg
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let response: Value = serde_json::from_str(&stdout).map_err(|e| {
            WeaveError::LlmApi(format!("Failed to parse Claude Code output: {}", e))
        })?;

        // Extract token usage from response
        let usage = self.extract_usage(&response);

        // Extract structured output
        let content = if let Some(structured) = response.get("structured_output") {
            debug!("Got structured_output from Claude Code");
            structured.clone()
        } else if let Some(result) = response.get("result") {
            if result.is_object() || result.is_array() {
                result.clone()
            } else if let Some(s) = result.as_str() {
                // Try to parse string as JSON; if it fails, return the raw string as Value
                // This preserves content even when LLM returns non-JSON text
                match serde_json::from_str(s) {
                    Ok(parsed) => parsed,
                    Err(e) => {
                        debug!(
                            "Result string is not valid JSON, returning as string value: {}",
                            e
                        );
                        Value::String(s.to_string())
                    }
                }
            } else {
                return Err(WeaveError::LlmApi(
                    "No structured output in response".to_string(),
                ));
            }
        } else {
            return Err(WeaveError::LlmApi(
                "No structured_output in Claude Code response".to_string(),
            ));
        };

        // Extract actual cost from CLI response
        let cost_usd = self.extract_cost(&response);

        // Extract API timing from CLI response
        let api_ms = response.get("duration_api_ms").and_then(|v| v.as_u64());

        Ok(LlmResponse::with_metrics(
            content,
            usage,
            cost_usd,
            ResponseTiming::with_api_time(elapsed, api_ms),
            ResponseMetadata {
                model: self.model.clone(),
                provider: "claude-code".to_string(),
            },
        ))
    }

    /// Extract token usage from Claude Code response
    fn extract_usage(&self, response: &Value) -> TokenUsage {
        let usage = response.get("usage");

        TokenUsage {
            input_tokens: usage
                .and_then(|u| u.get("input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: usage
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_read_tokens: usage
                .and_then(|u| u.get("cache_read_input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_write_tokens: usage
                .and_then(|u| u.get("cache_creation_input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        }
    }

    /// Extract actual cost in USD from Claude Code response
    fn extract_cost(&self, response: &Value) -> f64 {
        response
            .get("total_cost_usd")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    }
}

#[async_trait]
impl LlmProvider for ClaudeCodeProvider {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse> {
        info!(
            "Generating with Claude Code CLI (model: {}, temperature: {})",
            self.model, self.temperature
        );
        // Single execution - retry logic is in ProviderChain
        self.execute(prompt, schema).await
    }

    fn name(&self) -> &str {
        "claude-code"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn health_check(&self) -> Result<bool> {
        let output = Command::new("claude")
            .arg("--version")
            .output()
            .await
            .map_err(|e| WeaveError::LlmApi(format!("Claude Code not found: {}", e)))?;

        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout);
            info!("Claude Code CLI available: {}", version.trim());
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires claude CLI installed"]
    async fn test_health_check() {
        let provider = ClaudeCodeProvider::new(ProviderConfig::default());
        let result = provider.health_check().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_usage() {
        let provider = ClaudeCodeProvider::new(ProviderConfig::default());

        let response = serde_json::json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "cache_read_input_tokens": 100,
                "cache_creation_input_tokens": 50
            }
        });

        let usage = provider.extract_usage(&response);
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 100);
        assert_eq!(usage.cache_write_tokens, 50);
        assert_eq!(usage.total(), 1500);
    }

    #[test]
    fn test_extract_cost() {
        let provider = ClaudeCodeProvider::new(ProviderConfig::default());

        let response = serde_json::json!({
            "total_cost_usd": 0.0471472,
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500
            }
        });

        let cost = provider.extract_cost(&response);
        assert!((cost - 0.0471472).abs() < 0.0000001);
    }

    #[test]
    fn test_extract_cost_missing() {
        let provider = ClaudeCodeProvider::new(ProviderConfig::default());

        let response = serde_json::json!({
            "usage": {
                "input_tokens": 1000
            }
        });

        let cost = provider.extract_cost(&response);
        assert_eq!(cost, 0.0);
    }
}
