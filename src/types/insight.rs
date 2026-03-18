//! Insight types for extracted knowledge from codebase analysis

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tier classification for content value assessment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TierClassification {
    Tier0Hallucinated,
    #[default]
    Tier1Generic,
    Tier2Convention,
    Tier3Constraint,
}

impl TierClassification {
    /// Check if content should be kept based on tier classification
    ///
    /// # ADVISORY HEURISTIC - BINARY BOUNDARY
    ///
    /// This creates a BINARY decision from what is actually a SPECTRUM:
    /// - Tier1 (generic) is rejected, Tier2 (convention) is kept
    /// - But some Tier1 content may be valuable in specific contexts
    /// - Borderline content (almost Tier2) gets the same treatment as obvious Tier1
    ///
    /// ## When this heuristic is useful:
    /// - Quick filtering of obviously generic content ("use cargo build")
    /// - Reducing noise in generated artifacts
    ///
    /// ## When this heuristic may mislead:
    /// - Generic knowledge with project-specific application
    /// - Content that's Tier2 in one context but Tier1 in another
    /// - LLM-classified borderline content
    ///
    /// LLM should have final say on content value. Use this for pre-filtering,
    /// not as authoritative rejection.
    pub fn should_keep(&self) -> bool {
        matches!(self, Self::Tier2Convention | Self::Tier3Constraint)
    }

    /// Check if content is essential (Tier3 constraint)
    ///
    /// Tier3 content represents hidden constraints and gotchas that are
    /// most valuable for Claude Code. This classification is more reliable
    /// than the should_keep() boundary since Tier3 content is distinctively
    /// different from generic knowledge.
    pub fn is_essential(&self) -> bool {
        matches!(self, Self::Tier3Constraint)
    }

    /// Check if content should be rejected (Tier0/Tier1)
    ///
    /// # ADVISORY HEURISTIC - See `should_keep()` documentation
    ///
    /// Same binary boundary applies. Use for pre-filtering, not final rejection.
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Tier0Hallucinated | Self::Tier1Generic)
    }

    pub fn value(&self) -> u8 {
        match self {
            Self::Tier0Hallucinated => 0,
            Self::Tier1Generic => 1,
            Self::Tier2Convention => 2,
            Self::Tier3Constraint => 3,
        }
    }

    pub fn from_value(v: u8) -> Self {
        match v {
            0 => Self::Tier0Hallucinated,
            1 => Self::Tier1Generic,
            2 => Self::Tier2Convention,
            _ => Self::Tier3Constraint,
        }
    }
}

/// Alias for TierClassification - used in artifact types for clarity
pub type ContentTier = TierClassification;

impl std::fmt::Display for TierClassification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tier0Hallucinated => write!(f, "Tier0-Hallucinated"),
            Self::Tier1Generic => write!(f, "Tier1-Generic"),
            Self::Tier2Convention => write!(f, "Tier2-Convention"),
            Self::Tier3Constraint => write!(f, "Tier3-Constraint"),
        }
    }
}

/// Target artifact type for insight classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactClassification {
    #[default]
    Skill,
    Agent,
    Rule,
    ClaudeMd,
    Multiple,
}

/// Module-level context for insight scoping
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModuleContext {
    pub path: String,
    pub name: String,
    pub responsibility: String,
    pub constraints: Vec<String>,
    pub dependencies: Vec<String>,
    pub key_files: Vec<String>,
}

impl ModuleContext {
    pub fn new(path: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            responsibility: String::new(),
            constraints: Vec::new(),
            dependencies: Vec::new(),
            key_files: Vec::new(),
        }
    }

    pub fn responsibility(mut self, responsibility: impl Into<String>) -> Self {
        self.responsibility = responsibility.into();
        self
    }

    pub fn constraints(mut self, constraints: Vec<String>) -> Self {
        self.constraints = constraints;
        self
    }

    pub fn dependencies(mut self, deps: Vec<String>) -> Self {
        self.dependencies = deps;
        self
    }

    pub fn key_files(mut self, files: Vec<String>) -> Self {
        self.key_files = files;
        self
    }
}

/// Domain-level context for business-aware insights
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DomainContext {
    pub domain: String,
    pub business_rules: Vec<String>,
    pub terminology: HashMap<String, String>,
    pub compliance: Vec<String>,
}

impl DomainContext {
    pub fn new(domain: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            business_rules: Vec::new(),
            terminology: HashMap::new(),
            compliance: Vec::new(),
        }
    }

    pub fn business_rules(mut self, rules: Vec<String>) -> Self {
        self.business_rules = rules;
        self
    }

    pub fn terminology(mut self, terms: HashMap<String, String>) -> Self {
        self.terminology = terms;
        self
    }

    pub fn compliance(mut self, compliance: Vec<String>) -> Self {
        self.compliance = compliance;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_classification() {
        assert!(TierClassification::Tier0Hallucinated.is_rejected());
        assert!(TierClassification::Tier1Generic.is_rejected());
        assert!(TierClassification::Tier2Convention.should_keep());
        assert!(TierClassification::Tier3Constraint.is_essential());
    }

    #[test]
    fn test_module_context_builder() {
        let ctx = ModuleContext::new("src/api", "api")
            .responsibility("HTTP request handling")
            .constraints(vec!["Rate limiting required".into()])
            .key_files(vec!["src/api/mod.rs".into()]);

        assert_eq!(ctx.name, "api");
        assert!(!ctx.constraints.is_empty());
    }
}
