//! Agent Helper Functions
//!
//! Shared utilities for characterization agents to eliminate code duplication.
//! Provides generic parsing, insight extraction, and confidence calculation.
//!
//! ## Agent Runner Abstraction
//!
//! The `run_agent` function eliminates ~35 lines of boilerplate per agent by
//! handling the common execution pattern:
//! 1. Log entry → 2. Build prompt → 3. Call LLM → 4. Parse/fallback → 5. Return output

use crate::types::error::WeaveError;
use crate::wiki::exhaustive::characterization::schemas::AgentPrompts;
use crate::wiki::exhaustive::characterization::{AgentOutput, CharacterizationContext};
use serde::Serialize;
use serde::de::DeserializeOwned;

// =============================================================================
// Agent Runner Abstraction
// =============================================================================

/// Agent execution configuration.
///
/// Defines all agent-specific behavior for the generic runner.
pub struct AgentConfig<'a, T> {
    /// Agent name (e.g., "structure", "purpose")
    pub name: &'a str,
    /// Turn number (1, 2, or 3)
    pub turn: u8,
    /// Schema for LLM response validation
    pub schema: serde_json::Value,
    /// Prompt builder function
    pub build_prompt: Box<dyn Fn(&CharacterizationContext) -> String + Send + Sync + 'a>,
    /// Fallback analysis when LLM fails
    pub fallback: Box<dyn Fn(&CharacterizationContext) -> T + Send + Sync + 'a>,
    /// Confidence calculator based on result
    pub confidence: Box<dyn Fn(&T) -> f32 + Send + Sync + 'a>,
    /// Debug message formatter for logging
    pub debug_result: Box<dyn Fn(&T) -> String + Send + Sync + 'a>,
}

/// Generic agent runner that handles all common execution patterns.
///
/// Eliminates ~35 lines of boilerplate per agent by handling:
/// - Logging (entry and result)
/// - Prompt construction with system prompt
/// - LLM call with error handling
/// - Response parsing with fallback
/// - JSON serialization
/// - AgentOutput construction
pub async fn run_agent<T>(
    context: &CharacterizationContext,
    config: AgentConfig<'_, T>,
) -> Result<AgentOutput, WeaveError>
where
    T: DeserializeOwned + Serialize + Send,
{
    let type_name = std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .unwrap_or("Insight");

    tracing::debug!(
        "{}Agent: Analyzing with {} files, {} prior insights",
        capitalize_first(config.name),
        context.files.len(),
        context.prior_insights.len()
    );

    // Build full prompt with system context
    let user_prompt = (config.build_prompt)(context);
    let full_prompt = format!("{}\n\n{}", AgentPrompts::system_prompt(), user_prompt);

    // Call LLM
    let response = context
        .provider
        .generate(&full_prompt, &config.schema)
        .await
        .map_err(|e| WeaveError::LlmApi(format!("{} agent LLM call failed: {}", config.name, e)))?;

    // Parse with fallback
    let insight: T = match parse_json_response(&response.content, type_name) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(
                "{}Agent: Fallback due to parse error (files={}, prior_insights={}): {}",
                capitalize_first(config.name),
                context.files.len(),
                context.prior_insights.len(),
                e
            );
            (config.fallback)(context)
        }
    };

    // Calculate confidence
    let confidence = (config.confidence)(&insight);

    // Log result
    tracing::debug!(
        "{}Agent: {}",
        capitalize_first(config.name),
        (config.debug_result)(&insight)
    );

    // Serialize and return
    let insight_json = serde_json::to_value(&insight)
        .map_err(|e| WeaveError::LlmApi(format!("Failed to serialize insight: {}", e)))?;

    Ok(AgentOutput {
        agent_name: config.name.to_string(),
        turn: config.turn,
        insight_json,
        confidence,
    })
}

// Re-export from shared utils
use crate::types::utils::capitalize_first;

// =============================================================================
// Generic Response Parsing
// =============================================================================

