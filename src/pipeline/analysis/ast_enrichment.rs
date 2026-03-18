//! AST Enrichment Module
//!
//! Uses tree-sitter parsers to extract definitive structural facts from the codebase.
//! These facts serve as ground truth for validating LLM analysis and preventing hallucinations.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::analyzer::parser::{Language, Parser, create_parser_for_path};
use crate::types::{NodeType, Result};

/// Ground-truth facts extracted via AST parsing
#[derive(Debug, Clone, Default)]
pub struct AstFacts {
    /// All functions/methods with their exact locations
    pub functions: HashMap<String, FunctionFact>,
    /// All structs/classes with their locations
    pub types: HashMap<String, TypeFact>,
    /// All traits/interfaces
    pub traits: HashMap<String, TraitFact>,
    /// Module/import dependencies (file -> imported modules)
    pub imports: HashMap<String, Vec<ImportFact>>,
    /// Files that were successfully parsed
    pub parsed_files: HashSet<String>,
    /// Files that failed to parse (for diagnostics)
    pub parse_failures: Vec<ParseFailure>,
}

#[derive(Debug, Clone)]
pub struct FunctionFact {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub visibility: Visibility,
    pub is_async: bool,
    pub parameter_count: usize,
}

#[derive(Debug, Clone)]
pub struct TypeFact {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub kind: TypeKind,
    pub visibility: Visibility,
    pub field_count: usize,
}

#[derive(Debug, Clone)]
pub struct TraitFact {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub method_count: usize,
}

#[derive(Debug, Clone)]
pub struct ImportFact {
    pub target: String,
    pub line: u32,
    pub is_internal: bool,
}

#[derive(Debug, Clone)]
pub struct ParseFailure {
    pub file: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Internal,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Struct,
    Enum,
    Class,
    Interface,
    TypeAlias, // TypeScript type alias (e.g., type Foo = ...)
    Module,    // Namespace/module
}

impl std::fmt::Display for TypeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl AstFacts {
    /// Check if a function exists at the claimed location
    pub fn validate_function_reference(&self, name: &str, file: &str, line: u32) -> AstValidation {
        let key = format!("{}:{}", file, name);

        if let Some(fact) = self.functions.get(&key) {
            if fact.line == line {
                return AstValidation::Exact;
            }
            // Allow small line number variance (within 5 lines)
            if (fact.line as i32 - line as i32).abs() <= 5 {
                return AstValidation::Close {
                    actual_line: fact.line,
                };
            }
            return AstValidation::WrongLine {
                actual_line: fact.line,
            };
        }

        // Check if function exists anywhere in the file
        for (k, fact) in &self.functions {
            if k.ends_with(&format!(":{}", name)) && fact.file == file {
                return AstValidation::WrongLine {
                    actual_line: fact.line,
                };
            }
        }

        // Check if function exists in a different file
        for fact in self.functions.values() {
            if fact.name == name {
                return AstValidation::WrongFile {
                    actual_file: fact.file.clone(),
                };
            }
        }

        AstValidation::NotFound
    }

    /// Check if a type (struct/enum/class) exists at the claimed location
    pub fn validate_type_reference(&self, name: &str, file: &str, line: u32) -> AstValidation {
        let key = format!("{}:{}", file, name);

        if let Some(fact) = self.types.get(&key) {
            if fact.line == line {
                return AstValidation::Exact;
            }
            if (fact.line as i32 - line as i32).abs() <= 5 {
                return AstValidation::Close {
                    actual_line: fact.line,
                };
            }
            return AstValidation::WrongLine {
                actual_line: fact.line,
            };
        }

        for fact in self.types.values() {
            if fact.name == name {
                return AstValidation::WrongFile {
                    actual_file: fact.file.clone(),
                };
            }
        }

        AstValidation::NotFound
    }

    /// Get all public functions for a file
    pub fn public_functions_in(&self, file: &str) -> Vec<&FunctionFact> {
        self.functions
            .values()
            .filter(|f| f.file == file && f.visibility == Visibility::Public)
            .collect()
    }

    /// Get all public types for a file
    pub fn public_types_in(&self, file: &str) -> Vec<&TypeFact> {
        self.types
            .values()
            .filter(|t| t.file == file && t.visibility == Visibility::Public)
            .collect()
    }

    /// Get internal imports (crate::, super::) for dependency analysis
    pub fn internal_imports(&self) -> Vec<(&String, &ImportFact)> {
        self.imports
            .iter()
            .flat_map(|(file, imports)| {
                imports
                    .iter()
                    .filter(|i| i.is_internal)
                    .map(move |i| (file, i))
            })
            .collect()
    }

