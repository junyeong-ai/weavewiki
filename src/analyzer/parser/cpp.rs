//! C++ Language Parser
//!
//! AST-based code analysis for C++ source files.

use chrono::Utc;
use tree_sitter::{Parser as TsParser, Query, QueryCursor, StreamingIterator};

use super::{Language, ParseResult, Parser, create_file_node};
use crate::types::{
    Edge, EdgeMetadata, EdgeType, EvidenceLocation, ImportType, InformationTier, Node,
    NodeMetadata, NodeStatus, NodeType, Result, Visibility, WeaveError,
};

pub struct CppLangParser;

impl CppLangParser {
    pub fn new() -> Result<Self> {
        let mut parser = TsParser::new();
        parser
            .set_language(&tree_sitter_cpp::LANGUAGE.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set C++ language: {}", e),
                path: String::new(),
            })?;
        Ok(Self)
    }
}

impl Parser for CppLangParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult> {
        let mut parser = TsParser::new();
        let language = tree_sitter_cpp::LANGUAGE;
        parser
            .set_language(&language.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set C++ language: {}", e),
                path: path.to_string(),
            })?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| WeaveError::Parse {
                message: "Failed to parse C++ file".to_string(),
                path: path.to_string(),
            })?;

        let mut result = ParseResult::new();
        let root = tree.root_node();

        let file_node = create_file_node(path);
        result.nodes.push(file_node);

        extract_includes(root, content, path, &mut result);
        extract_classes(root, content, path, &mut result);
        extract_structs(root, content, path, &mut result);
        extract_functions(root, content, path, &mut result);
        extract_namespaces(root, content, path, &mut result);

        Ok(result)
    }

    fn language(&self) -> Language {
        Language::Cpp
    }
}

fn extract_includes(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (preproc_include
            path: [
                (string_literal) @path
                (system_lib_string) @path
            ]
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_cpp::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let include_path = node.utf8_text(content.as_bytes()).unwrap_or("");
                let include_path = include_path.trim_matches(|c| c == '"' || c == '<' || c == '>');

                let edge = Edge {
                    id: format!("include:{}:{}", path, include_path),
                    edge_type: EdgeType::DependsOn,
                    source_id: format!("file:{}", path),
                    target_id: format!("header:{}", include_path),
                    metadata: EdgeMetadata {
                        import_type: Some(ImportType::Static),
                        ..Default::default()
                    },
                    evidence: EvidenceLocation {
                        file: path.to_string(),
                        start_line: node.start_position().row as u32 + 1,
                        end_line: node.end_position().row as u32 + 1,
                        start_column: Some(node.start_position().column as u32),
                        end_column: Some(node.end_position().column as u32),
                    },
                    tier: InformationTier::Fact,
                    confidence: 1.0,
                    last_verified: Utc::now(),
                };
                result.edges.push(edge);
            }
        }
    }
}

fn extract_classes(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (class_specifier
            name: (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_cpp::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                let class_node = Node {
                    id: format!("class:{}:{}", path, name),
                    node_type: NodeType::Class,
                    path: path.to_string(),
                    name: name.clone(),
                    metadata: NodeMetadata::default(),
                    evidence: EvidenceLocation {
                        file: path.to_string(),
                        start_line: node.start_position().row as u32 + 1,
                        end_line: node.end_position().row as u32 + 1,
                        start_column: Some(node.start_position().column as u32),
                        end_column: Some(node.end_position().column as u32),
                    },
                    tier: InformationTier::Fact,
                    confidence: 1.0,
                    last_verified: Utc::now(),
                    status: NodeStatus::Verified,
                };
                result.nodes.push(class_node);
            }
        }
    }
}

fn extract_structs(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (struct_specifier
            name: (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_cpp::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                let struct_node = Node {
                    id: format!("struct:{}:{}", path, name),
                    node_type: NodeType::Class,
                    path: path.to_string(),
                    name: name.clone(),
                    metadata: NodeMetadata::default(),
                    evidence: EvidenceLocation {
                        file: path.to_string(),
                        start_line: node.start_position().row as u32 + 1,
                        end_line: node.end_position().row as u32 + 1,
                        start_column: Some(node.start_position().column as u32),
                        end_column: Some(node.end_position().column as u32),
                    },
                    tier: InformationTier::Fact,
                    confidence: 1.0,
                    last_verified: Utc::now(),
                    status: NodeStatus::Verified,
                };
                result.nodes.push(struct_node);
            }
        }
    }
}

fn extract_functions(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (function_definition
            declarator: (function_declarator
                declarator: [
                    (identifier) @name
                    (qualified_identifier) @name
                ]
            )
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_cpp::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                let func_node = Node {
                    id: format!("function:{}:{}", path, name),
                    node_type: NodeType::Function,
                    path: path.to_string(),
                    name: name.clone(),
                    metadata: NodeMetadata {
                        visibility: Some(Visibility::Public),
                        ..Default::default()
                    },
                    evidence: EvidenceLocation {
                        file: path.to_string(),
                        start_line: node.start_position().row as u32 + 1,
                        end_line: node.end_position().row as u32 + 1,
                        start_column: Some(node.start_position().column as u32),
                        end_column: Some(node.end_position().column as u32),
                    },
                    tier: InformationTier::Fact,
                    confidence: 1.0,
                    last_verified: Utc::now(),
                    status: NodeStatus::Verified,
                };
                result.nodes.push(func_node);
            }
        }
    }
}

fn extract_namespaces(
    root: tree_sitter::Node,
    content: &str,
    path: &str,
    result: &mut ParseResult,
) {
    let query_str = r#"
        (namespace_definition
            name: (identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_cpp::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                let ns_node = Node {
                    id: format!("namespace:{}:{}", path, name),
                    node_type: NodeType::Module,
                    path: path.to_string(),
                    name,
                    metadata: NodeMetadata::default(),
                    evidence: EvidenceLocation {
                        file: path.to_string(),
                        start_line: node.start_position().row as u32 + 1,
                        end_line: node.end_position().row as u32 + 1,
                        start_column: Some(node.start_position().column as u32),
                        end_column: Some(node.end_position().column as u32),
                    },
                    tier: InformationTier::Fact,
                    confidence: 1.0,
                    last_verified: Utc::now(),
                    status: NodeStatus::Verified,
                };
                result.nodes.push(ns_node);
            }
        }
    }
}
