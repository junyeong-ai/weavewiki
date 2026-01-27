//! Domain Analysis Types
//!
//! Types for extracting and representing domain-specific knowledge:
//! - Core policies (validation, authorization, business rules, invariants)
//! - Core domain logic (calculations, transformations, decisions)
//! - Domain terminology (entities, actions, states)
//! - Business workflows (sequences, state machines, transactions)

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

use super::EvidenceLocation;

// =============================================================================
// DOMAIN POLICY TYPES
// =============================================================================

/// Domain core policy extracted from codebase
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DomainPolicy {
    pub name: String,
    pub description: String,
    pub policy_type: PolicyType,
    pub enforcement: EnforcementLevel,
    pub evidence: Vec<EvidenceLocation>,
    pub related_modules: Vec<String>,
}

impl DomainPolicy {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        policy_type: PolicyType,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            policy_type,
            enforcement: EnforcementLevel::Strict,
            evidence: Vec::new(),
            related_modules: Vec::new(),
        }
    }

    pub fn with_enforcement(mut self, level: EnforcementLevel) -> Self {
        self.enforcement = level;
        self
    }

    pub fn with_evidence(mut self, evidence: Vec<EvidenceLocation>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn with_modules(mut self, modules: Vec<String>) -> Self {
        self.related_modules = modules;
        self
    }
}

/// Type of domain policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyType {
    /// Input validation rules
    Validation,
    /// Permission and access control rules
    Authorization,
    /// Business logic rules
    BusinessRule,
    /// Invariant conditions that must always hold
    Invariant,
    /// State transition rules
    StateTransition,
    /// Data integrity constraints
    DataIntegrity,
    /// Rate limiting or throttling rules
    RateLimiting,
    /// Audit and logging requirements
    Audit,
}

impl PolicyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Validation => "validation",
            Self::Authorization => "authorization",
            Self::BusinessRule => "business_rule",
            Self::Invariant => "invariant",
            Self::StateTransition => "state_transition",
            Self::DataIntegrity => "data_integrity",
            Self::RateLimiting => "rate_limiting",
            Self::Audit => "audit",
        }
    }
}

impl fmt::Display for PolicyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Enforcement level for policies
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementLevel {
    /// Must be followed, failure results in error
    #[default]
    Strict,
    /// Should be followed, issues warning if violated
    Warning,
    /// Recommended but not enforced
    Advisory,
}

impl EnforcementLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Warning => "warning",
            Self::Advisory => "advisory",
        }
    }
}

impl fmt::Display for EnforcementLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// CORE DOMAIN LOGIC TYPES
// =============================================================================

/// Core domain logic identified in the codebase
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CoreDomainLogic {
    pub name: String,
    pub description: String,
    pub logic_type: DomainLogicType,
    pub location: EvidenceLocation,
    pub dependencies: Vec<String>,
    pub business_impact: String,
}

impl CoreDomainLogic {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        logic_type: DomainLogicType,
        location: EvidenceLocation,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            logic_type,
            location,
            dependencies: Vec::new(),
            business_impact: String::new(),
        }
    }

    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.dependencies = deps;
        self
    }

    pub fn with_business_impact(mut self, impact: impl Into<String>) -> Self {
        self.business_impact = impact.into();
        self
    }
}

/// Type of domain logic
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DomainLogicType {
    /// Calculation logic (pricing, scoring, statistics)
    Calculation,
    /// Data transformation
    Transformation,
    /// Aggregation logic
    Aggregation,
    /// Decision-making logic
    Decision,
    /// Workflow orchestration
    Orchestration,
    /// External system integration
    Integration,
    /// Data validation and sanitization
    Sanitization,
    /// Event handling and dispatch
    EventHandling,
}

impl DomainLogicType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Calculation => "calculation",
            Self::Transformation => "transformation",
            Self::Aggregation => "aggregation",
            Self::Decision => "decision",
            Self::Orchestration => "orchestration",
            Self::Integration => "integration",
            Self::Sanitization => "sanitization",
            Self::EventHandling => "event_handling",
        }
    }
}

impl fmt::Display for DomainLogicType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// DOMAIN TERMINOLOGY TYPES
// =============================================================================

/// Domain glossary containing terminology
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DomainGlossary {
    pub terms: Vec<DomainTerm>,
    pub abbreviations: Vec<Abbreviation>,
    pub relationships: Vec<TermRelationship>,
}

