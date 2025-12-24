//! JSON Schemas for Characterization Agent Outputs
//!
//! These schemas define the expected structure of LLM outputs for each
//! characterization agent. Used with Claude's --json-schema for guaranteed
//! structured output.
//!
//! Best Practices Applied:
//! - All objects have `additionalProperties: false`
//! - All fields have clear `description`
//! - Required fields explicitly listed
//! - Enums for constrained values

use serde_json::json;

/// Schema provider for characterization agents
pub struct AgentSchemas;

impl AgentSchemas {
    /// JSON schema for StructureAgent output
    pub fn structure_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Project structure analysis identifying organization patterns and module boundaries",
            "required": ["directory_patterns", "module_boundaries", "organization_style"],
            "additionalProperties": false,
            "properties": {
                "directory_patterns": {
                    "type": "array",
                    "description": "Detected directory organization patterns (e.g., 'src/lib pattern', 'feature-based modules')",
                    "items": {"type": "string"}
                },
                "module_boundaries": {
                    "type": "array",
                    "description": "Module/package boundaries with their purposes",
                    "items": {
                        "type": "object",
                        "required": ["name", "path"],
                        "additionalProperties": false,
                        "properties": {
                            "name": {"type": "string", "description": "Module name"},
                            "path": {"type": "string", "description": "Relative path from project root"},
                            "purpose": {"type": "string", "description": "Brief purpose description"}
                        }
                    }
                },
                "organization_style": {
                    "type": "string",
                    "description": "Overall code organization style",
                    "enum": ["layered", "feature_based", "domain_driven", "flat", "monorepo", "mixed"]
                },
                "naming_conventions": {
                    "type": "array",
                    "description": "Detected naming conventions (e.g., 'snake_case files', 'PascalCase types')",
                    "items": {"type": "string"}
                },
                "test_organization": {
                    "type": "string",
                    "description": "How tests are organized (e.g., 'colocated', 'separate_tests_dir', 'inline')"
                }
            }
        })
    }

    /// JSON schema for DependencyAgent output
    pub fn dependency_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Project dependency analysis identifying internal and external dependencies",
            "required": ["internal_deps", "external_deps"],
            "additionalProperties": false,
            "properties": {
                "internal_deps": {
                    "type": "array",
                    "description": "Internal module dependencies showing how modules relate",
                    "items": {
                        "type": "object",
                        "required": ["from", "to"],
                        "additionalProperties": false,
                        "properties": {
                            "from": {"type": "string", "description": "Source module path"},
                            "to": {"type": "string", "description": "Target module path"},
                            "dependency_type": {"type": "string", "description": "Type of dependency (import, use, inherit, implement)"}
                        }
                    }
                },
                "external_deps": {
                    "type": "array",
                    "description": "External library/crate dependencies",
                    "items": {"type": "string"}
                },
                "framework_indicators": {
                    "type": "array",
                    "description": "Detected framework patterns (e.g., 'tokio async runtime', 'actix-web')",
                    "items": {"type": "string"}
                },
                "circular_deps": {
                    "type": "array",
                    "description": "Detected circular dependency chains as arrays of module names",
                    "items": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                }
            }
        })
    }

    /// JSON schema for EntryPointAgent output
    pub fn entry_point_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Project entry point analysis identifying application entry points and public APIs",
            "required": ["entry_points"],
            "additionalProperties": false,
            "properties": {
                "entry_points": {
                    "type": "array",
                    "description": "Application entry points (main functions, bin targets, exports)",
                    "items": {
                        "type": "object",
                        "required": ["entry_type", "file"],
                        "additionalProperties": false,
                        "properties": {
                            "entry_type": {
                                "type": "string",
                                "description": "Type of entry point",
                                "enum": ["main", "bin", "lib", "api", "handler", "export", "cli", "web"]
                            },
                            "file": {"type": "string", "description": "File path in format 'path/file.rs:line'"},
                            "symbol": {"type": "string", "description": "Function or symbol name"}
                        }
                    }
                },
                "public_surface": {
                    "type": "array",
                    "description": "Public API surfaces exposed to external consumers",
                    "items": {"type": "string"}
                },
                "cli_commands": {
                    "type": "array",
                    "description": "CLI command names if this is a CLI application",
                    "items": {"type": "string"}
                }
            }
        })
    }

    /// JSON schema for PurposeAgent output
    pub fn purpose_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Project purpose discovery identifying what the project does and for whom",
            "required": ["purposes", "target_users"],
            "additionalProperties": false,
            "properties": {
                "purposes": {
                    "type": "array",
                    "description": "Discovered project purposes (what the project does)",
                    "items": {"type": "string"}
                },
                "target_users": {
                    "type": "array",
                    "description": "Target users (developers, end-users, operators, etc.)",
                    "items": {"type": "string"}
                },
                "problems_solved": {
                    "type": "array",
                    "description": "Problems this project solves",
                    "items": {"type": "string"}
                }
            }
        })
    }

    /// JSON schema for TechnicalAgent output
    pub fn technical_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Technical patterns analysis identifying architecture and implementation patterns",
            "required": ["technical_traits"],
            "additionalProperties": false,
            "properties": {
                "technical_traits": {
                    "type": "array",
                    "description": "Technical patterns detected (e.g., 'strongly typed', 'async/await', 'trait-based abstraction')",
                    "items": {"type": "string"}
                },
                "architecture_patterns": {
                    "type": "array",
                    "description": "Architecture patterns detected (e.g., 'hexagonal', 'layered', 'event-driven')",
                    "items": {"type": "string"}
                },
                "quality_focus": {
                    "type": "array",
                    "description": "Quality focus areas (e.g., 'performance', 'safety', 'extensibility')",
                    "items": {"type": "string"}
                },
                "async_patterns": {
                    "type": "array",
                    "description": "Async patterns if applicable (e.g., 'tokio runtime', 'channels', 'futures')",
                    "items": {"type": "string"}
                }
            }
        })
    }

    /// JSON schema for DomainAgent output
    pub fn domain_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Domain analysis identifying domain-specific traits and terminology",
            "required": ["domain_traits"],
            "additionalProperties": false,
            "properties": {
                "domain_traits": {
                    "type": "array",
                    "description": "Domain-specific traits (e.g., 'data processing', 'web service', 'CLI tool')",
                    "items": {"type": "string"}
                },
                "terminology": {
                    "type": "array",
                    "description": "Domain-specific terms and their meanings",
                    "items": {
                        "type": "object",
                        "required": ["term", "meaning"],
                        "additionalProperties": false,
                        "properties": {
                            "term": {"type": "string", "description": "The domain term"},
                            "meaning": {"type": "string", "description": "What this term means in this project's context"},
                            "evidence": {"type": "string", "description": "File:line where term is used"}
                        }
                    }
                },
                "domain_patterns": {
                    "type": "array",
                    "description": "Domain-specific patterns detected in the codebase",
                    "items": {"type": "string"}
                }
            }
        })
    }

    /// Get all schemas as a map
    pub fn all_schemas() -> std::collections::HashMap<String, serde_json::Value> {
        let mut schemas = std::collections::HashMap::new();
        schemas.insert("structure".to_string(), Self::structure_schema());
        schemas.insert("dependency".to_string(), Self::dependency_schema());
        schemas.insert("entry_point".to_string(), Self::entry_point_schema());
        schemas.insert("purpose".to_string(), Self::purpose_schema());
        schemas.insert("technical".to_string(), Self::technical_schema());
        schemas.insert("domain".to_string(), Self::domain_schema());
        schemas
    }
}

