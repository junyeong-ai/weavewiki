//! Top-Down Agent Helper Functions
//!
//! Shared utilities for top-down agents to eliminate code duplication.
//! Provides generic execution pattern handling.
//!
//! ## Agent Runner Abstraction
//!
//! The `run_top_down_agent` function eliminates ~40 lines of boilerplate per agent by
//! handling the common execution pattern:
//! 1. Log entry -> 2. Build context -> 3. Build prompt -> 4. Call LLM -> 5. Parse/populate -> 6. Log result

use crate::types::error::WeaveError;
use crate::wiki::exhaustive::top_down::TopDownContext;
use crate::wiki::exhaustive::top_down::insights::ProjectInsight;

// =============================================================================
// Agent Runner Abstraction
// =============================================================================

/// Top-down agent execution configuration.
///
/// Defines all agent-specific behavior for the generic runner.
#[allow(clippy::type_complexity)]
pub struct TopDownAgentConfig<'a> {
    /// Agent name (e.g., "architecture", "flow", "risk", "domain")
    pub name: &'a str,
    /// Context builder function (builds agent-specific summary)
    pub build_context: Box<dyn Fn(&TopDownContext) -> String + Send + Sync + 'a>,
    /// Prompt builder function (receives context summary, returns full prompt)
    pub build_prompt: Box<dyn Fn(&TopDownContext, &str) -> String + Send + Sync + 'a>,
    /// Schema for LLM response validation
    pub schema: serde_json::Value,
    /// Result parser that populates ProjectInsight fields
    pub parse_result: Box<dyn Fn(&serde_json::Value, &mut ProjectInsight) + Send + Sync + 'a>,
    /// Debug message formatter for logging
    pub debug_result: Box<dyn Fn(&ProjectInsight) -> String + Send + Sync + 'a>,
}

/// Generic top-down agent runner that handles all common execution patterns.
///
/// Eliminates ~40 lines of boilerplate per agent by handling:
/// - Logging (entry and result)
/// - Context construction
/// - Prompt construction
/// - LLM call with error handling
/// - ProjectInsight creation and population
pub async fn run_top_down_agent(
    context: &TopDownContext,
    config: TopDownAgentConfig<'_>,
) -> Result<ProjectInsight, WeaveError> {
    tracing::debug!(
        "{}Agent: Analyzing with {} file insights",
        capitalize_first(config.name),
        context.file_insights.len()
    );

    // Build context-specific summary
    let context_summary = (config.build_context)(context);

    // Build full prompt
    let prompt = (config.build_prompt)(context, &context_summary);

    // Call LLM
    let result = context
        .provider
        .generate(&prompt, &config.schema)
        .await
        .map_err(|e| WeaveError::LlmApi(format!("{} agent LLM call failed: {}", config.name, e)))?
        .content;

    // Create and populate insight
    let mut insight = ProjectInsight::new(config.name);
    (config.parse_result)(&result, &mut insight);

    // Log result
    tracing::debug!(
        "{}Agent: {}",
        capitalize_first(config.name),
        (config.debug_result)(&insight)
    );

    Ok(insight)
}

// Re-export from shared utils
use crate::types::utils::capitalize_first;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capitalize_first() {
        assert_eq!(capitalize_first("architecture"), "Architecture");
        assert_eq!(capitalize_first("flow"), "Flow");
        assert_eq!(capitalize_first(""), "");
    }
}
