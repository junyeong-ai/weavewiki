//! JSON Repair Mechanism
//!
//! Unified JSON extraction and repair for LLM responses.
//!
//! Handles common LLM JSON output issues:
//! - Markdown code fence wrapping (```json ... ```)
//! - Missing closing braces/brackets
//! - Trailing commas
//! - Truncated strings
//! - Control characters in strings
//! - JSON embedded in explanatory text

use serde_json::Value;
use tracing::{debug, warn};

use crate::types::{Result, WeaveError};

// =============================================================================
// Convenience Functions
// =============================================================================

/// Extract and parse JSON from an LLM response
///
/// This is the primary entry point for parsing LLM JSON output.
/// Handles markdown code blocks, embedded JSON, and common formatting issues.
pub fn extract_json_from_response(content: &str) -> Result<Value> {
    let repairer = JsonRepairer::new();
    repairer.parse_or_repair(content).map(|(value, _)| value)
}

/// Extract and parse JSON, returning whether repair was needed
pub fn extract_json_with_repair_status(content: &str) -> Result<(Value, bool)> {
    let repairer = JsonRepairer::new();
    repairer.parse_or_repair(content)
}

// =============================================================================
// JsonRepairer
// =============================================================================

/// JSON repair strategies
pub struct JsonRepairer {
    max_repair_attempts: usize,
}

impl Default for JsonRepairer {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonRepairer {
    pub fn new() -> Self {
        Self {
            max_repair_attempts: 3,
        }
    }

    /// Parse JSON, attempting repair if initial parse fails
    ///
    /// Returns (Value, was_repaired)
    pub fn parse_or_repair(&self, raw: &str) -> Result<(Value, bool)> {
        // First, try direct parse
        let cleaned = self.preprocess(raw);

        if let Ok(value) = serde_json::from_str::<Value>(&cleaned) {
            return Ok((value, false));
        }

        // Attempt repairs
        debug!("Initial JSON parse failed, attempting repair");

        for attempt in 1..=self.max_repair_attempts {
            let repaired = self.repair_attempt(&cleaned, attempt);

            if let Ok(value) = serde_json::from_str::<Value>(&repaired) {
                warn!("JSON repaired on attempt {}", attempt);
                return Ok((value, true));
            }
        }

        // Final attempt: extract JSON from mixed content
        if let Some(extracted) = self.extract_json_from_mixed(&cleaned)
            && let Ok(value) = serde_json::from_str::<Value>(&extracted)
        {
            warn!("JSON extracted from mixed content");
            return Ok((value, true));
        }

        Err(WeaveError::LlmApi(format!(
            "Failed to parse or repair JSON after {} attempts. Content preview: {}...",
            self.max_repair_attempts,
            &cleaned.chars().take(200).collect::<String>()
        )))
    }

    /// Preprocess raw input
    fn preprocess(&self, raw: &str) -> String {
        let mut s = raw.trim().to_string();

        // Remove markdown code fences
        s = self.strip_code_fences(&s);

        // Remove BOM if present
        s = s.trim_start_matches('\u{feff}').to_string();

        // Remove leading/trailing whitespace
        s = s.trim().to_string();

        s
    }

    /// Strip markdown code fences
    fn strip_code_fences(&self, s: &str) -> String {
        let mut result = s.to_string();

        // Remove ```json ... ``` or ``` ... ```
        if result.starts_with("```")
            && let Some(first_newline) = result.find('\n')
        {
            result = result[first_newline + 1..].to_string();
        }

        if result.ends_with("```") {
            result = result[..result.len() - 3].trim_end().to_string();
        }

        result
    }

    /// Attempt repair with increasing aggressiveness
    fn repair_attempt(&self, s: &str, level: usize) -> String {
        let mut result = s.to_string();

        match level {
            1 => {
                // Level 1: Fix trailing commas and simple bracket issues
                result = self.fix_trailing_commas(&result);
                result = self.balance_brackets(&result);
            }
            2 => {
                // Level 2: Also fix truncated strings
                result = self.fix_trailing_commas(&result);
                result = self.fix_truncated_strings(&result);
                result = self.balance_brackets(&result);
            }
            _ => {
                // Level 3: Aggressive repair
                result = self.fix_trailing_commas(&result);
                result = self.remove_control_chars(&result);
                result = self.fix_truncated_strings(&result);
                result = self.balance_brackets(&result);
                result = self.truncate_to_valid(&result);
            }
        }

        result
    }

    /// Fix trailing commas before ] or }
    fn fix_trailing_commas(&self, s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let chars: Vec<char> = s.chars().collect();

        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];

            if ch == ',' {
                // Look ahead, skipping whitespace
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }

                if j < chars.len() && (chars[j] == ']' || chars[j] == '}') {
                    // Skip this comma
                    i += 1;
                    continue;
                }
            }

