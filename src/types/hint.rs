//! Analysis Hint Types
//!
//! Provides confidence-tagged hints from programmatic analysis.
//! These hints are advisory - LLM validates and refines them with full context.
//!
//! Confidence levels indicate reliability:
//! - Definitive: 100% certain (file exists, manifest parsed)
//! - High: Strong signal (dependency-based)
//! - Medium: Pattern-based inference
//! - Low: Directory/naming heuristic
//! - RequiresValidation: Must be LLM-verified

use serde::{Deserialize, Serialize};

/// Confidence level for programmatic analysis results
///
/// Variants are ordered from lowest to highest confidence for Ord trait:
/// RequiresValidation < Low < Medium < High < Definitive
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum HintConfidence {
    /// Requires LLM validation before use
    #[default]
    RequiresValidation,
    /// Low confidence - based on directory names or simple heuristics
    Low,
    /// Medium confidence - based on pattern matching
    Medium,
    /// High confidence - based on dependency analysis
    High,
    /// 100% certain - based on file existence, manifest parsing, etc.
    Definitive,
}


impl HintConfidence {
    /// Returns true if this confidence level is reliable enough for direct use
    pub fn is_reliable(&self) -> bool {
        matches!(self, Self::Definitive | Self::High)
    }

    /// Returns true if LLM should validate this hint
    pub fn needs_validation(&self) -> bool {
        matches!(self, Self::Medium | Self::Low | Self::RequiresValidation)
    }

    /// Numeric weight for aggregation (higher = more reliable)
    pub fn weight(&self) -> f32 {
        match self {
            Self::RequiresValidation => 0.1,
            Self::Low => 0.3,
            Self::Medium => 0.5,
            Self::High => 0.8,
            Self::Definitive => 1.0,
        }
    }
}

/// Category of analysis hint
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HintCategory {
    /// Architecture pattern (layered, hexagonal, etc.)
    Architecture,
    /// Error handling approach (Result, exceptions, etc.)
    ErrorHandling,
    /// Async/concurrency patterns
    AsyncPattern,
    /// Naming conventions (case, prefixes, suffixes)
    NamingConvention,
    /// Project type (CLI, library, backend, etc.)
    ProjectType,
    /// Testing framework and patterns
    TestingFramework,
    /// Directory purpose/role
    DirectoryRole,
    /// Module relationship
    ModuleRelationship,
}

/// A hint from programmatic analysis
///
/// Hints are advisory signals for LLM to validate and refine.
/// They should NOT be used as authoritative classifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisHint {
    /// Category of this hint
    pub category: HintCategory,
    /// Confidence level
    pub confidence: HintConfidence,
    /// Human-readable signal description
    pub signal: String,
    /// Evidence supporting this hint
    pub evidence: Vec<String>,
}

impl AnalysisHint {
    /// Create a new hint
    pub fn new(
        category: HintCategory,
        confidence: HintConfidence,
        signal: impl Into<String>,
    ) -> Self {
        Self {
            category,
            confidence,
            signal: signal.into(),
            evidence: Vec::new(),
        }
    }

    /// Add evidence supporting this hint
    pub fn with_evidence(mut self, evidence: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.evidence = evidence.into_iter().map(Into::into).collect();
        self
    }

    /// Create a definitive hint (100% certain)
    pub fn definitive(category: HintCategory, signal: impl Into<String>) -> Self {
        Self::new(category, HintConfidence::Definitive, signal)
    }

    /// Create a high-confidence hint
    pub fn high_confidence(category: HintCategory, signal: impl Into<String>) -> Self {
        Self::new(category, HintConfidence::High, signal)
    }

    /// Create a medium-confidence hint (requires validation)
    pub fn medium_confidence(category: HintCategory, signal: impl Into<String>) -> Self {
        Self::new(category, HintConfidence::Medium, signal)
    }

    /// Create a low-confidence hint (requires validation)
    pub fn low_confidence(category: HintCategory, signal: impl Into<String>) -> Self {
        Self::new(category, HintConfidence::Low, signal)
    }

    /// Format for inclusion in LLM prompt
    pub fn to_prompt_format(&self) -> String {
        let confidence_tag = match self.confidence {
            HintConfidence::Definitive => "[DEFINITIVE]",
            HintConfidence::High => "[HIGH]",
            HintConfidence::Medium => "[MEDIUM - verify]",
            HintConfidence::Low => "[LOW - verify]",
            HintConfidence::RequiresValidation => "[UNVERIFIED - validate]",
        };

        let mut result = format!("{} {:?}: {}", confidence_tag, self.category, self.signal);

        if !self.evidence.is_empty() {
            result.push_str("\n  Evidence:");
            for e in &self.evidence {
                result.push_str(&format!("\n  - {}", e));
            }
        }

        result
    }
}

/// Collection of hints for a project
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HintCollection {
    hints: Vec<AnalysisHint>,
}

