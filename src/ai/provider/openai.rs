//! OpenAI API Provider
//!
//! LLM provider using OpenAI's Chat Completions API.
//! Returns LlmResponse with token usage metrics for cost tracking.

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use super::{
    LlmProvider, LlmResponse, ProviderConfig, ResponseMetadata, ResponseTiming, TokenUsage,
};
use crate::ai::validation::extract_json_from_response;
use crate::types::{Result, WeaveError};

const DEFAULT_API_BASE: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-4-turbo-preview";

/// OpenAI API Provider with secure API key handling
pub struct OpenAiProvider {
    /// API key stored securely - never exposed in logs or debug output
    api_key: SecretString,
    api_base: String,
    model: String,
    temperature: f32,
    max_tokens: usize,
    client: reqwest::Client,
}

impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiProvider")
            .field("api_key", &"[REDACTED]")
            .field("api_base", &self.api_base)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl OpenAiProvider {
    pub fn new(config: ProviderConfig) -> Result<Self> {
        let api_key_str = config
            .api_key
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .ok_or_else(|| {
                WeaveError::Config(
                    "OpenAI API key not found. Set OPENAI_API_KEY env var or provide in config"
                        .to_string(),
                )
            })?;

        let api_base = config
            .api_base
            .unwrap_or_else(|| DEFAULT_API_BASE.to_string());

        let model = config.model.unwrap_or_else(|| DEFAULT_MODEL.to_string());

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| WeaveError::LlmApi(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            api_key: SecretString::from(api_key_str),
            api_base,
            model,
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            client,
        })
    }

    fn build_request(&self, prompt: &str, schema: &Value) -> ChatCompletionRequest {
        let system_content = if schema.is_null() {
            "You are a code documentation expert. Always respond with valid JSON.".to_string()
        } else {
            // Serialize schema to pretty JSON for the system prompt
            // In the rare case serialization fails, log warning and use compact format
            let schema_str = match serde_json::to_string_pretty(schema) {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to pretty-print schema, using compact format: {}", e);
                    // Fall back to compact serialization (should not fail if pretty failed)
                    serde_json::to_string(schema).unwrap_or_else(|_| "{}".to_string())
                }
            };
            format!(
                "You are a code documentation expert. Always respond with valid JSON matching this schema:\n\n```json\n{}\n```\n\nRespond ONLY with valid JSON, no explanation.",
                schema_str
            )
        };

        ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_content,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: prompt.to_string(),
                },
            ],
            temperature: self.temperature,
            max_tokens: Some(self.max_tokens),
            response_format: Some(ResponseFormat {
                format_type: "json_object".to_string(),
            }),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse> {
        info!(
            "Generating with OpenAI (model: {}, temperature: {})",
            self.model, self.temperature
        );

        let start_time = Instant::now();
        let request = self.build_request(prompt, schema);
        let url = format!("{}/chat/completions", self.api_base);

        debug!("Sending request to OpenAI API");

        let response = self
            .client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            )
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| WeaveError::LlmApi(format!("OpenAI request failed: {}", e)))?;

        let elapsed = start_time.elapsed();

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(WeaveError::LlmApi(format!(
                "OpenAI API error ({}): {}",
                status, body
            )));
        }

        let response_body: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| WeaveError::LlmApi(format!("Failed to parse OpenAI response: {}", e)))?;

        // Extract token usage
        let usage = response_body
            .usage
            .map(|u| TokenUsage::from_openai(u.prompt_tokens, u.completion_tokens))
            .unwrap_or_default();

        let content_str = response_body
            .choices
            .first()
            .and_then(|c| c.message.content.as_ref())
            .ok_or_else(|| WeaveError::LlmApi("No content in OpenAI response".to_string()))?;

        debug!("Received response from OpenAI, parsing JSON");
        let content = extract_json_from_response(content_str)?;

        // OpenAI API doesn't return cost in response, use 0.0
        // Cost tracking should be done via OpenAI dashboard or external billing
        Ok(LlmResponse::with_metrics(
            content,
            usage,
            0.0, // OpenAI API doesn't provide cost in response
            ResponseTiming::from_duration(elapsed),
            ResponseMetadata {
                model: self.model.clone(),
                provider: "openai".to_string(),
            },
        ))
    }

    fn name(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/models", self.api_base);

        let response = self
            .client
            .get(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            )
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                info!("OpenAI API is available");
                Ok(true)
            }
            Ok(resp) => {
                warn!("OpenAI API check failed: {}", resp.status());
                Ok(false)
            }
            Err(e) => {
                warn!("OpenAI API check failed: {}", e);
                Ok(false)
            }
        }
    }
}

// Request/Response types

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_from_openai() {
        let usage = TokenUsage::from_openai(100, 50);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total(), 150);
    }
}
