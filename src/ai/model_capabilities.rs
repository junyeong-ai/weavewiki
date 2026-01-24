//! Model Capabilities Registry
//!
//! Centralized registry for model-specific capabilities including:
//! - Context window sizes (standard and extended)
//! - Output token limits
//! - Feature support flags
//!
//! Supports dynamic context management for different providers and auth modes.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tracing::warn;

/// Authentication mode affects available features
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AuthMode {
    /// API key authentication - supports all features including beta
    ApiKey,
    /// OAuth authentication (Claude Code CLI) - limited beta features
    #[default]
    OAuth,
}

/// Model family for grouping similar models
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelFamily {
    ClaudeHaiku,
    ClaudeSonnet,
    ClaudeOpus,
    Gpt4,
    Gpt4O,
    Other,
}

/// Capabilities for a specific model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// Model identifier (e.g., "claude-sonnet-4-5-20250929")
    pub model_id: String,
    /// Human-readable name
    pub display_name: String,
    /// Model family
    pub family: ModelFamily,
    /// Standard context window size (tokens)
    pub context_window: u64,
    /// Extended context window size if supported (tokens)
    pub extended_context_window: Option<u64>,
    /// Maximum output tokens per request
    pub max_output_tokens: u32,
    /// Whether extended context requires API key (not available via OAuth)
    pub extended_requires_api_key: bool,
    /// Whether model supports prompt caching
    pub supports_caching: bool,
    /// Whether model supports tool use
    pub supports_tools: bool,
    /// Cost per million input tokens (USD)
    pub input_cost_per_mtok: Option<f32>,
    /// Cost per million output tokens (USD)
    pub output_cost_per_mtok: Option<f32>,
}

impl ModelCapabilities {
    /// Get effective context window based on auth mode and extended flag
    pub fn effective_context_window(&self, auth_mode: AuthMode, use_extended: bool) -> u64 {
        if use_extended {
            match (
                self.extended_context_window,
                self.extended_requires_api_key,
                auth_mode,
            ) {
                // Extended available and no API key requirement
                (Some(extended), false, _) => extended,
                // Extended available and API key is used
                (Some(extended), true, AuthMode::ApiKey) => extended,
                // Extended requires API key but OAuth is used - fallback to standard
                (Some(_), true, AuthMode::OAuth) => self.context_window,
                // No extended support
                (None, _, _) => self.context_window,
            }
        } else {
            self.context_window
        }
    }

    /// Check if extended context is available for given auth mode
    pub fn extended_available(&self, auth_mode: AuthMode) -> bool {
        matches!(
            (
                self.extended_context_window,
                self.extended_requires_api_key,
                auth_mode
            ),
            (Some(_), false, _) | (Some(_), true, AuthMode::ApiKey)
        )
    }
}

/// Context configuration derived from model capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Available context window (tokens)
    pub window_size: u64,
    /// Ratio of context for input vs reserved for output (0.0-1.0)
    pub input_ratio: f32,
    /// Safety margin for overhead (prompt prefix, schema, etc.)
    pub safety_margin_tokens: u64,
    /// Maximum tokens per single batch
    pub max_batch_tokens: u64,
}

impl ContextConfig {
    /// Calculate available tokens for content (excluding output reserve and safety margin)
    pub fn available_for_content(&self) -> u64 {
        let input_budget = (self.window_size as f32 * self.input_ratio) as u64;
        input_budget.saturating_sub(self.safety_margin_tokens)
    }

    /// Create config for a model with given capabilities
    pub fn for_model(caps: &ModelCapabilities, auth_mode: AuthMode, use_extended: bool) -> Self {
        let window = caps.effective_context_window(auth_mode, use_extended);

        // Reserve ~10% for output, 5% safety margin
        let input_ratio = 0.90;
        let safety_margin = (window as f32 * 0.05) as u64;

        // Batch size depends on context window
        // Small context (100K): use ~30% per batch
        // Medium context (200K): use ~25% per batch
        // Large context (1M): use ~10% per batch (avoid overloading)
        let batch_ratio = if window >= 500_000 {
            0.10
        } else if window >= 200_000 {
            0.25
        } else {
            0.30
        };

        let available = (window as f32 * input_ratio) as u64 - safety_margin;
        let max_batch_tokens = (available as f32 * batch_ratio) as u64;

        Self {
            window_size: window,
            input_ratio,
            safety_margin_tokens: safety_margin,
            max_batch_tokens,
        }
    }

    /// Create default config for 200K context
    pub fn standard_200k() -> Self {
        Self {
            window_size: 200_000,
            input_ratio: 0.90,
            safety_margin_tokens: 10_000,
            max_batch_tokens: 45_000,
        }
    }

    /// Create default config for 100K context (Haiku)
    pub fn standard_100k() -> Self {
        Self {
            window_size: 100_000,
            input_ratio: 0.90,
            safety_margin_tokens: 5_000,
            max_batch_tokens: 25_000,
        }
    }

