//! Cross-Reference Synthesis Module
//!
//! Synthesizes bottom-up and top-down analysis results:
//! - Hidden dependency discovery
//! - Cross-module constraint detection
//! - Architecture violation detection
//! - Domain-architecture mapping
//! - Policy-implementation alignment
//! - Tier classification

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ai::LlmProvider;
use crate::types::domain::DomainAnalysisResult;
use crate::types::{EvidenceLocation, Result};

use super::super::context::VerifiedFileRegistry;
use super::aggregator::AggregatedAnalysis;
use super::deep_analyzer::DiscoveredConstraint;

// =============================================================================
// SYNTHESIS RESULT TYPES
// =============================================================================

/// Complete synthesized insights from cross-reference analysis
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SynthesizedInsights {
    pub hidden_dependencies: Vec<HiddenDependency>,
    pub cross_constraints: Vec<CrossModuleConstraint>,
    pub architecture_violations: Vec<ArchitectureViolation>,
    pub policy_violations: Vec<PolicyViolation>,
    pub domain_arch_mapping: DomainArchMapping,
    pub tier3_insights: Vec<Tier3Insight>,
    pub tier2_insights: Vec<Tier2Insight>,
    pub coverage_analysis: CoverageAnalysis,
}

/// Hidden dependency discovered through cross-analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenDependency {
    pub from_module: String,
    pub to_module: String,
    pub dependency_type: HiddenDependencyType,
    pub description: String,
    pub evidence: Vec<EvidenceLocation>,
    pub impact: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HiddenDependencyType {
    RuntimeOnly,
    InitializationOrder,
    SharedState,
    EventBased,
    ConfigBased,
    EnvironmentBased,
    /// Unknown type - LLM should classify based on context
    #[default]
    Unknown,
}

impl std::fmt::Display for HiddenDependencyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Constraint that spans multiple modules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossModuleConstraint {
    pub name: String,
    pub description: String,
    pub affected_modules: Vec<String>,
    pub constraint_type: CrossConstraintType,
    pub enforcement: String,
    pub evidence: Vec<EvidenceLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CrossConstraintType {
    Ordering,
    Consistency,
    Transaction,
    Concurrency,
    ResourceSharing,
    DataFlow,
    /// Unknown type - LLM should classify based on context
    #[default]
    Unknown,
}

impl std::fmt::Display for CrossConstraintType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Architecture pattern violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureViolation {
    pub violation_type: ViolationType,
    pub description: String,
    pub from_layer: String,
    pub to_layer: String,
    pub evidence: Vec<EvidenceLocation>,
    pub suggested_fix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViolationType {
    LayerBypass,
    CircularDependency,
    WrongDirection,
    MissingAbstraction,
    LeakyAbstraction,
}

impl std::fmt::Display for ViolationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Policy implementation violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyViolation {
    pub policy_name: String,
    pub violation_description: String,
    pub expected_behavior: String,
    pub actual_behavior: String,
    pub affected_files: Vec<String>,
    pub severity: PolicyViolationSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyViolationSeverity {
    Critical,
    Major,
    Minor,
    Advisory,
}

/// Mapping between domain concepts and architecture
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomainArchMapping {
    pub entity_locations: HashMap<String, Vec<String>>,
    pub action_handlers: HashMap<String, Vec<String>>,
    pub workflow_modules: HashMap<String, Vec<String>>,
    pub policy_enforcers: HashMap<String, Vec<String>>,
}

/// Tier3 insight (essential constraint/gotcha)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tier3Insight {
    pub title: String,
    pub description: String,
    pub category: Tier3Category,
    pub evidence: Vec<EvidenceLocation>,
    pub prevention_guidance: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier3Category {
    HiddenDependency,
    OrderConstraint,
    ConcurrencyTrap,
    ResourceLeak,
    StateInvariant,
    SecurityBoundary,
    PerformanceTrap,
}

impl std::fmt::Display for Tier3Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Discovered insight from cross-analysis (used in generation prompts)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredInsight {
    pub title: String,
    pub description: String,
    pub category: String,
    pub evidence: Vec<EvidenceLocation>,
    pub prevention_guidance: String,
}

/// Tier2 insight (project convention)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tier2Insight {
    pub title: String,
    pub description: String,
    pub category: Tier2Category,
    pub scope: String,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier2Category {
    NamingConvention,
    FileOrganization,
    ErrorHandling,
    TestingPattern,
    DocumentationStyle,
    CodeStyle,
}

impl std::fmt::Display for Tier2Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NamingConvention => write!(f, "Naming Convention"),
            Self::FileOrganization => write!(f, "File Organization"),
            Self::ErrorHandling => write!(f, "Error Handling"),
            Self::TestingPattern => write!(f, "Testing Pattern"),
            Self::DocumentationStyle => write!(f, "Documentation Style"),
            Self::CodeStyle => write!(f, "Code Style"),
        }
    }
}

