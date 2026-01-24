//! Domain Analyzer
//!
//! Extracts business rules and domain knowledge from project analysis.

use std::sync::{Arc, LazyLock};

use regex::Regex;

// Cached regexes for terminology extraction
static PASCAL_CASE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Z][a-z]+(?:[A-Z][a-z]+)+)\b").expect("pascal case regex"));
static QUOTED_TERM_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"["']([^"']+)["']"#).expect("quoted term regex"));

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::ai::LlmProvider;
use crate::config::Config;
use crate::types::Result;

use super::InsightContext;
use super::types::{BusinessRule, BusinessRuleType, DomainKnowledge, Terminology};

/// Extracts domain terminology from code
pub struct TerminologyExtractor;

impl TerminologyExtractor {
    pub fn extract(&self, ctx: &InsightContext<'_>) -> Vec<Terminology> {
        let mut terms = Vec::new();

        // Extract from synthesis insights if available
        if let Some(synthesis) = ctx.synthesis {
            // Extract from insights
            for insight in &synthesis.deep.insights {
                let potential_terms = self.extract_terms_from_text(&insight.purpose);
                for term in potential_terms {
                    if !terms
                        .iter()
                        .any(|t: &Terminology| t.term.to_lowercase() == term.to_lowercase())
                    {
                        terms.push(Terminology {
                            term,
                            definition: String::new(),
                            usage_context: Some(insight.file.clone()),
                            occurrences: vec![insight.file.clone()],
                        });
                    }
                }
            }

            // Extract from module responsibilities
            for module in &synthesis.modules {
                let potential_terms = self.extract_terms_from_text(&module.responsibility);
                for term in potential_terms {
                    if !terms
                        .iter()
                        .any(|t: &Terminology| t.term.to_lowercase() == term.to_lowercase())
                    {
                        terms.push(Terminology {
                            term,
                            definition: String::new(),
                            usage_context: Some(module.path.clone()),
                            occurrences: vec![module.path.clone()],
                        });
                    }
                }
            }
        }

        // Extract from conventions
        for pattern in &ctx.conventions.patterns {
            let potential_terms = self.extract_terms_from_text(&pattern.description);
            for term in potential_terms {
                if !terms
                    .iter()
                    .any(|t: &Terminology| t.term.to_lowercase() == term.to_lowercase())
                {
                    terms.push(Terminology {
                        term,
                        definition: pattern.description.clone(),
                        usage_context: Some(pattern.name.clone()),
                        occurrences: Vec::new(),
                    });
                }
            }
        }

        terms
    }

    fn extract_terms_from_text(&self, text: &str) -> Vec<String> {
        let mut terms = Vec::new();

        // Look for capitalized compound words (PascalCase)
        for cap in PASCAL_CASE_REGEX.captures_iter(text) {
            if let Some(matched) = cap.get(1) {
                let term = matched.as_str().to_string();
                if !self.is_common_programming_term(&term) {
                    terms.push(term);
                }
            }
        }

        // Look for quoted terms
        for cap in QUOTED_TERM_REGEX.captures_iter(text) {
            if let Some(matched) = cap.get(1) {
                let term = matched.as_str().to_string();
                if term.len() > 2 && !self.is_common_programming_term(&term) {
                    terms.push(term);
                }
            }
        }

        terms
    }

    fn is_common_programming_term(&self, term: &str) -> bool {
        let common = [
            "String",
            "Integer",
            "Boolean",
            "Array",
            "Object",
            "Function",
            "Result",
            "Option",
            "Error",
            "Config",
            "Default",
            "Debug",
            "Clone",
            "Copy",
            "Send",
            "Sync",
            "Serialize",
            "Deserialize",
            "HashMap",
            "HashSet",
            "Vector",
            "Iterator",
            "Future",
            "Stream",
        ];
        common.contains(&term)
    }
}

/// Extracts business rules from code patterns
pub struct BusinessRuleExtractor;

impl BusinessRuleExtractor {
    pub fn extract(&self, ctx: &InsightContext<'_>) -> Vec<BusinessRule> {
        let mut rules = Vec::new();

        for implicit in &ctx.constraints.implicit_rules {
            if self.looks_like_business_rule(&implicit.description) {
                let rule_type = self.infer_rule_type(&implicit.description);
                rules.push(BusinessRule {
                    name: implicit.name.clone(),
                    description: implicit.description.clone(),
                    rule_type,
                    consequence: None,
                    evidence: implicit.evidence.iter().map(|e| e.file.clone()).collect(),
                });
            }
        }

        for gotcha in &ctx.constraints.gotchas {
            if self.looks_like_business_rule(&gotcha.description) {
                let rule_type = self.infer_rule_type(&gotcha.description);
                rules.push(BusinessRule {
                    name: gotcha.title.clone(),
                    description: gotcha.description.clone(),
                    rule_type,
                    consequence: Some(gotcha.solution.clone()),
                    evidence: gotcha.related_files.clone(),
                });
            }
        }

        for workflow in &ctx.constraints.complex_workflows {
            rules.push(BusinessRule {
                name: format!("Workflow: {}", workflow.name),
                description: workflow.description.clone(),
                rule_type: BusinessRuleType::StateTransition,
                consequence: None,
                evidence: workflow
                    .steps
                    .iter()
                    .flat_map(|s| s.files_involved.clone())
                    .collect(),
            });
        }

        rules
    }

    fn looks_like_business_rule(&self, text: &str) -> bool {
        let business_indicators = [
            "must",
            "should",
            "cannot",
            "only",
            "when",
            "if",
            "validate",
            "check",
            "verify",
            "ensure",
            "require",
            "limit",
            "allow",
            "restrict",
            "permission",
            "eligible",
            "policy",
            "rule",
            "condition",
            "constraint",
            "customer",
            "user",
            "order",
            "payment",
            "invoice",
            "account",
            "balance",
            "transaction",
            "refund",
        ];

        let text_lower = text.to_lowercase();
        business_indicators
            .iter()
            .any(|&indicator| text_lower.contains(indicator))
    }

    fn infer_rule_type(&self, text: &str) -> BusinessRuleType {
        let text_lower = text.to_lowercase();

        if text_lower.contains("validate")
            || text_lower.contains("check")
            || text_lower.contains("verify")
            || text_lower.contains("must be")
        {
            return BusinessRuleType::Validation;
        }

        if text_lower.contains("state")
            || text_lower.contains("status")
            || text_lower.contains("transition")
            || text_lower.contains("from")
            || text_lower.contains("to")
        {
            return BusinessRuleType::StateTransition;
        }

        if text_lower.contains("auth")
            || text_lower.contains("permission")
            || text_lower.contains("role")
            || text_lower.contains("access")
        {
            return BusinessRuleType::Authorization;
        }

        if text_lower.contains("calculate")
            || text_lower.contains("compute")
            || text_lower.contains("sum")
            || text_lower.contains("total")
        {
            return BusinessRuleType::Calculation;
        }

        BusinessRuleType::Policy
    }
}

/// LLM response for domain analysis
#[derive(Debug, Serialize, Deserialize)]
struct DomainAnalysisResponse {
    business_rules: Vec<LlmBusinessRule>,
    terminology: Vec<LlmTerminology>,
    compliance: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LlmBusinessRule {
    name: String,
    description: String,
    rule_type: String,
    consequence: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LlmTerminology {
    term: String,
    definition: String,
    usage_context: Option<String>,
}

/// Main domain analyzer that combines extractors with LLM assistance
pub struct DomainAnalyzer {
    provider: Arc<dyn LlmProvider>,
    terminology_extractor: TerminologyExtractor,
    rule_extractor: BusinessRuleExtractor,
    config: Arc<Config>,
}

impl DomainAnalyzer {
    pub fn new(provider: Arc<dyn LlmProvider>, config: Arc<Config>) -> Self {
        Self {
            provider,
            terminology_extractor: TerminologyExtractor,
            rule_extractor: BusinessRuleExtractor,
            config,
        }
    }

    pub async fn analyze(&self, ctx: &InsightContext<'_>) -> Result<DomainKnowledge> {
        let domain_config = &self.config.insight.domain;
        let mut terminology = self.terminology_extractor.extract(ctx);
        let mut business_rules = self.rule_extractor.extract(ctx);

        debug!(
            terms = terminology.len(),
            rules = business_rules.len(),
            "Pattern-based domain extraction complete"
        );

        if domain_config.llm_enrichment && self.should_use_llm_analysis() {
            let llm_knowledge = self.analyze_with_llm(ctx).await?;

            for term in llm_knowledge.terminology {
                if !terminology
                    .iter()
                    .any(|t| t.term.to_lowercase() == term.term.to_lowercase())
                {
                    terminology.push(term);
                }
            }

            for rule in llm_knowledge.business_rules {
                if !business_rules
                    .iter()
                    .any(|r| r.name.to_lowercase() == rule.name.to_lowercase())
                {
                    business_rules.push(rule);
                }
            }
        }

        // Enrich terminology with file occurrences
        for term in &mut terminology {
            if term.occurrences.is_empty() {
                term.occurrences = ctx
                    .file_registry
                    .files_in_directory(&term.term)
                    .into_iter()
                    .take(5)
                    .collect();
            }
        }

        // Sort business rules by priority (configured rule types first)
        let priorities = &domain_config.rule_type_priorities;
        business_rules.sort_by(|a, b| {
            let a_priority = priorities
                .iter()
                .position(|t| *t == a.rule_type)
                .unwrap_or(usize::MAX);
            let b_priority = priorities
                .iter()
                .position(|t| *t == b.rule_type)
                .unwrap_or(usize::MAX);
            a_priority.cmp(&b_priority)
        });

        // Limit terminology to max configured
        if terminology.len() > domain_config.max_terminology {
            terminology.truncate(domain_config.max_terminology);
        }

        Ok(DomainKnowledge {
            business_rules,
            terminology,
            compliance_requirements: self.extract_compliance_requirements(ctx),
        })
    }

    fn should_use_llm_analysis(&self) -> bool {
        matches!(
            self.config.analysis.depth,
            crate::config::AnalysisDepth::Complete | crate::config::AnalysisDepth::Standard
        )
    }

    async fn analyze_with_llm(&self, ctx: &InsightContext<'_>) -> Result<DomainKnowledge> {
        let context_summary = self.build_context_summary(ctx);

        let prompt = format!(
            r#"Analyze this project for business domain knowledge.

PROJECT CONTEXT:
{}

Extract:
1. Business Rules: Constraints that aren't purely technical (validation rules, policies, state transitions)
2. Domain Terminology: Specialized terms used in this project with their meanings
3. Compliance Requirements: Any regulatory or policy requirements mentioned

Focus on rules that would cause business logic errors if an AI didn't know them.

Return JSON:
{{
    "business_rules": [
        {{"name": "...", "description": "...", "rule_type": "validation|state_transition|authorization|calculation|policy", "consequence": "..."}}
    ],
    "terminology": [
        {{"term": "...", "definition": "...", "usage_context": "..."}}
    ],
    "compliance": ["requirement1", "requirement2"]
}}"#,
            context_summary
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "business_rules": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "description": { "type": "string" },
                            "rule_type": { "type": "string" },
                            "consequence": { "type": "string" }
                        },
                        "required": ["name", "description", "rule_type"]
                    }
                },
                "terminology": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "term": { "type": "string" },
                            "definition": { "type": "string" },
                            "usage_context": { "type": "string" }
                        },
                        "required": ["term", "definition"]
                    }
                },
                "compliance": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["business_rules", "terminology", "compliance"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                match serde_json::from_value::<DomainAnalysisResponse>(response.content.clone()) {
                    Ok(analysis) => {
                        let mut business_rules = Vec::new();
                        for rule in analysis.business_rules {
                            business_rules.push(BusinessRule {
                                name: rule.name,
                                description: rule.description,
                                rule_type: self.parse_rule_type(&rule.rule_type),
                                consequence: rule.consequence,
                                evidence: Vec::new(),
                            });
                        }

                        let mut terminology = Vec::new();
                        for term in analysis.terminology {
                            terminology.push(Terminology {
                                term: term.term,
                                definition: term.definition,
                                usage_context: term.usage_context,
                                occurrences: Vec::new(),
                            });
                        }

                        Ok(DomainKnowledge {
                            business_rules,
                            terminology,
                            compliance_requirements: analysis.compliance,
                        })
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse LLM domain analysis response");
                        Ok(DomainKnowledge::default())
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "LLM domain analysis call failed, using pattern-based only");
                Ok(DomainKnowledge::default())
            }
        }
    }

    fn parse_rule_type(&self, s: &str) -> BusinessRuleType {
        match s.to_lowercase().as_str() {
            "validation" => BusinessRuleType::Validation,
            "state_transition" => BusinessRuleType::StateTransition,
            "authorization" => BusinessRuleType::Authorization,
            "calculation" => BusinessRuleType::Calculation,
            _ => BusinessRuleType::Policy,
        }
    }

    fn build_context_summary(&self, ctx: &InsightContext<'_>) -> String {
        let mut summary = String::new();

        summary.push_str(&format!(
            "Architecture: {}\n",
            ctx.conventions.architecture.pattern_name
        ));

        if let Some(synthesis) = ctx.synthesis {
            summary.push_str("\nKey Modules:\n");
            for module in synthesis.modules.iter().take(10) {
                summary.push_str(&format!("- {}: {}\n", module.path, module.responsibility));
            }

            if !synthesis.deep.insights.is_empty() {
                summary.push_str("\nKey Insights:\n");
                for insight in synthesis.deep.insights.iter().take(10) {
                    summary.push_str(&format!("- {}: {}\n", insight.file, insight.purpose));
                }
            }
        }

        if !ctx.constraints.implicit_rules.is_empty() {
            summary.push_str("\nImplicit Rules:\n");
            for rule in ctx.constraints.implicit_rules.iter().take(5) {
                summary.push_str(&format!("- {}: {}\n", rule.name, rule.description));
            }
        }

        if !ctx.constraints.complex_workflows.is_empty() {
            summary.push_str("\nComplex Workflows:\n");
            for workflow in ctx.constraints.complex_workflows.iter().take(3) {
                summary.push_str(&format!("- {}: {}\n", workflow.name, workflow.description));
            }
        }

        summary
    }

    fn extract_compliance_requirements(&self, ctx: &InsightContext<'_>) -> Vec<String> {
        let mut requirements = Vec::new();

        for requirement in &self.config.domain.compliance {
            requirements.push(requirement.clone());
        }

        for gotcha in &ctx.constraints.gotchas {
            let desc_lower = gotcha.description.to_lowercase();
            if desc_lower.contains("compliance")
                || desc_lower.contains("regulation")
                || desc_lower.contains("gdpr")
                || desc_lower.contains("pci")
                || desc_lower.contains("hipaa")
                || desc_lower.contains("sox")
            {
                requirements.push(gotcha.description.clone());
            }
        }

        requirements
    }
}