impl DomainGlossary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_term(&mut self, term: DomainTerm) {
        if !self.terms.iter().any(|t| t.term == term.term) {
            self.terms.push(term);
        }
    }

    pub fn add_abbreviation(&mut self, abbr: Abbreviation) {
        if !self.abbreviations.iter().any(|a| a.short == abbr.short) {
            self.abbreviations.push(abbr);
        }
    }

    pub fn add_relationship(&mut self, rel: TermRelationship) {
        self.relationships.push(rel);
    }

    pub fn find_term(&self, name: &str) -> Option<&DomainTerm> {
        let lower = name.to_lowercase();
        self.terms.iter().find(|t| {
            t.term.to_lowercase() == lower || t.synonyms.iter().any(|s| s.to_lowercase() == lower)
        })
    }
}

/// Single domain term with definition
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DomainTerm {
    pub term: String,
    pub definition: String,
    pub category: TermCategory,
    pub occurrences: Vec<EvidenceLocation>,
    pub synonyms: Vec<String>,
}

impl DomainTerm {
    pub fn new(
        term: impl Into<String>,
        definition: impl Into<String>,
        category: TermCategory,
    ) -> Self {
        Self {
            term: term.into(),
            definition: definition.into(),
            category,
            occurrences: Vec::new(),
            synonyms: Vec::new(),
        }
    }

    pub fn with_occurrences(mut self, occurrences: Vec<EvidenceLocation>) -> Self {
        self.occurrences = occurrences;
        self
    }

    pub fn with_synonyms(mut self, synonyms: Vec<String>) -> Self {
        self.synonyms = synonyms;
        self
    }
}

/// Category of domain term
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TermCategory {
    /// Core entity (User, Order, Product)
    Entity,
    /// Action (Purchase, Cancel, Refund)
    Action,
    /// State (Pending, Active, Completed)
    State,
    /// Metric (Revenue, Conversion, Retention)
    Metric,
    /// Role (Admin, Customer, Vendor)
    Role,
    /// Concept (Session, Transaction, Subscription)
    Concept,
    /// Event (OrderCreated, PaymentReceived)
    Event,
}

impl TermCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::Action => "action",
            Self::State => "state",
            Self::Metric => "metric",
            Self::Role => "role",
            Self::Concept => "concept",
            Self::Event => "event",
        }
    }
}

impl fmt::Display for TermCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Abbreviation used in the codebase
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Abbreviation {
    pub short: String,
    pub full: String,
    pub context: Option<String>,
}

impl Abbreviation {
    pub fn new(short: impl Into<String>, full: impl Into<String>) -> Self {
        Self {
            short: short.into(),
            full: full.into(),
            context: None,
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

/// Relationship between domain terms
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TermRelationship {
    pub from_term: String,
    pub to_term: String,
    pub relationship_type: TermRelationType,
}

impl TermRelationship {
    pub fn new(from: impl Into<String>, to: impl Into<String>, rel_type: TermRelationType) -> Self {
        Self {
            from_term: from.into(),
            to_term: to.into(),
            relationship_type: rel_type,
        }
    }
}

/// Type of relationship between terms
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TermRelationType {
    /// A is a type of B (inheritance)
    IsA,
    /// A has B (composition)
    HasA,
    /// A belongs to B
    BelongsTo,
    /// A depends on B
    DependsOn,
    /// A triggers B
    Triggers,
    /// A and B are related
    RelatedTo,
}

// =============================================================================
// BUSINESS WORKFLOW TYPES
// =============================================================================

/// Business workflow identified in the codebase
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BusinessWorkflow {
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub entry_points: Vec<EvidenceLocation>,
    pub involved_modules: Vec<String>,
    pub triggers: Vec<String>,
}

impl BusinessWorkflow {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            steps: Vec::new(),
            entry_points: Vec::new(),
            involved_modules: Vec::new(),
            triggers: Vec::new(),
        }
    }

    pub fn add_step(&mut self, step: WorkflowStep) {
        self.steps.push(step);
    }

    pub fn with_entry_points(mut self, entry_points: Vec<EvidenceLocation>) -> Self {
        self.entry_points = entry_points;
        self
    }

    pub fn with_modules(mut self, modules: Vec<String>) -> Self {
        self.involved_modules = modules;
        self
    }

    pub fn with_triggers(mut self, triggers: Vec<String>) -> Self {
        self.triggers = triggers;
        self
    }
}

/// Single step in a business workflow
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowStep {
    pub order: usize,
    pub name: String,
    pub action: String,
    pub next_steps: Vec<String>,
    pub conditions: Vec<String>,
    pub is_terminal: bool,
}

impl WorkflowStep {
    pub fn new(order: usize, name: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            order,
            name: name.into(),
            action: action.into(),
            next_steps: Vec::new(),
            conditions: Vec::new(),
            is_terminal: false,
        }
    }

