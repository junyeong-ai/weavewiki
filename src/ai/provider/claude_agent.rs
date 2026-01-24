//! Claude Agent SDK Provider
//!
//! Direct API integration using the claude-agent SDK with OAuth support.
//! Supports 1M extended context window via BetaFeature::Context1M (API key only).
//!
//! Context Window Behavior:
//! - OAuth (Claude Code CLI): Always uses standard context (200K for Sonnet/Opus)
//! - API Key: Can enable extended context (1M) for supported models

#[cfg(feature = "claude-agent")]
mod inner {
    use async_trait::async_trait;
    use claude_agent::client::{
        BetaFeature, CreateMessageRequest, ProviderConfig as SdkProviderConfig,
        transform_for_strict,
    };
    use claude_agent::{Auth, Client, Message, OAuthConfig};
    use serde_json::Value;
    use std::time::Instant;

    use crate::ai::model_capabilities::{AuthMode, ModelRegistry};
    use crate::ai::provider::{
        LlmProvider, LlmResponse, ProviderConfig, ResponseMetadata, ResponseTiming, TokenUsage,
    };
    use crate::constants::provider::{CLAUDE_AGENT_MAX_TOKENS, HEALTH_CHECK_MAX_TOKENS};

    const DEFAULT_MODEL: &str = "claude-sonnet-4-5-20250929";
    use crate::types::Result;

    pub struct ClaudeAgentProvider {
        client: Client,
        model: String,
        max_tokens: usize,
        extended_context: bool,
        auth_mode: AuthMode,
    }

    impl ClaudeAgentProvider {
        /// Get effective context window based on model and auth mode
        pub fn context_window(&self) -> u64 {
            let registry = ModelRegistry::global();
            let caps = registry.get_or_default(&self.model);
            caps.effective_context_window(self.auth_mode, self.extended_context)
        }

        pub fn is_extended_context(&self) -> bool {
            self.extended_context
        }

        pub fn auth_mode(&self) -> AuthMode {
            self.auth_mode
        }

        /// Check if extended context is available for this model with current auth mode
        pub fn extended_available(&self) -> bool {
            let registry = ModelRegistry::global();
            if let Some(caps) = registry.get(&self.model) {
                caps.extended_available(self.auth_mode)
            } else {
                false
            }
        }

        /// Check if model supports extended context (requires API key)
        pub fn supports_extended_context(model: &str) -> bool {
            let registry = ModelRegistry::global();
            if let Some(caps) = registry.get(model) {
                caps.extended_context_window.is_some()
            } else {
                false
            }
        }

        async fn build_client(
            auth: Auth,
            api_key: Option<&str>,
            extended_context: bool,
        ) -> Result<Client> {
            let mut builder = Client::builder().auth(auth).await.map_err(|e| {
                crate::types::ClaudegenError::Config(format!(
                    "Auth failed: {e}. Run 'claude login' or set ANTHROPIC_API_KEY."
                ))
            })?;

            if api_key.is_none() {
                builder = builder.oauth_config(OAuthConfig::default());
            }

            // Always enable StructuredOutputs for JSON schema support
            let mut sdk_config =
                SdkProviderConfig::default().with_beta(BetaFeature::StructuredOutputs);

            if extended_context {
                sdk_config = sdk_config.with_beta(BetaFeature::Context1M);
            }

            builder = builder.config(sdk_config);

            builder.build().await.map_err(|e| {
                crate::types::ClaudegenError::Config(format!("Client build failed: {e}"))
            })
        }