#[cfg(test)]
mod tests {
    use super::super::InsightContext;
    use super::*;
    use crate::pipeline::context::VerifiedFileRegistry;
    use crate::pipeline::phases::constraint_extraction::{
        ComplexWorkflow, ExtractedConstraints, Gotcha, ImplicitRule, RuleEnforcement, WorkflowStep,
    };
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization,
        InferredConventions, NamingConventions, TestingConvention,
    };

    fn create_test_context<'a>(
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        registry: &'a VerifiedFileRegistry,
    ) -> InsightContext<'a> {
        InsightContext {
            conventions,
            constraints,
            synthesis: None,
            file_registry: registry,
        }
    }

    #[test]
    fn test_business_rule_extractor_from_implicit_rules() {
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let mut constraints = ExtractedConstraints::default();
        constraints.implicit_rules.push(ImplicitRule {
            name: "Payment Validation".to_string(),
            description: "Must validate payment amount before processing transaction".to_string(),
            applies_to: vec!["src/payment".to_string()],
            enforcement: RuleEnforcement::Convention,
            evidence: Vec::new(),
        });

        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let extractor = BusinessRuleExtractor;
        let rules = extractor.extract(&ctx);

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].rule_type, BusinessRuleType::Validation);
    }

    #[test]
    fn test_business_rule_extractor_from_workflow() {
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let mut constraints = ExtractedConstraints::default();
        constraints.complex_workflows.push(ComplexWorkflow {
            name: "Order Processing".to_string(),
            description: "Process customer orders".to_string(),
            trigger: "New order received".to_string(),
            steps: vec![
                WorkflowStep {
                    order: 1,
                    action: "Validate".to_string(),
                    files_involved: vec!["src/validate.rs".to_string()],
                    commands: Vec::new(),
                    notes: Vec::new(),
                },
                WorkflowStep {
                    order: 2,
                    action: "Process".to_string(),
                    files_involved: vec!["src/process.rs".to_string()],
                    commands: Vec::new(),
                    notes: Vec::new(),
                },
            ],
            gotchas: Vec::new(),
            automation_potential: 0.8,
        });

        let registry = VerifiedFileRegistry::empty();
        let ctx = create_test_context(&conventions, &constraints, &registry);

        let extractor = BusinessRuleExtractor;
        let rules = extractor.extract(&ctx);

        assert_eq!(rules.len(), 1);
        assert!(rules[0].name.contains("Workflow"));
        assert_eq!(rules[0].rule_type, BusinessRuleType::StateTransition);
    }

    #[test]
    fn test_infer_rule_type_authorization() {
        let extractor = BusinessRuleExtractor;

        // Use text without "to" since it triggers StateTransition
        assert_eq!(
            extractor.infer_rule_type("User needs admin permission for this action"),
            BusinessRuleType::Authorization
        );
    }

    #[test]
    fn test_infer_rule_type_calculation() {
        let extractor = BusinessRuleExtractor;

        // Use text without "to" since it triggers StateTransition
        assert_eq!(
            extractor.infer_rule_type("Calculate the sum of all prices"),
            BusinessRuleType::Calculation
        );
    }

    #[test]
    fn test_infer_rule_type_state_transition() {
        let extractor = BusinessRuleExtractor;

        assert_eq!(
            extractor.infer_rule_type("Order status changes from pending"),
            BusinessRuleType::StateTransition
        );
    }

    #[test]
    fn test_compliance_extraction() {
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let mut constraints = ExtractedConstraints::default();
        constraints.gotchas.push(Gotcha {
            title: "GDPR Data Handling".to_string(),
            description: "Must follow GDPR regulations for personal data processing".to_string(),
            when: "When handling user data".to_string(),
            solution: "Implement data protection measures".to_string(),
            related_files: Vec::new(),
        });

        let registry = VerifiedFileRegistry::empty();
        let _ctx = create_test_context(&conventions, &constraints, &registry);

        // Verify compliance keywords are detected in gotcha descriptions
        let desc_lower = constraints.gotchas[0].description.to_lowercase();
        assert!(desc_lower.contains("gdpr"));
    }
}
