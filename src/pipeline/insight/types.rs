//! Core types for the insight engine

use serde::{Deserialize, Serialize};

pub use crate::config::BusinessRuleType;
pub use crate::types::insight::TierClassification;

// ArtifactClassification re-exported from canonical location
pub use crate::types::insight::ArtifactClassification;

// ============================================================================
// Value Score
// ============================================================================

/// Value score for an insight (3-dimensional quality metric)
#[derive(Debug, Clone, Default)]
pub struct ValueScore {
    /// How well this prevents AI mistakes (0.0 - 1.0)
    pub mistake_prevention: f32,
    /// How hard this is to discover from code (0.0 - 1.0)
    pub discoverability: f32,
    /// How well this fits the artifact type (0.0 - 1.0)
    pub artifact_fitness: f32,
    /// Overall weighted score
    pub overall: f32,
}

impl ValueScore {
    pub fn new(mistake_prevention: f32, discoverability: f32, artifact_fitness: f32) -> Self {
        Self {
            mistake_prevention,
            discoverability,
            artifact_fitness,
            overall: (mistake_prevention + discoverability + artifact_fitness) / 3.0,
        }
    }
}

// ============================================================================
// Core Insight Types
// ============================================================================

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

    pub fn category(mut self, category: InsightCategory) -> Self {
        self.category = category;
        self
    }

    pub fn evidence(mut self, evidence: Vec<String>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn source(mut self, source: InsightSource) -> Self {
        self.source = source;
        self
    }

    pub fn severity(mut self, severity: impl Into<String>) -> Self {
        self.severity = Some(severity.into());
        self
    }

    pub fn prevention(mut self, info: impl Into<String>) -> Self {
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

    pub fn value(mut self, value: ValueScore) -> Self {
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

/// Type of constraint - extensible for diverse languages/frameworks
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintType {
    Concurrency,
    InitOrder,
    Security,
    Boundary,
    Performance,
    // Extended types for diverse languages/frameworks
    Memory,          // C++, Rust memory management
    ThreadSafety,    // Java, C# thread safety
    NullHandling,    // C#, Kotlin null safety
    TypeSafety,      // TypeScript, Flow type constraints
    StateManagement, // React, Vue state constraints
    Transaction,     // Database transaction constraints
    ApiContract,     // REST/GraphQL API constraints
    /// Catch-all for domain-specific or language-specific constraints
    /// LLM can classify freely without being limited to predefined types
    #[serde(other)]
    Other,
}

impl ConstraintType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Concurrency => "concurrency",
            Self::InitOrder => "init_order",
            Self::Security => "security",
            Self::Boundary => "boundary",
            Self::Performance => "performance",
            Self::Memory => "memory",
            Self::ThreadSafety => "thread_safety",
            Self::NullHandling => "null_handling",
            Self::TypeSafety => "type_safety",
            Self::StateManagement => "state_management",
            Self::Transaction => "transaction",
            Self::ApiContract => "api_contract",
            Self::Other => "other",
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
