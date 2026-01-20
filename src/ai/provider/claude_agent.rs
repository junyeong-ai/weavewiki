//! Claude Agent SDK Provider
//!
//! Direct API integration using the claude-agent SDK with OAuth support.
//! Uses Claude Code CLI OAuth for seamless authentication.

#[cfg(feature = "claude-agent")]
mod inner {
    use async_trait::async_trait;
    use claude_agent::client::CreateMessageRequest;
    use claude_agent::{Auth, Client, Message, OAuthConfig};
    use serde_json::Value;
    use std::time::Instant;

    use crate::ai::provider::{
        LlmProvider, LlmResponse, ProviderConfig, ResponseMetadata, ResponseTiming, TokenUsage,
    };
    use crate::constants::provider::{
        CLAUDE_AGENT_MAX_TOKENS, HEALTH_CHECK_MAX_TOKENS, claude as claude_constants,
    };
    use crate::types::Result;

    /// Claude Agent SDK Provider
    ///
    /// Uses the claude-agent crate for direct API access with OAuth support.
    /// Authentication priority:
    /// 1. Claude Code CLI OAuth (if logged in via `claude login`)
    /// 2. ANTHROPIC_API_KEY environment variable
    /// 3. Explicit API key
    pub struct ClaudeAgentProvider {
        client: Client,
        model: String,
        max_tokens: usize,
    }

