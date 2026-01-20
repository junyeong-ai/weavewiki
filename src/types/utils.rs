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

/// Check if a string is valid kebab-case format.
///
/// Valid kebab-case:
/// - Contains only lowercase letters, digits, and hyphens
/// - Does not start or end with a hyphen
/// - Does not contain consecutive hyphens
#[inline]
pub fn is_kebab_case(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
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
}
