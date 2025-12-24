//! Prompt building utilities for LLM providers.
//!
//! Shared utilities for constructing prompts with JSON schema instructions.

use serde_json::Value;

/// Build a prompt with JSON schema instructions.
///
/// Used by providers that need explicit schema guidance (Ollama, OpenAI).
/// Returns the original prompt if schema is null.
pub fn build_schema_prompt(user_prompt: &str, schema: &Value) -> String {
    if schema.is_null() {
        return user_prompt.to_string();
    }

    let schema_str = serde_json::to_string_pretty(schema).unwrap_or_default();
    format!(
        "{}\n\n---\n\nRespond with valid JSON matching this schema:\n```json\n{}\n```\n\nRespond ONLY with valid JSON, no explanation.",
        user_prompt, schema_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_build_schema_prompt_null_schema() {
        let prompt = "Analyze this code";
        let result = build_schema_prompt(prompt, &Value::Null);
        assert_eq!(result, prompt);
    }

    #[test]
    fn test_build_schema_prompt_with_schema() {
        let prompt = "Analyze this code";
        let schema = json!({"type": "object", "properties": {"result": {"type": "string"}}});
        let result = build_schema_prompt(prompt, &schema);

        assert!(result.contains(prompt));
        assert!(result.contains("JSON"));
        assert!(result.contains("schema"));
    }
}