    pub fn terminal(order: usize, name: impl Into<String>, action: impl Into<String>) -> Self {
        let mut step = Self::new(order, name, action);
        step.is_terminal = true;
        step
    }

    pub fn with_next(mut self, next_steps: Vec<String>) -> Self {
        self.next_steps = next_steps;
        self
    }

    pub fn with_conditions(mut self, conditions: Vec<String>) -> Self {
        self.conditions = conditions;
        self
    }
}

// =============================================================================
// DOMAIN ANALYSIS RESULT
// =============================================================================

/// Complete domain analysis result
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DomainAnalysisResult {
    pub policies: Vec<DomainPolicy>,
    pub core_logic: Vec<CoreDomainLogic>,
    pub glossary: DomainGlossary,
    pub workflows: Vec<BusinessWorkflow>,
    pub domain_type: Option<String>,
    pub confidence: f32,
}

impl DomainAnalysisResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
            && self.core_logic.is_empty()
            && self.glossary.terms.is_empty()
            && self.workflows.is_empty()
    }

    pub fn merge(&mut self, other: DomainAnalysisResult) {
        self.policies.extend(other.policies);
        self.core_logic.extend(other.core_logic);

        for term in other.glossary.terms {
            self.glossary.add_term(term);
        }
        for abbr in other.glossary.abbreviations {
            self.glossary.add_abbreviation(abbr);
        }
        for rel in other.glossary.relationships {
            self.glossary.add_relationship(rel);
        }

        self.workflows.extend(other.workflows);

        if self.domain_type.is_none() {
            self.domain_type = other.domain_type;
        }

        self.confidence = (self.confidence + other.confidence) / 2.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_policy_creation() {
        let policy = DomainPolicy::new(
            "ValidateUserInput",
            "All user input must be validated",
            PolicyType::Validation,
        )
        .with_enforcement(EnforcementLevel::Strict);

        assert_eq!(policy.name, "ValidateUserInput");
        assert_eq!(policy.policy_type, PolicyType::Validation);
        assert_eq!(policy.enforcement, EnforcementLevel::Strict);
    }

    #[test]
    fn test_domain_glossary() {
        let mut glossary = DomainGlossary::new();

        glossary.add_term(DomainTerm::new(
            "User",
            "A registered user",
            TermCategory::Entity,
        ));
        glossary.add_term(DomainTerm::new(
            "Order",
            "A purchase order",
            TermCategory::Entity,
        ));

        assert_eq!(glossary.terms.len(), 2);
        assert!(glossary.find_term("user").is_some());
        assert!(glossary.find_term("Order").is_some());
    }

    #[test]
    fn test_workflow_step() {
        let step = WorkflowStep::new(1, "CreateOrder", "Create a new order")
            .with_next(vec!["ValidateOrder".into(), "RejectOrder".into()])
            .with_conditions(vec!["User is authenticated".into()]);

        assert_eq!(step.order, 1);
        assert_eq!(step.next_steps.len(), 2);
        assert!(!step.is_terminal);
    }

    #[test]
    fn test_domain_analysis_merge() {
        let mut result1 = DomainAnalysisResult::new();
        result1.policies.push(DomainPolicy::new(
            "Policy1",
            "First policy",
            PolicyType::Validation,
        ));
        result1.confidence = 0.8;

        let mut result2 = DomainAnalysisResult::new();
        result2.policies.push(DomainPolicy::new(
            "Policy2",
            "Second policy",
            PolicyType::Authorization,
        ));
        result2.confidence = 0.6;

        result1.merge(result2);

        assert_eq!(result1.policies.len(), 2);
        assert!((result1.confidence - 0.7).abs() < 0.001);
    }
}
