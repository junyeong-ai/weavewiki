//! Graph Context for Parser-Enriched Prompts
//!
//! Provides structural facts from the Knowledge Graph (parser-extracted)
//! to enhance LLM analysis prompts. This enables the LLM to focus on
//! semantic analysis rather than rediscovering structural information.
//!
//! ## Design Principles
//!
//! 1. **Fact vs Inference Separation**: Only query tier=Fact nodes (parser-derived)
//! 2. **Minimal Serialization**: Format data for prompt inclusion, not storage
//! 3. **Efficient Queries**: Single query per file, batch-friendly

use crate::storage::Database;
use crate::types::Node;
use crate::types::node::{NodeType, Visibility};

/// Structural context for a single file, ready for prompt injection
#[derive(Debug, Clone, Default)]
pub struct FileStructuralContext {
    /// Functions declared in this file
    pub functions: Vec<FunctionFact>,
    /// Structs/Classes declared in this file
    pub structs: Vec<TypeFact>,
    /// Enums declared in this file
    pub enums: Vec<TypeFact>,
    /// Traits/Interfaces declared in this file
    pub traits: Vec<TypeFact>,
    /// Internal dependencies (crate::/super:: imports)
    pub internal_deps: Vec<DependencyFact>,
    /// Trait implementations
    pub implements: Vec<ImplementsFact>,
}

impl FileStructuralContext {
    /// Check if context has any structural data
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
            && self.structs.is_empty()
            && self.enums.is_empty()
            && self.traits.is_empty()
            && self.internal_deps.is_empty()
            && self.implements.is_empty()
    }

    /// Format context for prompt inclusion
    pub fn to_prompt_section(&self) -> String {
        if self.is_empty() {
            return String::new();
        }

        let mut output = String::new();
        output.push_str("\n## Structural Facts (Parser-Extracted)\n");
        output.push_str("These facts are already extracted by the parser. Use them to inform your analysis:\n\n");

        // Functions
        if !self.functions.is_empty() {
            output.push_str("### Functions\n");
            for f in &self.functions {
                let visibility = match f.visibility {
                    Visibility::Public => "pub",
                    Visibility::Private => "priv",
                    Visibility::Protected => "protected",
                    Visibility::Internal => "pub(crate)",
                };
                let async_marker = if f.is_async { " async" } else { "" };
                output.push_str(&format!(
                    "- `{}{}fn {}({})` [line {}]\n",
                    visibility, async_marker, f.name, f.params_summary, f.line
                ));
            }
            output.push('\n');
        }

        // Structs
        if !self.structs.is_empty() {
            output.push_str("### Structs\n");
            for s in &self.structs {
                let visibility = match s.visibility {
                    Visibility::Public => "pub",
                    Visibility::Private => "priv",
                    Visibility::Protected => "protected",
                    Visibility::Internal => "pub(crate)",
                };
                output.push_str(&format!(
                    "- `{} struct {}` [line {}]\n",
                    visibility, s.name, s.line
                ));
            }
            output.push('\n');
        }

        // Enums
        if !self.enums.is_empty() {
            output.push_str("### Enums\n");
            for e in &self.enums {
                let visibility = match e.visibility {
                    Visibility::Public => "pub",
                    Visibility::Private => "priv",
                    Visibility::Protected => "protected",
                    Visibility::Internal => "pub(crate)",
                };
                output.push_str(&format!(
                    "- `{} enum {}` [line {}]\n",
                    visibility, e.name, e.line
                ));
            }
            output.push('\n');
        }

        // Traits
        if !self.traits.is_empty() {
            output.push_str("### Traits\n");
            for t in &self.traits {
                output.push_str(&format!("- `trait {}` [line {}]\n", t.name, t.line));
            }
            output.push('\n');
        }

        // Dependencies
        if !self.internal_deps.is_empty() {
            output.push_str("### Internal Dependencies\n");
            for d in &self.internal_deps {
                output.push_str(&format!("- `{}` ({})\n", d.target, d.dep_type));
            }
            output.push('\n');
        }

        // Implementations
        if !self.implements.is_empty() {
            output.push_str("### Trait Implementations\n");
            for i in &self.implements {
                output.push_str(&format!("- `impl {} for {}`\n", i.trait_name, i.type_name));
            }
            output.push('\n');
        }

        output
    }
}

/// Function fact from parser
#[derive(Debug, Clone)]
pub struct FunctionFact {
    pub name: String,
    pub params_summary: String,
    pub visibility: Visibility,
    pub is_async: bool,
    pub line: u32,
}