            result.push(ch);
            i += 1;
        }

        result
    }

    /// Balance brackets by adding missing closers
    fn balance_brackets(&self, s: &str) -> String {
        let mut result = s.to_string();

        let mut brace_count = 0;
        let mut bracket_count = 0;
        let mut in_string = false;
        let mut escape = false;

        for ch in result.chars() {
            if escape {
                escape = false;
                continue;
            }

            match ch {
                '\\' if in_string => escape = true,
                '"' => in_string = !in_string,
                '{' if !in_string => brace_count += 1,
                '}' if !in_string => brace_count -= 1,
                '[' if !in_string => bracket_count += 1,
                ']' if !in_string => bracket_count -= 1,
                _ => {}
            }
        }

        // Close unclosed strings
        if in_string {
            result.push('"');
        }

        // Add missing brackets/braces
        for _ in 0..bracket_count {
            result.push(']');
        }

        for _ in 0..brace_count {
            result.push('}');
        }

        result
    }

    /// Fix truncated strings by closing them
    fn fix_truncated_strings(&self, s: &str) -> String {
        let mut result = String::with_capacity(s.len() + 10);
        let mut in_string = false;
        let mut escape = false;

        for ch in s.chars() {
            if escape {
                escape = false;
                result.push(ch);
                continue;
            }

            match ch {
                '\\' if in_string => {
                    escape = true;
                    result.push(ch);
                }
                '"' => {
                    in_string = !in_string;
                    result.push(ch);
                }
                '\n' | '\r' if in_string => {
                    // Unterminated string at newline - close it
                    result.push('"');
                    in_string = false;
                    result.push(ch);
                }
                _ => result.push(ch),
            }
        }

        // Close final unterminated string
        if in_string {
            result.push('"');
        }

        result
    }

    /// Remove control characters that break JSON parsing
    fn remove_control_chars(&self, s: &str) -> String {
        s.chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\r' || *c == '\t')
            .collect()
    }

    /// Truncate to last valid JSON structure
    fn truncate_to_valid(&self, s: &str) -> String {
        // Find the last complete object/array
        let mut last_valid = 0;
        let mut brace_count = 0;
        let mut bracket_count = 0;
        let mut in_string = false;
        let mut escape = false;

        for (i, ch) in s.char_indices() {
            if escape {
                escape = false;
                continue;
            }

            match ch {
                '\\' if in_string => escape = true,
                '"' => in_string = !in_string,
                '{' if !in_string => brace_count += 1,
                '}' if !in_string => {
                    brace_count -= 1;
                    if brace_count == 0 && bracket_count == 0 {
                        last_valid = i + 1;
                    }
                }
                '[' if !in_string => bracket_count += 1,
                ']' if !in_string => {
                    bracket_count -= 1;
                    if brace_count == 0 && bracket_count == 0 {
                        last_valid = i + 1;
                    }
                }
                _ => {}
            }
        }

        if last_valid > 0 && last_valid < s.len() {
            s[..last_valid].to_string()
        } else {
            s.to_string()
        }
    }

    /// Extract JSON from mixed content (e.g., LLM explanations around JSON)
    fn extract_json_from_mixed(&self, s: &str) -> Option<String> {
        // Find first { or [
        let start = s.find(['{', '['])?;
        let start_char = s.chars().nth(start)?;
        let end_char = if start_char == '{' { '}' } else { ']' };

        // Find matching closer
        let mut brace_depth = 0;
        let mut bracket_depth = 0;
        let mut in_string = false;
        let mut escape = false;
        let mut end = start;

        for (i, ch) in s[start..].char_indices() {
            if escape {
                escape = false;
                continue;
            }

            match ch {
                '\\' if in_string => escape = true,
                '"' => in_string = !in_string,
                '{' if !in_string => brace_depth += 1,
                '}' if !in_string => {
                    brace_depth -= 1;
                    if brace_depth == 0 && bracket_depth == 0 && ch == end_char {
                        end = start + i + 1;
                        break;
                    }
                }
                '[' if !in_string => bracket_depth += 1,
                ']' if !in_string => {
                    bracket_depth -= 1;
                    if brace_depth == 0 && bracket_depth == 0 && ch == end_char {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        if end > start {
            Some(s[start..end].to_string())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_json() {
        let repairer = JsonRepairer::new();
        let (_, repaired) = repairer.parse_or_repair(r#"{"key": "value"}"#).unwrap();
        assert!(!repaired);
    }

    #[test]
    fn test_strip_code_fences() {
        let repairer = JsonRepairer::new();
        let input = "```json\n{\"key\": \"value\"}\n```";
        let (value, _) = repairer.parse_or_repair(input).unwrap();
        assert_eq!(value["key"], "value");
    }

    #[test]
    fn test_fix_trailing_comma() {
        let repairer = JsonRepairer::new();
        let input = r#"{"files": [{"path": "test.rs"},]}"#;
        let (value, repaired) = repairer.parse_or_repair(input).unwrap();
        assert!(repaired);
        assert!(value["files"].is_array());
    }

    #[test]
    fn test_balance_brackets() {
        let repairer = JsonRepairer::new();
        let input = r#"{"files": [{"path": "test.rs"}"#;
        let (value, repaired) = repairer.parse_or_repair(input).unwrap();
        assert!(repaired);
        assert!(value["files"].is_array());
    }

    #[test]
    fn test_extract_from_mixed() {
        let repairer = JsonRepairer::new();
        let input = r#"Here's the analysis:
{"files": [{"path": "test.rs", "sections": []}]}
Hope this helps!"#;
        let (value, repaired) = repairer.parse_or_repair(input).unwrap();
        assert!(repaired);
        assert!(value["files"].is_array());
    }

    #[test]
    fn test_truncated_string() {
        let repairer = JsonRepairer::new();
        let input = r#"{"name": "unterminated
, "other": "value"}"#;
        let result = repairer.parse_or_repair(input);
        assert!(result.is_ok());
    }
}
