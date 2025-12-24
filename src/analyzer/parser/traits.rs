use std::path::Path;

use chrono::Utc;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

use crate::types::{
    Edge, EdgeMetadata, EdgeType, EvidenceLocation, ImportType, InformationTier, Node, NodeId,
    NodeMetadata, NodeStatus, NodeType, Result,
};

pub struct ParseResult {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl ParseResult {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn merge(&mut self, other: ParseResult) {
        self.nodes.extend(other.nodes);
        self.edges.extend(other.edges);
    }
}

impl Default for ParseResult {
    fn default() -> Self {
        Self::new()
    }
}

pub trait Parser: Send + Sync {
    fn parse(&self, path: &str, content: &str) -> Result<ParseResult>;
    fn language(&self) -> super::Language;
}

/// Extract text content from a tree-sitter node.
/// Returns empty string if extraction fails (with debug logging).
#[inline]
pub fn get_node_text<'a>(node: tree_sitter::Node, content: &'a [u8]) -> &'a str {
    node.utf8_text(content).unwrap_or_else(|e| {
        tracing::debug!(
            "UTF-8 extraction failed at {}:{}-{}:{}: {}",
            node.start_position().row + 1,
            node.start_position().column,
            node.end_position().row + 1,
            node.end_position().column,
            e
        );
        ""
    })
}

/// Extract position information from a tree-sitter node.
pub fn get_node_position(node: tree_sitter::Node) -> (u32, u32, u32, u32) {
    let start = node.start_position();
    let end = node.end_position();
    (
        start.row as u32 + 1,
        end.row as u32 + 1,
        start.column as u32,
        end.column as u32,
    )
}

/// Creates a file node for the knowledge graph.
/// Shared across all language parsers.
pub fn create_file_node(path: &str) -> Node {
    Node {
        id: NodeId::file(path).into_inner(),
        node_type: NodeType::File,
        path: path.to_string(),
        name: Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string(),
        metadata: NodeMetadata::default(),
        evidence: EvidenceLocation {
            file: path.to_string(),
            start_line: 1,
            end_line: 1,
            start_column: None,
            end_column: None,
        },
        tier: InformationTier::Fact,
        confidence: 1.0,
        last_verified: Utc::now(),
        status: NodeStatus::Verified,
    }
}

/// Create a tree-sitter parser for the given language.
/// This helper reduces boilerplate in language-specific parsers.
pub fn create_ts_parser<L: Into<tree_sitter::Language>>(
    language: L,
    lang_name: &str,
) -> crate::types::Result<tree_sitter::Parser> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language.into())
        .map_err(|e| crate::types::WeaveError::Parse {
            message: format!("Failed to set {} language: {}", lang_name, e),
            path: String::new(),
        })?;
    Ok(parser)
}

/// Create EvidenceLocation from a tree-sitter node.
/// Eliminates ~10 lines of boilerplate per usage.
#[inline]
pub fn evidence_from_node(node: tree_sitter::Node, path: &str) -> EvidenceLocation {
    let (start_line, end_line, start_col, end_col) = get_node_position(node);
    EvidenceLocation {
        file: path.to_string(),
        start_line,
        end_line,
        start_column: Some(start_col),
        end_column: Some(end_col),
    }
}

/// Create a dependency edge between source and target.
/// Common pattern for import/use statements.
pub fn create_dependency_edge(
    path: &str,
    source_id: String,
    target_id: String,
    import_path: &str,
    node: tree_sitter::Node,
    import_type: ImportType,
) -> Edge {
    Edge {
        id: format!("dep:{}:{}", path, import_path),
        edge_type: EdgeType::DependsOn,
        source_id,
        target_id,
        metadata: EdgeMetadata {
            import_type: Some(import_type),
            ..Default::default()
        },
        evidence: evidence_from_node(node, path),
        tier: InformationTier::Fact,
        confidence: 1.0,
        last_verified: Utc::now(),
    }
}

/// Execute a tree-sitter query and process matches with a callback.
/// Reduces query execution boilerplate from ~15 lines to 1.
pub fn execute_query<F>(
    language: &tree_sitter::Language,
    query_str: &str,
    root: tree_sitter::Node,
    content: &[u8],
    mut callback: F,
) where
    F: FnMut(tree_sitter::Node, &str),
{
    if let Ok(query) = Query::new(language, query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content);

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let text = get_node_text(cap.node, content);
                callback(cap.node, text);
            }
        }
    }
}

/// Captured text from a tree-sitter query match with position info.
#[derive(Debug, Clone)]
pub struct QueryMatch {
    pub text: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
    pub end_col: u32,
}

/// Execute a query and collect captured text with position info.
pub fn query_captures(
    language: &tree_sitter::Language,
    query_str: &str,
    root: tree_sitter::Node,
    content: &[u8],
) -> Vec<QueryMatch> {
    let mut results = Vec::new();

    if let Ok(query) = Query::new(language, query_str) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, content);

        while let Some(m) = matches.next() {
            for cap in m.captures.iter() {
                let text = get_node_text(cap.node, content).to_string();
                let (start_line, end_line, start_col, end_col) = get_node_position(cap.node);
                results.push(QueryMatch {
                    text,
                    start_line,
                    end_line,
                    start_col,
                    end_col,
                });
            }
        }
    }

    results
}

/// Create a code element node (struct, function, class, etc.) with standard fields.
/// Language-specific metadata (visibility, signature, etc.) should be set after creation.
pub fn create_code_node(
    id: String,
    node_type: NodeType,
    path: &str,
    name: String,
    ts_node: tree_sitter::Node,
) -> Node {
    Node {
        id,
        node_type,
        path: path.to_string(),
        name,
        metadata: NodeMetadata::default(),
        evidence: evidence_from_node(ts_node, path),
        tier: InformationTier::Fact,
        confidence: 1.0,
        last_verified: Utc::now(),
        status: NodeStatus::Verified,
    }
}

/// Create a code edge (ownership, inheritance, etc.) with standard fields.
pub fn create_code_edge(
    id: String,
    edge_type: EdgeType,
    source_id: String,
    target_id: String,
    ts_node: tree_sitter::Node,
    path: &str,
) -> Edge {
    Edge {
        id,
        edge_type,
        source_id,
        target_id,
        metadata: EdgeMetadata::default(),
        evidence: evidence_from_node(ts_node, path),
        tier: InformationTier::Fact,
        confidence: 1.0,
        last_verified: Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_file_node() {
        let node = create_file_node("src/main.rs");

        assert_eq!(node.id, "file:src/main.rs");
        assert_eq!(node.node_type, NodeType::File);
        assert_eq!(node.path, "src/main.rs");
        assert_eq!(node.name, "main.rs");
        assert_eq!(node.tier, InformationTier::Fact);
        assert_eq!(node.confidence, 1.0);
        assert_eq!(node.status, NodeStatus::Verified);
    }

    #[test]
    fn test_create_file_node_nested_path() {
        let node = create_file_node("src/analyzer/parser/python.rs");

        assert_eq!(node.name, "python.rs");
        assert_eq!(node.evidence.file, "src/analyzer/parser/python.rs");
    }

    #[test]
    fn test_parse_result_merge() {
        let mut result1 = ParseResult::new();
        result1.nodes.push(create_file_node("file1.rs"));

        let mut result2 = ParseResult::new();
        result2.nodes.push(create_file_node("file2.rs"));

        result1.merge(result2);

        assert_eq!(result1.nodes.len(), 2);
    }
}
