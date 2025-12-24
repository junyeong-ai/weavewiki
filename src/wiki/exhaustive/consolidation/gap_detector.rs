//! Gap Detector for Consolidation
//!
//! Identifies potential documentation gaps in domains.

use crate::wiki::exhaustive::consolidation::DomainInsight;

/// Detect gaps in domain summary (returns simple string descriptions)
pub fn detect_gaps(summary: &DomainInsight) -> Vec<String> {
    let detector = GapDetector;
    detector.detect(summary)
}

pub struct GapDetector;

impl Default for GapDetector {
    fn default() -> Self {
        Self
    }
}

impl GapDetector {
    /// Detect gaps in domain documentation
    pub fn detect(&self, summary: &DomainInsight) -> Vec<String> {
        let mut gaps = vec![];

        // Check for missing content in large domains
        if summary.files.len() > 5 && !summary.has_content() {
            gaps.push("No documentation content for multi-file domain".to_string());
        }

        // Check for missing diagram in complex domains
        if summary.files.len() > 3 && summary.diagram.is_none() {
            gaps.push("No architecture diagram for multi-file domain".to_string());
        }

        // Check for missing relationships in connected domains
        if summary.files.len() > 5 && summary.related_files.is_empty() {
            gaps.push("No cross-references documented for multi-file domain".to_string());
        }

        // Check for state management without documentation
        if summary.files.iter().any(|f| {
            f.contains("state")
                || f.contains("status")
                || f.contains("machine")
                || f.contains("session")
        }) && !summary.content.to_lowercase().contains("state")
        {
            gaps.push("State-related files may need state machine documentation".to_string());
        }

        // Check for API-related files without API documentation
        if summary.files.iter().any(|f| {
            f.contains("api")
                || f.contains("handler")
                || f.contains("controller")
                || f.contains("route")
        }) && !summary.content.to_lowercase().contains("api")
            && !summary.content.to_lowercase().contains("endpoint")
        {
            gaps.push("API-related files may need API contract documentation".to_string());
        }

        // Check for integration files without integration documentation
        if summary.files.iter().any(|f| {
            f.contains("client")
                || f.contains("external")
                || f.contains("integration")
                || f.contains("provider")
        }) && !summary.content.to_lowercase().contains("integration")
            && !summary.content.to_lowercase().contains("external")
        {
            gaps.push("Integration files may need integration point documentation".to_string());
        }

        gaps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_gaps_for_small_domain() {
        let summary = DomainInsight::new("small".to_string());
        let gaps = detect_gaps(&summary);
        assert!(gaps.is_empty());
    }

    #[test]
    fn test_gap_for_large_empty_domain() {
        let mut summary = DomainInsight::new("large".to_string());
        summary.files = vec![
            "file1.rs".to_string(),
            "file2.rs".to_string(),
            "file3.rs".to_string(),
            "file4.rs".to_string(),
            "file5.rs".to_string(),
            "file6.rs".to_string(),
        ];
        let gaps = detect_gaps(&summary);
        assert!(!gaps.is_empty());
    }
}
