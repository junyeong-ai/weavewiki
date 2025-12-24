use chrono::Utc;
use tree_sitter::{Parser as TsParser, Query, QueryCursor, StreamingIterator};

use super::{Language, ParseResult, Parser, create_file_node};
use crate::types::{
    Edge, EdgeMetadata, EdgeType, EvidenceLocation, FunctionSignature, ImportType, InformationTier,
    Node, NodeMetadata, NodeStatus, NodeType, Parameter, Result, Visibility, WeaveError,
};

pub struct KotlinParser;

impl KotlinParser {
    pub fn new() -> Result<Self> {
        let mut parser = TsParser::new();
        parser
            .set_language(&tree_sitter_kotlin_sg::LANGUAGE.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set Kotlin language: {}", e),
                path: String::new(),
            })?;
        Ok(Self)
    }
}

impl Parser for KotlinParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult> {
        let mut parser = TsParser::new();
        let language = tree_sitter_kotlin_sg::LANGUAGE;
        parser
            .set_language(&language.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set Kotlin language: {}", e),
                path: path.to_string(),
            })?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| WeaveError::Parse {
                message: "Failed to parse Kotlin file".to_string(),
                path: path.to_string(),
            })?;

        let mut result = ParseResult::new();
        let root = tree.root_node();

        let file_node = create_file_node(path);
        result.nodes.push(file_node);

        extract_package(root, content, path, &mut result);
        extract_imports(root, content, path, &mut result);
        extract_classes(root, content, path, &mut result);
        extract_objects(root, content, path, &mut result);
        extract_interfaces(root, content, path, &mut result);
        extract_functions(root, content, path, &mut result);

        Ok(result)
    }

    fn language(&self) -> Language {
        Language::Kotlin
    }
}

fn extract_package(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (package_header
            (identifier) @package
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_kotlin_sg::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let pkg_name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                let pkg_node = Node {
                    id: format!("package:{}:{}", path, pkg_name),
                    node_type: NodeType::Module,
                    path: path.to_string(),
                    name: pkg_name,
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
                result.nodes.push(pkg_node);
            }
        }
    }
}

fn extract_imports(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (import_header
            (identifier) @import
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_kotlin_sg::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let import_path = node.utf8_text(content.as_bytes()).unwrap_or("");

                let edge = Edge {
                    id: format!("import:{}:{}", path, import_path),
                    edge_type: EdgeType::DependsOn,
                    source_id: format!("file:{}", path),
                    target_id: format!("class:{}", import_path),
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
        (class_declaration
            (type_identifier) @name
            (delegation_specifier
                (user_type (type_identifier) @parent)
            )?
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_kotlin_sg::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut parents: Vec<String> = Vec::new();
            let mut start_pos = (0usize, 0usize);
            let mut end_pos = (0usize, 0usize);

            for cap in m.captures.iter() {
                let node = cap.node;
                let text = node.utf8_text(content.as_bytes()).unwrap_or("");

                if cap.index == 0 {
                    // class name
                    name = text.to_string();
                    start_pos = (node.start_position().row, node.start_position().column);
                    end_pos = (node.end_position().row, node.end_position().column);
                } else if cap.index == 1 {
                    // parent class/interface
                    parents.push(text.to_string());
                }
            }

            if name.is_empty() {
                continue;
            }

            let extends = parents.first().cloned();
            let implements: Option<Vec<String>> = if parents.len() > 1 {
                Some(parents[1..].to_vec())
            } else {
                None
            };

            let class_node = Node {
                id: format!("class:{}:{}", path, name),
                node_type: NodeType::Class,
                path: path.to_string(),
                name: name.clone(),
                metadata: NodeMetadata {
                    visibility: Some(Visibility::Public),
                    extends,
                    implements,
                    ..Default::default()
                },
                evidence: EvidenceLocation {
                    file: path.to_string(),
                    start_line: start_pos.0 as u32 + 1,
                    end_line: end_pos.0 as u32 + 1,
                    start_column: Some(start_pos.1 as u32),
                    end_column: Some(end_pos.1 as u32),
                },
                tier: InformationTier::Fact,
                confidence: 1.0,
                last_verified: Utc::now(),
                status: NodeStatus::Verified,
            };
            result.nodes.push(class_node);

            // Create inheritance edges
            for parent in &parents {
                let extends_edge = Edge {
                    id: format!("extends:{}:{}:{}", path, name, parent),
                    edge_type: EdgeType::Extends,
                    source_id: format!("class:{}:{}", path, name),
                    target_id: format!("class:{}", parent),
                    metadata: EdgeMetadata::default(),
                    evidence: EvidenceLocation {
                        file: path.to_string(),
                        start_line: start_pos.0 as u32 + 1,
                        end_line: end_pos.0 as u32 + 1,
                        start_column: None,
                        end_column: None,
                    },
                    tier: InformationTier::Fact,
                    confidence: 1.0,
                    last_verified: Utc::now(),
                };
                result.edges.push(extends_edge);
            }
        }
    }
}

fn extract_objects(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (object_declaration
            (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_kotlin_sg::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                let obj_node = Node {
                    id: format!("object:{}:{}", path, name),
                    node_type: NodeType::Class,
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
                result.nodes.push(obj_node);
            }
        }
    }
}

fn extract_interfaces(
    root: tree_sitter::Node,
    content: &str,
    path: &str,
    result: &mut ParseResult,
) {
    let query_str = r#"
        (interface_declaration
            (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_kotlin_sg::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                let iface_node = Node {
                    id: format!("interface:{}:{}", path, name),
                    node_type: NodeType::Interface,
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
                result.nodes.push(iface_node);
            }
        }
    }
}

fn extract_functions(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (function_declaration
            (simple_identifier) @name
            (function_value_parameters) @params
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_kotlin_sg::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut params_text = String::new();
            let mut start_pos = (0u32, 0u32);
            let mut end_pos = (0u32, 0u32);
            let mut parent_node = None;

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
                    parent_node = node.parent();
                } else if cap.index == 1 {
                    params_text = text.to_string();
                }
            }

            if name.is_empty() {
                continue;
            }

            let is_suspend = parent_node
                .map(|p| {
                    p.utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .contains("suspend fun")
                })
                .unwrap_or(false);

            let params = parse_kotlin_parameters(&params_text);

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
                        is_async: is_suspend,
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

fn parse_kotlin_parameters(params_text: &str) -> Vec<Parameter> {
    let inner = params_text.trim_start_matches('(').trim_end_matches(')');
    if inner.is_empty() {
        return Vec::new();
    }

    inner
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }

            let has_default = p.contains('=');
            let parts: Vec<&str> = p.splitn(2, '=').collect();
            let type_parts: Vec<&str> = parts[0].splitn(2, ':').collect();

            let name = type_parts[0]
                .trim()
                .trim_start_matches("val ")
                .trim_start_matches("var ")
                .to_string();
            let param_type = type_parts.get(1).map(|t| t.trim().to_string());
            let optional = has_default
                || param_type
                    .as_ref()
                    .map(|t| t.ends_with('?'))
                    .unwrap_or(false);
            let default_value = if has_default {
                parts.get(1).map(|v| v.trim().to_string())
            } else {
                None
            };

            Some(Parameter {
                name,
                param_type,
                optional,
                default_value,
            })
        })
        .collect()
}
