//! Schema Generation Utilities
//!
//! Provides type-safe schema generation from Rust types using schemars.
//! Automatically transforms schemas for Claude's strict structured output mode.

use schemars::JsonSchema;
use serde_json::Value;

/// Generate a JSON Schema from a Rust type with strict mode transformations.
///
/// This function:
/// 1. Uses schemars to generate a schema from the type
/// 2. Transforms it for Claude's strict structured output (additionalProperties: false)
/// 3. Converts to serde_json::Value for use with LLM providers
///
/// # Example
///
/// ```rust,ignore
/// use crate::ai::response::generate_schema;
///
/// #[derive(JsonSchema)]
/// struct MyResponse {
///     name: String,
///     count: u32,
/// }
///
/// let schema = generate_schema::<MyResponse>();
/// let response = provider.generate(&prompt, &schema).await?;
/// ```
pub fn generate_schema<T: JsonSchema>() -> Value {
    let schema = schemars::schema_for!(T);
    let json = serde_json::to_value(schema).unwrap_or_else(|_| Value::Object(Default::default()));
    transform_for_strict(json)
}

/// Transform a schemars-generated schema for Claude's strict structured output.
///
/// Claude's structured output requires:
/// - `additionalProperties: false` on all objects
/// - All properties must be listed in `required`
/// - No `oneOf`, `anyOf`, `allOf` combinators (not supported)
///
/// This function recursively applies these transformations.
pub fn transform_for_strict(schema: Value) -> Value {
    transform_object(&schema)
}

fn transform_object(value: &Value) -> Value {
    match value {
        Value::Object(obj) => {
            // Handle oneOf (Rust enums) - convert to string enum or first variant
            if let Some(one_of) = obj.get("oneOf").and_then(|v| v.as_array()) {
                return transform_one_of(one_of, obj);
            }

            // Handle anyOf similarly
            if let Some(any_of) = obj.get("anyOf").and_then(|v| v.as_array()) {
                return transform_one_of(any_of, obj);
            }

            let mut new_obj = serde_json::Map::new();

            for (key, val) in obj {
                // Skip oneOf/anyOf as we handle them above
                if key == "oneOf" || key == "anyOf" {
                    continue;
                }
                let transformed = transform_object(val);
                new_obj.insert(key.clone(), transformed);
            }

            // If this is a schema object with "type": "object" and "properties"
            if obj.get("type").and_then(|v| v.as_str()) == Some("object")
                && let Some(props) = obj.get("properties")
                    && let Some(props_obj) = props.as_object() {
                        // Add additionalProperties: false
                        new_obj.insert(
                            "additionalProperties".to_string(),
                            Value::Bool(false),
                        );

                        // Make all properties required (unless already specified)
                        if !new_obj.contains_key("required") {
                            let all_props: Vec<Value> = props_obj
                                .keys()
                                .map(|k| Value::String(k.clone()))
                                .collect();

                            if !all_props.is_empty() {
                                new_obj.insert("required".to_string(), Value::Array(all_props));
                            }
                        }
                    }

            Value::Object(new_obj)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(transform_object).collect()),
        other => other.clone(),
    }
}