    impl ClaudeAgentProvider {
        /// Create provider using Claude Code CLI OAuth
        /// Requires prior login via `claude login`
        pub async fn from_cli(model: &str) -> Result<Self> {
            let client = Client::builder()
                .auth(Auth::ClaudeCli)
                .await
                .map_err(|e| {
                    crate::types::ClaudegenError::Config(format!(
                        "Claude CLI auth failed: {e}. Run 'claude login' first."
                    ))
                })?
                .oauth_config(OAuthConfig::default())
                .build()
                .await
                .map_err(|e| {
                    crate::types::ClaudegenError::Config(format!("Client build failed: {e}"))
                })?;

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
            })
        }

        /// Create provider with automatic auth detection
        /// Tries CLI OAuth first, falls back to environment variable
        pub async fn from_env(model: &str) -> Result<Self> {
            // Try CLI OAuth first
            if let Ok(provider) = Self::from_cli(model).await {
                tracing::info!("Using Claude Code CLI OAuth authentication");
                return Ok(provider);
            }

            // Fallback to environment variable
            tracing::info!("CLI OAuth not available, trying ANTHROPIC_API_KEY");
            let client = Client::builder()
                .auth(Auth::FromEnv)
                .await
                .map_err(|e| {
                    crate::types::ClaudegenError::Config(format!(
                        "Auth failed: {e}. Run 'claude login' or set ANTHROPIC_API_KEY."
                    ))
                })?
                .build()
                .await
                .map_err(|e| {
                    crate::types::ClaudegenError::Config(format!("Client build failed: {e}"))
                })?;

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
            })
        }

        /// Create provider with explicit API key
        pub async fn with_api_key(api_key: &str, model: &str) -> Result<Self> {
            let client = Client::builder()
                .auth(Auth::ApiKey(api_key.to_string()))
                .await
                .map_err(|e| crate::types::ClaudegenError::Config(format!("Auth failed: {e}")))?
                .build()
                .await
                .map_err(|e| {
                    crate::types::ClaudegenError::Config(format!("Client build failed: {e}"))
                })?;

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
            })
        }

        /// Create provider from ProviderConfig
        pub async fn from_config(config: &ProviderConfig) -> Result<Self> {
            let model = config
                .model
                .as_deref()
                .unwrap_or(claude_constants::DEFAULT_MODEL);

            // Priority: explicit API key > CLI OAuth > environment
            let auth = if let Some(api_key) = &config.api_key {
                Auth::ApiKey(api_key.clone())
            } else {
                Auth::ClaudeCli
            };

            let mut builder = Client::builder().auth(auth).await.map_err(|e| {
                crate::types::ClaudegenError::Config(format!(
                    "Auth failed: {e}. Run 'claude login' or set ANTHROPIC_API_KEY."
                ))
            })?;

            // Add OAuth config for CLI auth
            if config.api_key.is_none() {
                builder = builder.oauth_config(OAuthConfig::default());
            }

            let client = builder.build().await.map_err(|e| {
                crate::types::ClaudegenError::Config(format!("Client build failed: {e}"))
            })?;

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: config.max_tokens,
            })
        }

        /// Set max tokens for responses
        pub fn with_max_tokens(mut self, max_tokens: usize) -> Self {
            self.max_tokens = max_tokens;
            self
        }
    }

    #[async_trait]
    impl LlmProvider for ClaudeAgentProvider {
        async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse> {
            let start = Instant::now();

            // Build system prompt with schema instructions
            let system_prompt = format!(
                "You are a documentation generation assistant. \
                 Respond with valid JSON matching this schema:\n```json\n{}\n```\n\
                 Output ONLY the JSON, no other text.",
                serde_json::to_string_pretty(schema).unwrap_or_default()
            );

            let request = CreateMessageRequest::new(&self.model, vec![Message::user(prompt)])
                .with_system(system_prompt)
                .with_max_tokens(self.max_tokens as u32);

            let response = self.client.send(request).await.map_err(|e| {
                crate::types::ClaudegenError::LlmApi(format!("Claude API error: {e}"))
            })?;

            let elapsed = start.elapsed();

            // Extract text content
            let text = response.text();

            // Parse JSON from response
            let content: Value = serde_json::from_str(&text).unwrap_or_else(|_| {
                // Try to extract JSON from markdown code blocks
                if let Some(json_start) = text.find("```json") {
                    let after_start = &text[json_start + 7..];
                    if let Some(json_end) = after_start.find("```") {
                        let json_str = after_start[..json_end].trim();
                        return serde_json::from_str(json_str)
                            .unwrap_or(Value::String(text.clone()));
                    }
                }
                Value::String(text.clone())
            });

            let usage = TokenUsage {
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                cache_read_tokens: response.usage.cache_read_input_tokens.unwrap_or(0),
                cache_write_tokens: response.usage.cache_creation_input_tokens.unwrap_or(0),
            };

            Ok(LlmResponse::with_metrics(
                content,
                usage,
                0.0, // Cost calculated externally
                ResponseTiming::from_duration(elapsed),
                ResponseMetadata {
                    model: self.model.clone(),
                    provider: "claude-agent".to_string(),
                },
            ))
        }

        fn name(&self) -> &str {
            "claude-agent"
        }

        fn model(&self) -> &str {
            &self.model
        }

        async fn health_check(&self) -> Result<bool> {
            let request = CreateMessageRequest::new(&self.model, vec![Message::user("ping")])
                .with_max_tokens(HEALTH_CHECK_MAX_TOKENS);

            match self.client.send(request).await {
                Ok(_) => Ok(true),
                Err(e) => {
                    tracing::warn!("Claude Agent health check failed: {}", e);
                    Ok(false)
                }
            }
        }
    }
}

#[cfg(feature = "claude-agent")]
pub use inner::ClaudeAgentProvider;

#[cfg(not(feature = "claude-agent"))]
pub struct ClaudeAgentProvider;

#[cfg(not(feature = "claude-agent"))]
impl ClaudeAgentProvider {
    pub async fn from_env(_model: &str) -> crate::types::Result<Self> {
        Err(crate::types::ClaudegenError::Config(
            "claude-agent feature not enabled".to_string(),
        ))
    }

    pub async fn from_config(_config: &super::ProviderConfig) -> crate::types::Result<Self> {
        Err(crate::types::ClaudegenError::Config(
            "claude-agent feature not enabled".to_string(),
        ))
    }
}
