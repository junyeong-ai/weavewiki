//! C Language Parser
//!
//! AST-based code analysis for C source files.

use chrono::Utc;
use tree_sitter::{Parser as TsParser, Query, QueryCursor, StreamingIterator};

use super::{Language, ParseResult, Parser, create_file_node};
use crate::types::{
    Edge, EdgeMetadata, EdgeType, EvidenceLocation, FunctionSignature, ImportType, InformationTier,
    Node, NodeMetadata, NodeStatus, NodeType, Parameter, Result, Visibility, WeaveError,
};

pub struct CLangParser;

impl CLangParser {
    pub fn new() -> Result<Self> {
        let mut parser = TsParser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set C language: {}", e),
                path: String::new(),
            })?;
        Ok(Self)
    }
}

impl Parser for CLangParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult> {
        let mut parser = TsParser::new();
        let language = tree_sitter_c::LANGUAGE;
        parser
            .set_language(&language.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set C language: {}", e),
                path: path.to_string(),
            })?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| WeaveError::Parse {
                message: "Failed to parse C file".to_string(),
                path: path.to_string(),
            })?;

        let mut result = ParseResult::new();
        let root = tree.root_node();

        let file_node = create_file_node(path);
        result.nodes.push(file_node);

        extract_includes(root, content, path, &mut result);
        extract_structs(root, content, path, &mut result);
        extract_functions(root, content, path, &mut result);

        Ok(result)
    }

    fn language(&self) -> Language {
        Language::C
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

    if let Ok(query) = Query::new(&tree_sitter_c::LANGUAGE.into(), query_str) {
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

fn extract_structs(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (struct_specifier
            name: (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_c::LANGUAGE.into(), query_str) {
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
                declarator: (identifier) @name
                parameters: (parameter_list) @params
            )
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_c::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut params_text = String::new();
            let mut start_pos = (0u32, 0u32);
            let mut end_pos = (0u32, 0u32);

            for cap in m.captures.iter() {
                let node = cap.node;
                let text = node.utf8_text(content.as_bytes()).unwrap_or("");

                if cap.index == 0 {
                    name = text.to_string();
                    start_pos = (
                        node.start_position().row as u32 + 1,
                        node.start_position().column as u32,
                    );
                    end_pos = (
                        node.end_position().row as u32 + 1,
                        node.end_position().column as u32,
                    );
                } else if cap.index == 1 {
                    params_text = text.to_string();
                }
            }

            if name.is_empty() {
                continue;
            }

            let params = parse_c_parameters(&params_text);

            let func_node = Node {
                id: format!("function:{}:{}", path, name),
                node_type: NodeType::Function,
                path: path.to_string(),
                name: name.clone(),
                metadata: NodeMetadata {
                    visibility: Some(Visibility::Public),
                    signature: Some(FunctionSignature {
                        parameters: params,
                        return_type: None,
                        is_async: false,
                        generator: false,
                    }),
                    ..Default::default()
                },
                evidence: EvidenceLocation {
                    file: path.to_string(),
                    start_line: start_pos.0,
                    end_line: end_pos.0,
                    start_column: Some(start_pos.1),
                    end_column: Some(end_pos.1),
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

fn parse_c_parameters(params_text: &str) -> Vec<Parameter> {
    let inner = params_text.trim_start_matches('(').trim_end_matches(')');
    if inner.is_empty() || inner == "void" {
        return Vec::new();
    }

    inner
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }

            let parts: Vec<&str> = p
                .rsplitn(2, |c: char| c.is_whitespace() || c == '*')
                .collect();
            if parts.is_empty() {
                return None;
            }

            let name = parts[0].trim_start_matches('*').trim().to_string();
            let param_type = if parts.len() > 1 {
                Some(parts[1].trim().to_string())
            } else {
                None
            };

            Some(Parameter {
                name,
                param_type,
                optional: false,
                default_value: None,
            })
        })
        .collect()
}