/// Transform oneOf/anyOf into Claude-compatible format.
///
/// Strategy:
/// 1. If all variants are simple string constants → convert to `{"type": "string", "enum": [...]}`
/// 2. If variants have object types → merge all properties as optional fields
/// 3. Fallback: use first variant's schema
fn transform_one_of(variants: &[Value], parent: &serde_json::Map<String, Value>) -> Value {
    // Check if all variants are simple string constants
    let string_constants: Vec<&str> = variants
        .iter()
        .filter_map(|v| {
            let obj = v.as_object()?;
            if obj.get("type").and_then(|t| t.as_str()) == Some("string")
                && let Some(const_val) = obj.get("const").and_then(|c| c.as_str())
            {
                return Some(const_val);
            }
            if obj.len() == 1
                && let Some(const_val) = obj.get("const").and_then(|c| c.as_str())
            {
                return Some(const_val);
            }
            None
        })
        .collect();

    // If all variants are string constants, create enum
    if string_constants.len() == variants.len() && !string_constants.is_empty() {
        let mut result = serde_json::Map::new();
        result.insert("type".to_string(), Value::String("string".to_string()));
        result.insert(
            "enum".to_string(),
            Value::Array(string_constants.iter().map(|s| Value::String(s.to_string())).collect()),
        );
        return Value::Object(result);
    }

    // Check if variants are tagged objects (internally/adjacently tagged enums)
    // Merge all properties as optional
    let mut merged_props = serde_json::Map::new();
    let mut has_object_variants = false;

    for variant in variants {
        if let Some(obj) = variant.as_object()
            && obj.get("type").and_then(|t| t.as_str()) == Some("object")
        {
            has_object_variants = true;
            if let Some(props) = obj.get("properties").and_then(|p| p.as_object()) {
                for (key, val) in props {
                    merged_props.insert(key.clone(), transform_object(val));
                }
            }
        }
    }

    if has_object_variants && !merged_props.is_empty() {
        let mut result = serde_json::Map::new();
        result.insert("type".to_string(), Value::String("object".to_string()));
        result.insert("properties".to_string(), Value::Object(merged_props.clone()));
        result.insert("additionalProperties".to_string(), Value::Bool(false));
        // All merged properties are optional (no required array)
        return Value::Object(result);
    }

    // Fallback: use first variant or return empty object
    if let Some(first) = variants.first() {
        let transformed = transform_object(first);
        // Preserve any description from parent
        if let (Some(desc), Value::Object(mut obj)) = (parent.get("description"), transformed) {
            obj.insert("description".to_string(), desc.clone());
            return Value::Object(obj);
        }
        return transform_object(first);
    }

    // Last resort: generic string
    serde_json::json!({"type": "string"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct TestType {
        name: String,
        count: u32,
        optional: Option<String>,
    }

    #[test]
    fn test_generate_schema() {
        let schema = generate_schema::<TestType>();

        assert!(schema.is_object());
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_transform_for_strict() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "count": {"type": "integer"}
            }
        });

        let transformed = transform_for_strict(schema);

        assert_eq!(
            transformed.get("additionalProperties"),
            Some(&Value::Bool(false))
        );

        let required = transformed.get("required").unwrap().as_array().unwrap();
        assert_eq!(required.len(), 2);
    }

    #[test]
    fn test_nested_objects_transformed() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "inner": {
                    "type": "object",
                    "properties": {
                        "value": {"type": "string"}
                    }
                }
            }
        });

        let transformed = transform_for_strict(schema);

        let inner = transformed
            .get("properties")
            .unwrap()
            .get("inner")
            .unwrap();
        assert_eq!(
            inner.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn test_oneof_string_constants_to_enum() {
        // schemars generates this for simple Rust enums like enum Status { Active, Inactive }
        let schema = serde_json::json!({
            "oneOf": [
                {"type": "string", "const": "active"},
                {"type": "string", "const": "inactive"},
                {"type": "string", "const": "pending"}
            ]
        });

        let transformed = transform_for_strict(schema);

        assert_eq!(transformed.get("type"), Some(&Value::String("string".to_string())));
        let enum_vals = transformed.get("enum").unwrap().as_array().unwrap();
        assert_eq!(enum_vals.len(), 3);
        assert!(enum_vals.contains(&Value::String("active".to_string())));
        assert!(enum_vals.contains(&Value::String("inactive".to_string())));
        assert!(enum_vals.contains(&Value::String("pending".to_string())));
    }

    #[test]
    fn test_oneof_tagged_objects_merged() {
        // schemars generates this for tagged enums
        let schema = serde_json::json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "tag": {"const": "TypeA"},
                        "value_a": {"type": "string"}
                    }
                },
                {
                    "type": "object",
                    "properties": {
                        "tag": {"const": "TypeB"},
                        "value_b": {"type": "integer"}
                    }
                }
            ]
        });

        let transformed = transform_for_strict(schema);

        assert_eq!(transformed.get("type"), Some(&Value::String("object".to_string())));
        let props = transformed.get("properties").unwrap().as_object().unwrap();
        // Should have merged properties from both variants
        assert!(props.contains_key("tag"));
        assert!(props.contains_key("value_a"));
        assert!(props.contains_key("value_b"));
        // No required array (all optional)
        assert!(transformed.get("required").is_none());
    }
}
