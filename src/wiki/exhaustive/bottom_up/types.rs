//! Bottom-Up Analysis Types
//!
//! Content-first types for rich documentation generation.
//! Designed for hierarchical context building and Deep Research workflow.

use serde::{Deserialize, Serialize};

pub use crate::wiki::exhaustive::types::Importance;

// =============================================================================
// Processing Tier
// =============================================================================

/// Processing tier determines analysis depth and Deep Research configuration
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum ProcessingTier {
    /// Utilities/helpers - single pass, concise documentation
    Leaf = 0,
    /// Standard files - normal analysis depth
    #[default]
    Standard = 1,
    /// Important files - Deep Research with 3 iterations
    Important = 2,
    /// Core architecture - Deep Research with 4 iterations
    Core = 3,
}

impl ProcessingTier {
    /// Whether this tier uses Deep Research workflow
    pub fn uses_deep_research(&self) -> bool {
        matches!(self, ProcessingTier::Important | ProcessingTier::Core)
    }

    /// Number of research iterations for Deep Research workflow
    pub fn research_iterations(&self) -> u8 {
        match self {
            ProcessingTier::Core => 4,      // Plan → Update1 → Update2 → Synthesis
            ProcessingTier::Important => 3, // Plan → Update → Synthesis
            _ => 1,                         // Single pass (no Deep Research)
        }
    }

    /// Whether this tier should receive child documentation context
    pub fn uses_child_context(&self) -> bool {
        matches!(self, ProcessingTier::Important | ProcessingTier::Core)
    }

    /// Maximum tokens for final content output at this tier
    ///
    /// Deep Research generates ~12000 tokens across 4 iterations for Core tier.
    /// Higher limits reduce compression loss and preserve research richness.
    pub fn max_content_tokens(&self) -> usize {
        match self {
            ProcessingTier::Leaf => 500,
            ProcessingTier::Standard => 1200,
            ProcessingTier::Important => 3000,
            ProcessingTier::Core => 5000,
        }
    }

    /// Token budget per research iteration
    ///
    /// Each iteration should produce substantive findings.
    /// Higher budgets allow richer analysis per iteration.
    pub fn tokens_per_iteration(&self) -> usize {
        match self {
            ProcessingTier::Core => 3500,
            ProcessingTier::Important => 3000,
            ProcessingTier::Standard => 1200,
            ProcessingTier::Leaf => 500,
        }
    }
}

// =============================================================================
// File Insight
// =============================================================================

/// File documentation with natural content
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileInsight {
    /// File path relative to project root
    pub file_path: String,

    /// Detected programming language
    #[serde(default)]
    pub language: Option<String>,

    /// Number of lines in the file
    #[serde(default)]
    pub line_count: usize,

    /// Clear 1-2 sentence purpose statement
    pub purpose: String,

    /// Architectural importance of this file
    pub importance: Importance,

    /// Processing tier used for this file
    #[serde(default)]
    pub tier: ProcessingTier,

    /// Rich markdown documentation with natural sections
    #[serde(default)]
    pub content: String,

    /// Primary Mermaid diagram (validated)
    #[serde(default)]
    pub diagram: Option<String>,

    /// Files this code interacts with
    #[serde(default)]
    pub related_files: Vec<RelatedFile>,

    /// Approximate token count of content
    #[serde(default)]
    pub token_count: usize,

    /// Deep Research iteration findings (serialized JSON) - for Important/Core tiers
    /// Used for checkpoint/resume to preserve multi-turn research state
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research_iterations_json: Option<String>,

    /// Covered aspects from Deep Research (serialized JSON array)
    /// Used for anti-repetition during resume
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research_aspects_json: Option<String>,
}

impl FileInsight {
    pub fn new(file_path: String, language: Option<String>, line_count: usize) -> Self {
        Self {
            file_path,
            language,
            line_count,
            ..Default::default()
        }
    }

    /// Check if this insight has meaningful content
    pub fn has_content(&self) -> bool {
        !self.content.is_empty() && self.content.len() > 50
    }

    /// Check if this file has a diagram
    pub fn has_diagram(&self) -> bool {
        self.diagram.as_ref().is_some_and(|d| !d.is_empty())
    }

    /// Get content word count
    pub fn content_word_count(&self) -> usize {
        self.content.split_whitespace().count()
    }

    /// Create a summary for use as child context
    pub fn to_child_context(&self) -> ChildDocContext {
        ChildDocContext {
            path: self.file_path.clone(),
            purpose: self.purpose.clone(),
            importance: self.importance,
            summary: self.extract_summary(),
        }
    }

    /// Extract first 2-3 sentences as summary
    fn extract_summary(&self) -> String {
        let sentences: Vec<&str> = self.content.split(". ").take(3).collect();
        let summary = sentences.join(". ");
        if summary.len() > 300 {
            format!("{}...", &summary[..297])
        } else if !summary.is_empty() && !summary.ends_with('.') {
            format!("{}.", summary)
        } else {
            summary
        }
    }
}

// =============================================================================
// Related File
// =============================================================================

