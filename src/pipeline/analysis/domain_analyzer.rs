//! Domain Analyzer Module
//!
//! Extracts domain-specific knowledge from codebase using LLM semantic analysis.
//! All pattern data is passed to LLM without pre-filtering - LLM decides relevance.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::ai::LlmProvider;
use crate::ai::validation::deserialize_llm_response;
use crate::types::Result;
use crate::types::domain::{
    Abbreviation, BusinessWorkflow, CoreDomainLogic, DomainAnalysisResult, DomainGlossary,
    DomainPolicy, DomainTerm, TermRelationship,
};

use super::aggregator::AggregatedAnalysis;

pub struct DomainAnalyzer {
    provider: Arc<dyn LlmProvider>,
}

impl DomainAnalyzer {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    pub async fn analyze(&self, aggregated: &AggregatedAnalysis) -> Result<DomainAnalysisResult> {
        if aggregated.patterns.is_empty() && aggregated.constraints.is_empty() {
            return Ok(DomainAnalysisResult::default());
        }

        let patterns_context = Self::format_patterns(aggregated);
        let constraints_context = Self::format_constraints(aggregated);
        let dependencies_context = Self::format_dependencies(aggregated);

        let policies = self
            .extract_policies(&patterns_context, &constraints_context)
            .await?;
        let core_logic = self
            .identify_core_logic(&patterns_context, &dependencies_context)
            .await?;
        let glossary = self
            .extract_terminology(&patterns_context, &constraints_context)
            .await?;
        let workflows = self
            .detect_workflows(&patterns_context, &dependencies_context)
            .await?;

        let confidence = Self::calculate_confidence(&policies, &core_logic, &glossary, &workflows);

        Ok(DomainAnalysisResult {
            policies,
            core_logic,
            glossary,
            workflows,
            domain_type: None,
            confidence,
        })
    }

