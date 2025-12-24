use tree_sitter::{Query, QueryCursor, StreamingIterator};

use super::{
    Language, ParseResult, Parser, create_code_edge, create_code_node, create_file_node,
    create_ts_parser, get_node_text,
};
use crate::types::{
    EdgeType, FunctionSignature, ImportType, NodeMetadata, NodeType, Parameter, Result, Visibility,
    WeaveError,
};

pub struct GoParser;

impl GoParser {
    pub fn new() -> Result<Self> {
        let _ = create_ts_parser(tree_sitter_go::LANGUAGE, "Go")?;
        Ok(Self)
    }
}

impl Parser for GoParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult> {
        let mut parser = create_ts_parser(tree_sitter_go::LANGUAGE, "Go").map_err(|mut e| {
            if let WeaveError::Parse {
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
                message: "Failed to parse Go file".to_string(),
                path: path.to_string(),
            })?;

        let mut result = ParseResult::new();
        let root = tree.root_node();

        result.nodes.push(create_file_node(path));

        extract_imports(root, content, path, &mut result);
        extract_structs(root, content, path, &mut result);
        extract_interfaces(root, content, path, &mut result);
        extract_functions(root, content, path, &mut result);
        extract_methods(root, content, path, &mut result);

        Ok(result)
    }

    fn language(&self) -> Language {
        Language::Go
    }
}

fn extract_imports(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (import_spec
            path: (interpreted_string_literal) @path
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_go::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let import_path = get_node_text(node, content.as_bytes()).trim_matches('"');

                let mut edge = create_code_edge(
                    format!("import:{}:{}", path, import_path),
                    EdgeType::DependsOn,
                    format!("file:{}", path),
                    format!("package:{}", import_path),
                    node,
                    path,
                );
                edge.metadata.import_type = Some(ImportType::Static);
                result.edges.push(edge);
            }
        }
    }
}

fn extract_structs(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (type_declaration
            (type_spec
                name: (type_identifier) @name
                type: (struct_type)
            )
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_go::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = get_node_text(node, content.as_bytes()).to_string();

                // Go visibility: uppercase = public, lowercase = private
                let mut struct_node = create_code_node(
                    format!("struct:{}:{}", path, name),
                    NodeType::Class,
                    path,
                    name.clone(),
                    node,
                );
                struct_node.metadata.visibility = Some(go_visibility(&name));
                result.nodes.push(struct_node);
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
        (type_declaration
            (type_spec
                name: (type_identifier) @name
                type: (interface_type)
            )
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_go::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = get_node_text(node, content.as_bytes()).to_string();

                let iface_node = create_code_node(
                    format!("interface:{}:{}", path, name),
                    NodeType::Interface,
                    path,
                    name,
                    node,
                );
                result.nodes.push(iface_node);
            }
        }
    }
}

fn extract_functions(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (function_declaration
            name: (identifier) @name
            parameters: (parameter_list) @params
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_go::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut params_text = String::new();
            let mut name_node = None;

            for cap in m.captures.iter() {
                let node = cap.node;
                let text = get_node_text(node, content.as_bytes());

                if cap.index == 0 {
                    name = text.to_string();
                    name_node = Some(node);
                } else if cap.index == 1 {
                    params_text = text.to_string();
                }
            }

            let Some(node) = name_node else { continue };
            if name.is_empty() {
                continue;
            }

            let mut func_node = create_code_node(
                format!("function:{}:{}", path, name),
                NodeType::Function,
                path,
                name.clone(),
                node,
            );
            func_node.metadata = NodeMetadata {
                visibility: Some(go_visibility(&name)),
                signature: Some(FunctionSignature {
                    parameters: parse_go_parameters(&params_text),
                    return_type: None,
                    is_async: false,
                    generator: false,
                }),
                ..Default::default()
            };
            result.nodes.push(func_node);
        }
    }
}

fn extract_methods(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (method_declaration
            receiver: (parameter_list
                (parameter_declaration
                    type: [
                        (type_identifier) @receiver_type
                        (pointer_type (type_identifier) @receiver_type)
                    ]
                )
            )
            name: (field_identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_go::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            let mut receiver_type = String::new();
            let mut method_name = String::new();
            let mut method_node = None;

            for cap in m.captures.iter() {
                let node = cap.node;
                let text = get_node_text(node, content.as_bytes()).to_string();

                if cap.index == 0 {
                    receiver_type = text;
                } else if cap.index == 1 {
                    method_name = text;
                    method_node = Some(node);
                }
            }

            let Some(node) = method_node else { continue };
            if method_name.is_empty() {
                continue;
            }

            let mut method = create_code_node(
                format!("method:{}:{}:{}", path, receiver_type, method_name),
                NodeType::Method,
                path,
                method_name.clone(),
                node,
            );
            method.metadata.visibility = Some(go_visibility(&method_name));
            result.nodes.push(method);

            // Create ownership edge: struct owns method
            if !receiver_type.is_empty() {
                let edge = create_code_edge(
                    format!("member:{}:{}:{}", path, receiver_type, method_name),
                    EdgeType::Owns,
                    format!("struct:{}:{}", path, receiver_type),
                    format!("method:{}:{}:{}", path, receiver_type, method_name),
                    node,
                    path,
                );
                result.edges.push(edge);
            }
        }
    }
}

/// Go visibility: uppercase first letter = public, lowercase = private
#[inline]
fn go_visibility(name: &str) -> Visibility {
    if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

fn parse_go_parameters(params_text: &str) -> Vec<Parameter> {
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
            if parts.is_empty() {
                return None;
            }

            let (name, param_type) = if parts.len() >= 2 {
                (parts[0].to_string(), Some(parts[1..].join(" ")))
            } else {
                (parts[0].to_string(), None)
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
