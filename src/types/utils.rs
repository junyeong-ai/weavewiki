//! Shared utility functions for type serialization and common operations.
//!
//! ## JSON Extraction Helpers
//!
//! Provides ergonomic helpers for extracting values from `serde_json::Value`:
//! - `json_string`, `json_string_or` - Extract strings
//! - `json_string_array` - Extract string arrays
//! - `json_bool`, `json_i64`, `json_f64` - Extract primitives

use crate::types::{InformationTier, NodeStatus, NodeType};
use serde::Serialize;
use std::fmt::Display;

// =============================================================================
// JSON Value Extraction Helpers
// =============================================================================

/// Extract string from JSON value by key.
///
/// Replaces verbose `v.get("key")?.as_str()?.to_string()` patterns.
#[inline]
pub fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(String::from)
}

/// Extract string with default value.
#[inline]
pub fn json_string_or(value: &serde_json::Value, key: &str, default: &str) -> String {
    json_string(value, key).unwrap_or_else(|| default.to_string())
}

/// Extract string array from JSON value by key.
#[inline]
pub fn json_string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract boolean with default.
#[inline]
pub fn json_bool(value: &serde_json::Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// Extract i64 with default.
#[inline]
pub fn json_i64(value: &serde_json::Value, key: &str, default: i64) -> i64 {
    value.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}

/// Extract f64 with default.
#[inline]
pub fn json_f64(value: &serde_json::Value, key: &str, default: f64) -> f64 {
    value.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
}

// =============================================================================
// String Utilities
// =============================================================================

/// Capitalize the first character of a string.
/// Used for formatting purpose statements and titles.
#[inline]
pub fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

// =============================================================================
// Type Parsing
// =============================================================================

/// Trait for parsing strings into enum types with a default fallback.
/// Used for deserializing database values where invalid strings should fall back gracefully.
/// Logs a warning when an invalid value is encountered.
pub trait ParseWithDefault: Sized {
    /// The name of this type for logging purposes.
    fn type_name() -> &'static str;

    /// The default value to use when parsing fails.
    fn default_value() -> Self;

    /// Try to parse the string, returning None if invalid.
    fn try_parse(s: &str) -> Option<Self>;

    /// Parse a string into this type, returning a default value if parsing fails.
    /// Logs a warning for invalid values to help detect data corruption.
    fn parse_or_default(s: &str) -> Self {
        match Self::try_parse(s) {
            Some(v) => v,
            None => {
                tracing::warn!("Invalid {} value '{}', using default", Self::type_name(), s);
                Self::default_value()
            }
        }
    }
}

impl ParseWithDefault for NodeType {
    fn type_name() -> &'static str {
        "NodeType"
    }

    fn default_value() -> Self {
        NodeType::File
    }

    fn try_parse(s: &str) -> Option<Self> {
        match s {
            "module" => Some(NodeType::Module),
            "file" => Some(NodeType::File),
            "function" => Some(NodeType::Function),
            "method" => Some(NodeType::Method),
            "class" => Some(NodeType::Class),
            "interface" => Some(NodeType::Interface),
            "type" => Some(NodeType::Type),
            "enum" => Some(NodeType::Enum),
            "api" => Some(NodeType::Api),
            "entity" => Some(NodeType::Entity),
            "component" => Some(NodeType::Component),
            "route" => Some(NodeType::Route),
            "config" => Some(NodeType::Config),
            _ => None,
        }
    }
}

impl ParseWithDefault for InformationTier {
    fn type_name() -> &'static str {
        "InformationTier"
    }

    fn default_value() -> Self {
        InformationTier::Fact
    }

    fn try_parse(s: &str) -> Option<Self> {
        match s {
            "fact" => Some(InformationTier::Fact),
            "inference" => Some(InformationTier::Inference),
            "interpretation" => Some(InformationTier::Interpretation),
            _ => None,
        }
    }
}

impl ParseWithDefault for NodeStatus {
    fn type_name() -> &'static str {
        "NodeStatus"
    }

    fn default_value() -> Self {
        NodeStatus::Unknown
    }

    fn try_parse(s: &str) -> Option<Self> {
        match s {
            "verified" => Some(NodeStatus::Verified),
            "stale" => Some(NodeStatus::Stale),
            "conflict" => Some(NodeStatus::Conflict),
            "unknown" => Some(NodeStatus::Unknown),
            _ => None,
        }
    }
}

/// Serialize an enum to its serde string representation (without quotes).
/// Uses serde_json internally to ensure consistent serialization with
/// the `#[serde(rename_all = ...)]` attributes on enums.
pub fn enum_to_str<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

/// Filter an iterator of Results, logging errors at debug level before discarding.
///
/// Use this instead of `.filter_map(|r| r.ok())` when you want visibility into
/// what errors are being discarded.
///
/// # Example
/// ```ignore
/// let values: Vec<_> = results
///     .filter_map(|r| log_filter_error(r, "loading items"))
///     .collect();
/// ```
pub fn log_filter_error<T, E: Display>(result: Result<T, E>, context: &str) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::debug!("{}: {}", context, e);
            None
        }
    }
}

/// Like log_filter_error but logs at warn level for more important operations.
pub fn log_filter_warn<T, E: Display>(result: Result<T, E>, context: &str) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!("{}: {}", context, e);
            None
        }
    }
}