    fn format_patterns(aggregated: &AggregatedAnalysis) -> String {
        aggregated
            .patterns
            .iter()
            .take(50)
            .map(|p| {
                format!(
                    "- {} [{:?}]: {} (modules: {})",
                    p.pattern.name,
                    p.pattern.category,
                    p.pattern.description,
                    p.modules.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_constraints(aggregated: &AggregatedAnalysis) -> String {
        aggregated
            .constraints
            .iter()
            .take(30)
            .map(|c| {
                format!(
                    "- {} [{:?}]: {} (modules: {})",
                    c.constraint.title,
                    c.constraint.kind,
                    c.constraint.description,
                    c.modules.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_dependencies(aggregated: &AggregatedAnalysis) -> String {
        aggregated
            .dependency_graph
            .edges
            .iter()
            .take(30)
            .map(|e| format!("{} -> {} ({})", e.from, e.to, e.edge_type))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn calculate_confidence(
        policies: &[DomainPolicy],
        core_logic: &[CoreDomainLogic],
        glossary: &DomainGlossary,
        workflows: &[BusinessWorkflow],
    ) -> f32 {
        let mut score = 0.0f32;
        if !policies.is_empty() {
            score += 0.25;
        }
        if !core_logic.is_empty() {
            score += 0.25;
        }
        if !glossary.terms.is_empty() {
            score += 0.25;
        }
        if !workflows.is_empty() {
            score += 0.25;
        }
        score
    }

    async fn extract_policies(
        &self,
        patterns: &str,
        constraints: &str,
    ) -> Result<Vec<DomainPolicy>> {
        let prompt = format!(
            r#"Analyze these code patterns and constraints to identify domain policies.

PATTERNS:
{patterns}

CONSTRAINTS:
{constraints}

Identify domain policies - rules that govern how the system must behave:
- Validation rules (input checks, format requirements)
- Authorization rules (access control, permissions)
- Business rules (domain-specific logic requirements)
- Invariants (conditions that must always hold)
- State transitions (allowed state changes)
- Data integrity rules (consistency requirements)

For each policy found:
1. Name and clear description
2. Policy type
3. Enforcement level (strict/warning/advisory)
4. Evidence locations from the patterns
5. Related modules

Focus on project-specific policies. Ignore generic language features."#
        );

        let response = self
            .provider
            .generate(&prompt, &Self::policies_schema())
            .await?;
        let parsed: PoliciesOutput = deserialize_llm_response(&response.content, "policies");
        Ok(parsed.policies)
    }

    async fn identify_core_logic(
        &self,
        patterns: &str,
        dependencies: &str,
    ) -> Result<Vec<CoreDomainLogic>> {
        let prompt = format!(
            r#"Analyze these patterns and dependencies to identify core domain logic.

PATTERNS:
{patterns}

DEPENDENCIES:
{dependencies}

Identify core domain logic - the essential business operations:
- Calculations (pricing, scoring, statistics)
- Transformations (data conversion, mapping)
- Aggregations (collecting, summarizing)
- Decisions (business rule evaluation, routing)
- Orchestrations (workflow coordination)
- Integrations (external system communication)

For each core logic found:
1. Name and description
2. Logic type
3. Location (file and line if available)
4. Dependencies on other components
5. Business impact

Focus on business-critical logic, not infrastructure."#
        );

        let response = self
            .provider
            .generate(&prompt, &Self::core_logic_schema())
            .await?;
        let parsed: CoreLogicOutput = deserialize_llm_response(&response.content, "core_logic");
        Ok(parsed.core_logic)
    }

    async fn extract_terminology(
        &self,
        patterns: &str,
        constraints: &str,
    ) -> Result<DomainGlossary> {
        let prompt = format!(
            r#"Extract domain terminology from these patterns and constraints.

PATTERNS:
{patterns}

CONSTRAINTS:
{constraints}

Extract:
1. Domain terms with definitions
   - Entities (core business objects)
   - Actions (operations performed)
   - States (possible conditions)
   - Metrics (measurements)
   - Roles (actors in the system)
   - Concepts (abstract domain ideas)
   - Events (things that happen)

2. Abbreviations with full forms

3. Term relationships (is_a, has_a, belongs_to, depends_on, triggers, related_to)

Focus on business domain terms, not technical jargon."#
        );

        let response = self
            .provider
            .generate(&prompt, &Self::terminology_schema())
            .await?;
        let parsed: TerminologyOutput = deserialize_llm_response(&response.content, "terminology");

        let mut glossary = DomainGlossary::new();
        for term in parsed.terms {
            glossary.add_term(term);
        }
        for abbr in parsed.abbreviations {
            glossary.add_abbreviation(abbr);
        }
        for rel in parsed.relationships {
            glossary.add_relationship(rel);
        }
        Ok(glossary)
    }

    async fn detect_workflows(
        &self,
        patterns: &str,
        dependencies: &str,
    ) -> Result<Vec<BusinessWorkflow>> {
        let prompt = format!(
            r#"Detect business workflows from these patterns and dependencies.

PATTERNS:
{patterns}

DEPENDENCIES:
{dependencies}

Identify business workflows - sequences of operations that accomplish business goals:
- Multi-step processes
- State machines
- Transaction flows
- Event-driven sequences
- Approval chains
- Data pipelines

For each workflow:
1. Name and description
2. Sequential steps with actions
3. Entry points
4. Involved modules
5. Triggers that initiate the workflow

Focus on business processes, not technical implementation."#
        );

        let response = self
            .provider
            .generate(&prompt, &Self::workflows_schema())
            .await?;
        let parsed: WorkflowsOutput = deserialize_llm_response(&response.content, "workflows");
        Ok(parsed.workflows)
    }

    fn policies_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "policies": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "description": {"type": "string"},
                            "policy_type": {"type": "string", "enum": ["validation", "authorization", "business_rule", "invariant", "state_transition", "data_integrity", "rate_limiting", "audit"]},
                            "enforcement": {"type": "string", "enum": ["strict", "warning", "advisory"]},
                            "evidence": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "file": {"type": "string"},
                                        "start_line": {"type": "integer"},
                                        "end_line": {"type": "integer"}
                                    }
                                }
                            },
                            "related_modules": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["name", "description", "policy_type"]
                    }
                }
            },
            "required": ["policies"]
        })
    }

