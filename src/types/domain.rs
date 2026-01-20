//! Domain Terminology Types

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainTerm {
    pub term: String,
    pub definition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl DomainTerm {
    pub fn new(term: impl Into<String>, definition: impl Into<String>) -> Self {
        Self {
            term: term.into(),
            definition: definition.into(),
            context: None,
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
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
    fn test_domain_term_serialization() {
        let term = DomainTerm::new("Test", "A test term");
        let json = serde_json::to_string(&term).unwrap();
        assert!(json.contains("\"term\":\"Test\""));
        assert!(json.contains("\"definition\":\"A test term\""));
        assert!(!json.contains("context"));
    }
}
