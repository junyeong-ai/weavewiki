use chrono::Utc;
use tree_sitter::{Parser as TsParser, Query, QueryCursor, StreamingIterator};

use super::{Language, ParseResult, Parser, create_file_node};
use crate::types::{
    Edge, EdgeMetadata, EdgeType, EvidenceLocation, FunctionSignature, ImportType, InformationTier,
    Node, NodeMetadata, NodeStatus, NodeType, Parameter, Result, Visibility, WeaveError,
};

pub struct TypeScriptParser;

impl TypeScriptParser {
    pub fn new() -> Result<Self> {
        let mut parser = TsParser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set TypeScript language: {}", e),
                path: String::new(),
            })?;
        Ok(Self)
    }
}

impl Parser for TypeScriptParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult> {
        let mut parser = TsParser::new();
        let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT;
        parser
            .set_language(&language.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set TypeScript language: {}", e),
                path: path.to_string(),
            })?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| WeaveError::Parse {
                message: "Failed to parse TypeScript file".to_string(),
                path: path.to_string(),
            })?;

        let mut result = ParseResult::new();
        let root = tree.root_node();

        let file_node = create_file_node(path);
        result.nodes.push(file_node);

        extract_imports(root, content, path, &mut result);
        extract_exports(root, content, path, &mut result);
        extract_classes(root, content, path, &mut result);
        extract_functions(root, content, path, &mut result);
        extract_interfaces(root, content, path, &mut result);

        Ok(result)
    }

    fn language(&self) -> Language {
        Language::TypeScript
    }
}

fn extract_imports(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (import_statement
            source: (string) @source
        )
    "#;

    if let Ok(query) = Query::new(
        &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        query_str,
    ) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let source_text = node.utf8_text(content.as_bytes()).unwrap_or("");
                let source = source_text.trim_matches(|c| c == '"' || c == '\'');

                if !source.starts_with('.') && !source.starts_with('/') {
                    continue;
                }

                let edge = Edge {
                    id: format!("dep:{}:{}", path, source),
                    edge_type: EdgeType::DependsOn,
                    source_id: format!("file:{}", path),
                    target_id: format!("file:{}", resolve_import(path, source)),
                    metadata: EdgeMetadata {
                        import_type: Some(ImportType::Static),
                        imported_symbols: None,
                        call_count: None,
                        is_async: None,
                        exposed_as: None,
                        extra: Default::default(),
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

fn extract_exports(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (export_statement
            declaration: [
                (function_declaration name: (identifier) @name)
                (class_declaration name: (type_identifier) @name)
                (lexical_declaration (variable_declarator name: (identifier) @name))
            ]
        )
    "#;

    if let Ok(query) = Query::new(
        &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        query_str,
    ) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or("").to_string();

                let edge = Edge {
                    id: format!("export:{}:{}", path, name),
                    edge_type: EdgeType::Exposes,
                    source_id: format!("file:{}", path),
                    target_id: format!("symbol:{}:{}", path, name),
                    metadata: EdgeMetadata {
                        exposed_as: Some(name),
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
            name: (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(
        &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        query_str,
    ) {
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

                let owns_edge = Edge {
                    id: format!("owns:{}:{}", path, name),
                    edge_type: EdgeType::Owns,
                    source_id: format!("file:{}", path),
                    target_id: format!("class:{}:{}", path, name),
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

                result.edges.push(owns_edge);
            }
        }
    }
}

fn extract_functions(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (function_declaration
            name: (identifier) @name
            parameters: (formal_parameters) @params
        )
    "#;

    if let Ok(query) = Query::new(
        &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        query_str,
    ) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut params_text = String::new();
            let mut start_byte = 0usize;
            let mut start_pos = (0u32, 0u32);
            let mut end_pos = (0u32, 0u32);

            for cap in m.captures.iter() {
                let node = cap.node;
                let text = node.utf8_text(content.as_bytes()).unwrap_or("");

                if cap.index == 0 {
                    name = text.to_string();
                    start_byte = node.start_byte();
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

            let params = parse_parameters(&params_text);
            let is_async = start_byte >= 5
                && content
                    .get(..start_byte)
                    .map(|s| s.trim_end().ends_with("async"))
                    .unwrap_or(false);

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

fn extract_interfaces(
    root: tree_sitter::Node,
    content: &str,
    path: &str,
    result: &mut ParseResult,
) {
    let query_str = r#"
        (interface_declaration
            name: (type_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(
        &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        query_str,
    ) {
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

fn parse_parameters(params_text: &str) -> Vec<Parameter> {
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

            let optional = p.contains('?');
            let parts: Vec<&str> = p.split(':').collect();
            let name = parts[0].trim().trim_end_matches('?').to_string();
            let param_type = parts.get(1).map(|t| t.trim().to_string());

            Some(Parameter {
                name,
                param_type,
                optional,
                default_value: None,
            })
        })
        .collect()
}

fn resolve_import(current_path: &str, import_path: &str) -> String {
    let current_dir = std::path::Path::new(current_path)
        .parent()
        .unwrap_or(std::path::Path::new(""));

    let resolved = current_dir.join(import_path);

    let path_str = resolved.to_string_lossy().to_string();

    if !path_str.ends_with(".ts") && !path_str.ends_with(".tsx") {
        format!("{}.ts", path_str)
    } else {
        path_str
    }
}
