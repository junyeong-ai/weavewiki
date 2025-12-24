//! LLM Response Parsers
//!
//! Functions for parsing AI-generated JSON responses into typed data.
//! Supports the content-first schema with rich markdown documentation.

use super::types::{FileInsight, Importance, ProcessingTier, RelatedFile};
use crate::types::{json_string, json_string_or};

/// Parse the complete FileInsight from LLM response
///
/// New schema fields:
/// - purpose: Clear explanation of what this file does
/// - importance: critical/high/medium/low
/// - content: Rich markdown documentation (natural sections, code refs)
/// - diagram: Primary Mermaid diagram
/// - related_files: Files this code interacts with
pub fn parse_file_insight(
    file_path: &str,
    language: Option<String>,
    line_count: usize,
    response: serde_json::Value,
) -> FileInsight {
    let content = json_string_or(&response, "content", "");
    let token_count = content.len() / 4; // Rough estimate

    FileInsight {
        file_path: file_path.to_string(),
        language,
        line_count,
        purpose: json_string_or(&response, "purpose", ""),
        importance: parse_importance(&response),
        tier: ProcessingTier::default(),
        content,
        diagram: json_string(&response, "diagram").filter(|s| !s.is_empty()),
        related_files: parse_related_files(&response),
        token_count,
        // Research context is only populated by Deep Research workflow
        research_iterations_json: None,
        research_aspects_json: None,
    }
}

/// Parse importance level from response
fn parse_importance(response: &serde_json::Value) -> Importance {
    json_string(response, "importance")
        .map(|s| Importance::parse(&s))
        .unwrap_or(Importance::Medium)
}

/// Parse related files from response
fn parse_related_files(response: &serde_json::Value) -> Vec<RelatedFile> {
    response
        .get("related_files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let path = json_string(item, "path")?;
                    let relationship = json_string(item, "relationship")?;
                    Some(RelatedFile { path, relationship })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_purpose() {
        let response = json!({
            "purpose": "Handles user authentication"
        });
        assert_eq!(
            json_string_or(&response, "purpose", ""),
            "Handles user authentication"
        );
    }

    #[test]
    fn test_parse_importance() {
        assert_eq!(
            parse_importance(&json!({"importance": "critical"})),
            Importance::Critical
        );
        assert_eq!(
            parse_importance(&json!({"importance": "high"})),
            Importance::High
        );
        assert_eq!(
            parse_importance(&json!({"importance": "medium"})),
            Importance::Medium
        );
        assert_eq!(
            parse_importance(&json!({"importance": "low"})),
            Importance::Low
        );
        // Default to medium for unknown
        assert_eq!(
            parse_importance(&json!({"importance": "unknown"})),
            Importance::Medium
        );
    }

    #[test]
    fn test_parse_content() {
        let response = json!({
            "content": "## How It Works\n\nThe module processes requests by..."
        });
        let content = json_string_or(&response, "content", "");
        assert!(content.contains("How It Works"));
        assert!(content.contains("processes requests"));
    }

    #[test]
    fn test_parse_diagram() {
        let response = json!({
            "diagram": "graph TD; A-->B; B-->C"
        });
        let diagram = json_string(&response, "diagram").filter(|s| !s.is_empty());
        assert!(
            diagram
                .as_ref()
                .is_some_and(|d: &String| d.contains("graph TD")),
            "Expected diagram to contain 'graph TD'"
        );

        // Empty diagram should return None
        let empty = json!({"diagram": ""});
        assert!(
            json_string(&empty, "diagram")
                .filter(|s| !s.is_empty())
                .is_none()
        );
    }

    #[test]
    fn test_parse_related_files() {
        let response = json!({
            "related_files": [
                {
                    "path": "database/mod.rs",
                    "relationship": "imports"
                },
                {
                    "path": "config.rs",
                    "relationship": "configures"
                }
            ]
        });

        let related = parse_related_files(&response);
        assert_eq!(related.len(), 2);
        assert_eq!(related[0].path, "database/mod.rs");
        assert_eq!(related[0].relationship, "imports");
        assert_eq!(related[1].path, "config.rs");
    }

    #[test]
    fn test_parse_complete_file_insight() {
        let response = json!({
            "purpose": "Main entry point for the application",
            "importance": "critical",
            "content": "## Overview\n\nThis is the main entry point...\n\n## Request Lifecycle\n\n1. Parse CLI args\n2. Load config\n3. Start server",
            "diagram": "graph TD; CLI-->Config; Config-->Server",
            "related_files": [
                {"path": "config.rs", "relationship": "imports"},
                {"path": "server.rs", "relationship": "calls"}
            ]
        });

        let insight = parse_file_insight("src/main.rs", Some("rust".to_string()), 100, response);

        assert_eq!(insight.file_path, "src/main.rs");
        assert_eq!(insight.importance, Importance::Critical);
        assert!(insight.has_content());
        assert!(insight.has_diagram());
        assert_eq!(insight.related_files.len(), 2);
    }

    #[test]
    fn test_parse_minimal_response() {
        // Only required fields
        let response = json!({
            "purpose": "Simple utility",
            "importance": "low",
            "content": "Just a helper function that does one thing."
        });

        let insight = parse_file_insight("src/utils.rs", Some("rust".to_string()), 10, response);

        assert_eq!(insight.purpose, "Simple utility");
        assert_eq!(insight.importance, Importance::Low);
        assert!(!insight.has_diagram()); // No diagram for simple files
        assert!(insight.related_files.is_empty());
    }
}