        /// Create provider with extended context (1M)
        /// NOTE: Extended context requires API key authentication.
        /// OAuth (Claude Code CLI) does not support extended context.
        pub async fn with_extended_context(model: &str) -> Result<Self> {
            if !Self::supports_extended_context(model) {
                let registry = ModelRegistry::global();
                let supported: Vec<_> = registry
                    .model_ids()
                    .into_iter()
                    .filter(|id| Self::supports_extended_context(id))
                    .collect();
                return Err(crate::types::ClaudegenError::Config(format!(
                    "Model {model} does not support extended context. Supported models: {:?}",
                    supported
                )));
            }

            // Extended context requires API key, not OAuth
            let client = Self::build_client(Auth::FromEnv, None, true).await?;
            let registry = ModelRegistry::global();
            let caps = registry.get_or_default(model);
            let context_window = caps.effective_context_window(AuthMode::ApiKey, true);

            tracing::info!(
                model,
                context_window,
                "Created provider with extended context (API key required)"
            );

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
                extended_context: true,
                auth_mode: AuthMode::ApiKey,
            })
        }

        /// Create provider using Claude Code CLI OAuth
        /// NOTE: OAuth does not support extended context (limited to 200K)
        pub async fn from_cli(model: &str) -> Result<Self> {
            let client = Self::build_client(Auth::ClaudeCli, None, false).await?;
            let registry = ModelRegistry::global();
            let caps = registry.get_or_default(model);
            let context_window = caps.effective_context_window(AuthMode::OAuth, false);

            tracing::debug!(
                model,
                context_window,
                "Created OAuth provider (extended context not available)"
            );

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
                extended_context: false,
                auth_mode: AuthMode::OAuth,
            })
        }

        /// Create provider from environment (prefers OAuth, falls back to API key)
        pub async fn from_env(model: &str) -> Result<Self> {
            if let Ok(provider) = Self::from_cli(model).await {
                tracing::info!(
                    context_window = provider.context_window(),
                    "Using Claude Code CLI OAuth (200K context)"
                );
                return Ok(provider);
            }

            tracing::info!("CLI OAuth not available, trying ANTHROPIC_API_KEY");
            let client = Self::build_client(Auth::FromEnv, None, false).await?;
            let registry = ModelRegistry::global();
            let caps = registry.get_or_default(model);
            let context_window = caps.effective_context_window(AuthMode::ApiKey, false);

            tracing::info!(context_window, "Using API key authentication");

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
                extended_context: false,
                auth_mode: AuthMode::ApiKey,
            })
        }

        /// Create provider with explicit API key
        pub async fn with_api_key(api_key: &str, model: &str) -> Result<Self> {
            let client =
                Self::build_client(Auth::ApiKey(api_key.to_string()), Some(api_key), false).await?;

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
                extended_context: false,
                auth_mode: AuthMode::ApiKey,
            })
        }

        /// Create provider with explicit API key and extended context
        pub async fn with_api_key_extended(api_key: &str, model: &str) -> Result<Self> {
            if !Self::supports_extended_context(model) {
                return Err(crate::types::ClaudegenError::Config(format!(
                    "Model {model} does not support extended context"
                )));
            }

            let client =
                Self::build_client(Auth::ApiKey(api_key.to_string()), Some(api_key), true).await?;

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
                extended_context: true,
                auth_mode: AuthMode::ApiKey,
            })
        }

        /// Create provider from configuration
        pub async fn from_config(config: &ProviderConfig) -> Result<Self> {
            let model = config.model.as_deref().unwrap_or(DEFAULT_MODEL);

            let (auth, auth_mode) = if let Some(api_key) = &config.api_key {
                (Auth::ApiKey(api_key.clone()), AuthMode::ApiKey)
            } else {
                (Auth::ClaudeCli, AuthMode::OAuth)
            };

            // Extended context only available with API key
            let use_extended = config.extended_context && auth_mode == AuthMode::ApiKey;
            if config.extended_context && auth_mode == AuthMode::OAuth {
                tracing::warn!(
                    "Extended context requested but OAuth doesn't support it. Using standard context."
                );
            }

            let client = Self::build_client(auth, config.api_key.as_deref(), use_extended).await?;

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: config.max_tokens,
                extended_context: use_extended,
                auth_mode,
            })
        }

        pub fn with_max_tokens(mut self, max_tokens: usize) -> Self {
            self.max_tokens = max_tokens;
            self
        }
    }

    #[async_trait]
    impl LlmProvider for ClaudeAgentProvider {
        async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse> {
            use crate::ai::validation::extract_json_from_response;

            let start = Instant::now();

            // Build request with native JSON schema structured output
            let mut request = CreateMessageRequest::new(&self.model, vec![Message::user(prompt)])
                .with_system("You are a code documentation expert.")
                .with_max_tokens(self.max_tokens as u32);

            // Use native JSON schema when schema is provided
            // Transform schema for strict mode (adds additionalProperties: false)
            if !schema.is_null() && schema.is_object() {
                let strict_schema = transform_for_strict(schema.clone());
                request = request.with_json_schema(strict_schema);
            }

            let response = self.client.send(request).await.map_err(|e| {
                crate::types::ClaudegenError::LlmApi(format!("Claude API error: {e}"))
            })?;

            let elapsed = start.elapsed();

            // Parse response with robust JSON extraction
            let content = extract_json_from_response(&response.text())?;

            let usage = TokenUsage {
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                cache_read_tokens: response.usage.cache_read_input_tokens.unwrap_or(0),
                cache_write_tokens: response.usage.cache_creation_input_tokens.unwrap_or(0),
            };

            Ok(LlmResponse::with_metrics(
                content,
                usage,
                0.0,
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

// Re-export ClaudeAgentProvider
// Note: Context window constants removed - use ModelRegistry instead
#[cfg(feature = "claude-agent")]
pub use inner::ClaudeAgentProvider;

// Stub for when claude-agent feature is disabled
#[cfg(not(feature = "claude-agent"))]
pub struct ClaudeAgentProvider;

#[cfg(not(feature = "claude-agent"))]
impl ClaudeAgentProvider {
    pub async fn from_env(_model: &str) -> crate::types::Result<Self> {
        Err(crate::types::ClaudegenError::Config(
            "claude-agent feature not enabled. Enable it in Cargo.toml or use API key.".to_string(),
        ))
    }

    pub async fn from_config(_config: &super::ProviderConfig) -> crate::types::Result<Self> {
        Err(crate::types::ClaudegenError::Config(
            "claude-agent feature not enabled. Enable it in Cargo.toml or use API key.".to_string(),
        ))
    }

    pub fn context_window(&self) -> u64 {
        200_000 // Default fallback
    }
}
