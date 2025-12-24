//! Ollama Local LLM Provider
//!
//! LLM provider for locally-running Ollama models.
//! Returns LlmResponse with token usage metrics (estimated for local models).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use super::{
    LlmProvider, LlmResponse, ProviderConfig, ResponseMetadata, ResponseTiming, TokenUsage,
    prompt_utils,
};
use crate::ai::validation::extract_json_from_response;
use crate::types::{Result, WeaveError};

const DEFAULT_API_BASE: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "llama3:latest";

/// Ollama Local LLM Provider
pub struct OllamaProvider {
    api_base: String,
    model: String,
    temperature: f32,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(config: ProviderConfig) -> Result<Self> {
        let api_base = config
            .api_base
            .unwrap_or_else(|| DEFAULT_API_BASE.to_string());

        // Validate endpoint URL for security (SSRF prevention)
        let api_base = Self::validate_endpoint(&api_base)?;

        let model = config.model.unwrap_or_else(|| DEFAULT_MODEL.to_string());

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| WeaveError::LlmApi(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            api_base,
            model,
            temperature: config.temperature,
            client,
        })
    }

    /// Validate endpoint URL for security (SSRF prevention)
    ///
    /// Only allows http/https schemes and warns for non-localhost endpoints.
    fn validate_endpoint(endpoint: &str) -> Result<String> {
        // Parse URL
        let url = url::Url::parse(endpoint).map_err(|e| {
            WeaveError::Config(format!("Invalid Ollama endpoint URL '{}': {}", endpoint, e))
        })?;

        // Only allow http/https schemes
        if !matches!(url.scheme(), "http" | "https") {
            return Err(WeaveError::Config(format!(
                "Ollama endpoint must use http or https scheme, got: {}",
                url.scheme()
            )));
        }

        // Warn for non-localhost endpoints (potential SSRF)
        if let Some(host) = url.host_str()
            && !matches!(host, "localhost" | "127.0.0.1" | "::1")
        {
            warn!(
                "Ollama endpoint is not localhost: {}. Ensure this is intentional.",
                host
            );
        }

        // Remove trailing slash for consistency
        let mut result = url.to_string();
        if result.ends_with('/') {
            result.pop();
        }
        Ok(result)
    }

    fn build_request(&self, prompt: &str, schema: &Value) -> OllamaRequest {
        OllamaRequest {
            model: self.model.clone(),
            prompt: prompt_utils::build_schema_prompt(prompt, schema),
            stream: false,
            options: Some(OllamaOptions {
                temperature: self.temperature,
            }),
            format: Some("json".to_string()),
        }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse> {
        info!(
            "Generating with Ollama (model: {}, temperature: {})",
            self.model, self.temperature
        );

        let start_time = Instant::now();
        let request = self.build_request(prompt, schema);
        let url = format!("{}/api/generate", self.api_base);

        debug!("Sending request to Ollama API");

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    WeaveError::LlmApi(format!(
                        "Failed to connect to Ollama at {}. Is Ollama running? Start with: ollama serve",
                        self.api_base
                    ))
                } else {
                    WeaveError::LlmApi(format!("Ollama request failed: {}", e))
                }
            })?;

        let elapsed = start_time.elapsed();

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(WeaveError::LlmApi(format!(
                "Ollama API error ({}): {}",
                status, body
            )));
        }

        let response_body: OllamaResponse = response
            .json()
            .await
            .map_err(|e| WeaveError::LlmApi(format!("Failed to parse Ollama response: {}", e)))?;

        // Ollama returns token counts in response
        let usage = TokenUsage::from_ollama(
            response_body.prompt_eval_count.unwrap_or(0),
            response_body.eval_count.unwrap_or(0),
        );

        debug!("Received response from Ollama, parsing JSON");
        let content = extract_json_from_response(&response_body.response)?;

        // Ollama is a local model - no cost
        Ok(LlmResponse::with_metrics(
            content,
            usage,
            0.0, // Local model, no API cost
            ResponseTiming::from_duration(elapsed),
            ResponseMetadata {
                model: self.model.clone(),
                provider: "ollama".to_string(),
            },
        ))
    }

    fn name(&self) -> &str {
        "ollama"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/api/tags", self.api_base);

        let response = self.client.get(&url).send().await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(tags) = resp.json::<OllamaTagsResponse>().await {
                    let model_available = tags.models.iter().any(|m| {
                        m.name == self.model
                            || m.name.starts_with(&self.model.replace(":latest", ""))
                    });

                    if model_available {
                        info!("Ollama is available with model: {}", self.model);
                        Ok(true)
                    } else {
                        warn!(
                            "Ollama is running but model '{}' not found. Pull with: ollama pull {}",
                            self.model, self.model
                        );
                        Ok(false)
                    }
                } else {
                    info!("Ollama is available");
                    Ok(true)
                }
            }
            Ok(resp) => {
                warn!("Ollama API check failed: {}", resp.status());
                Ok(false)
            }
            Err(e) => {
                warn!("Ollama not available: {}. Start with: ollama serve", e);
                Ok(false)
            }
        }
    }
}

// Request/Response types

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    response: String,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_from_ollama() {
        let usage = TokenUsage::from_ollama(100, 50);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total(), 150);
    }

    #[test]
    fn test_default_config() {
        let config = ProviderConfig {
            provider: "ollama".to_string(),
            ..Default::default()
        };

        let provider = OllamaProvider::new(config).expect("Failed to create provider");
        assert_eq!(provider.api_base, DEFAULT_API_BASE);
        assert_eq!(provider.model, DEFAULT_MODEL);
    }
}
