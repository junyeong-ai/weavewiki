//! Domain Terminology Types
//!
//! Unified types for domain-specific terminology used across
//! characterization and top-down analysis phases.

use serde::{Deserialize, Serialize};

/// Unified domain terminology entry
///
/// Used by both characterization (TerminologyAgent) and top-down (DomainAgent)
/// phases to represent domain-specific terms and their definitions.
///
/// Note: Supports both old field names (meaning, evidence) and new names
/// (definition, context) via serde aliases for backward compatibility.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DomainTerm {
    /// The domain term or concept name
    pub term: String,
    /// Definition or meaning of the term
    /// Aliases: "meaning" for backward compatibility
    #[serde(alias = "meaning")]
    pub definition: String,
    /// Optional context or evidence where this term is used
    /// Aliases: "evidence" for backward compatibility
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "evidence")]
    pub context: Option<String>,
}

impl DomainTerm {
    /// Create a new domain term with required fields
    pub fn new(term: impl Into<String>, definition: impl Into<String>) -> Self {
        Self {
            term: term.into(),
            definition: definition.into(),
            context: None,
        }
    }

    /// Add context/evidence to the term
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Create from legacy DomainTermInfo format (meaning field)
    pub fn from_meaning(term: impl Into<String>, meaning: impl Into<String>) -> Self {
        Self {
            term: term.into(),
            definition: meaning.into(),
            context: None,
        }
    }

    /// Create from legacy format with evidence
    pub fn from_meaning_with_evidence(
        term: impl Into<String>,
        meaning: impl Into<String>,
        evidence: Option<String>,
    ) -> Self {
        Self {
            term: term.into(),
            definition: meaning.into(),
            context: evidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_term_new() {
        let term = DomainTerm::new("Pipeline", "A sequence of processing stages");
        assert_eq!(term.term, "Pipeline");
        assert_eq!(term.definition, "A sequence of processing stages");
        assert!(term.context.is_none());
    }

    #[test]
    fn test_domain_term_with_context() {
        let term =
            DomainTerm::new("TALE", "Token-Aware Learning Engine").with_context("src/ai/budget.rs");
        assert_eq!(term.context, Some("src/ai/budget.rs".to_string()));
    }

    #[test]
    fn test_domain_term_from_meaning() {
        let term = DomainTerm::from_meaning("Agent", "An autonomous processing unit");
        assert_eq!(term.definition, "An autonomous processing unit");
    }

    #[test]
    fn test_domain_term_serialization() {
        let term = DomainTerm::new("Test", "A test term");
        let json = serde_json::to_string(&term).unwrap();
        assert!(json.contains("\"term\":\"Test\""));
        assert!(json.contains("\"definition\":\"A test term\""));
        // context should be omitted when None
        assert!(!json.contains("context"));
    }
}