/// Prompts for characterization agents
pub struct AgentPrompts;

impl AgentPrompts {
    /// System prompt for characterization agents
    pub fn system_prompt() -> &'static str {
        r#"You are a code analysis expert that extracts project characteristics.
Your responses must be valid JSON matching the provided schema exactly.
Focus on FACTS observable in the code - never speculate or hallucinate.
When providing file references, use the format: file.ext:line_number"#
    }

    /// Prompt for StructureAgent
    pub fn structure_prompt(file_list: &str) -> String {
        format!(
            r#"Analyze the directory structure and identify organization patterns.

FILE LIST:
{}

Based on this file listing, identify:
1. Directory organization patterns (how code is organized)
2. Module boundaries (logical groupings)
3. Overall organization style
4. Naming conventions
5. Test file organization

Respond with valid JSON matching the schema."#,
            file_list
        )
    }

    /// Prompt for DependencyAgent
    pub fn dependency_prompt(imports_data: &str) -> String {
        format!(
            r#"Analyze the import/use statements and identify dependency patterns.

IMPORT DATA:
{}

Based on this import data, identify:
1. Internal module dependencies (which modules use which)
2. External library dependencies
3. Framework indicators
4. Any circular dependencies

Respond with valid JSON matching the schema."#,
            imports_data
        )
    }

    /// Prompt for EntryPointAgent
    pub fn entry_point_prompt(file_samples: &str) -> String {
        format!(
            r#"Analyze the code and identify entry points and public APIs.

CODE SAMPLES:
{}

Based on this code, identify:
1. Application entry points (main functions, bin targets, exports)
2. Public API surfaces
3. CLI commands (if applicable)

Respond with valid JSON matching the schema."#,
            file_samples
        )
    }

    /// Prompt for PurposeAgent (Turn 2 - receives Turn 1 context)
    pub fn purpose_prompt(structure_insight: &str, entry_points: &str) -> String {
        format!(
            r#"Based on the project structure and entry points, determine the project's purpose.

PROJECT STRUCTURE:
{}

ENTRY POINTS:
{}

Based on this context, determine:
1. What purposes this project serves
2. Who are the target users
3. What problems it solves

Respond with valid JSON matching the schema."#,
            structure_insight, entry_points
        )
    }

    /// Prompt for TechnicalAgent (Turn 2)
    pub fn technical_prompt(structure_insight: &str, dependencies: &str) -> String {
        format!(
            r#"Based on the project structure and dependencies, identify technical patterns.

PROJECT STRUCTURE:
{}

DEPENDENCIES:
{}

Based on this context, identify:
1. Technical traits and patterns
2. Architecture patterns
3. Quality focus areas
4. Async patterns (if any)

Respond with valid JSON matching the schema."#,
            structure_insight, dependencies
        )
    }

    /// Prompt for DomainAgent (Turn 2)
    pub fn domain_prompt(structure_insight: &str, code_samples: &str) -> String {
        format!(
            r#"Based on the project structure and code, identify domain-specific patterns.

PROJECT STRUCTURE:
{}

CODE SAMPLES:
{}

Based on this context, identify:
1. Domain-specific traits
2. Domain terminology with meanings
3. Domain patterns

Respond with valid JSON matching the schema."#,
            structure_insight, code_samples
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structure_schema_has_additional_properties_false() {
        let schema = AgentSchemas::structure_schema();
        assert_eq!(schema.get("additionalProperties"), Some(&json!(false)));
    }

    #[test]
    fn test_all_schemas_have_additional_properties_false() {
        let schemas = AgentSchemas::all_schemas();
        for (name, schema) in schemas {
            assert_eq!(
                schema.get("additionalProperties"),
                Some(&json!(false)),
                "Schema '{}' missing additionalProperties: false",
                name
            );
        }
    }

    #[test]
    fn test_all_schemas_complete() {
        let schemas = AgentSchemas::all_schemas();
        assert_eq!(schemas.len(), 6);
        assert!(schemas.contains_key("structure"));
        assert!(schemas.contains_key("dependency"));
        assert!(schemas.contains_key("entry_point"));
        assert!(schemas.contains_key("purpose"));
        assert!(schemas.contains_key("technical"));
        assert!(schemas.contains_key("domain"));
    }
}
