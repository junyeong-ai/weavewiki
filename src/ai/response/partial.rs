//! Partial Parsing Implementation
//!
//! Implements field-level parsing with graceful degradation.
//! When a field fails to parse, use its default and continue.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::types::{FieldError, ParseResult, ParseStatus, RecoveryAction};

/// Parse LLM response with field-level recovery.
///
/// Unlike standard deserialization that fails on any error, this:
/// 1. Attempts full deserialization first
/// 2. On failure, falls back to field-by-field parsing
/// 3. Uses defaults for fields that fail to parse
/// 4. Reports all errors for diagnostics
///
/// # Type Parameters
/// * `T` - Target type that implements `DeserializeOwned`, `Default`, and `PartialParseable`
pub fn parse_partial<T>(content: &Value, context: &str) -> ParseResult<T>
where
    T: DeserializeOwned + Default + PartialParseable,
{
    // Attempt 1: Full deserialization
    if let Ok(full) = serde_json::from_value::<T>(content.clone()) {
        tracing::trace!(context = %context, "Full parse succeeded");
        return ParseResult::complete(full);
    }

    // Attempt 2: Field-by-field parsing with recovery
    parse_fields(content, context)
}

/// Parse content with partial recovery using the PartialParseable trait.
fn parse_fields<T>(content: &Value, context: &str) -> ParseResult<T>
where
    T: Default + PartialParseable,
{
    let obj = match content.as_object() {
        Some(o) => o,
        None => {
            tracing::warn!(
                context = %context,
                content_type = %value_type_name(content),
                "Expected JSON object, using default"
            );
            return ParseResult::failed(T::default(), "Expected JSON object");
        }
    };

    let mut result = T::default();
    let mut failed_fields = Vec::new();
    let mut recovered_count = 0;

    for (field_name, field_value) in obj {
        match result.try_set_field(field_name, field_value) {
            Ok(SetFieldResult::Set) => recovered_count += 1,
            Ok(SetFieldResult::Ignored) => {
                // Unknown field, ignore silently
            }
            Err(e) => {
                failed_fields.push(FieldError::new(
                    field_name,
                    e.expected,
                    Some(field_value.to_string()),
                    RecoveryAction::UsedDefault,
                ));
            }
        }
    }

    // Check required fields
    for required in T::required_fields() {
        if !obj.contains_key(*required) {
            failed_fields.push(FieldError::new(
                *required,
                "required field",
                None,
                RecoveryAction::UsedDefault,
            ));
        }
    }

    let status = if failed_fields.is_empty() {
        ParseStatus::Complete
    } else {
        let total = recovered_count + failed_fields.len();
        ParseStatus::Partial {
            success_ratio: if total == 0 {
                0.0
            } else {
                recovered_count as f32 / total as f32
            },
        }
    };

    if !failed_fields.is_empty() {
        tracing::debug!(
            context = %context,
            recovered = %recovered_count,
            failed = %failed_fields.len(),
            failed_fields = ?failed_fields.iter().map(|f| &f.field).collect::<Vec<_>>(),
            "Partial parse with field-level recovery"
        );
    }

    ParseResult {
        data: result,
        status,
        failed_fields,
        recovered_count,
    }
}

/// Result of setting a field
pub enum SetFieldResult {
    /// Field was set successfully
    Set,
    /// Field was ignored (unknown field name)
    Ignored,
}

/// Error when setting a field
pub struct FieldParseError {
    pub expected: String,
    pub message: String,
}

impl FieldParseError {
    pub fn new(expected: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            expected: expected.into(),
            message: message.into(),
        }
    }

    pub fn type_mismatch(expected: &str, actual: &Value) -> Self {
        Self::new(expected, format!("expected {expected}, got {}", value_type_name(actual)))
    }
}

/// Trait for types that support partial field-by-field parsing.
///
/// Implement this for types used as LLM response schemas to enable
/// graceful degradation when individual fields fail to parse.
pub trait PartialParseable: Default {
    /// Try to set a field by name from a JSON value.
    ///
    /// # Returns
    /// - `Ok(SetFieldResult::Set)` if the field was successfully set
    /// - `Ok(SetFieldResult::Ignored)` if the field name is not recognized
    /// - `Err(FieldParseError)` if the field is known but the value failed to parse
    fn try_set_field(&mut self, name: &str, value: &Value) -> Result<SetFieldResult, FieldParseError>;

