//! Insight types for extracted knowledge from codebase analysis

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tier classification for content value assessment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierClassification {
    Tier0Hallucinated,
    #[default]
    Tier1Generic,
    Tier2Convention,
    Tier3Constraint,
}

impl TierClassification {
    pub fn should_keep(&self) -> bool {
        matches!(self, Self::Tier2Convention | Self::Tier3Constraint)
    }

    pub fn is_essential(&self) -> bool {
        matches!(self, Self::Tier3Constraint)
    }

    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Tier0Hallucinated | Self::Tier1Generic)
    }

    pub fn is_rejectable(&self) -> bool {
        self.is_rejected()
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tier0Hallucinated => "tier0",
            Self::Tier1Generic => "tier1",
            Self::Tier2Convention => "tier2",
            Self::Tier3Constraint => "tier3",
        }
    }

    pub fn as_priority(&self) -> u8 {
        match self {
            Self::Tier0Hallucinated => 0,
            Self::Tier1Generic => 1,
            Self::Tier2Convention => 2,
            Self::Tier3Constraint => 3,
        }
    }

    pub fn value_multiplier(&self) -> f32 {
        match self {
            Self::Tier0Hallucinated => 0.0,
            Self::Tier1Generic => 0.3,
            Self::Tier2Convention => 0.6,
            Self::Tier3Constraint => 1.0,
        }
    }
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    pub fn with_responsibility(mut self, responsibility: impl Into<String>) -> Self {
        self.responsibility = responsibility.into();
        self
    }

    pub fn with_constraints(mut self, constraints: Vec<String>) -> Self {
        self.constraints = constraints;
        self
    }

    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.dependencies = deps;
        self
    }

    pub fn with_key_files(mut self, files: Vec<String>) -> Self {
        self.key_files = files;
        self
    }
}

/// Domain-level context for business-aware insights
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    pub fn with_business_rules(mut self, rules: Vec<String>) -> Self {
        self.business_rules = rules;
        self
    }

    pub fn with_terminology(mut self, terms: HashMap<String, String>) -> Self {
        self.terminology = terms;
        self
    }

    pub fn with_compliance(mut self, compliance: Vec<String>) -> Self {
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
            .with_responsibility("HTTP request handling")
            .with_constraints(vec!["Rate limiting required".into()])
            .with_key_files(vec!["src/api/mod.rs".into()]);

        assert_eq!(ctx.name, "api");
        assert!(!ctx.constraints.is_empty());
    }
}
