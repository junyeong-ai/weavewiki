use chrono::Utc;
use tree_sitter::{Parser as TsParser, Query, QueryCursor, StreamingIterator};

use super::{Language, ParseResult, Parser, create_file_node};
use crate::types::{
    EvidenceLocation, FunctionSignature, InformationTier, Node, NodeMetadata, NodeStatus, NodeType,
    Result, Visibility, WeaveError,
};

pub struct BashParser;

impl BashParser {
    pub fn new() -> Result<Self> {
        let mut parser = TsParser::new();
        parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set Bash language: {}", e),
                path: String::new(),
            })?;
        Ok(Self)
    }
}

impl Parser for BashParser {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult> {
        let mut parser = TsParser::new();
        let language = tree_sitter_bash::LANGUAGE;
        parser
            .set_language(&language.into())
            .map_err(|e| WeaveError::Parse {
                message: format!("Failed to set Bash language: {}", e),
                path: path.to_string(),
            })?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| WeaveError::Parse {
                message: "Failed to parse Bash file".to_string(),
                path: path.to_string(),
            })?;

        let mut result = ParseResult::new();
        let root = tree.root_node();

        let file_node = create_file_node(path);
        result.nodes.push(file_node);

        extract_functions(root, content, path, &mut result);

        Ok(result)
    }

    fn language(&self) -> Language {
        Language::Bash
    }
}

fn extract_functions(root: tree_sitter::Node, content: &str, path: &str, result: &mut ParseResult) {
    let query_str = r#"
        (function_definition
            name: (word) @name
        )
    "#;

    if let Ok(query) = Query::new(&tree_sitter_bash::LANGUAGE.into(), query_str) {
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
                        signature: Some(FunctionSignature {
                            parameters: Vec::new(),
                            return_type: None,
                            is_async: false,
                            generator: false,
                        }),
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