/// Coverage analysis from synthesis
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoverageAnalysis {
    pub modules_with_constraints: usize,
    pub modules_with_policies: usize,
    pub cross_module_coverage: f32,
    pub domain_coverage: f32,
    pub gaps: Vec<CoverageGap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageGap {
    pub gap_type: GapType,
    pub description: String,
    pub affected_area: String,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapType {
    MissingConstraint,
    MissingPolicy,
    UndocumentedWorkflow,
    OrphanedModule,
    MissingTests,
}

// =============================================================================
// CROSS SYNTHESIZER
// =============================================================================

pub struct CrossSynthesizer {
    _provider: Arc<dyn LlmProvider>,
}

impl CrossSynthesizer {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            _provider: provider,
        }
    }

    /// Perform cross-reference synthesis
    pub async fn synthesize(
        &self,
        aggregated: &AggregatedAnalysis,
        domain: &DomainAnalysisResult,
        registry: &VerifiedFileRegistry,
    ) -> Result<SynthesizedInsights> {
        let hidden_deps = self.find_hidden_dependencies(aggregated);
        let cross_constraints = self.find_cross_module_constraints(aggregated);
        let violations = self.detect_architecture_violations(aggregated);
        let policy_violations = self.check_policy_implementation_alignment(domain, aggregated);
        let domain_arch_mapping = self.map_domain_to_architecture(domain);

        let (tier3_insights, tier2_insights) = self.extract_high_value_insights(
            &hidden_deps,
            &cross_constraints,
            &violations,
            aggregated,
        );

        let coverage_analysis = self.analyze_coverage(aggregated, domain, registry);

        Ok(SynthesizedInsights {
            hidden_dependencies: hidden_deps,
            cross_constraints,
            architecture_violations: violations,
            policy_violations,
            domain_arch_mapping,
            tier3_insights,
            tier2_insights,
            coverage_analysis,
        })
    }

    fn find_hidden_dependencies(&self, aggregated: &AggregatedAnalysis) -> Vec<HiddenDependency> {
        let mut hidden_deps = Vec::new();

        for constraint in &aggregated.constraints {
            if constraint.cross_module {
                let modules = constraint.modules.to_vec();
                if modules.len() >= 2 {
                    hidden_deps.push(HiddenDependency {
                        from_module: modules[0].clone(),
                        to_module: modules[1].clone(),
                        dependency_type: Self::infer_dependency_type(&constraint.constraint),
                        description: constraint.constraint.description.clone(),
                        evidence: Self::convert_evidence(&constraint.constraint),
                        impact: format!(
                            "Affects {} modules: {}",
                            modules.len(),
                            modules.join(", ")
                        ),
                    });
                }
            }
        }

        hidden_deps
    }

    fn convert_evidence(constraint: &DiscoveredConstraint) -> Vec<EvidenceLocation> {
        constraint
            .evidence
            .iter()
            .map(|e| EvidenceLocation {
                file: e.file.clone(),
                start_line: e.line.unwrap_or(0),
                end_line: e.line.unwrap_or(0),
                start_column: None,
                end_column: None,
            })
            .collect()
    }

    /// Returns Unknown - accurate type requires semantic analysis by LLM.
    /// Programmatic keyword matching would be fragile across languages/frameworks.
    fn infer_dependency_type(_constraint: &DiscoveredConstraint) -> HiddenDependencyType {
        HiddenDependencyType::Unknown
    }

    fn find_cross_module_constraints(
        &self,
        aggregated: &AggregatedAnalysis,
    ) -> Vec<CrossModuleConstraint> {
        aggregated
            .constraints
            .iter()
            .filter(|c| c.cross_module)
            .map(|c| CrossModuleConstraint {
                name: c.constraint.title.clone(),
                description: c.constraint.description.clone(),
                affected_modules: c.modules.clone(),
                constraint_type: Self::infer_constraint_type(&c.constraint),
                enforcement: format!("{:?}", c.constraint.kind),
                evidence: Self::convert_evidence(&c.constraint),
            })
            .collect()
    }

    /// Returns Unknown - accurate type requires semantic analysis by LLM.
    /// Constraint types depend on domain context (e.g., "must call X before Y"
    /// could be Ordering, Transaction, or Concurrency depending on context).
    fn infer_constraint_type(_constraint: &DiscoveredConstraint) -> CrossConstraintType {
        CrossConstraintType::Unknown
    }

    fn detect_architecture_violations(
        &self,
        aggregated: &AggregatedAnalysis,
    ) -> Vec<ArchitectureViolation> {
        let mut violations = Vec::new();

        let mut module_deps: HashMap<String, HashSet<String>> = HashMap::new();
        for edge in &aggregated.dependency_graph.edges {
            module_deps
                .entry(edge.from.clone())
                .or_default()
                .insert(edge.to.clone());
        }

        for (from, deps) in &module_deps {
            for to in deps {
                if module_deps
                    .get(to)
                    .is_some_and(|back_deps| back_deps.contains(from))
                {
                    violations.push(ArchitectureViolation {
                        violation_type: ViolationType::CircularDependency,
                        description: format!("Circular dependency between {} and {}", from, to),
                        from_layer: from.clone(),
                        to_layer: to.clone(),
                        evidence: Vec::new(),
                        suggested_fix: format!(
                            "Break cycle by introducing abstraction between {} and {}",
                            from, to
                        ),
                    });
                }
            }
        }

        violations
    }

    fn check_policy_implementation_alignment(
        &self,
        domain: &DomainAnalysisResult,
        aggregated: &AggregatedAnalysis,
    ) -> Vec<PolicyViolation> {
        let mut violations = Vec::new();

        let constraint_titles: HashSet<_> = aggregated
            .constraints
            .iter()
            .map(|c| c.constraint.title.to_lowercase())
            .collect();

        for policy in &domain.policies {
            let policy_lower = policy.name.to_lowercase();
            let has_implementation = constraint_titles
                .iter()
                .any(|t| t.contains(&policy_lower) || policy_lower.contains(t));

            if !has_implementation {
                violations.push(PolicyViolation {
                    policy_name: policy.name.clone(),
                    violation_description: format!(
                        "Policy '{}' has no corresponding constraint implementation",
                        policy.name
                    ),
                    expected_behavior: policy.description.clone(),
                    actual_behavior: "No implementation found".to_string(),
                    affected_files: policy.related_modules.clone(),
                    severity: PolicyViolationSeverity::Major,
                });
            }
        }

        violations
    }

    fn map_domain_to_architecture(&self, domain: &DomainAnalysisResult) -> DomainArchMapping {
        let mut mapping = DomainArchMapping::default();

        for term in &domain.glossary.terms {
            if term.category == crate::types::domain::TermCategory::Entity {
                let locations: Vec<_> = term.occurrences.iter().map(|e| e.file.clone()).collect();
                mapping
                    .entity_locations
                    .insert(term.term.clone(), locations);
            }
        }

        for logic in &domain.core_logic {
            mapping
                .action_handlers
                .entry(logic.name.clone())
                .or_default()
                .push(logic.location.file.clone());
        }

        for workflow in &domain.workflows {
            mapping
                .workflow_modules
                .insert(workflow.name.clone(), workflow.involved_modules.clone());
        }

        for policy in &domain.policies {
            mapping
                .policy_enforcers
                .insert(policy.name.clone(), policy.related_modules.clone());
        }

        mapping
    }

    fn extract_high_value_insights(
        &self,
        hidden_deps: &[HiddenDependency],
        cross_constraints: &[CrossModuleConstraint],
        violations: &[ArchitectureViolation],
        aggregated: &AggregatedAnalysis,
    ) -> (Vec<Tier3Insight>, Vec<Tier2Insight>) {
        let mut tier3 = Vec::new();
        let mut tier2 = Vec::new();

        for dep in hidden_deps {
            tier3.push(Tier3Insight {
                title: format!(
                    "Hidden dependency: {} -> {}",
                    dep.from_module, dep.to_module
                ),
                description: dep.description.clone(),
                category: Tier3Category::HiddenDependency,
                evidence: dep.evidence.clone(),
                prevention_guidance: format!(
                    "Ensure {} is initialized before {}",
                    dep.to_module, dep.from_module
                ),
            });
        }

        for constraint in cross_constraints {
            tier3.push(Tier3Insight {
                title: constraint.name.clone(),
                description: constraint.description.clone(),
                category: match constraint.constraint_type {
                    CrossConstraintType::Ordering => Tier3Category::OrderConstraint,
                    CrossConstraintType::Concurrency => Tier3Category::ConcurrencyTrap,
                    CrossConstraintType::ResourceSharing => Tier3Category::ResourceLeak,
                    _ => Tier3Category::StateInvariant,
                },
                evidence: constraint.evidence.clone(),
                prevention_guidance: format!(
                    "Affects modules: {}",
                    constraint.affected_modules.join(", ")
                ),
            });
        }

        for violation in violations {
            tier3.push(Tier3Insight {
                title: format!(
                    "{:?}: {} -> {}",
                    violation.violation_type, violation.from_layer, violation.to_layer
                ),
                description: violation.description.clone(),
                category: Tier3Category::StateInvariant,
                evidence: violation.evidence.clone(),
                prevention_guidance: violation.suggested_fix.clone(),
            });
        }

        if let Some(naming) = &aggregated.conventions.primary_naming {
            tier2.push(Tier2Insight {
                title: format!("Primary naming convention: {:?}", naming),
                description: format!("The project primarily uses {:?} naming convention", naming),
                category: Tier2Category::NamingConvention,
                scope: "Project-wide".to_string(),
                examples: Vec::new(),
            });
        }

        if let Some(error_style) = &aggregated.conventions.primary_error_handling {
            tier2.push(Tier2Insight {
                title: format!("Error handling style: {:?}", error_style),
                description: format!("The project uses {:?} for error handling", error_style),
                category: Tier2Category::ErrorHandling,
                scope: "Project-wide".to_string(),
                examples: Vec::new(),
            });
        }

        (tier3, tier2)
    }

    fn analyze_coverage(
        &self,
        aggregated: &AggregatedAnalysis,
        domain: &DomainAnalysisResult,
        registry: &VerifiedFileRegistry,
    ) -> CoverageAnalysis {
        let modules_with_constraints = aggregated
            .constraints
            .iter()
            .flat_map(|c| c.modules.iter())
            .collect::<HashSet<_>>()
            .len();

        let modules_with_policies = domain
            .policies
            .iter()
            .flat_map(|p| p.related_modules.iter())
            .collect::<HashSet<_>>()
            .len();

        let total_modules = registry.modules().len();
        let cross_module_coverage = if total_modules > 0 {
            modules_with_constraints as f32 / total_modules as f32
        } else {
            0.0
        };

        let domain_coverage = if !domain.is_empty() { 1.0 } else { 0.0 };

        let mut gaps = Vec::new();

        if aggregated.constraints.is_empty() {
            gaps.push(CoverageGap {
                gap_type: GapType::MissingConstraint,
                description: "No constraints discovered".to_string(),
                affected_area: "Project-wide".to_string(),
                suggested_action: "Add more detailed analysis of code patterns".to_string(),
            });
        }

        if domain.policies.is_empty() {
            gaps.push(CoverageGap {
                gap_type: GapType::MissingPolicy,
                description: "No domain policies identified".to_string(),
                affected_area: "Domain layer".to_string(),
                suggested_action: "Review validation and authorization patterns".to_string(),
            });
        }

        if domain.workflows.is_empty() {
            gaps.push(CoverageGap {
                gap_type: GapType::UndocumentedWorkflow,
                description: "No business workflows detected".to_string(),
                affected_area: "Business logic".to_string(),
                suggested_action: "Look for state machines and process flows".to_string(),
            });
        }

        CoverageAnalysis {
            modules_with_constraints,
            modules_with_policies,
            cross_module_coverage,
            domain_coverage,
            gaps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::LlmResponse;
    use crate::types::Severity;
    use serde_json::{Value, json};

    #[test]
    fn test_infer_dependency_type_returns_unknown() {
        use super::super::deep_analyzer::ConstraintKind;

        let constraint = DiscoveredConstraint {
            kind: ConstraintKind::HiddenDependency,
            title: "Test".to_string(),
            description: "Must initialize config before startup".to_string(),
            rationale: "Prevents undefined behavior".to_string(),
            severity: Severity::High,
            evidence: Vec::new(),
        };

        // Returns Unknown - accurate classification requires LLM semantic analysis
        let dep_type = CrossSynthesizer::infer_dependency_type(&constraint);
        assert_eq!(dep_type, HiddenDependencyType::Unknown);
    }

    #[test]
    fn test_coverage_gap_detection() {
        let domain = DomainAnalysisResult::default();
        let aggregated = AggregatedAnalysis::default();
        let registry = VerifiedFileRegistry::default();

        let synthesizer = CrossSynthesizer::new(Arc::new(MockProvider));
        let coverage = synthesizer.analyze_coverage(&aggregated, &domain, &registry);

        assert!(!coverage.gaps.is_empty());
    }

    struct MockProvider;

    #[async_trait::async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _schema: &Value) -> Result<LlmResponse> {
            Ok(LlmResponse::content_only(json!({})))
        }

        fn name(&self) -> &str {
            "mock"
        }

        fn model(&self) -> &str {
            "mock-model"
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(true)
        }
    }
}
