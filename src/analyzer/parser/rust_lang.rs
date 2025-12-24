use chrono::Utc;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

use super::{Language, ParseResult, Parser, create_file_node, create_ts_parser, get_node_text};
use crate::types::{
    Edge, EdgeMetadata, EdgeType, EvidenceLocation, FunctionSignature, ImportType, InformationTier,
    Node, NodeId, NodeMetadata, NodeStatus, NodeType, Parameter, Result, Visibility, WeaveError,
};

pub struct RustParser;

impl RustParser {
    pub fn new() -> Result<Self> {
        // Validate parser creation at construction time
        let _ = create_ts_parser(tree_sitter_rust::LANGUAGE, "Rust")?;
        Ok(Self)
    }
}

impl Parser for RustParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult> {
        let mut parser =
            create_ts_parser(tree_sitter_rust::LANGUAGE, "Rust").map_err(|mut e| {
                // Update error path to current file being parsed
                if let crate::types::WeaveError::Parse {
                    path: ref mut p, ..
                } = e
                {
                    *p = path.to_string();
                }
                e
            })?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| WeaveError::Parse {
                message: "Failed to parse Rust file".to_string(),
                path: path.to_string(),
            })?;

        let mut result = ParseResult::new();
        let root = tree.root_node();

        let file_node = create_file_node(path);
        result.nodes.push(file_node);

        extract_use_statements(root, content, path, &mut result);
        extract_mod_declarations(root, content, path, &mut result);
        extract_structs(root, content, path, &mut result);
        extract_enums(root, content, path, &mut result);
        extract_traits(root, content, path, &mut result);
        extract_functions(root, content, path, &mut result);
        extract_impl_blocks(root, content, path, &mut result);

        Ok(result)
    }

    fn language(&self) -> Language {
        Language::Rust
    }
}

fn extract_use_statements(
    root: tree_sitter::Node,
    content: &str,
    path: &str,
    result: &mut ParseResult,
) {
    let query_str = r#"
        (use_declaration
            argument: (scoped_identifier) @path
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_rust::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let use_path = get_node_text(node, content.as_bytes());

                if use_path.starts_with("crate::") || use_path.starts_with("super::") {
                    let edge = Edge {
                        id: format!("use:{}:{}", path, use_path),
                        edge_type: EdgeType::DependsOn,
                        source_id: NodeId::file(path).into_inner(),
                        target_id: NodeId::module(use_path).into_inner(),
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
}

fn extract_mod_declarations(
    root: tree_sitter::Node,
    content: &str,
    path: &str,
    result: &mut ParseResult,
) {
    let query_str = r#"
        (mod_item
            name: (identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_rust::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = get_node_text(node, content.as_bytes()).to_string();

                let mod_node = Node {
                    id: format!("module:{}:{}", path, name),
                    node_type: NodeType::Module,
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
                result.nodes.push(mod_node);
            }
        }
    }
}

fn extract_structs(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (struct_item
            name: (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_rust::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = get_node_text(node, content.as_bytes()).to_string();

                let visibility = detect_visibility(node.parent(), content);

                let struct_node = Node {
                    id: NodeId::class(path, &name).into_inner(),
                    node_type: NodeType::Class,
                    path: path.to_string(),
                    name: name.clone(),
                    metadata: NodeMetadata {
                        visibility: Some(visibility),
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
                result.nodes.push(struct_node);
            }
        }
    }
}

fn extract_enums(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (enum_item
            name: (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_rust::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = get_node_text(node, content.as_bytes()).to_string();

                let visibility = detect_visibility(node.parent(), content);

                let enum_node = Node {
                    id: format!("enum:{}:{}", path, name),
                    node_type: NodeType::Enum,
                    path: path.to_string(),
                    name: name.clone(),
                    metadata: NodeMetadata {
                        visibility: Some(visibility),
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
                result.nodes.push(enum_node);
            }
        }
    }
}

fn extract_traits(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (trait_item
            name: (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_rust::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = get_node_text(node, content.as_bytes()).to_string();

                let trait_node = Node {
                    id: format!("trait:{}:{}", path, name),
                    node_type: NodeType::Interface,
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
                result.nodes.push(trait_node);
            }
        }
    }
}

fn extract_functions(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (function_item
            name: (identifier) @name
            parameters: (parameters) @params
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_rust::LANGUAGE.into(), query_str) {
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
                let text = get_node_text(node, content.as_bytes());

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

            let visibility = detect_visibility(parent_node, content);
            let params = parse_rust_parameters(&params_text);
            let is_async = parent_node
                .map(|p| get_node_text(p, content.as_bytes()).contains("async fn"))
                .unwrap_or(false);

            let func_node = Node {
                id: NodeId::function(path, &name).into_inner(),
                node_type: NodeType::Function,
                path: path.to_string(),
                name: name.clone(),
                metadata: NodeMetadata {
                    visibility: Some(visibility),
                    signature: Some(FunctionSignature {
                        parameters: params,
                        return_type: None,
                        is_async,
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

fn extract_impl_blocks(
    root: tree_sitter::Node,
    content: &str,
    path: &str,
    result: &mut ParseResult,
) {
    let query_str = r#"
        (impl_item
            type: (type_identifier) @type
            trait: (type_identifier)? @trait
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_rust::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            let mut impl_type = String::new();
            let mut impl_trait = None;
            let mut node_ref = None;

            for cap in m.captures.iter() {
                let node = cap.node;
                let text = get_node_text(node, content.as_bytes()).to_string();

                if cap.index == 0 {
                    impl_type = text;
                    node_ref = Some(node);
                } else if cap.index == 1 {
                    impl_trait = Some(text);
                }
            }

            if let (Some(trait_name), Some(node)) = (impl_trait, node_ref) {
                let edge = Edge {
                    id: format!("impl:{}:{}:{}", path, impl_type, trait_name),
                    edge_type: EdgeType::Implements,
                    source_id: NodeId::class(path, &impl_type).into_inner(),
                    target_id: format!("trait:{}", trait_name),
                    metadata: EdgeMetadata::default(),
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

fn detect_visibility(node: Option<tree_sitter::Node>, content: &str) -> Visibility {
    if let Some(parent) = node {
        let text = get_node_text(parent, content.as_bytes());
        if text.starts_with("pub ") || text.contains("\npub ") {
            if text.contains("pub(crate)") {
                return Visibility::Internal;
            }
            if text.contains("pub(super)") {
                return Visibility::Protected;
            }
            return Visibility::Public;
        }
    }
    Visibility::Private
}

fn parse_rust_parameters(params_text: &str) -> Vec<Parameter> {
    let inner = params_text.trim_start_matches('(').trim_end_matches(')');
    if inner.is_empty() {
        return Vec::new();
    }

    inner
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() || p == "&self" || p == "&mut self" || p == "self" || p == "mut self" {
                return None;
            }

            let parts: Vec<&str> = p.splitn(2, ':').collect();
            if parts.len() < 2 {
                return None;
            }

            let name = parts[0].trim().trim_start_matches("mut ").to_string();
            let param_type = Some(parts[1].trim().to_string());

            Some(Parameter {
                name,
                param_type,
                optional: false,
                default_value: None,
            })
        })
        .collect()
}