    /// Create config for 1M extended context
    pub fn extended_1m() -> Self {
        Self {
            window_size: 1_000_000,
            input_ratio: 0.90,
            safety_margin_tokens: 50_000,
            max_batch_tokens: 85_000,
        }
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self::standard_200k()
    }
}

/// Registry of all supported models
pub struct ModelRegistry {
    models: HashMap<String, ModelCapabilities>,
    aliases: HashMap<String, String>,
}

impl ModelRegistry {
    /// Get the global model registry
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<ModelRegistry> = OnceLock::new();
        REGISTRY.get_or_init(Self::new)
    }

    /// Create a new registry with all known models
    pub fn new() -> Self {
        let mut registry = Self {
            models: HashMap::new(),
            aliases: HashMap::new(),
        };

        registry.register_claude_models();
        registry.register_openai_models();

        registry
    }

    fn register_claude_models(&mut self) {
        // Claude 4.5 Haiku
        self.register(ModelCapabilities {
            model_id: "claude-haiku-4-5-20251001".into(),
            display_name: "Claude 4.5 Haiku".into(),
            family: ModelFamily::ClaudeHaiku,
            context_window: 200_000,
            extended_context_window: None, // Haiku doesn't support 1M
            max_output_tokens: 8192,
            extended_requires_api_key: false,
            supports_caching: true,
            supports_tools: true,
            input_cost_per_mtok: Some(0.80),
            output_cost_per_mtok: Some(4.00),
        });
        self.add_alias("haiku", "claude-haiku-4-5-20251001");
        self.add_alias("claude-haiku", "claude-haiku-4-5-20251001");

        // Claude 4.5 Sonnet
        self.register(ModelCapabilities {
            model_id: "claude-sonnet-4-5-20250929".into(),
            display_name: "Claude 4.5 Sonnet".into(),
            family: ModelFamily::ClaudeSonnet,
            context_window: 200_000,
            extended_context_window: Some(1_000_000),
            max_output_tokens: 16384,
            extended_requires_api_key: true, // OAuth doesn't support 1M
            supports_caching: true,
            supports_tools: true,
            input_cost_per_mtok: Some(3.00),
            output_cost_per_mtok: Some(15.00),
        });
        self.add_alias("sonnet", "claude-sonnet-4-5-20250929");
        self.add_alias("claude-sonnet", "claude-sonnet-4-5-20250929");
        self.add_alias("claude-sonnet-4-20250514", "claude-sonnet-4-5-20250929");

        // Claude 4.5 Opus
        self.register(ModelCapabilities {
            model_id: "claude-opus-4-5-20251101".into(),
            display_name: "Claude 4.5 Opus".into(),
            family: ModelFamily::ClaudeOpus,
            context_window: 200_000,
            extended_context_window: Some(1_000_000),
            max_output_tokens: 32768,
            extended_requires_api_key: true, // OAuth doesn't support 1M
            supports_caching: true,
            supports_tools: true,
            input_cost_per_mtok: Some(15.00),
            output_cost_per_mtok: Some(75.00),
        });
        self.add_alias("opus", "claude-opus-4-5-20251101");
        self.add_alias("claude-opus", "claude-opus-4-5-20251101");
    }

    fn register_openai_models(&mut self) {
        // GPT-4 Turbo
        self.register(ModelCapabilities {
            model_id: "gpt-4-turbo-preview".into(),
            display_name: "GPT-4 Turbo".into(),
            family: ModelFamily::Gpt4,
            context_window: 128_000,
            extended_context_window: None,
            max_output_tokens: 4096,
            extended_requires_api_key: false,
            supports_caching: false,
            supports_tools: true,
            input_cost_per_mtok: Some(10.00),
            output_cost_per_mtok: Some(30.00),
        });
        self.add_alias("gpt-4-turbo", "gpt-4-turbo-preview");

        // GPT-4o
        self.register(ModelCapabilities {
            model_id: "gpt-4o".into(),
            display_name: "GPT-4o".into(),
            family: ModelFamily::Gpt4O,
            context_window: 128_000,
            extended_context_window: None,
            max_output_tokens: 16384,
            extended_requires_api_key: false,
            supports_caching: false,
            supports_tools: true,
            input_cost_per_mtok: Some(2.50),
            output_cost_per_mtok: Some(10.00),
        });

        // GPT-4o mini
        self.register(ModelCapabilities {
            model_id: "gpt-4o-mini".into(),
            display_name: "GPT-4o Mini".into(),
            family: ModelFamily::Gpt4O,
            context_window: 128_000,
            extended_context_window: None,
            max_output_tokens: 16384,
            extended_requires_api_key: false,
            supports_caching: false,
            supports_tools: true,
            input_cost_per_mtok: Some(0.15),
            output_cost_per_mtok: Some(0.60),
        });
    }

    fn register(&mut self, caps: ModelCapabilities) {
        self.models.insert(caps.model_id.clone(), caps);
    }

    fn add_alias(&mut self, alias: &str, model_id: &str) {
        self.aliases.insert(alias.to_string(), model_id.to_string());
    }

    /// Get capabilities for a model (resolves aliases)
    pub fn get(&self, model_id: &str) -> Option<&ModelCapabilities> {
        // Try direct lookup
        if let Some(caps) = self.models.get(model_id) {
            return Some(caps);
        }

        // Try alias resolution
        if let Some(resolved) = self.aliases.get(model_id) {
            return self.models.get(resolved);
        }

        None
    }

    /// Get capabilities or create default for unknown model
    ///
    /// Warning: Returns conservative defaults (200K context) for unknown models.
    /// This may not match the actual model's capabilities.
    pub fn get_or_default(&self, model_id: &str) -> ModelCapabilities {
        self.get(model_id).cloned().unwrap_or_else(|| {
            warn!(
                model_id = %model_id,
                default_context = 200_000,
                "Using default capabilities for unknown model - context window may be incorrect"
            );
            // Default for unknown models: assume 200K context (conservative)
            ModelCapabilities {
                model_id: model_id.to_string(),
                display_name: model_id.to_string(),
                family: ModelFamily::Other,
                context_window: 200_000,
                extended_context_window: None,
                max_output_tokens: 4096,
                extended_requires_api_key: false,
                supports_caching: false,
                supports_tools: true,
                input_cost_per_mtok: None,
                output_cost_per_mtok: None,
            }
        })
    }

    /// Get context config for a model
    pub fn context_config(
        &self,
        model_id: &str,
        auth_mode: AuthMode,
        use_extended: bool,
    ) -> ContextConfig {
        let caps = self.get_or_default(model_id);
        ContextConfig::for_model(&caps, auth_mode, use_extended)
    }

    /// List all registered model IDs
    pub fn model_ids(&self) -> Vec<&str> {
        self.models.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effective_context_oauth() {
        let registry = ModelRegistry::new();
        let caps = registry.get("claude-sonnet-4-5-20250929").unwrap();

        // OAuth with extended request - should fallback to 200K
        let ctx = caps.effective_context_window(AuthMode::OAuth, true);
        assert_eq!(ctx, 200_000);

        // OAuth without extended - should be 200K
        let ctx = caps.effective_context_window(AuthMode::OAuth, false);
        assert_eq!(ctx, 200_000);
    }

    #[test]
    fn test_effective_context_api_key() {
        let registry = ModelRegistry::new();
        let caps = registry.get("claude-sonnet-4-5-20250929").unwrap();

        // API key with extended - should be 1M
        let ctx = caps.effective_context_window(AuthMode::ApiKey, true);
        assert_eq!(ctx, 1_000_000);

        // API key without extended - should be 200K
        let ctx = caps.effective_context_window(AuthMode::ApiKey, false);
        assert_eq!(ctx, 200_000);
    }

    #[test]
    fn test_extended_available() {
        let registry = ModelRegistry::new();

        // Sonnet with OAuth - extended not available
        let caps = registry.get("claude-sonnet-4-5-20250929").unwrap();
        assert!(!caps.extended_available(AuthMode::OAuth));
        assert!(caps.extended_available(AuthMode::ApiKey));

        // Haiku - no extended at all
        let caps = registry.get("claude-haiku-4-5-20251001").unwrap();
        assert!(!caps.extended_available(AuthMode::OAuth));
        assert!(!caps.extended_available(AuthMode::ApiKey));
    }

    #[test]
    fn test_context_config_oauth() {
        let registry = ModelRegistry::new();
        let config = registry.context_config("claude-sonnet-4-5-20250929", AuthMode::OAuth, true);

        // Should use 200K even though extended was requested
        assert_eq!(config.window_size, 200_000);
        assert!(config.available_for_content() > 150_000);
        assert!(config.max_batch_tokens > 40_000);
    }

    #[test]
    fn test_context_config_1m() {
        let registry = ModelRegistry::new();
        let config = registry.context_config("claude-sonnet-4-5-20250929", AuthMode::ApiKey, true);

        assert_eq!(config.window_size, 1_000_000);
        assert!(config.available_for_content() > 800_000);
    }

    #[test]
    fn test_alias_resolution() {
        let registry = ModelRegistry::new();

        assert!(registry.get("sonnet").is_some());
        assert!(registry.get("haiku").is_some());
        assert!(registry.get("opus").is_some());

        let sonnet = registry.get("sonnet").unwrap();
        assert_eq!(sonnet.model_id, "claude-sonnet-4-5-20250929");
    }

    #[test]
    fn test_unknown_model_default() {
        let registry = ModelRegistry::new();
        let caps = registry.get_or_default("unknown-model-xyz");

        assert_eq!(caps.context_window, 200_000);
        assert_eq!(caps.family, ModelFamily::Other);
    }
}