/// Generic JSON response parser for any insight type.
///
/// Converts LLM JSON response into strongly-typed insight struct.
pub fn parse_json_response<T: DeserializeOwned>(
    response: &serde_json::Value,
    type_name: &str,
) -> Result<T, WeaveError> {
    serde_json::from_value::<T>(response.clone())
        .map_err(|e| WeaveError::LlmApi(format!("Failed to parse {}: {}", type_name, e)))
}

// =============================================================================
// Prior Insight Extraction
// =============================================================================

/// Extract a specific agent's insight from prior outputs.
///
/// Returns the raw JSON value if found, or None if the agent hasn't run yet.
pub fn extract_prior_insight(
    prior_insights: &[AgentOutput],
    agent_name: &str,
) -> Option<serde_json::Value> {
    prior_insights
        .iter()
        .find(|o| o.agent_name == agent_name)
        .map(|o| o.insight_json.clone())
}

/// Extract insight as formatted JSON string for prompts.
///
/// Returns pretty-printed JSON or a fallback message.
pub fn extract_prior_insight_string(prior_insights: &[AgentOutput], agent_name: &str) -> String {
    extract_prior_insight(prior_insights, agent_name)
        .and_then(|json| serde_json::to_string_pretty(&json).ok())
        .unwrap_or_else(|| format!("No {} insight available", agent_name))
}

// =============================================================================
// Confidence Calculation
// =============================================================================

/// Calculate confidence score based on content availability.
///
/// Returns 0.3 for empty results (fallback), 0.8 for successful extraction.
pub fn calculate_confidence(is_empty: bool) -> f32 {
    if is_empty { 0.3 } else { 0.8 }
}

// =============================================================================
// File List Formatting
// =============================================================================

/// Format file list for prompts with metadata.
///
/// Limits output to specified count with truncation notice.
pub fn format_file_list(
    files: &[crate::wiki::exhaustive::characterization::FileInfo],
    limit: usize,
) -> String {
    let mut result = String::new();

    for file in files.iter().take(limit) {
        result.push_str(&format!(
            "{} ({}, {} lines)\n",
            file.path,
            file.language.as_deref().unwrap_or("unknown"),
            file.line_count
        ));
    }

    if files.len() > limit {
        result.push_str(&format!("... and {} more files\n", files.len() - limit));
    }

    result
}

// =============================================================================
// Token Management (re-exported from crate::types)
// =============================================================================

pub use crate::types::{estimate_tokens, truncate_to_token_limit};

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestInsight {
        name: String,
        value: i32,
    }

    #[test]
    fn test_parse_json_response_success() {
        let json = serde_json::json!({
            "name": "test",
            "value": 42
        });

        let result: Result<TestInsight, _> = parse_json_response(&json, "TestInsight");
        assert!(result.is_ok());

        let insight = result.unwrap();
        assert_eq!(insight.name, "test");
        assert_eq!(insight.value, 42);
    }

    #[test]
    fn test_parse_json_response_failure() {
        let json = serde_json::json!({ "wrong": "schema" });

        let result: Result<TestInsight, _> = parse_json_response(&json, "TestInsight");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_prior_insight() {
        let outputs = vec![
            AgentOutput {
                agent_name: "structure".to_string(),
                turn: 1,
                insight_json: serde_json::json!({"patterns": ["layered"]}),
                confidence: 0.8,
            },
            AgentOutput {
                agent_name: "dependency".to_string(),
                turn: 1,
                insight_json: serde_json::json!({"deps": []}),
                confidence: 0.7,
            },
        ];

        let structure = extract_prior_insight(&outputs, "structure");
        assert!(structure.is_some());

        let missing = extract_prior_insight(&outputs, "technical");
        assert!(missing.is_none());
    }

    #[test]
    fn test_calculate_confidence() {
        assert_eq!(calculate_confidence(true), 0.3);
        assert_eq!(calculate_confidence(false), 0.8);
    }

    #[test]
    fn test_truncate_to_token_limit() {
        let short = "Short content";
        assert_eq!(truncate_to_token_limit(short, 100), short);

        let long = "Line 1\n\nLine 2\n\nLine 3";
        let truncated = truncate_to_token_limit(long, 2); // Force truncation
        assert!(truncated.contains("truncated"));
    }
}
