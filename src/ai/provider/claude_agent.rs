//! Claude Agent SDK Provider

#[cfg(feature = "claude-agent")]
mod inner {
    use async_trait::async_trait;
    use claude_agent::client::{
        BetaFeature, CreateMessageRequest, ProviderConfig as SdkProviderConfig,
        transform_for_strict,
    };
    use claude_agent::types::StopReason as SdkStopReason;
    use claude_agent::{Auth, Client, Message, OAuthConfig};
    use serde_json::Value;
    use std::time::Instant;

    use crate::ai::model_capabilities::{AuthMode, ModelRegistry};
    use crate::ai::provider::{
        LlmProvider, LlmResponse, ProviderConfig, ResponseMetadata, ResponseTiming, StopReason,
        TokenUsage,
    };
    use crate::ai::validation::parse_structured_output;
    use crate::constants::provider::{CLAUDE_AGENT_MAX_TOKENS, HEALTH_CHECK_MAX_TOKENS};
    use crate::types::{ClaudegenError, Result};

    const DEFAULT_MODEL: &str = "claude-sonnet-4-5-20250929";
    const MAX_ATTEMPTS: u32 = 4;
    const TOKEN_INCREASE_FACTOR: f64 = 2.0;

    pub struct ClaudeAgentProvider {
        client: Client,
        model: String,
        max_tokens: usize,
        extended_context: bool,
        auth_mode: AuthMode,
    }

    impl ClaudeAgentProvider {
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

        pub fn extended_available(&self) -> bool {
            let registry = ModelRegistry::global();
            registry
                .get(&self.model)
                .map(|caps| caps.extended_available(self.auth_mode))
                .unwrap_or(false)
        }

        pub fn supports_extended_context(model: &str) -> bool {
            let registry = ModelRegistry::global();
            registry
                .get(model)
                .map(|caps| caps.extended_context_window.is_some())
                .unwrap_or(false)
        }

        async fn build_client(
            auth: Auth,
            api_key: Option<&str>,
            extended_context: bool,
        ) -> Result<Client> {
            let mut builder = Client::builder().auth(auth).await.map_err(|e| {
                ClaudegenError::Config(format!(
                    "Auth failed: {e}. Run 'claude login' or set ANTHROPIC_API_KEY."
                ))
            })?;

            if api_key.is_none() {
                builder = builder.oauth_config(OAuthConfig::default());
            }

            let mut sdk_config =
                SdkProviderConfig::default().with_beta(BetaFeature::StructuredOutputs);

            if extended_context {
                sdk_config = sdk_config.with_beta(BetaFeature::Context1M);
            }

            builder = builder.config(sdk_config);

            builder
                .build()
                .await
                .map_err(|e| ClaudegenError::Config(format!("Client build failed: {e}")))
        }

