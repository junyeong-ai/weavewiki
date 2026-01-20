//! Core types for the insight engine

use serde::{Deserialize, Serialize};

use super::{ArtifactClassification, TierClassification, ValueScore};

// Re-export BusinessRuleType from config (single source of truth)
pub use crate::config::BusinessRuleType;

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

/// Insight with classification and scoring
#[derive(Debug, Clone)]
pub struct ExtractedInsight {
    pub insight: Insight,
    pub tier: TierClassification,
    pub artifact: ArtifactClassification,
    pub value: ValueScore,
}

/// Category of insight
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsightCategory {
    TechnicalConstraint,
    BusinessRule,
    SecurityConstraint,
    PerformanceConstraint,
    DomainKnowledge,
    ArchitectureIntent,
    Gotcha,
    Compliance,
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