impl HintCollection {
    pub fn new() -> Self {
        Self { hints: Vec::new() }
    }

    pub fn push(&mut self, hint: AnalysisHint) {
        self.hints.push(hint);
    }

    pub fn extend(&mut self, hints: impl IntoIterator<Item = AnalysisHint>) {
        self.hints.extend(hints);
    }

    pub fn by_category(&self, category: HintCategory) -> impl Iterator<Item = &AnalysisHint> {
        self.hints.iter().filter(move |h| h.category == category)
    }

    pub fn reliable_hints(&self) -> impl Iterator<Item = &AnalysisHint> {
        self.hints.iter().filter(|h| h.confidence.is_reliable())
    }

    pub fn hints_needing_validation(&self) -> impl Iterator<Item = &AnalysisHint> {
        self.hints.iter().filter(|h| h.confidence.needs_validation())
    }

    pub fn all(&self) -> &[AnalysisHint] {
        &self.hints
    }

    pub fn is_empty(&self) -> bool {
        self.hints.is_empty()
    }

    pub fn len(&self) -> usize {
        self.hints.len()
    }

    /// Format all hints for LLM prompt
    pub fn to_prompt_section(&self, title: &str) -> String {
        if self.hints.is_empty() {
            return String::new();
        }

        let mut sections = vec![format!("## {}\n", title)];

        // Group by confidence for clarity
        let definitive: Vec<_> = self
            .hints
            .iter()
            .filter(|h| h.confidence == HintConfidence::Definitive)
            .collect();
        let high: Vec<_> = self
            .hints
            .iter()
            .filter(|h| h.confidence == HintConfidence::High)
            .collect();
        let needs_validation: Vec<_> = self
            .hints
            .iter()
            .filter(|h| h.confidence.needs_validation())
            .collect();

        if !definitive.is_empty() {
            sections.push("### Verified Facts".to_string());
            for hint in definitive {
                sections.push(hint.to_prompt_format());
            }
        }

        if !high.is_empty() {
            sections.push("\n### High-Confidence Signals".to_string());
            for hint in high {
                sections.push(hint.to_prompt_format());
            }
        }

        if !needs_validation.is_empty() {
            sections.push("\n### Hints Requiring Validation".to_string());
            sections.push("The following are heuristic-based and may be incorrect:".to_string());
            for hint in needs_validation {
                sections.push(hint.to_prompt_format());
            }
        }

        sections.join("\n")
    }
}

impl FromIterator<AnalysisHint> for HintCollection {
    fn from_iter<T: IntoIterator<Item = AnalysisHint>>(iter: T) -> Self {
        Self {
            hints: iter.into_iter().collect(),
        }
    }
}

impl IntoIterator for HintCollection {
    type Item = AnalysisHint;
    type IntoIter = std::vec::IntoIter<AnalysisHint>;

    fn into_iter(self) -> Self::IntoIter {
        self.hints.into_iter()
    }
}

impl<'a> IntoIterator for &'a HintCollection {
    type Item = &'a AnalysisHint;
    type IntoIter = std::slice::Iter<'a, AnalysisHint>;

    fn into_iter(self) -> Self::IntoIter {
        self.hints.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hint_confidence_ordering() {
        assert!(HintConfidence::Definitive > HintConfidence::High);
        assert!(HintConfidence::High > HintConfidence::Medium);
        assert!(HintConfidence::Medium > HintConfidence::Low);
        assert!(HintConfidence::Low > HintConfidence::RequiresValidation);
    }

    #[test]
    fn test_hint_confidence_reliability() {
        assert!(HintConfidence::Definitive.is_reliable());
        assert!(HintConfidence::High.is_reliable());
        assert!(!HintConfidence::Medium.is_reliable());
        assert!(!HintConfidence::Low.is_reliable());
        assert!(!HintConfidence::RequiresValidation.is_reliable());
    }

    #[test]
    fn test_hint_collection_grouping() {
        let mut collection = HintCollection::new();
        collection.push(AnalysisHint::definitive(
            HintCategory::ProjectType,
            "Rust project",
        ));
        collection.push(AnalysisHint::low_confidence(
            HintCategory::Architecture,
            "Might be hexagonal",
        ));

        assert_eq!(collection.reliable_hints().count(), 1);
        assert_eq!(collection.hints_needing_validation().count(), 1);
    }

    #[test]
    fn test_hint_to_prompt_format() {
        let hint = AnalysisHint::medium_confidence(
            HintCategory::DirectoryRole,
            "hooks/ suggests React hooks pattern",
        )
        .with_evidence(["hooks/ directory exists"]);

        let formatted = hint.to_prompt_format();
        assert!(formatted.contains("[MEDIUM - verify]"));
        assert!(formatted.contains("DirectoryRole"));
        assert!(formatted.contains("Evidence:"));
    }
}
