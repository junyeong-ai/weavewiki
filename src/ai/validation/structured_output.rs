//! Structured Output Parser
//!
//! Robust JSON extraction from LLM responses.
//! Handles code fences, BOM, and leading/trailing text.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::types::{ClaudegenError, Result};

pub fn parse_structured_output(raw: &str) -> Result<Value> {
    let cleaned = preprocess(raw);

    serde_json::from_str::<Value>(&cleaned).map_err(|e| {
        ClaudegenError::LlmApi(format!(
            "Invalid JSON from API: {} (preview: {})",
            e,
            truncate_preview(&cleaned, 100)
        ))
    })
}

/// Deserialize LLM response content into typed struct.
/// Returns error if parsing fails, allowing caller to decide recovery strategy.
pub fn deserialize_llm_response<T: DeserializeOwned>(content: &Value, context: &str) -> Result<T> {
    let content_str = match content {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    serde_json::from_str(&content_str).map_err(|e| {
        tracing::error!(
            context = %context,
            error = %e,
            content_preview = %truncate_preview(&content_str, 200),
            "LLM response parse failed"
        );
        ClaudegenError::LlmApi(format!("[{context}] JSON parse failed: {e}"))
    })
}

fn preprocess(raw: &str) -> String {
    let mut s = raw.trim();

    // Remove BOM
    s = s.trim_start_matches('\u{feff}');

    // Handle code fences (```json ... ``` or ``` ... ```)
    if let Some(rest) = s.strip_prefix("```").and_then(|r| r.split_once('\n')) {
        s = rest.1;
    }
    if s.ends_with("```") {
        s = s[..s.len() - 3].trim_end();
    }

    let trimmed = s.trim();

    // Try direct parsing first for clean JSON
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        // Looks like clean JSON, return as-is for parsing
        return s.to_string();
    }

    // Extract JSON from text with leading/trailing content
    // Handles: "Here is the JSON:\n{...}" or "{...}\nExplanation..." or mixed
    if let Some(extracted) = extract_json_from_text(s) {
        return extracted;
    }

    s.to_string()
}

/// Extract JSON object or array from text with leading/trailing content
fn extract_json_from_text(text: &str) -> Option<String> {
    // Find first { or [ character
    let start_obj = text.find('{');
    let start_arr = text.find('[');

    let (start, is_object) = match (start_obj, start_arr) {
        (Some(o), Some(a)) => {
            if o < a {
                (o, true)
            } else {
                (a, false)
            }
        }
        (Some(o), None) => (o, true),
        (None, Some(a)) => (a, false),
        (None, None) => return None,
    };

    let chars: Vec<char> = text[start..].chars().collect();
    let open_char = if is_object { '{' } else { '[' };
    let close_char = if is_object { '}' } else { ']' };

    // Balance braces/brackets to find the end
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut end_pos = 0;

    for (i, &c) in chars.iter().enumerate() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match c {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            _ if in_string => {}
            c if c == open_char => depth += 1,
            c if c == close_char => {
                depth -= 1;
                if depth == 0 {
                    end_pos = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if end_pos > 0 {
        Some(chars[..end_pos].iter().collect())
    } else {
        None
    }
}

fn truncate_preview(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clean_json() {
        let input = r#"{"name": "test", "value": 42}"#;
        let result = parse_structured_output(input).unwrap();
        assert_eq!(result["name"], "test");
        assert_eq!(result["value"], 42);
    }

    #[test]
    fn test_parse_with_code_fence() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        let result = parse_structured_output(input).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_with_bom() {
        let input = "\u{feff}{\"key\": \"value\"}";
        let result = parse_structured_output(input).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_parse_with_leading_text() {
        let input = r#"Here is the JSON response:
{"name": "test", "value": 42}"#;
        let result = parse_structured_output(input).unwrap();
        assert_eq!(result["name"], "test");
        assert_eq!(result["value"], 42);
    }

    #[test]
    fn test_parse_with_trailing_text() {
        let input = r#"{"name": "test", "value": 42}

This JSON contains the test data."#;
        let result = parse_structured_output(input).unwrap();
        assert_eq!(result["name"], "test");
        assert_eq!(result["value"], 42);
    }

    #[test]
    fn test_parse_array_with_surrounding_text() {
        let input = r#"Here are the results:
[{"id": 1}, {"id": 2}]
End of response."#;
        let result = parse_structured_output(input).unwrap();
        assert!(result.is_array());
        assert_eq!(result[0]["id"], 1);
        assert_eq!(result[1]["id"], 2);
    }

    #[test]
    fn test_truncated_returns_error() {
        let input = r#"{"items": [{"name": "a"}, {"name": "b"#;
        let result = parse_structured_output(input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid JSON"));
    }

    #[test]
    fn test_trailing_comma_returns_error() {
        let input = r#"{"items": ["a", "b",]}"#;
        let result = parse_structured_output(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_utf8_error_message_safe() {
        let input = r#"{"한글": "테스트", "emoji": "🎉"#;
        let result = parse_structured_output(input);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid JSON"));
    }

    #[test]
    fn test_extract_nested_json() {
        let input = r#"I'll analyze and return:
{"outer": {"inner": [1, 2, {"deep": true}]}}
That's the complete structure."#;
        let result = parse_structured_output(input).unwrap();
        assert!(result["outer"]["inner"][2]["deep"].as_bool().unwrap());
    }
}
