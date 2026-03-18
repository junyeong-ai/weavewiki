//! Generation Context Module
//!
//! Provides unified context for LLM-first artifact generation.
//! All analysis data is accessible without filtering - LLM decides relevance.

mod budget;
mod fmt;

pub use budget::{
    BudgetedSections, OmittedReference, SummarizationLevel, Tier1Sections, Tier2Sections,
    Tier3Sections,
};

use crate::pipeline::analysis::cross_synthesis::{
    ArchitectureViolation, CrossModuleConstraint, HiddenDependency, Tier3Insight,
};
use crate::pipeline::analysis::{
    DeepAnalysisResult, ModuleSummary, PatternInstance, SynthesizedAnalysis, SynthesizedInsights,
    VerifiedReferencePool,
};
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::phases::{
    constraint_extraction::ExtractedConstraints,
    service_detection::DetectedService,
};
use crate::types::{InferredConventions, ProjectDetection};
use crate::types::domain::{
    BusinessWorkflow, CoreDomainLogic, DomainAnalysisResult, DomainLogicType, DomainPolicy,
    EnforcementLevel, PolicyType,
};
use crate::types::module_map::{DetectedModule, Domain, ModuleGroup, TechStack};
use crate::pipeline::evidence::artifact_ref;

#[derive(Debug, Clone)]
pub struct FileContext {
    pub path: String,
    pub abstractions: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DomainKnowledge {
    pub policies: Vec<String>,
    pub core_logic: Vec<String>,
    pub terminology: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PolicyContext {
    pub name: String,
    pub description: String,
    pub policy_type: PolicyType,
    pub enforcement: EnforcementLevel,
    pub affected_modules: Vec<String>,
    pub evidence: Vec<String>,
}

impl From<&DomainPolicy> for PolicyContext {
    fn from(p: &DomainPolicy) -> Self {
        Self {
            name: p.name.clone(),
            description: p.description.clone(),
            policy_type: p.policy_type,
            enforcement: p.enforcement,
            affected_modules: p.related_modules.clone(),
            evidence: p
                .evidence
                .iter()
                .map(|e| artifact_ref(&e.file, e.start_line))
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogicContext {
    pub name: String,
    pub description: String,
    pub logic_type: DomainLogicType,
    pub business_impact: String,
    pub location: String,
    pub dependencies: Vec<String>,
}

impl From<&CoreDomainLogic> for LogicContext {
    fn from(l: &CoreDomainLogic) -> Self {
        Self {
            name: l.name.clone(),
            description: l.description.clone(),
            logic_type: l.logic_type,
            business_impact: l.business_impact.clone(),
            location: artifact_ref(&l.location.file, l.location.start_line),
            dependencies: l.dependencies.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowContext {
    pub name: String,
    pub description: String,
    pub step_count: usize,
    pub involved_modules: Vec<String>,
    pub triggers: Vec<String>,
    pub entry_points: Vec<String>,
}

impl From<&BusinessWorkflow> for WorkflowContext {
    fn from(w: &BusinessWorkflow) -> Self {
        Self {
            name: w.name.clone(),
            description: w.description.clone(),
            step_count: w.steps.len(),
            involved_modules: w.involved_modules.clone(),
            triggers: w.triggers.clone(),
            entry_points: w
                .entry_points
                .iter()
                .map(|e| artifact_ref(&e.file, e.start_line))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EnrichedDomainKnowledge {
    pub policies: Vec<PolicyContext>,
    pub core_logic: Vec<LogicContext>,
    pub workflows: Vec<WorkflowContext>,
    pub terminology: Vec<String>,
    pub domain_type: Option<String>,
}

const DOMAIN_KEYWORDS: &[(&str, &[&str])] = &[
    ("FinTech/Financial", &["payment", "transaction", "financial", "billing"]),
    ("Healthcare", &["patient", "hipaa", "medical", "health"]),
    ("E-commerce", &["cart", "order", "inventory", "product"]),
];

impl EnrichedDomainKnowledge {
    pub fn from_analysis(analysis: &DomainAnalysisResult) -> Self {
        Self {
            policies: analysis.policies.iter().map(PolicyContext::from).collect(),
            core_logic: analysis.core_logic.iter().map(LogicContext::from).collect(),
            workflows: analysis.workflows.iter().map(WorkflowContext::from).collect(),
            terminology: analysis.glossary.terms.iter().map(|t| t.term.clone()).collect(),
            domain_type: analysis.domain_type.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
            && self.core_logic.is_empty()
            && self.workflows.is_empty()
            && self.terminology.is_empty()
    }

    pub fn infer_domain_from_policies(&self) -> Option<&'static str> {
        if self.domain_type.is_some() {
            return None;
        }
        for policy in &self.policies {
            let desc = policy.description.to_lowercase();
            for (domain, keywords) in DOMAIN_KEYWORDS {
                if keywords.iter().any(|k| desc.contains(k)) {
                    return Some(domain);
                }
            }
        }
        None
    }
}

/// A domain-specific custom rule category discovered during analysis.
#[derive(Debug, Clone)]
pub struct DiscoveredCategory {
    pub name: String,
    pub description: String,
    pub suggested_priority: u8,
    pub trigger_patterns: Vec<String>,
}

impl DiscoveredCategory {
    pub fn from_cross_insights(insights: &SynthesizedInsights) -> Vec<Self> {
        insights
            .cross_constraints
            .iter()
            .filter(|c| c.affected_modules.len() >= 3)
            .map(|constraint| DiscoveredCategory {
                name: format!(
                    "cross-{}",
                    constraint.name.to_lowercase().replace(' ', "-")
                ),
                description: format!(
                    "Cross-cutting constraint: {} (affects {} modules)",
                    constraint.name,
                    constraint.affected_modules.len()
                ),
                suggested_priority: 75,
                trigger_patterns: constraint.affected_modules.clone(),
            })
            .collect()
    }

    pub fn from_services(
        services: &[crate::pipeline::phases::service_detection::DetectedService],
    ) -> Vec<Self> {
        services
            .iter()
            .map(|s| DiscoveredCategory {
                name: format!("service-{}", s.name.to_lowercase().replace(' ', "-")),
                description: format!("Rules for the {} service ({})", s.name, s.service_type),
                suggested_priority: 70,
                trigger_patterns: vec![format!("{}/**", s.path)],
            })
            .collect()
    }
}

pub struct GenerationContext<'a> {
    pub detection: &'a ProjectDetection,
    pub tech_stack: &'a TechStack,
    pub project_name: &'a str,
    pub modules: &'a [DetectedModule],
    pub groups: &'a [ModuleGroup],
    pub domains: &'a [Domain],
    pub deep_analysis: Option<&'a DeepAnalysisResult>,
    pub synthesis: Option<&'a SynthesizedAnalysis>,
    pub domain_analysis: Option<&'a DomainAnalysisResult>,
    pub cross_insights: Option<&'a SynthesizedInsights>,
    pub conventions: &'a InferredConventions,
    pub constraints: &'a ExtractedConstraints,
    pub file_registry: &'a VerifiedFileRegistry,
    pub reference_pool: Option<VerifiedReferencePool>,
    pub budget: Option<BudgetedSections>,
    pub services: &'a [DetectedService],
    /// Generated skill names, populated after skill generation phase.
    pub generated_skill_names: Vec<String>,
    /// Domain-specific categories discovered during analysis for LLM-guided generation.
    pub discovered_categories: Vec<DiscoveredCategory>,
}

impl<'a> GenerationContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        detection: &'a ProjectDetection,
        tech_stack: &'a TechStack,
        project_name: &'a str,
        modules: &'a [DetectedModule],
        groups: &'a [ModuleGroup],
        domains: &'a [Domain],
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        file_registry: &'a VerifiedFileRegistry,
    ) -> Self {
        Self {
            detection,
            tech_stack,
            project_name,
            modules,
            groups,
            domains,
            deep_analysis: None,
            synthesis: None,
            domain_analysis: None,
            cross_insights: None,
            conventions,
            constraints,
            file_registry,
            reference_pool: None,
            budget: None,
            services: &[],
            generated_skill_names: Vec::new(),
            discovered_categories: Vec::new(),
        }
    }

    pub fn deep_analysis(mut self, analysis: &'a DeepAnalysisResult) -> Self {
        self.deep_analysis = Some(analysis);
        self
    }

    pub fn synthesis(mut self, synthesis: &'a SynthesizedAnalysis) -> Self {
        self.synthesis = Some(synthesis);
        self
    }

    pub fn domain_analysis(mut self, domain: &'a DomainAnalysisResult) -> Self {
        self.domain_analysis = Some(domain);
        self
    }

    pub fn cross_insights(mut self, insights: &'a SynthesizedInsights) -> Self {
        self.cross_insights = Some(insights);
        self
    }

    pub fn services(mut self, services: &'a [DetectedService]) -> Self {
        self.services = services;
        self
    }

    pub fn available_skill_names(&self) -> &[String] {
        &self.generated_skill_names
    }

    // =========================================================================
    // LLM-First Methods - No filtering, no truncation
    // =========================================================================

    pub fn module_summaries(&self) -> Vec<ModuleSummary> {
        use crate::pipeline::analysis::types::PatternSummary;

        if let Some(synth) = self.synthesis
            && !synth.modules.is_empty()
        {
            return synth
                .modules
                .iter()
                .map(|m| ModuleSummary {
                    module_path: m.path.clone(),
                    responsibility: m.responsibility.clone(),
                    file_count: m.reference_count,
                    total_lines: 0,
                    patterns: m
                        .patterns
                        .iter()
                        .map(|p| PatternSummary {
                            name: p.clone(),
                            category: String::new(),
                            description: String::new(),
                            locations: vec![],
                        })
                        .collect(),
                    constraints: vec![],
                    gotchas: vec![],
                    key_abstractions: m.public_items.clone(),
                    internal_deps: m.internal_deps.clone(),
                    external_deps: vec![],
                    public_api: m.public_items.clone(),
                    confidence: 0.0,
                    source_chunk_ids: vec![],
                })
                .collect();
        }
        Vec::new()
    }

    pub fn all_patterns(&self) -> Vec<&PatternInstance> {
        self.deep_analysis
            .map(|d| d.patterns.iter().collect())
            .unwrap_or_default()
    }

    pub fn all_discovered_insights(&self) -> Vec<&Tier3Insight> {
        self.cross_insights
            .map(|c| c.tier3_insights.iter().collect())
            .unwrap_or_default()
    }

    pub fn all_hidden_dependencies(&self) -> Vec<&HiddenDependency> {
        self.cross_insights
            .map(|c| c.hidden_dependencies.iter().collect())
            .unwrap_or_default()
    }

    pub fn all_architecture_violations(&self) -> Vec<&ArchitectureViolation> {
        self.cross_insights
            .map(|c| c.architecture_violations.iter().collect())
            .unwrap_or_default()
    }

    pub fn all_cross_constraints(&self) -> Vec<&CrossModuleConstraint> {
        self.cross_insights
            .map(|c| c.cross_constraints.iter().collect())
            .unwrap_or_default()
    }

    pub fn domain_knowledge(&self) -> Option<DomainKnowledge> {
        self.domain_analysis.map(|d| DomainKnowledge {
            policies: d.policies.iter().map(|p| p.description.clone()).collect(),
            core_logic: d.core_logic.iter().map(|c| c.description.clone()).collect(),
            terminology: d.glossary.terms.iter().map(|t| t.term.clone()).collect(),
        })
    }

    pub fn enriched_domain_knowledge(&self) -> Option<EnrichedDomainKnowledge> {
        self.domain_analysis.map(EnrichedDomainKnowledge::from_analysis)
    }

    pub fn verified_references_for_skill(&self, skill_name: &str) -> Vec<String> {
        self.reference_pool
            .as_ref()
            .map(|p| p.references_for_skill(skill_name))
            .unwrap_or_default()
    }

    pub fn all_files_with_context(&self) -> Vec<FileContext> {
        self.file_registry
            .all_files()
            .map(|path| {
                let abstractions: Vec<String> = self
                    .deep_analysis
                    .map(|d| {
                        d.key_abstractions
                            .iter()
                            .filter(|a| a.file == path.as_str())
                            .map(|a| format!("{}:{} - {}", a.file, a.line, a.name))
                            .collect()
                    })
                    .unwrap_or_default();
                FileContext { path: path.clone(), abstractions }
            })
            .collect()
    }

    // =========================================================================
    // Statistics and Metadata
    // =========================================================================

    pub fn constraint_count(&self) -> usize {
        self.constraints.gotchas.len()
            + self.constraints.hidden_dependencies.len()
            + self.constraints.anti_patterns.len()
            + self.constraints.implicit_rules.len()
            + self.constraints.complex_workflows.len()
    }

    pub fn pattern_count(&self) -> usize {
        self.deep_analysis.map(|d| d.patterns.len()).unwrap_or(0)
    }

    pub fn detected_frameworks(&self) -> Vec<String> {
        self.tech_stack
            .frameworks
            .iter()
            .map(|f| f.name.clone())
            .collect()
    }

    pub fn overall_confidence(&self) -> f32 {
        self.synthesis.map(|s| s.confidence.overall).unwrap_or(0.0)
    }

    pub fn discovered_insights_by_category(
        &self,
        category: crate::pipeline::analysis::Tier3Category,
    ) -> Vec<&Tier3Insight> {
        self.cross_insights
            .map(|c| {
                c.tier3_insights
                    .iter()
                    .filter(|ti| ti.category == category)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn hidden_deps_for_module(&self, module: &str) -> Vec<&HiddenDependency> {
        self.cross_insights
            .map(|c| {
                c.hidden_dependencies
                    .iter()
                    .filter(|hd| hd.from_module == module || hd.to_module == module)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Build a `RuleGenerationContext` from this context.
    pub fn to_rule_context(&self) -> super::rules::RuleGenerationContext<'_> {
        super::rules::RuleGenerationContext {
            detection: self.detection,
            conventions: self.conventions,
            constraints: self.constraints,
            tech_stack: self.tech_stack,
            modules: self.modules,
            groups: self.groups,
            project_name: self.project_name,
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::context::VerifiedFileRegistry;
    use crate::pipeline::phases::{
        constraint_extraction::ExtractedConstraints, convention_inference::InferredConventions,
        project_detection::ProjectDetection,
    };
    use crate::types::module_map::TechStack;

    fn test_context<'a>(
        detection: &'a ProjectDetection,
        tech_stack: &'a TechStack,
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        registry: &'a VerifiedFileRegistry,
    ) -> GenerationContext<'a> {
        GenerationContext::new(
            detection,
            tech_stack,
            "test-project",
            &[],
            &[],
            &[],
            conventions,
            constraints,
            registry,
        )
    }

    #[test]
    fn test_plan_budget_returns_valid_sections() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(&detection, &tech_stack, &conventions, &constraints, &registry);

        let budgeted = ctx.plan_budget(200_000);

        assert!(!budgeted.system_prompt.is_empty());
        assert!(!budgeted.tier1.project_identity.is_empty());
        assert!(budgeted.budget.total_tokens > 0);
        assert!(budgeted.total_tokens() <= budgeted.budget.total_tokens);
    }

    #[test]
    fn test_estimate_total_tokens() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(&detection, &tech_stack, &conventions, &constraints, &registry);

        let tokens = ctx.estimate_total_tokens();
        assert!(tokens > 0);
    }

    #[test]
    fn test_plan_budget_respects_model_limit() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(&detection, &tech_stack, &conventions, &constraints, &registry);

        let budgeted = ctx.plan_budget(10_000);

        let total_allocated: usize = budgeted.budget.allocated.values().sum();
        assert!(total_allocated <= budgeted.budget.total_tokens);
    }

    #[test]
    fn test_plan_budget_tier3_omitted_when_tight() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(&detection, &tech_stack, &conventions, &constraints, &registry);

        let budgeted = ctx.plan_budget(1_000);

        assert!(budgeted.tier3.domain_knowledge.is_empty() || budgeted.budget.remaining() == 0);
    }

    #[test]
    fn test_budgeted_sections_total_tokens() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(&detection, &tech_stack, &conventions, &constraints, &registry);

        let budgeted = ctx.plan_budget(200_000);
        let total = budgeted.total_tokens();

        let allocated_sum: usize = budgeted.budget.allocated.values().sum();
        assert_eq!(total, allocated_sum);
    }

    #[test]
    fn test_format_conventions_empty() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(&detection, &tech_stack, &conventions, &constraints, &registry);

        let formatted = ctx.format_conventions();
        assert!(formatted.is_empty() || formatted.contains("###"));
    }

    fn make_policy(description: &str) -> PolicyContext {
        PolicyContext {
            name: "test".to_string(),
            description: description.to_string(),
            policy_type: PolicyType::BusinessRule,
            enforcement: EnforcementLevel::Strict,
            affected_modules: vec![],
            evidence: vec![],
        }
    }

    #[test]
    fn test_infer_domain_fintech() {
        for keyword in &["payment", "transaction", "financial", "billing"] {
            let dk = EnrichedDomainKnowledge {
                policies: vec![make_policy(&format!("Requires {} validation", keyword))],
                ..Default::default()
            };
            assert_eq!(dk.infer_domain_from_policies(), Some("FinTech/Financial"),
                "keyword '{}' should infer FinTech/Financial", keyword);
        }
    }

    #[test]
    fn test_infer_domain_healthcare() {
        for keyword in &["patient", "hipaa", "medical", "health"] {
            let dk = EnrichedDomainKnowledge {
                policies: vec![make_policy(&format!("Requires {} compliance", keyword))],
                ..Default::default()
            };
            assert_eq!(dk.infer_domain_from_policies(), Some("Healthcare"),
                "keyword '{}' should infer Healthcare", keyword);
        }
    }

    #[test]
    fn test_infer_domain_ecommerce() {
        for keyword in &["cart", "order", "inventory", "product"] {
            let dk = EnrichedDomainKnowledge {
                policies: vec![make_policy(&format!("Manages {} lifecycle", keyword))],
                ..Default::default()
            };
            assert_eq!(dk.infer_domain_from_policies(), Some("E-commerce"),
                "keyword '{}' should infer E-commerce", keyword);
        }
    }

    #[test]
    fn test_infer_domain_skips_when_domain_type_set() {
        let dk = EnrichedDomainKnowledge {
            policies: vec![make_policy("Requires payment validation")],
            domain_type: Some("Custom".to_string()),
            ..Default::default()
        };
        assert_eq!(dk.infer_domain_from_policies(), None);
    }

    #[test]
    fn test_infer_domain_none_when_no_match() {
        let dk = EnrichedDomainKnowledge {
            policies: vec![make_policy("Generic logging policy")],
            ..Default::default()
        };
        assert_eq!(dk.infer_domain_from_policies(), None);
    }

    #[test]
    fn test_infer_domain_none_when_no_policies() {
        let dk = EnrichedDomainKnowledge::default();
        assert_eq!(dk.infer_domain_from_policies(), None);
    }

    #[test]
    fn test_to_rule_context_wires_fields() {
        let detection = ProjectDetection::default();
        let tech_stack = TechStack::new("rust");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();
        let ctx = test_context(&detection, &tech_stack, &conventions, &constraints, &registry);

        let rule_ctx = ctx.to_rule_context();

        assert_eq!(rule_ctx.project_name, "test-project");
        assert_eq!(rule_ctx.tech_stack.primary_language, "rust");
    }
}