/// Relationship to another file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedFile {
    /// Relative file path
    pub path: String,

    /// Relationship type: imports, exports, calls, implements, configures, extends
    pub relationship: String,
}

impl RelatedFile {
    pub fn new(path: impl Into<String>, relationship: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            relationship: relationship.into(),
        }
    }
}

// =============================================================================
// Child Documentation Context
// =============================================================================

/// Summary of already-documented child file for parent context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildDocContext {
    /// File path
    pub path: String,

    /// Purpose statement
    pub purpose: String,

    /// Importance level
    pub importance: Importance,

    /// Brief summary (2-3 sentences)
    pub summary: String,
}

impl ChildDocContext {
    /// Estimated token count for this context
    pub fn estimated_tokens(&self) -> usize {
        // Rough estimate: 1 token per 4 chars
        (self.path.len() + self.purpose.len() + self.summary.len()) / 4
    }
}

// =============================================================================
// Analysis Request
// =============================================================================

/// Request for file analysis with context
#[derive(Debug, Clone)]
pub struct AnalysisRequest {
    /// File path to analyze
    pub file_path: String,

    /// Processing tier for this file
    pub tier: ProcessingTier,

    /// Current iteration (0-indexed)
    pub iteration: usize,

    /// Child documentation context (for Important/Core tiers)
    pub child_contexts: Vec<ChildDocContext>,

    /// Previous iteration result (for multi-iteration)
    pub previous_insight: Option<FileInsight>,
}

impl AnalysisRequest {
    pub fn new(file_path: String, tier: ProcessingTier) -> Self {
        Self {
            file_path,
            tier,
            iteration: 0,
            child_contexts: Vec::new(),
            previous_insight: None,
        }
    }

    pub fn with_child_contexts(mut self, contexts: Vec<ChildDocContext>) -> Self {
        self.child_contexts = contexts;
        self
    }

    pub fn with_previous(mut self, insight: FileInsight) -> Self {
        self.iteration += 1;
        self.previous_insight = Some(insight);
        self
    }

    /// Check if this is a deepening iteration
    pub fn is_deepening(&self) -> bool {
        self.iteration > 0
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processing_tier_research_iterations() {
        assert_eq!(ProcessingTier::Leaf.research_iterations(), 1);
        assert_eq!(ProcessingTier::Standard.research_iterations(), 1);
        assert_eq!(ProcessingTier::Important.research_iterations(), 3);
        assert_eq!(ProcessingTier::Core.research_iterations(), 4);
    }

    #[test]
    fn test_processing_tier_uses_deep_research() {
        assert!(!ProcessingTier::Leaf.uses_deep_research());
        assert!(!ProcessingTier::Standard.uses_deep_research());
        assert!(ProcessingTier::Important.uses_deep_research());
        assert!(ProcessingTier::Core.uses_deep_research());
    }

    #[test]
    fn test_processing_tier_uses_child_context() {
        assert!(!ProcessingTier::Leaf.uses_child_context());
        assert!(!ProcessingTier::Standard.uses_child_context());
        assert!(ProcessingTier::Important.uses_child_context());
        assert!(ProcessingTier::Core.uses_child_context());
    }

    #[test]
    fn test_file_insight_default() {
        let insight = FileInsight::default();
        assert!(insight.file_path.is_empty());
        assert_eq!(insight.importance, Importance::Medium);
        assert_eq!(insight.tier, ProcessingTier::Standard);
        assert!(!insight.has_content());
    }

    #[test]
    fn test_file_insight_to_child_context() {
        let mut insight = FileInsight::new(
            "src/utils/helper.rs".to_string(),
            Some("rust".to_string()),
            50,
        );
        insight.purpose = "Utility functions for string manipulation".to_string();
        insight.importance = Importance::Low;
        insight.content =
            "This module provides helper functions. It handles edge cases.".to_string();

        let ctx = insight.to_child_context();
        assert_eq!(ctx.path, "src/utils/helper.rs");
        assert_eq!(ctx.purpose, "Utility functions for string manipulation");
        assert_eq!(ctx.importance, Importance::Low);
        assert!(!ctx.summary.is_empty());
    }

    #[test]
    fn test_analysis_request_deepening() {
        let req = AnalysisRequest::new("src/main.rs".to_string(), ProcessingTier::Core);
        assert!(!req.is_deepening());
        assert_eq!(req.iteration, 0);

        let insight = FileInsight::default();
        let req2 = req.with_previous(insight);
        assert!(req2.is_deepening());
        assert_eq!(req2.iteration, 1);
    }

    #[test]
    fn test_has_content() {
        let mut insight = FileInsight::default();
        assert!(!insight.has_content());

        insight.content = "Short".to_string();
        assert!(!insight.has_content());

        insight.content =
            "This is a longer content that explains how the code works in detail.".to_string();
        assert!(insight.has_content());
    }

    #[test]
    fn test_has_diagram() {
        let mut insight = FileInsight::default();
        assert!(!insight.has_diagram());

        insight.diagram = Some("graph TD; A-->B".to_string());
        assert!(insight.has_diagram());
    }
}
