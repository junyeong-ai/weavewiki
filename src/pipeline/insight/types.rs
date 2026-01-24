//! Core types for the insight engine

use serde::{Deserialize, Serialize};

use super::ValueScore;

pub use crate::config::BusinessRuleType;
pub use crate::types::insight::TierClassification;

// ArtifactClassification re-exported from canonical location
pub use crate::types::insight::ArtifactClassification;

// ============================================================================
// Keyword Matching Utilities
// ============================================================================

/// Check if text contains any of the given keywords
pub fn text_contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|k| text.contains(k))
}

/// Raw insight extracted from analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    pub id: String,
    pub category: InsightCategory,
    pub title: String,
    pub description: String,
    pub prevention_info: Option<String>,
    pub evidence: Vec<String>,
    pub source: InsightSource,
    pub severity: Option<String>,
}

impl Insight {
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            category: InsightCategory::default(),
            title: title.into(),
            description: description.into(),
            prevention_info: None,
            evidence: Vec::new(),
            source: InsightSource::ManualAnnotation,
            severity: None,
        }
    }

    pub fn with_category(mut self, category: InsightCategory) -> Self {
        self.category = category;
        self
    }

    pub fn with_evidence(mut self, evidence: Vec<String>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn with_source(mut self, source: InsightSource) -> Self {
        self.source = source;
        self
    }

    pub fn with_severity(mut self, severity: impl Into<String>) -> Self {
        self.severity = Some(severity.into());
        self
    }

    pub fn with_prevention(mut self, info: impl Into<String>) -> Self {
        self.prevention_info = Some(info.into());
        self
    }
}

/// Insight with classification and scoring
#[derive(Debug, Clone)]
pub struct ExtractedInsight {
    pub insight: Insight,
    pub tier: TierClassification,
    pub artifact: ArtifactClassification,
    pub value: ValueScore,
}

impl ExtractedInsight {
    pub fn new(
        insight: Insight,
        tier: TierClassification,
        artifact: ArtifactClassification,
    ) -> Self {
        Self {
            insight,
            tier,
            artifact,
            value: ValueScore::default(),
        }
    }

    pub fn with_value(mut self, value: ValueScore) -> Self {
        self.value = value;
        self
    }
}

/// Category of insight
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsightCategory {
    #[default]
    TechnicalConstraint,
    BusinessRule,
    SecurityConstraint,
    PerformanceConstraint,
    DomainKnowledge,
    ArchitectureIntent,
    Gotcha,
    Compliance,
    // Additional variants used by generation code
    Workflow,
    Convention,
    Architecture,
    Security,
    Performance,
    General,
}

impl InsightCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TechnicalConstraint => "technical_constraint",
            Self::BusinessRule => "business_rule",
            Self::SecurityConstraint => "security_constraint",
            Self::PerformanceConstraint => "performance_constraint",
            Self::DomainKnowledge => "domain_knowledge",
            Self::ArchitectureIntent => "architecture_intent",
            Self::Gotcha => "gotcha",
            Self::Compliance => "compliance",
            Self::Workflow => "workflow",
            Self::Convention => "convention",
            Self::Architecture => "architecture",
            Self::Security => "security",
            Self::Performance => "performance",
            Self::General => "general",
        }
    }
}

/// Source of insight extraction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsightSource {
    MistakeAnalysis,
    ConstraintDetection,
    DomainAnalysis,
    PatternMining,
    ManualAnnotation,
}

/// Detected constraint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    pub name: String,
    pub constraint_type: ConstraintType,
    pub description: String,
    pub prevention: Option<String>,
    pub evidence: Vec<String>,
    pub severity: String,
}

/// Type of constraint
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    Concurrency,
    InitOrder,
    Security,
    Boundary,
    Performance,
}

impl ConstraintType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Concurrency => "concurrency",
            Self::InitOrder => "init_order",
            Self::Security => "security",
            Self::Boundary => "boundary",
            Self::Performance => "performance",
        }
    }
}

/// Extracted business rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessRule {
    pub name: String,
    pub description: String,
    pub rule_type: BusinessRuleType,
    pub consequence: Option<String>,
    pub evidence: Vec<String>,
}

/// Domain terminology
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Terminology {
    pub term: String,
    pub definition: String,
    pub usage_context: Option<String>,
    pub occurrences: Vec<String>,
}

/// Domain knowledge result
#[derive(Debug, Clone, Default)]
pub struct DomainKnowledge {
    pub business_rules: Vec<BusinessRule>,
    pub terminology: Vec<Terminology>,
    pub compliance_requirements: Vec<String>,
}

/// Generic knowledge item
#[derive(Debug, Clone)]
pub struct Knowledge {
    pub title: String,
    pub content: String,
    pub category: InsightCategory,
    pub evidence: Vec<String>,
}
