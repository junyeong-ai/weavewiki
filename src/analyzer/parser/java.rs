use chrono::Utc;
use tree_sitter::{Parser as TsParser, Query, QueryCursor, StreamingIterator};

use super::{Language, ParseResult, Parser, create_file_node};
use crate::types::{
    Edge, EdgeMetadata, EdgeType, EvidenceLocation, FunctionSignature, ImportType, InformationTier,
    Node, NodeMetadata, NodeStatus, NodeType, Parameter, Result, Visibility, WeaveError,
};

pub struct JavaParser;

impl JavaParser {
    pub fn new() -> Result<Self> {
        let mut parser = TsParser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set Java language: {}", e),
                path: String::new(),
            })?;
        Ok(Self)
    }
}

impl Parser for JavaParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult> {
        let mut parser = TsParser::new();
        let language = tree_sitter_java::LANGUAGE;
        parser
            .set_language(&language.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set Java language: {}", e),
                path: path.to_string(),
            })?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| WeaveError::Parse {
                message: "Failed to parse Java file".to_string(),
                path: path.to_string(),
            })?;

        let mut result = ParseResult::new();
        let root = tree.root_node();

        let file_node = create_file_node(path);
        result.nodes.push(file_node);

        extract_package(root, content, path, &mut result);
        extract_imports(root, content, path, &mut result);
        extract_classes(root, content, path, &mut result);
        extract_interfaces(root, content, path, &mut result);
        extract_enums(root, content, path, &mut result);
        extract_methods(root, content, path, &mut result);

        Ok(result)
    }

    fn language(&self) -> Language {
        Language::Java
    }
}

fn extract_package(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (package_declaration
            (scoped_identifier) @package
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_java::LANGUAGE.into(), query_str) {
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
        (import_declaration
            (scoped_identifier) @import
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_java::LANGUAGE.into(), query_str) {
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
            name: (identifier) @name
            superclass: (superclass (type_identifier) @superclass)?
            interfaces: (super_interfaces (type_list (type_identifier) @interface))?
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_java::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            let mut class_name = String::new();
            let mut superclass = None;
            let mut interfaces = Vec::new();
            let mut node_pos = None;

            for cap in m.captures.iter() {
                let node = cap.node;
                let text = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                match cap.index {
                    0 => {
                        class_name = text;
                        node_pos = Some(node);
                    }
                    1 => superclass = Some(text),
                    2 => interfaces.push(text),
                    _ => {}
                }
            }

            if class_name.is_empty() {
                continue;
            }

            if let Some(node) = node_pos {
                let class_node = Node {
                    id: format!("class:{}:{}", path, class_name),
                    node_type: NodeType::Class,
                    path: path.to_string(),
                    name: class_name.clone(),
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
                result.nodes.push(class_node);

                if let Some(ref parent) = superclass {
                    let edge = Edge {
                        id: format!("extends:{}:{}:{}", path, class_name, parent),
                        edge_type: EdgeType::Extends,
                        source_id: format!("class:{}:{}", path, class_name),
                        target_id: format!("class:{}", parent),
                        metadata: EdgeMetadata::default(),
                        evidence: EvidenceLocation {
                            file: path.to_string(),
                            start_line: node.start_position().row as u32 + 1,
                            end_line: node.end_position().row as u32 + 1,
                            start_column: None,
                            end_column: None,
                        },
                        tier: InformationTier::Fact,
                        confidence: 1.0,
                        last_verified: Utc::now(),
                    };
                    result.edges.push(edge);
                }

                for iface in &interfaces {
                    let edge = Edge {
                        id: format!("implements:{}:{}:{}", path, class_name, iface),
                        edge_type: EdgeType::Implements,
                        source_id: format!("class:{}:{}", path, class_name),
                        target_id: format!("interface:{}", iface),
                        metadata: EdgeMetadata::default(),
                        evidence: EvidenceLocation {
                            file: path.to_string(),
                            start_line: node.start_position().row as u32 + 1,
                            end_line: node.end_position().row as u32 + 1,
                            start_column: None,
                            end_column: None,
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
}

fn extract_interfaces(
    root: tree_sitter::Node,
    content: &str,
    path: &str,
    result: &mut ParseResult,
) {
    let query_str = r#"
        (interface_declaration
            name: (identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_java::LANGUAGE.into(), query_str) {
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

fn extract_enums(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (enum_declaration
            name: (identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_java::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                let enum_node = Node {
                    id: format!("enum:{}:{}", path, name),
                    node_type: NodeType::Enum,
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
                result.nodes.push(enum_node);
            }
        }
    }
}

fn extract_methods(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (method_declaration
            name: (identifier) @name
            parameters: (formal_parameters) @params
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_java::LANGUAGE.into(), query_str) {
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

            let params = parse_java_parameters(&params_text);

            let method_node = Node {
                id: format!("method:{}:{}", path, name),
                node_type: NodeType::Method,
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

            result.nodes.push(method_node);
        }
    }
}

fn parse_java_parameters(params_text: &str) -> Vec<Parameter> {
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

            let parts: Vec<&str> = p.split_whitespace().collect();
            if parts.len() < 2 {
                return None;
            }

            let param_type = Some(parts[..parts.len() - 1].join(" "));
            let name = parts.last()?.to_string();

            Some(Parameter {
                name,
                param_type,
                optional: false,
                default_value: None,
            })
        })
        .collect()
}