    fn core_logic_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "core_logic": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "description": {"type": "string"},
                            "logic_type": {"type": "string", "enum": ["calculation", "transformation", "aggregation", "decision", "orchestration", "integration", "sanitization", "event_handling"]},
                            "location": {
                                "type": "object",
                                "properties": {
                                    "file": {"type": "string"},
                                    "start_line": {"type": "integer"},
                                    "end_line": {"type": "integer"}
                                }
                            },
                            "dependencies": {"type": "array", "items": {"type": "string"}},
                            "business_impact": {"type": "string"}
                        },
                        "required": ["name", "description", "logic_type"]
                    }
                }
            },
            "required": ["core_logic"]
        })
    }

    fn terminology_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "terms": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "term": {"type": "string"},
                            "definition": {"type": "string"},
                            "category": {"type": "string", "enum": ["entity", "action", "state", "metric", "role", "concept", "event"]},
                            "synonyms": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["term", "definition", "category"]
                    }
                },
                "abbreviations": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "short": {"type": "string"},
                            "full": {"type": "string"},
                            "context": {"type": "string"}
                        },
                        "required": ["short", "full"]
                    }
                },
                "relationships": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "from_term": {"type": "string"},
                            "to_term": {"type": "string"},
                            "relationship_type": {"type": "string", "enum": ["is_a", "has_a", "belongs_to", "depends_on", "triggers", "related_to"]}
                        },
                        "required": ["from_term", "to_term", "relationship_type"]
                    }
                }
            },
            "required": ["terms", "abbreviations", "relationships"]
        })
    }

    fn workflows_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflows": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "description": {"type": "string"},
                            "steps": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "order": {"type": "integer"},
                                        "name": {"type": "string"},
                                        "action": {"type": "string"},
                                        "next_steps": {"type": "array", "items": {"type": "string"}},
                                        "conditions": {"type": "array", "items": {"type": "string"}},
                                        "is_terminal": {"type": "boolean"}
                                    },
                                    "required": ["order", "name", "action"]
                                }
                            },
                            "entry_points": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "file": {"type": "string"},
                                        "start_line": {"type": "integer"}
                                    }
                                }
                            },
                            "involved_modules": {"type": "array", "items": {"type": "string"}},
                            "triggers": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["name", "description", "steps"]
                    }
                }
            },
            "required": ["workflows"]
        })
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct PoliciesOutput {
    #[serde(default)]
    policies: Vec<DomainPolicy>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct CoreLogicOutput {
    #[serde(default)]
    core_logic: Vec<CoreDomainLogic>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct TerminologyOutput {
    #[serde(default)]
    terms: Vec<DomainTerm>,
    #[serde(default)]
    abbreviations: Vec<Abbreviation>,
    #[serde(default)]
    relationships: Vec<TermRelationship>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct WorkflowsOutput {
    #[serde(default)]
    workflows: Vec<BusinessWorkflow>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EvidenceLocation;
    use crate::types::domain::{DomainLogicType, PolicyType, TermCategory};

    #[test]
    fn test_confidence_calculation() {
        let confidence = DomainAnalyzer::calculate_confidence(
            &[DomainPolicy::new("test", "desc", PolicyType::Validation)],
            &[],
            &DomainGlossary::new(),
            &[],
        );
        assert_eq!(confidence, 0.25);
    }

    #[test]
    fn test_full_confidence() {
        let mut glossary = DomainGlossary::new();
        glossary.add_term(DomainTerm::new("Test", "A test term", TermCategory::Entity));

        let confidence = DomainAnalyzer::calculate_confidence(
            &[DomainPolicy::new("test", "desc", PolicyType::Validation)],
            &[CoreDomainLogic::new(
                "calc",
                "desc",
                DomainLogicType::Calculation,
                EvidenceLocation::empty(),
            )],
            &glossary,
            &[BusinessWorkflow::new("flow", "desc")],
        );
        assert_eq!(confidence, 1.0);
    }
}
