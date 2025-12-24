use tree_sitter::{Query, QueryCursor, StreamingIterator};

use super::{
    Language, ParseResult, Parser, create_code_edge, create_code_node, create_file_node,
    create_ts_parser, evidence_from_node, get_node_text,
};
use crate::types::{
    EdgeType, FunctionSignature, ImportType, NodeMetadata, NodeType, Parameter, Result, Visibility,
    WeaveError,
};

pub struct PythonParser;

impl PythonParser {
    pub fn new() -> Result<Self> {
        // Validate that the language is available
        let _ = create_ts_parser(tree_sitter_python::LANGUAGE, "Python")?;
        Ok(Self)
    }
}

impl Parser for PythonParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult> {
        let mut parser =
            create_ts_parser(tree_sitter_python::LANGUAGE, "Python").map_err(|mut e| {
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
                message: "Failed to parse Python file".to_string(),
                path: path.to_string(),
            })?;

        let mut result = ParseResult::new();
        let root = tree.root_node();

        result.nodes.push(create_file_node(path));

        extract_imports(root, content, path, &mut result);
        extract_classes(root, content, path, &mut result);
        extract_functions(root, content, path, &mut result);

        Ok(result)
    }

    fn language(&self) -> Language {
        Language::Python
    }
}

fn extract_imports(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (import_statement
            name: (dotted_name) @name
        )
        (import_from_statement
            module_name: (dotted_name) @module
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_python::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let module_name = get_node_text(node, content.as_bytes()).to_string();

                // Only track relative imports (starting with .)
                if module_name.is_empty() || !module_name.starts_with('.') {
                    continue;
                }

                let mut edge = create_code_edge(
                    format!("dep:{}:{}", path, module_name),
                    EdgeType::DependsOn,
                    format!("file:{}", path),
                    format!("module:{}", module_name),
                    node,
                    path,
                );
                edge.metadata.import_type = Some(ImportType::Static);
                result.edges.push(edge);
            }
        }
    }
}

fn extract_classes(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (class_definition
            name: (identifier) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_python::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let node = cap.node;
                let name = get_node_text(node, content.as_bytes()).to_string();

                // Create class node with Python-specific visibility (underscore convention)
                let mut class_node = create_code_node(
                    format!("class:{}:{}", path, name),
                    NodeType::Class,
                    path,
                    name.clone(),
                    node,
                );
                class_node.metadata.visibility = Some(python_visibility(&name));
                result.nodes.push(class_node);

                // File owns the class
                let owns_edge = create_code_edge(
                    format!("owns:{}:{}", path, name),
                    EdgeType::Owns,
                    format!("file:{}", path),
                    format!("class:{}:{}", path, name),
                    node,
                    path,
                );
                result.edges.push(owns_edge);
            }
        }
    }
}

fn extract_functions(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (function_definition
            name: (identifier) @name
            parameters: (parameters) @params
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_python::LANGUAGE.into(), query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content.as_bytes());

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut params_text = String::new();
            let mut name_node = None;
            let mut start_byte = 0usize;

            for cap in m.captures.iter() {
                let node = cap.node;
                let text = get_node_text(node, content.as_bytes());

                if cap.index == 0 {
                    name = text.to_string();
                    name_node = Some(node);
                    start_byte = node.start_byte();
                } else if cap.index == 1 {
                    params_text = text.to_string();
                }
            }

            let Some(node) = name_node else { continue };
            if name.is_empty() {
                continue;
            }

            // Python async detection: check if "async" precedes the function
            let is_async = start_byte >= 6
                && content
                    .get(..start_byte)
                    .is_some_and(|s| s.trim_end().ends_with("async"));

            let mut func_node = create_code_node(
                format!("function:{}:{}", path, name),
                NodeType::Function,
                path,
                name.clone(),
                node,
            );
            func_node.metadata = NodeMetadata {
                visibility: Some(python_visibility(&name)),
                signature: Some(FunctionSignature {
                    parameters: parse_parameters(&params_text),
                    return_type: None,
                    is_async,
                    generator: false,
                }),
                ..Default::default()
            };
            // Preserve evidence from the actual function definition node
            func_node.evidence = evidence_from_node(node, path);
            result.nodes.push(func_node);
        }
    }
}

/// Python visibility: underscore prefix means private
#[inline]
fn python_visibility(name: &str) -> Visibility {
    if name.starts_with('_') {
        Visibility::Private
    } else {
        Visibility::Public
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
            if p.is_empty() || p == "self" || p == "cls" {
                return None;
            }

            let has_default = p.contains('=');
            let parts: Vec<&str> = p.split('=').next().unwrap_or(p).split(':').collect();
            let name = parts[0].trim().to_string();
            let param_type = parts.get(1).map(|t| t.trim().to_string());

            Some(Parameter {
                name,
                param_type,
                optional: has_default,
                default_value: None,
            })
        })
        .collect()
}