    /// Merge facts from another AstFacts instance
    pub fn merge(&mut self, other: AstFacts) {
        self.functions.extend(other.functions);
        self.types.extend(other.types);
        self.traits.extend(other.traits);
        for (file, imports) in other.imports {
            self.imports.entry(file).or_default().extend(imports);
        }
        self.parsed_files.extend(other.parsed_files);
        self.parse_failures.extend(other.parse_failures);
    }

    /// Get statistics about the parsed codebase
    pub fn stats(&self) -> AstStats {
        AstStats {
            files_parsed: self.parsed_files.len(),
            files_failed: self.parse_failures.len(),
            total_functions: self.functions.len(),
            total_types: self.types.len(),
            total_traits: self.traits.len(),
            public_functions: self
                .functions
                .values()
                .filter(|f| f.visibility == Visibility::Public)
                .count(),
            async_functions: self.functions.values().filter(|f| f.is_async).count(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AstStats {
    pub files_parsed: usize,
    pub files_failed: usize,
    pub total_functions: usize,
    pub total_types: usize,
    pub total_traits: usize,
    pub public_functions: usize,
    pub async_functions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstValidation {
    /// Reference is exactly correct
    Exact,
    /// Reference is close (within 5 lines)
    Close { actual_line: u32 },
    /// Function/type exists but at different line
    WrongLine { actual_line: u32 },
    /// Function/type exists but in different file
    WrongFile { actual_file: String },
    /// Function/type not found anywhere
    NotFound,
}

impl AstValidation {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Exact | Self::Close { .. })
    }

    pub fn corrected_line(&self) -> Option<u32> {
        match self {
            Self::Close { actual_line } | Self::WrongLine { actual_line } => Some(*actual_line),
            _ => None,
        }
    }
}

/// AST Enrichment Engine
///
/// Scans the codebase using tree-sitter parsers to extract ground-truth facts
/// that can be used to validate and enrich LLM analysis.
pub struct AstEnricher;

impl AstEnricher {
    /// Extract AST facts from all parseable files
    pub async fn extract_facts(
        project_root: &Path,
        files: impl Iterator<Item = &String>,
    ) -> AstFacts {
        let mut facts = AstFacts::default();

        for file_path in files {
            let full_path = project_root.join(file_path);

            // Skip non-source files
            let lang = Language::from_path(file_path);
            if !lang.has_parser_support() {
                continue;
            }

            // Read file content
            let content = match tokio::fs::read_to_string(&full_path).await {
                Ok(c) => c,
                Err(e) => {
                    facts.parse_failures.push(ParseFailure {
                        file: file_path.clone(),
                        reason: format!("Read error: {}", e),
                    });
                    continue;
                }
            };

            // Parse with tree-sitter
            if let Some(parser) = create_parser_for_path(file_path) {
                match Self::extract_file_facts(file_path, &content, parser.as_ref()) {
                    Ok(file_facts) => {
                        facts.merge(file_facts);
                        facts.parsed_files.insert(file_path.clone());
                    }
                    Err(e) => {
                        facts.parse_failures.push(ParseFailure {
                            file: file_path.clone(),
                            reason: e.to_string(),
                        });
                    }
                }
            }
        }

        let stats = facts.stats();
        tracing::info!(
            files_parsed = stats.files_parsed,
            functions = stats.total_functions,
            types = stats.total_types,
            "AST extraction complete"
        );

        facts
    }

    fn extract_file_facts(file_path: &str, content: &str, parser: &dyn Parser) -> Result<AstFacts> {
        let parse_result = parser.parse(file_path, content)?;
        let mut facts = AstFacts::default();

        for node in &parse_result.nodes {
            match node.node_type {
                NodeType::Function => {
                    let vis = match node.metadata.visibility {
                        Some(crate::types::Visibility::Public) => Visibility::Public,
                        Some(crate::types::Visibility::Internal) => Visibility::Internal,
                        _ => Visibility::Private,
                    };

                    let is_async = node
                        .metadata
                        .signature
                        .as_ref()
                        .map(|s| s.is_async)
                        .unwrap_or(false);

                    let param_count = node
                        .metadata
                        .signature
                        .as_ref()
                        .map(|s| s.parameters.len())
                        .unwrap_or(0);

                    let key = format!("{}:{}", file_path, node.name);
                    facts.functions.insert(
                        key,
                        FunctionFact {
                            name: node.name.clone(),
                            file: file_path.to_string(),
                            line: node.evidence.start_line,
                            visibility: vis,
                            is_async,
                            parameter_count: param_count,
                        },
                    );
                }
                NodeType::Class => {
                    let vis = match node.metadata.visibility {
                        Some(crate::types::Visibility::Public) => Visibility::Public,
                        Some(crate::types::Visibility::Internal) => Visibility::Internal,
                        _ => Visibility::Private,
                    };

                    let key = format!("{}:{}", file_path, node.name);
                    facts.types.insert(
                        key,
                        TypeFact {
                            name: node.name.clone(),
                            file: file_path.to_string(),
                            line: node.evidence.start_line,
                            kind: TypeKind::Class,
                            visibility: vis,
                            field_count: 0,
                        },
                    );
                }
                NodeType::Enum => {
                    let vis = match node.metadata.visibility {
                        Some(crate::types::Visibility::Public) => Visibility::Public,
                        Some(crate::types::Visibility::Internal) => Visibility::Internal,
                        _ => Visibility::Private,
                    };

                    let key = format!("{}:{}", file_path, node.name);
                    facts.types.insert(
                        key,
                        TypeFact {
                            name: node.name.clone(),
                            file: file_path.to_string(),
                            line: node.evidence.start_line,
                            kind: TypeKind::Enum,
                            visibility: vis,
                            field_count: 0,
                        },
                    );
                }
                NodeType::Interface => {
                    let key = format!("{}:{}", file_path, node.name);
                    facts.traits.insert(
                        key,
                        TraitFact {
                            name: node.name.clone(),
                            file: file_path.to_string(),
                            line: node.evidence.start_line,
                            method_count: 0,
                        },
                    );
                }
                NodeType::Type => {
                    // TypeScript type alias
                    let vis = match node.metadata.visibility {
                        Some(crate::types::Visibility::Public) => Visibility::Public,
                        Some(crate::types::Visibility::Internal) => Visibility::Internal,
                        _ => Visibility::Private,
                    };

                    let key = format!("{}:{}", file_path, node.name);
                    facts.types.insert(
                        key,
                        TypeFact {
                            name: node.name.clone(),
                            file: file_path.to_string(),
                            line: node.evidence.start_line,
                            kind: TypeKind::TypeAlias,
                            visibility: vis,
                            field_count: 0,
                        },
                    );
                }
                NodeType::Module => {
                    // Namespace/module
                    let vis = match node.metadata.visibility {
                        Some(crate::types::Visibility::Public) => Visibility::Public,
                        Some(crate::types::Visibility::Internal) => Visibility::Internal,
                        _ => Visibility::Private,
                    };

                    let key = format!("{}:{}", file_path, node.name);
                    facts.types.insert(
                        key,
                        TypeFact {
                            name: node.name.clone(),
                            file: file_path.to_string(),
                            line: node.evidence.start_line,
                            kind: TypeKind::Module,
                            visibility: vis,
                            field_count: 0,
                        },
                    );
                }
                _ => {}
            }
        }

        // Extract imports from edges
        for edge in &parse_result.edges {
            if edge.edge_type == crate::types::EdgeType::DependsOn {
                let is_internal =
                    edge.target_id.contains("crate::") || edge.target_id.contains("super::");

                facts
                    .imports
                    .entry(file_path.to_string())
                    .or_default()
                    .push(ImportFact {
                        target: edge.target_id.clone(),
                        line: edge.evidence.start_line,
                        is_internal,
                    });
            }
        }

        Ok(facts)
    }
}

/// Reference validator using AST facts
pub struct AstValidator<'a> {
    facts: &'a AstFacts,
}

impl<'a> AstValidator<'a> {
    pub fn new(facts: &'a AstFacts) -> Self {
        Self { facts }
    }

    /// Validate a @file:line reference and return correction if needed
    pub fn validate_reference(&self, reference: &str) -> ReferenceCheck {
        // Parse reference format: @path/to/file.rs:line
        let reference = reference.trim_start_matches('@');

        let (path, line) = if let Some(pos) = reference.rfind(':') {
            let (p, l) = reference.split_at(pos);
            let line_num = l[1..].parse::<u32>().ok();
            (p, line_num)
        } else {
            (reference, None)
        };

        // Check if file was parsed
        if !self.facts.parsed_files.contains(path) {
            return ReferenceCheck::UnverifiedFile;
        }

        // If no line number, just validate file exists
        let Some(line) = line else {
            return ReferenceCheck::Valid;
        };

        // Check if any function/type at this line
        for fact in self.facts.functions.values() {
            if fact.file == path && (fact.line as i32 - line as i32).abs() <= 5 {
                return ReferenceCheck::ValidWithHint {
                    hint: format!("Function '{}' at line {}", fact.name, fact.line),
                };
            }
        }

        for fact in self.facts.types.values() {
            if fact.file == path && (fact.line as i32 - line as i32).abs() <= 5 {
                return ReferenceCheck::ValidWithHint {
                    hint: format!("Type '{}' at line {}", fact.name, fact.line),
                };
            }
        }

        // Line exists but doesn't match known symbol
        ReferenceCheck::Valid
    }
}

#[derive(Debug, Clone)]
pub enum ReferenceCheck {
    /// Reference is valid
    Valid,
    /// Reference is valid and matches a known symbol
    ValidWithHint { hint: String },
    /// File was not parsed (can't verify)
    UnverifiedFile,
    /// Reference appears to be hallucinated
    LikelyHallucination { reason: String },
}

impl ReferenceCheck {
    pub fn is_valid(&self) -> bool {
        matches!(
            self,
            Self::Valid | Self::ValidWithHint { .. } | Self::UnverifiedFile
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_is_valid() {
        assert!(AstValidation::Exact.is_valid());
        assert!(AstValidation::Close { actual_line: 42 }.is_valid());
        assert!(!AstValidation::WrongLine { actual_line: 100 }.is_valid());
        assert!(!AstValidation::NotFound.is_valid());
    }

    #[test]
    fn test_ast_facts_merge() {
        let mut facts1 = AstFacts::default();
        facts1.functions.insert(
            "file1.rs:func1".to_string(),
            FunctionFact {
                name: "func1".to_string(),
                file: "file1.rs".to_string(),
                line: 10,
                visibility: Visibility::Public,
                is_async: false,
                parameter_count: 2,
            },
        );

        let mut facts2 = AstFacts::default();
        facts2.functions.insert(
            "file2.rs:func2".to_string(),
            FunctionFact {
                name: "func2".to_string(),
                file: "file2.rs".to_string(),
                line: 20,
                visibility: Visibility::Private,
                is_async: true,
                parameter_count: 1,
            },
        );

        facts1.merge(facts2);

        assert_eq!(facts1.functions.len(), 2);
        assert!(facts1.functions.contains_key("file1.rs:func1"));
        assert!(facts1.functions.contains_key("file2.rs:func2"));
    }

    #[test]
    fn test_validate_function_reference_exact() {
        let mut facts = AstFacts::default();
        facts.functions.insert(
            "src/main.rs:main".to_string(),
            FunctionFact {
                name: "main".to_string(),
                file: "src/main.rs".to_string(),
                line: 15,
                visibility: Visibility::Public,
                is_async: false,
                parameter_count: 0,
            },
        );

        let result = facts.validate_function_reference("main", "src/main.rs", 15);
        assert_eq!(result, AstValidation::Exact);
    }

    #[test]
    fn test_validate_function_reference_close() {
        let mut facts = AstFacts::default();
        facts.functions.insert(
            "src/main.rs:main".to_string(),
            FunctionFact {
                name: "main".to_string(),
                file: "src/main.rs".to_string(),
                line: 15,
                visibility: Visibility::Public,
                is_async: false,
                parameter_count: 0,
            },
        );

        let result = facts.validate_function_reference("main", "src/main.rs", 17);
        assert!(matches!(result, AstValidation::Close { actual_line: 15 }));
    }

    #[test]
    fn test_ast_stats() {
        let mut facts = AstFacts::default();
        facts.parsed_files.insert("file1.rs".to_string());
        facts.functions.insert(
            "file1.rs:pub_fn".to_string(),
            FunctionFact {
                name: "pub_fn".to_string(),
                file: "file1.rs".to_string(),
                line: 10,
                visibility: Visibility::Public,
                is_async: true,
                parameter_count: 2,
            },
        );
        facts.functions.insert(
            "file1.rs:priv_fn".to_string(),
            FunctionFact {
                name: "priv_fn".to_string(),
                file: "file1.rs".to_string(),
                line: 20,
                visibility: Visibility::Private,
                is_async: false,
                parameter_count: 0,
            },
        );

        let stats = facts.stats();
        assert_eq!(stats.files_parsed, 1);
        assert_eq!(stats.total_functions, 2);
        assert_eq!(stats.public_functions, 1);
        assert_eq!(stats.async_functions, 1);
    }
}