        pub async fn with_extended_context(model: &str) -> Result<Self> {
            if !Self::supports_extended_context(model) {
                let registry = ModelRegistry::global();
                let supported: Vec<_> = registry
                    .model_ids()
                    .into_iter()
                    .filter(|id| Self::supports_extended_context(id))
                    .collect();
                return Err(ClaudegenError::Config(format!(
                    "Model {model} does not support extended context. Supported: {:?}",
                    supported
                )));
            }

            let client = Self::build_client(Auth::FromEnv, None, true).await?;
            let registry = ModelRegistry::global();
            let caps = registry.get_or_default(model);
            let context_window = caps.effective_context_window(AuthMode::ApiKey, true);

            tracing::info!(
                model,
                context_window,
                "Created provider with extended context"
            );

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
                extended_context: true,
                auth_mode: AuthMode::ApiKey,
            })
        }

        pub async fn from_cli(model: &str) -> Result<Self> {
            let client = Self::build_client(Auth::ClaudeCli, None, false).await?;

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
                extended_context: false,
                auth_mode: AuthMode::OAuth,
            })
        }

        pub async fn from_env(model: &str) -> Result<Self> {
            if let Ok(provider) = Self::from_cli(model).await {
                tracing::info!(
                    context_window = provider.context_window(),
                    "Using Claude Code CLI OAuth"
                );
                return Ok(provider);
            }

            tracing::info!("CLI OAuth not available, trying ANTHROPIC_API_KEY");
            let client = Self::build_client(Auth::FromEnv, None, false).await?;

            Ok(Self {
                client,
                model: model.to_string(),
                max_tokens: CLAUDE_AGENT_MAX_TOKENS,
                extended_context: false,
                auth_mode: AuthMode::ApiKey,
            })
        }

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

        pub async fn with_api_key_extended(api_key: &str, model: &str) -> Result<Self> {
            if !Self::supports_extended_context(model) {
                return Err(ClaudegenError::Config(format!(
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

        pub async fn from_config(config: &ProviderConfig) -> Result<Self> {
            let model = config.model.as_deref().unwrap_or(DEFAULT_MODEL);

            let (auth, auth_mode) = match &config.api_key {
                Some(key) => (Auth::ApiKey(key.clone()), AuthMode::ApiKey),
                None => (Auth::ClaudeCli, AuthMode::OAuth),
            };

            let use_extended = config.extended_context && auth_mode == AuthMode::ApiKey;
            if config.extended_context && auth_mode == AuthMode::OAuth {
                tracing::warn!("Extended context requires API key, using standard context");
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

        fn convert_stop_reason(sdk_reason: Option<SdkStopReason>) -> StopReason {
            match sdk_reason {
                Some(SdkStopReason::EndTurn) => StopReason::EndTurn,
                Some(SdkStopReason::MaxTokens) => StopReason::MaxTokens,
                Some(SdkStopReason::StopSequence) => StopReason::StopSequence,
                Some(SdkStopReason::Refusal) => StopReason::Refusal,
                Some(SdkStopReason::ToolUse) | None => StopReason::EndTurn,
            }
        }
    }

    #[async_trait]
    impl LlmProvider for ClaudeAgentProvider {
        async fn generate(&self, prompt: &str, schema: &Value) -> Result<LlmResponse> {
            let start = Instant::now();
            let mut current_max_tokens = self.max_tokens as u32;

            for attempt in 1..=MAX_ATTEMPTS {
                let mut request =
                    CreateMessageRequest::new(&self.model, vec![Message::user(prompt)])
                        .with_system(
                            "You are a code documentation expert. Respond with valid JSON only.",
                        )
                        .with_max_tokens(current_max_tokens);

                if !schema.is_null() && schema.is_object() {
                    request = request.with_json_schema(transform_for_strict(schema.clone()));
                }

                let response = self
                    .client
                    .send(request)
                    .await
                    .map_err(|e| ClaudegenError::LlmApi(format!("Claude API error: {e}")))?;

                let sdk_stop_reason = response.stop_reason;
                let stop_reason = Self::convert_stop_reason(sdk_stop_reason);

                if stop_reason == StopReason::Refusal {
                    return Err(ClaudegenError::LlmApi(
                        "Model refused structured output request".to_string(),
                    ));
                }

                let raw_text = response.text();

                match parse_structured_output(&raw_text) {
                    Ok(content) => {
                        let usage = TokenUsage {
                            input_tokens: response.usage.input_tokens,
                            output_tokens: response.usage.output_tokens,
                            cache_read_tokens: response.usage.cache_read_input_tokens.unwrap_or(0),
                            cache_write_tokens: response
                                .usage
                                .cache_creation_input_tokens
                                .unwrap_or(0),
                        };

                        return Ok(LlmResponse::new(
                            content,
                            usage,
                            ResponseTiming::from_duration(start.elapsed()),
                            ResponseMetadata {
                                model: self.model.clone(),
                                provider: "claude-agent".to_string(),
                            },
                            stop_reason,
                        ));
                    }
                    Err(e) => {
                        let is_truncation = stop_reason == StopReason::MaxTokens
                            || e.to_string().contains("EOF while parsing");
                        let can_retry = is_truncation && attempt < MAX_ATTEMPTS;
                        if can_retry {
                            current_max_tokens =
                                (current_max_tokens as f64 * TOKEN_INCREASE_FACTOR) as u32;
                            tracing::warn!(
                                attempt,
                                new_max_tokens = current_max_tokens,
                                error = %e,
                                "Response truncated, retrying with increased tokens"
                            );
                            continue;
                        }
                        return Err(e);
                    }
                }
            }
            unreachable!("Loop always returns")
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
                    tracing::warn!("Health check failed: {}", e);
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

    pub fn context_window(&self) -> u64 {
        200_000
    }
}