    /// List of required field names.
    /// Missing required fields will be reported but won't prevent parsing.
    fn required_fields() -> &'static [&'static str] {
        &[]
    }
}

/// Helper to get a human-readable type name for a JSON value
fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Helper to extract and parse a field from a JSON object
pub fn extract_field<T: DeserializeOwned + Default>(obj: &Map<String, Value>, field: &str) -> T {
    obj.get(field)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Helper to extract an optional field
pub fn extract_optional<T: DeserializeOwned>(obj: &Map<String, Value>, field: &str) -> Option<T> {
    obj.get(field).and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// Helper to extract a string field
pub fn extract_string(obj: &Map<String, Value>, field: &str) -> String {
    obj.get(field)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Helper to extract a bool field
pub fn extract_bool(obj: &Map<String, Value>, field: &str, default: bool) -> bool {
    obj.get(field).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// Helper to extract a float field
pub fn extract_f32(obj: &Map<String, Value>, field: &str, default: f32) -> f32 {
    obj.get(field)
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(default)
}

/// Helper to extract a u32 field
pub fn extract_u32(obj: &Map<String, Value>, field: &str, default: u32) -> u32 {
    obj.get(field)
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(default)
}

/// Helper to extract an array of strings
pub fn extract_string_array(obj: &Map<String, Value>, field: &str) -> Vec<String> {
    obj.get(field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, Clone, Default, PartialEq)]
    struct TestStruct {
        name: String,
        count: u32,
        items: Vec<String>,
    }

    impl PartialParseable for TestStruct {
        fn try_set_field(&mut self, name: &str, value: &Value) -> Result<SetFieldResult, FieldParseError> {
            match name {
                "name" => {
                    self.name = value.as_str()
                        .ok_or_else(|| FieldParseError::type_mismatch("string", value))?
                        .to_string();
                    Ok(SetFieldResult::Set)
                }
                "count" => {
                    self.count = value.as_u64()
                        .ok_or_else(|| FieldParseError::type_mismatch("integer", value))? as u32;
                    Ok(SetFieldResult::Set)
                }
                "items" => {
                    self.items = value.as_array()
                        .ok_or_else(|| FieldParseError::type_mismatch("array", value))?
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    Ok(SetFieldResult::Set)
                }
                _ => Ok(SetFieldResult::Ignored),
            }
        }

        fn required_fields() -> &'static [&'static str] {
            &["name"]
        }
    }

    #[test]
    fn test_parse_partial_complete() {
        let input = json!({
            "name": "test",
            "count": 42,
            "items": ["a", "b"]
        });

        let result = parse_fields::<TestStruct>(&input, "test");
        assert!(result.is_complete());
        assert_eq!(result.data.name, "test");
        assert_eq!(result.data.count, 42);
        assert_eq!(result.data.items, vec!["a", "b"]);
    }

    #[test]
    fn test_parse_partial_with_missing_optional() {
        let input = json!({
            "name": "test"
        });

        let result = parse_fields::<TestStruct>(&input, "test");
        assert!(result.is_usable());
        assert_eq!(result.data.name, "test");
        assert_eq!(result.data.count, 0); // default
        assert!(result.data.items.is_empty()); // default
    }

    #[test]
    fn test_parse_partial_with_type_error() {
        let input = json!({
            "name": "test",
            "count": "not a number"  // Wrong type
        });

        let result = parse_fields::<TestStruct>(&input, "test");
        assert!(result.is_usable());
        assert_eq!(result.data.name, "test");
        assert_eq!(result.data.count, 0); // default due to type error
        assert_eq!(result.failed_fields.len(), 1);
        assert_eq!(result.failed_fields[0].field, "count");
    }

    #[test]
    fn test_parse_partial_non_object() {
        let input = json!("just a string");

        let result = parse_fields::<TestStruct>(&input, "test");
        assert!(!result.is_usable());
        assert!(matches!(result.status, ParseStatus::Failed { .. }));
    }
}