// =============================================================================
// Token Estimation
// =============================================================================

/// Token estimation configuration for different content types
#[derive(Debug, Clone, Copy)]
pub struct TokenEstimator {
    /// Characters per token for ASCII text (default: 4.0)
    pub ascii_chars_per_token: f32,
    /// Characters per token for non-ASCII (CJK, etc.) (default: 1.5)
    pub non_ascii_chars_per_token: f32,
    /// Extra tokens per line for code structure (default: 0.5)
    pub code_overhead_per_line: f32,
}

impl Default for TokenEstimator {
    fn default() -> Self {
        Self {
            ascii_chars_per_token: 4.0,
            non_ascii_chars_per_token: 1.5,
            code_overhead_per_line: 0.5,
        }
    }
}

impl TokenEstimator {
    /// Create estimator optimized for code
    pub fn for_code() -> Self {
        Self {
            ascii_chars_per_token: 3.5, // Code has more short tokens (operators, brackets)
            non_ascii_chars_per_token: 1.5,
            code_overhead_per_line: 0.8, // More structural tokens
        }
    }

    /// Estimate token count for content
    pub fn estimate(&self, content: &str) -> usize {
        if content.is_empty() {
            return 0;
        }

        let mut ascii_chars = 0usize;
        let mut non_ascii_chars = 0usize;

        for c in content.chars() {
            if c.is_ascii() {
                ascii_chars += 1;
            } else {
                non_ascii_chars += 1;
            }
        }

        let line_count = content.lines().count();
        let code_overhead = (line_count as f32 * self.code_overhead_per_line) as usize;

        let ascii_tokens = (ascii_chars as f32 / self.ascii_chars_per_token) as usize;
        let non_ascii_tokens = (non_ascii_chars as f32 / self.non_ascii_chars_per_token) as usize;

        ascii_tokens + non_ascii_tokens + code_overhead
    }
}

/// Estimate token count from content (convenience function)
///
/// Uses default estimator settings. For code-specific estimation,
/// use `TokenEstimator::for_code().estimate(content)`.
#[inline]
pub fn estimate_tokens(content: &str) -> usize {
    TokenEstimator::default().estimate(content)
}

/// Estimate tokens specifically for code content
#[inline]
pub fn estimate_code_tokens(content: &str) -> usize {
    TokenEstimator::for_code().estimate(content)
}

/// Truncate content to fit within token limit
///
/// Preserves paragraph boundaries when possible, falls back to line boundaries.
pub fn truncate_to_token_limit(content: &str, max_tokens: usize) -> String {
    let estimated = estimate_tokens(content);
    if estimated <= max_tokens {
        return content.to_string();
    }

    // Estimate character budget based on token ratio
    let ratio = max_tokens as f64 / estimated as f64;
    let max_chars = (content.len() as f64 * ratio * 0.95) as usize; // 5% buffer

    if max_chars >= content.len() {
        return content.to_string();
    }

    let truncated = &content[..max_chars.min(content.len())];

    // Try to break at paragraph boundary
    if let Some(pos) = truncated.rfind("\n\n") {
        return format!(
            "{}...\n\n*[Content truncated due to token budget]*",
            &content[..pos]
        );
    }

    // Fall back to line boundary
    if let Some(pos) = truncated.rfind('\n') {
        return format!(
            "{}...\n\n*[Content truncated due to token budget]*",
            &content[..pos]
        );
    }

    // Last resort: hard truncation
    format!(
        "{}...\n\n*[Content truncated due to token budget]*",
        truncated
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EdgeType, NodeType};

    #[test]
    fn test_enum_to_str_node_type() {
        assert_eq!(enum_to_str(&NodeType::File), "file");
        assert_eq!(enum_to_str(&NodeType::Function), "function");
        assert_eq!(enum_to_str(&NodeType::Class), "class");
    }

    #[test]
    fn test_enum_to_str_edge_type() {
        assert_eq!(enum_to_str(&EdgeType::DependsOn), "depends_on");
        assert_eq!(enum_to_str(&EdgeType::Owns), "owns");
    }

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_ascii() {
        // 12 ASCII chars / 4.0 + 1 line * 0.5 = 3 + 0 = 3
        let result = estimate_tokens("hello world!");
        assert!(result > 0);
        assert!(result < 10);
    }

    #[test]
    fn test_estimate_tokens_non_ascii() {
        // Non-ASCII uses 1.5 chars/token, should produce higher token-per-char ratio
        let korean = estimate_tokens("안녕하세요");

        // 5 Korean characters should estimate to meaningful token count
        assert!(korean > 0);
        assert!(korean >= 3); // At least 5 chars / 1.5 = ~3 tokens
    }

    #[test]
    fn test_estimate_code_tokens() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let code_estimate = estimate_code_tokens(code);
        let text_estimate = estimate_tokens(code);

        // Code estimator should give higher estimate due to structural overhead
        assert!(code_estimate >= text_estimate);
    }

    #[test]
    fn test_truncate_to_token_limit_no_truncation() {
        let content = "Short content.";
        let result = truncate_to_token_limit(content, 1000);
        assert_eq!(result, content);
    }

    #[test]
    fn test_truncate_to_token_limit_with_truncation() {
        let content = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let result = truncate_to_token_limit(content, 5);
        assert!(result.contains("truncated"));
    }
}
