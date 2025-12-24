//! Architecture Agent
//!
//! Analyzes overall structure, layer boundaries, and dependency direction.

use super::{TopDownAgent, TopDownAgentConfig, TopDownContext, run_top_down_agent};
use crate::types::error::WeaveError;
use crate::types::{json_string, json_string_array, json_string_or};
use crate::wiki::exhaustive::top_down::insights::{BoundaryViolation, Layer, ProjectInsight};
use serde_json::json;
use std::collections::HashMap;

#[derive(Default)]
pub struct ArchitectureAgent;

#[async_trait::async_trait]
impl TopDownAgent for ArchitectureAgent {
    fn name(&self) -> &str {
        "architecture"
    }

    async fn run(&self, context: &TopDownContext) -> Result<ProjectInsight, WeaveError> {
        run_top_down_agent(
            context,
            TopDownAgentConfig {
                name: "architecture",
                build_context: Box::new(Self::build_file_summary),
                build_prompt: Box::new(Self::build_prompt),
                schema: Self::schema(),
                parse_result: Box::new(Self::parse_result),
                debug_result: Box::new(|insight| {
                    format!(
                        "Found pattern={:?}, {} layers, {} violations",
                        insight.architecture_pattern,
                        insight.layers.len(),
                        insight.boundary_violations.len()
                    )
                }),
            },
        )
        .await
    }
}

impl ArchitectureAgent {
    fn build_file_summary(context: &TopDownContext) -> String {
        let mut summary = String::new();
        let mut by_dir: HashMap<String, Vec<String>> = HashMap::new();

        for fi in &context.file_insights {
            let dir = fi
                .file_path
                .rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .unwrap_or_else(|| ".".to_string());
            by_dir.entry(dir).or_default().push(fi.file_path.clone());
        }

        // Sort and format
        let mut dirs: Vec<_> = by_dir.keys().cloned().collect();
        dirs.sort();

        for dir in dirs.iter().take(20) {
            let files = &by_dir[dir];
            summary.push_str(&format!("{}/ ({} files)\n", dir, files.len()));
            for file in files.iter().take(5) {
                summary.push_str(&format!("  - {}\n", file));
            }
            if files.len() > 5 {
                summary.push_str(&format!("  ... and {} more\n", files.len() - 5));
            }
        }

        if dirs.len() > 20 {
            summary.push_str(&format!("... and {} more directories\n", dirs.len() - 20));
        }

        summary
    }

    fn build_prompt(context: &TopDownContext, file_summary: &str) -> String {
        format!(
            r#"<ROLE>
You are an expert software architecture analyst specializing in system design patterns,
layer boundaries, and dependency relationships.
Your task is to identify architectural patterns, map logical layers, and detect violations.
</ROLE>

<OBJECTIVES>
Analyze this project's architecture to:
1. Identify the overall architecture pattern (layered, modular, hexagonal, etc.)
2. Map logical layers with clear separation of concerns
3. Detect boundary violations and inappropriate dependencies
4. Assess architectural coherence and design consistency
</OBJECTIVES>

## Project Context
Name: {}
Purposes: {}
Technical Traits: {}

## File Structure
{}

<FOCUS>
IMPORTANT: Focus EXCLUSIVELY on architecture patterns observable in the file structure.
- Do NOT dive into implementation details of individual files
- Do NOT speculate about hidden dependencies not evident from structure
- Do NOT explain generic architecture concepts
- ONLY analyze this specific project's architecture based on observable patterns
- Reference files by path when explaining layers
</FOCUS>

## Analysis Task
Identify:
1. Architecture pattern with evidence from file structure
2. Logical layers with clear responsibilities
3. Dependency direction violations and boundary crossings

<ANTI-PATTERNS>
WRONG - Generic descriptions:
- "This uses a layered architecture"
- "The architecture follows best practices"
- "The system is well-structured"

WRONG - Explaining concepts:
- "A layer is a horizontal slice of functionality"
- "Microservices are small independent services"

CORRECT - Specific observations:
- "Layered architecture: src/api/ → src/domain/ → src/storage/ dependency flow confirmed by imports"
- "Boundary violation: storage/cache.rs imports from api/handlers.rs, bypassing domain layer"
- "Layer 'infrastructure' lacks clear boundary: mixes HTTP (server.rs) with database (db.rs)"
</ANTI-PATTERNS>
"#,
            context.profile.name,
            context.profile.purposes.join(", "),
            context.profile.technical_traits.join(", "),
            file_summary
        )
    }

    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "description": "Project architecture analysis identifying patterns, layers, and dependency violations. All observations MUST reference specific files.",
            "required": ["architecture_pattern", "layers"],
            "additionalProperties": false,
            "properties": {
                "architecture_pattern": {
                    "type": "string",
                    "description": "Overall architecture pattern with evidence",
                    "minLength": 5,
                    "maxLength": 100
                },
                "pattern_rationale": {
                    "type": "string",
                    "description": "Why this pattern was identified from file structure",
                    "minLength": 20,
                    "maxLength": 500
                },
                "layers": {
                    "type": "array",
                    "description": "Logical layers in the architecture",
                    "minItems": 1,
                    "maxItems": 15,
                    "items": {
                        "type": "object",
                        "required": ["name", "responsibility", "files"],
                        "additionalProperties": false,
                        "properties": {
                            "name": {"type": "string", "description": "Layer name", "minLength": 1, "maxLength": 50},
                            "responsibility": {"type": "string", "description": "What this layer handles", "minLength": 10, "maxLength": 300},
                            "files": {"type": "array", "description": "Files in this layer", "minItems": 1, "maxItems": 100, "items": {"type": "string", "minLength": 1, "maxLength": 300}},
                            "dependencies": {"type": "array", "description": "Layers this depends on", "maxItems": 10, "items": {"type": "string", "minLength": 1, "maxLength": 50}}
                        }
                    }
                },
                "boundary_violations": {
                    "type": "array",
                    "description": "Dependency direction violations between layers",
                    "maxItems": 30,
                    "items": {
                        "type": "object",
                        "required": ["from_layer", "to_layer", "file", "description"],
                        "additionalProperties": false,
                        "properties": {
                            "from_layer": {"type": "string", "description": "Layer making the violation", "minLength": 1, "maxLength": 50},
                            "to_layer": {"type": "string", "description": "Layer incorrectly depended on", "minLength": 1, "maxLength": 50},
                            "file": {"type": "string", "description": "File where violation occurs", "minLength": 1, "maxLength": 300},
                            "description": {"type": "string", "description": "Concrete explanation with evidence", "minLength": 10, "maxLength": 500}
                        }
                    }
                }
            }
        })
    }

    fn parse_result(result: &serde_json::Value, insight: &mut ProjectInsight) {
        insight.architecture_pattern = json_string(result, "architecture_pattern");

        insight.layers = result
            .get("layers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(Layer {
                            name: json_string(v, "name")?,
                            files: json_string_array(v, "files"),
                            dependencies: json_string_array(v, "dependencies"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        insight.boundary_violations = result
            .get("boundary_violations")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(BoundaryViolation {
                            from_layer: json_string(v, "from_layer")?,
                            to_layer: json_string(v, "to_layer")?,
                            file: json_string(v, "file")?,
                            description: json_string_or(v, "description", ""),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
}