/// Type fact (struct/enum/trait) from parser
#[derive(Debug, Clone)]
pub struct TypeFact {
    pub name: String,
    pub visibility: Visibility,
    pub line: u32,
}

/// Dependency fact from parser
#[derive(Debug, Clone)]
pub struct DependencyFact {
    pub target: String,
    pub dep_type: String,
}

/// Trait implementation fact from parser
#[derive(Debug, Clone)]
pub struct ImplementsFact {
    pub type_name: String,
    pub trait_name: String,
}

/// Query structural context from the Knowledge Graph
pub struct GraphContextProvider<'a> {
    db: &'a Database,
}

impl<'a> GraphContextProvider<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Get structural context for a file
    pub fn get_file_context(&self, file_path: &str) -> FileStructuralContext {
        let mut ctx = FileStructuralContext::default();

        // Get all fact-tier nodes for this file
        let nodes = match self.db.get_file_structural_nodes(file_path) {
            Ok(nodes) => nodes,
            Err(e) => {
                tracing::warn!("Failed to get structural nodes for {}: {}", file_path, e);
                return ctx;
            }
        };

        // Categorize nodes by type
        for node in nodes {
            match node.node_type {
                NodeType::Function | NodeType::Method => {
                    ctx.functions.push(self.node_to_function_fact(&node));
                }
                NodeType::Class => {
                    ctx.structs.push(self.node_to_type_fact(&node));
                }
                NodeType::Enum => {
                    ctx.enums.push(self.node_to_type_fact(&node));
                }
                NodeType::Interface => {
                    ctx.traits.push(self.node_to_type_fact(&node));
                }
                _ => {}
            }
        }

        // Get dependencies
        if let Ok(deps) = self.db.get_file_dependencies(file_path) {
            for (target, dep_type) in deps {
                ctx.internal_deps.push(DependencyFact { target, dep_type });
            }
        }

        ctx
    }

    fn node_to_function_fact(&self, node: &Node) -> FunctionFact {
        let visibility = node.metadata.visibility.unwrap_or(Visibility::Private);

        let is_async = node
            .metadata
            .signature
            .as_ref()
            .map(|s| s.is_async)
            .unwrap_or(false);

        let params_summary = node
            .metadata
            .signature
            .as_ref()
            .map(|s| {
                s.parameters
                    .iter()
                    .map(|p| {
                        if let Some(ref t) = p.param_type {
                            format!("{}: {}", p.name, t)
                        } else {
                            p.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        FunctionFact {
            name: node.name.clone(),
            params_summary,
            visibility,
            is_async,
            line: node.evidence.start_line,
        }
    }

    fn node_to_type_fact(&self, node: &Node) -> TypeFact {
        let visibility = node.metadata.visibility.unwrap_or(Visibility::Private);

        TypeFact {
            name: node.name.clone(),
            visibility,
            line: node.evidence.start_line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_context() {
        let ctx = FileStructuralContext::default();
        assert!(ctx.is_empty());
        assert!(ctx.to_prompt_section().is_empty());
    }

    #[test]
    fn test_context_with_functions() {
        let ctx = FileStructuralContext {
            functions: vec![
                FunctionFact {
                    name: "analyze_file".to_string(),
                    params_summary: "file_path: &str".to_string(),
                    visibility: Visibility::Public,
                    is_async: true,
                    line: 34,
                },
                FunctionFact {
                    name: "parse_result".to_string(),
                    params_summary: "".to_string(),
                    visibility: Visibility::Private,
                    is_async: false,
                    line: 77,
                },
            ],
            ..Default::default()
        };

        assert!(!ctx.is_empty());
        let prompt = ctx.to_prompt_section();
        assert!(prompt.contains("### Functions"));
        assert!(prompt.contains("analyze_file"));
        assert!(prompt.contains("line 34"));
    }

    #[test]
    fn test_context_with_types() {
        let ctx = FileStructuralContext {
            structs: vec![TypeFact {
                name: "FileAnalyzer".to_string(),
                visibility: Visibility::Public,
                line: 23,
            }],
            enums: vec![TypeFact {
                name: "Complexity".to_string(),
                visibility: Visibility::Public,
                line: 10,
            }],
            traits: vec![TypeFact {
                name: "Parser".to_string(),
                visibility: Visibility::Public,
                line: 5,
            }],
            ..Default::default()
        };

        let prompt = ctx.to_prompt_section();
        assert!(prompt.contains("### Structs"));
        assert!(prompt.contains("FileAnalyzer"));
        assert!(prompt.contains("### Enums"));
        assert!(prompt.contains("Complexity"));
        assert!(prompt.contains("### Traits"));
        assert!(prompt.contains("Parser"));
    }
}
