//! Enrichment Engine - Bridges Synthesis to Generation
//!
//! Solves the critical gap where synthesis findings are not consumed by generation.
//! Maps constraints, patterns, and insights from analysis to actual artifact content.
//!
//! Key responsibilities:
//! - Map synthesis findings to output plan items
//! - Inject Tier3 constraints into skill/agent prompts
//! - Track coverage (which constraints appear in which artifacts)
//! - Identify gaps and suggest additional artifacts

use std::collections::{HashMap, HashSet};

use crate::pipeline::analysis::SynthesizedAnalysis;
use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
use crate::pipeline::phases::output_router::OutputPlan;

/// Result of enrichment process
#[derive(Debug, Clone)]
pub struct EnrichedPlan {
    /// Original plan with enrichment data attached
    pub plan: OutputPlan,
    /// Mapping of skill names to relevant constraints
    pub skill_constraints: HashMap<String, Vec<EnrichedConstraint>>,
    /// Mapping of agent names to internal knowledge
    pub agent_knowledge: HashMap<String, AgentInternalKnowledge>,
    /// Coverage tracking
    pub coverage: ConstraintCoverage,
    /// Suggested additional artifacts to cover uncovered constraints
    pub suggested_artifacts: Vec<SuggestedArtifact>,
}

/// A constraint enriched with context for injection into artifacts
#[derive(Debug, Clone)]
pub struct EnrichedConstraint {
    /// The constraint text
    pub description: String,
    /// Category (gotcha, hidden_dependency, anti_pattern, order_dependency)
    pub category: ConstraintCategory,
    /// Severity level
    pub severity: ConstraintSeverity,
    /// File references that evidence this constraint
    pub file_refs: Vec<String>,
    /// Related modules
    pub related_modules: Vec<String>,
}

/// Agent internal knowledge extracted from synthesis
#[derive(Debug, Clone)]
pub struct AgentInternalKnowledge {
    /// Project-specific gotchas this agent should know
    pub gotchas: Vec<String>,
    /// Order-dependent workflows
    pub order_dependencies: Vec<String>,
    /// Anti-patterns to avoid
    pub anti_patterns: Vec<String>,
    /// Key file references with descriptions
    pub key_references: Vec<KeyReference>,
    /// Module responsibilities relevant to this agent
    pub module_context: Vec<String>,
}

/// A key reference with explanation
#[derive(Debug, Clone)]
pub struct KeyReference {
    pub path: String,
    pub line: Option<u32>,
    pub description: String,
}

/// Coverage tracking for constraints
#[derive(Debug, Clone, Default)]
pub struct ConstraintCoverage {
    /// Total unique constraints from analysis
    pub total_constraints: usize,
    /// Constraints mapped to at least one artifact
    pub covered_constraints: usize,
    /// Constraints with no artifact mapping
    pub uncovered: Vec<UncoveredConstraint>,
    /// Coverage ratio (0.0 - 1.0)
    pub coverage_ratio: f32,
    /// Per-artifact coverage details
    pub artifact_coverage: HashMap<String, Vec<String>>,
}

/// An uncovered constraint that needs an artifact
#[derive(Debug, Clone)]
pub struct UncoveredConstraint {
    pub description: String,
    pub category: ConstraintCategory,
    pub severity: ConstraintSeverity,
    pub suggested_artifact_type: SuggestedArtifactType,
}

/// Suggested artifact to cover uncovered constraints
#[derive(Debug, Clone)]
pub struct SuggestedArtifact {
    pub artifact_type: SuggestedArtifactType,
    pub name: String,
    pub reason: String,
    pub constraints_to_cover: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuggestedArtifactType {
    Skill,
    Agent,
    Rule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintCategory {
    Gotcha,
    HiddenDependency,
    AntiPattern,
    OrderDependency,
    Concurrency,
    ResourceManagement,
    InitializationOrder,
    SecurityConstraint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConstraintSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Enrichment Engine that bridges synthesis to generation
pub struct EnrichmentEngine {
    min_coverage_ratio: f32,
    max_constraints_per_skill: usize,
    max_knowledge_items_per_agent: usize,
}

impl Default for EnrichmentEngine {
    fn default() -> Self {
        Self {
            min_coverage_ratio: 0.8,
            max_constraints_per_skill: 5,
            max_knowledge_items_per_agent: 10,
        }
    }
}

impl EnrichmentEngine {
    pub fn new(min_coverage_ratio: f32) -> Self {
        Self {
            min_coverage_ratio,
            ..Default::default()
        }
    }

    /// Enrich the output plan with synthesis findings
    pub fn enrich(
        &self,
        plan: OutputPlan,
        synthesis: Option<&SynthesizedAnalysis>,
        constraints: &ExtractedConstraints,
    ) -> EnrichedPlan {
        let mut skill_constraints: HashMap<String, Vec<EnrichedConstraint>> = HashMap::new();
        let mut agent_knowledge: HashMap<String, AgentInternalKnowledge> = HashMap::new();
        let all_constraints = self.collect_all_constraints(synthesis, constraints);
        let mut artifact_coverage: HashMap<String, Vec<String>> = HashMap::new();

        // Map constraints to skills
        for planned_skill in &plan.skills_plan.planned_skills {
            let relevant = self.find_relevant_constraints(
                &planned_skill.name,
                &planned_skill.trigger,
                &all_constraints,
                synthesis,
            );

            // Track coverage
            for c in &relevant {
                artifact_coverage
                    .entry(format!("skill:{}", planned_skill.name))
                    .or_default()
                    .push(c.description.clone());
            }

            skill_constraints.insert(planned_skill.name.clone(), relevant);
        }

        // Build agent internal knowledge
        for planned_agent in &plan.agents_plan.planned_agents {
            let knowledge = self.build_agent_knowledge(
                &planned_agent.name,
                &planned_agent.role,
                synthesis,
                constraints,
            );

            // Track coverage
            for gotcha in &knowledge.gotchas {
                artifact_coverage
                    .entry(format!("agent:{}", planned_agent.name))
                    .or_default()
                    .push(gotcha.clone());
            }

            agent_knowledge.insert(planned_agent.name.clone(), knowledge);
        }

        // Calculate coverage
        let covered_set: HashSet<_> = artifact_coverage
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect();

        let total_constraints = all_constraints.len();
        let covered_constraints = covered_set.len();

        let uncovered: Vec<_> = all_constraints
            .iter()
            .filter(|c| !covered_set.contains(&c.description))
            .map(|c| UncoveredConstraint {
                description: c.description.clone(),
                category: c.category,
                severity: c.severity,
                suggested_artifact_type: self.suggest_artifact_type(c),
            })
            .collect();

        let coverage_ratio = if total_constraints > 0 {
            covered_constraints as f32 / total_constraints as f32
        } else {
            1.0
        };

        let coverage = ConstraintCoverage {
            total_constraints,
            covered_constraints,
            uncovered: uncovered.clone(),
            coverage_ratio,
            artifact_coverage,
        };

        // Generate suggestions for uncovered constraints
        let suggested_artifacts = self.generate_suggestions(&uncovered);

        EnrichedPlan {
            plan,
            skill_constraints,
            agent_knowledge,
            coverage,
            suggested_artifacts,
        }
    }

    /// Collect all constraints from synthesis and extraction
    fn collect_all_constraints(
        &self,
        synthesis: Option<&SynthesizedAnalysis>,
        constraints: &ExtractedConstraints,
    ) -> Vec<EnrichedConstraint> {
        let mut result = Vec::new();

        // From ExtractedConstraints - Gotchas
        for gotcha in &constraints.gotchas {
            result.push(EnrichedConstraint {
                description: gotcha.description.clone(),
                category: ConstraintCategory::Gotcha,
                severity: ConstraintSeverity::Medium, // Gotcha doesn't have severity field
                file_refs: gotcha.related_files.clone(),
                related_modules: Vec::new(),
            });
        }

        // From ExtractedConstraints - Anti-patterns
        for anti in &constraints.anti_patterns {
            let severity = match anti.severity {
                crate::pipeline::phases::constraint_extraction::Severity::Low => {
                    ConstraintSeverity::Low
                }
                crate::pipeline::phases::constraint_extraction::Severity::Medium => {
                    ConstraintSeverity::Medium
                }
                crate::pipeline::phases::constraint_extraction::Severity::High => {
                    ConstraintSeverity::High
                }
                crate::pipeline::phases::constraint_extraction::Severity::Critical => {
                    ConstraintSeverity::Critical
                }
            };
            let file_refs: Vec<String> = anti
                .evidence
                .iter()
                .map(|e| {
                    if let Some(line) = e.line {
                        format!("@{}:{}", e.file, line)
                    } else {
                        format!("@{}", e.file)
                    }
                })
                .collect();

            result.push(EnrichedConstraint {
                description: format!("{}: {}", anti.name, anti.why_bad),
                category: ConstraintCategory::AntiPattern,
                severity,
                file_refs,
                related_modules: Vec::new(),
            });
        }

        // From ExtractedConstraints - Hidden Dependencies
        for dep in &constraints.hidden_dependencies {
            let file_refs: Vec<String> = dep
                .evidence
                .iter()
                .map(|e| {
                    if let Some(line) = e.line {
                        format!("@{}:{}", e.file, line)
                    } else {
                        format!("@{}", e.file)
                    }
                })
                .collect();

            result.push(EnrichedConstraint {
                description: format!("{} → {}: {}", dep.source, dep.target, dep.description),
                category: ConstraintCategory::HiddenDependency,
                severity: ConstraintSeverity::High,
                file_refs,
                related_modules: vec![dep.source.clone(), dep.target.clone()],
            });
        }

        // From Synthesis deep analysis
        if let Some(synth) = synthesis {
            for constraint in &synth.deep.constraints {
                // Avoid duplicates
                if !result
                    .iter()
                    .any(|c| c.description.contains(&constraint.description))
                {
                    let severity = match constraint.severity {
                        crate::pipeline::analysis::deep_analyzer::ConstraintSeverity::Low => {
                            ConstraintSeverity::Low
                        }
                        crate::pipeline::analysis::deep_analyzer::ConstraintSeverity::Medium => {
                            ConstraintSeverity::Medium
                        }
                        crate::pipeline::analysis::deep_analyzer::ConstraintSeverity::High => {
                            ConstraintSeverity::High
                        }
                        crate::pipeline::analysis::deep_analyzer::ConstraintSeverity::Critical => {
                            ConstraintSeverity::Critical
                        }
                    };

                    let file_refs: Vec<String> = constraint
                        .evidence
                        .iter()
                        .map(|e| {
                            if let Some(line) = e.line {
                                format!("@{}:{}", e.file, line)
                            } else {
                                format!("@{}", e.file)
                            }
                        })
                        .collect();

                    result.push(EnrichedConstraint {
                        description: constraint.description.clone(),
                        category: self.categorize_constraint_kind(&constraint.kind),
                        severity,
                        file_refs,
                        related_modules: Vec::new(),
                    });
                }
            }

            // Module-level constraints from synthesis
            for module in &synth.modules {
                if !module.constraints.is_empty() {
                    for constraint in &module.constraints {
                        if !result.iter().any(|c| c.description == *constraint) {
                            result.push(EnrichedConstraint {
                                description: constraint.clone(),
                                category: ConstraintCategory::Gotcha,
                                severity: ConstraintSeverity::Medium,
                                file_refs: vec![format!("@{}", module.path)],
                                related_modules: vec![module.name.clone()],
                            });
                        }
                    }
                }
            }
        }

        result
    }

    /// Find constraints relevant to a specific skill
    fn find_relevant_constraints(
        &self,
        skill_name: &str,
        trigger: &str,
        all_constraints: &[EnrichedConstraint],
        _synthesis: Option<&SynthesizedAnalysis>,
    ) -> Vec<EnrichedConstraint> {
        let skill_lower = skill_name.to_lowercase();
        let trigger_lower = trigger.to_lowercase();

        // Keywords from skill name and trigger
        let keywords: Vec<&str> = skill_lower
            .split(|c: char| !c.is_alphanumeric())
            .chain(trigger_lower.split(|c: char| !c.is_alphanumeric()))
            .filter(|s| s.len() > 2)
            .collect();

        let mut relevant: Vec<_> = all_constraints
            .iter()
            .filter(|c| {
                let desc_lower = c.description.to_lowercase();

                // Match by keywords
                keywords.iter().any(|kw| desc_lower.contains(kw))
                    // Or match by related modules
                    || c.related_modules.iter().any(|m| {
                        let m_lower = m.to_lowercase();
                        keywords.iter().any(|kw| m_lower.contains(kw))
                    })
                    // Or high severity constraints that affect common areas
                    || (c.severity >= ConstraintSeverity::High && self.is_common_constraint(c))
            })
            .cloned()
            .collect();

        // Sort by severity (highest first) and take top N
        relevant.sort_by(|a, b| b.severity.cmp(&a.severity));
        relevant.truncate(self.max_constraints_per_skill);

        relevant
    }

    /// Build internal knowledge for an agent from synthesis
    fn build_agent_knowledge(
        &self,
        _agent_name: &str,
        role: &str,
        synthesis: Option<&SynthesizedAnalysis>,
        constraints: &ExtractedConstraints,
    ) -> AgentInternalKnowledge {
        let role_lower = role.to_lowercase();
        let mut gotchas = Vec::new();
        let mut order_dependencies = Vec::new();
        let mut anti_patterns = Vec::new();
        let mut key_references = Vec::new();
        let mut module_context = Vec::new();

        // Collect gotchas relevant to this agent's role
        for gotcha in &constraints.gotchas {
            if self.is_relevant_to_role(&gotcha.description, &role_lower) {
                gotchas.push(gotcha.description.clone());
            }
        }

        // Collect anti-patterns
        for anti in &constraints.anti_patterns {
            if self.is_relevant_to_role(&anti.name, &role_lower) {
                anti_patterns.push(format!("Avoid: {} - {}", anti.name, anti.why_bad));
            }
        }

        // From workflows (order dependencies)
        for workflow in &constraints.complex_workflows {
            if self.is_relevant_to_role(&workflow.name, &role_lower) {
                let steps: Vec<String> = workflow.steps.iter().map(|s| s.action.clone()).collect();
                order_dependencies.push(format!("{}: {}", workflow.name, steps.join(" → ")));

                // Add gotchas from workflows
                gotchas.extend(workflow.gotchas.iter().cloned());
            }
        }

        // From synthesis modules
        if let Some(synth) = synthesis {
            for module in &synth.modules {
                if self.is_relevant_to_role(&module.responsibility, &role_lower) {
                    module_context.push(format!("{}: {}", module.name, module.responsibility));

                    // Add module-specific constraints
                    gotchas.extend(module.constraints.iter().cloned());

                    // Add key references from module
                    if !module.path.is_empty() {
                        key_references.push(KeyReference {
                            path: format!("@{}", module.path),
                            line: None,
                            description: module.responsibility.clone(),
                        });
                    }
                }
            }

            // Add pattern locations as references
            for pattern in &synth.deep.patterns {
                if self.is_relevant_to_role(&pattern.name, &role_lower) {
                    for location in pattern.locations.iter().take(2) {
                        key_references.push(KeyReference {
                            path: format!("@{}:{}", location.file, location.line),
                            line: Some(location.line),
                            description: format!("Example of {} pattern", pattern.name),
                        });
                    }
                }
            }

            // Add multi-agent discovered constraints
            for constraint in &synth.deep.constraints {
                if self.is_relevant_to_role(&constraint.title, &role_lower)
                    || self.is_relevant_to_role(&constraint.description, &role_lower)
                {
                    use crate::pipeline::analysis::deep_analyzer::ConstraintKind;
                    match constraint.kind {
                        ConstraintKind::AntiPattern => {
                            anti_patterns.push(format!(
                                "{}: {}",
                                constraint.title, constraint.description
                            ));
                        }
                        ConstraintKind::HiddenDependency | ConstraintKind::Invariant => {
                            gotchas.push(format!(
                                "{}: {} ({})",
                                constraint.title, constraint.description, constraint.rationale
                            ));
                        }
                        ConstraintKind::WorkflowRequirement | ConstraintKind::NamingConvention => {
                            order_dependencies.push(format!(
                                "{}: {}",
                                constraint.title, constraint.description
                            ));
                        }
                    }

                    // Add evidence as key references
                    for evidence in constraint.evidence.iter().take(2) {
                        if let Some(line) = evidence.line {
                            key_references.push(KeyReference {
                                path: format!("@{}:{}", evidence.file, line),
                                line: Some(line),
                                description: format!("{} evidence", constraint.title),
                            });
                        }
                    }
                }
            }
        }

        // Deduplicate and limit
        gotchas.sort();
        gotchas.dedup();
        gotchas.truncate(self.max_knowledge_items_per_agent);

        anti_patterns.sort();
        anti_patterns.dedup();
        anti_patterns.truncate(5);

        key_references.truncate(6);

        AgentInternalKnowledge {
            gotchas,
            order_dependencies,
            anti_patterns,
            key_references,
            module_context,
        }
    }

    /// Suggest artifact type for uncovered constraint
    fn suggest_artifact_type(&self, constraint: &EnrichedConstraint) -> SuggestedArtifactType {
        match constraint.category {
            ConstraintCategory::OrderDependency | ConstraintCategory::InitializationOrder => {
                SuggestedArtifactType::Skill
            }
            ConstraintCategory::SecurityConstraint | ConstraintCategory::AntiPattern => {
                SuggestedArtifactType::Rule
            }
            _ => {
                if constraint.severity >= ConstraintSeverity::High {
                    SuggestedArtifactType::Agent
                } else {
                    SuggestedArtifactType::Skill
                }
            }
        }
    }

    /// Generate suggestions for uncovered constraints
    fn generate_suggestions(&self, uncovered: &[UncoveredConstraint]) -> Vec<SuggestedArtifact> {
        // Group by suggested artifact type
        let mut by_type: HashMap<SuggestedArtifactType, Vec<&UncoveredConstraint>> = HashMap::new();
        for c in uncovered {
            by_type.entry(c.suggested_artifact_type).or_default().push(c);
        }

        let mut suggestions = Vec::new();

        for (artifact_type, constraints) in by_type {
            if constraints.is_empty() {
                continue;
            }

            // Group related constraints together
            let name = match artifact_type {
                SuggestedArtifactType::Skill => {
                    format!(
                        "{}-workflow",
                        Self::extract_topic(&constraints[0].description)
                    )
                }
                SuggestedArtifactType::Agent => {
                    format!(
                        "{}-specialist",
                        Self::extract_topic(&constraints[0].description)
                    )
                }
                SuggestedArtifactType::Rule => {
                    format!(
                        "{}-guidelines",
                        Self::extract_topic(&constraints[0].description)
                    )
                }
            };

            suggestions.push(SuggestedArtifact {
                artifact_type,
                name,
                reason: format!(
                    "Cover {} uncovered {} constraints",
                    constraints.len(),
                    format!("{:?}", artifact_type).to_lowercase()
                ),
                constraints_to_cover: constraints.iter().map(|c| c.description.clone()).collect(),
            });
        }

        suggestions
    }

    /// Check if a constraint is relevant to a role
    fn is_relevant_to_role(&self, text: &str, role_lower: &str) -> bool {
        let text_lower = text.to_lowercase();

        // Extract keywords from role
        let role_keywords: Vec<&str> = role_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() > 2)
            .collect();

        // Check if any keyword matches
        role_keywords.iter().any(|kw| text_lower.contains(kw))
            // Or common mappings
            || (role_lower.contains("architect") && text_lower.contains("module"))
            || (role_lower.contains("debug") && (text_lower.contains("error") || text_lower.contains("log")))
            || (role_lower.contains("security") && text_lower.contains("security"))
            || (role_lower.contains("test") && text_lower.contains("test"))
    }

    /// Check if constraint affects common areas
    fn is_common_constraint(&self, constraint: &EnrichedConstraint) -> bool {
        matches!(
            constraint.category,
            ConstraintCategory::Concurrency
                | ConstraintCategory::ResourceManagement
                | ConstraintCategory::SecurityConstraint
        )
    }

    fn categorize_constraint_kind(
        &self,
        kind: &crate::pipeline::analysis::deep_analyzer::ConstraintKind,
    ) -> ConstraintCategory {
        use crate::pipeline::analysis::deep_analyzer::ConstraintKind;

        match kind {
            ConstraintKind::AntiPattern => ConstraintCategory::AntiPattern,
            ConstraintKind::HiddenDependency => ConstraintCategory::HiddenDependency,
            ConstraintKind::Invariant => ConstraintCategory::Gotcha,
            ConstraintKind::WorkflowRequirement => ConstraintCategory::OrderDependency,
            ConstraintKind::NamingConvention => ConstraintCategory::Gotcha,
        }
    }

    fn extract_topic(description: &str) -> String {
        // Extract first meaningful word as topic
        description
            .split_whitespace()
            .find(|w| w.len() > 3 && w.chars().all(|c| c.is_alphanumeric()))
            .unwrap_or("constraint")
            .to_lowercase()
    }

    /// Check if coverage meets minimum threshold
    pub fn meets_coverage_threshold(&self, coverage: &ConstraintCoverage) -> bool {
        coverage.coverage_ratio >= self.min_coverage_ratio
    }
}

/// Format enriched constraint for inclusion in skill body
impl EnrichedConstraint {
    pub fn format_for_skill(&self) -> String {
        let severity_marker = match self.severity {
            ConstraintSeverity::Critical => "⚠️ CRITICAL:",
            ConstraintSeverity::High => "⚠️",
            ConstraintSeverity::Medium => "📌",
            ConstraintSeverity::Low => "💡",
        };

        let mut result = format!("{} {}", severity_marker, self.description);

        if !self.file_refs.is_empty() {
            result.push_str(&format!(" (see {})", self.file_refs.join(", ")));
        }

        result
    }
}

/// Format agent internal knowledge as prompt section
impl AgentInternalKnowledge {
    pub fn format_as_prompt_section(&self) -> String {
        let mut sections = Vec::new();

        if !self.gotchas.is_empty() {
            let mut s = String::from("### Gotchas\n");
            for gotcha in &self.gotchas {
                s.push_str(&format!("- {}\n", gotcha));
            }
            sections.push(s);
        }

        if !self.order_dependencies.is_empty() {
            let mut s = String::from("### Order Dependencies\n");
            for dep in &self.order_dependencies {
                s.push_str(&format!("- {}\n", dep));
            }
            sections.push(s);
        }

        if !self.anti_patterns.is_empty() {
            let mut s = String::from("### Anti-Patterns\n");
            for anti in &self.anti_patterns {
                s.push_str(&format!("- {}\n", anti));
            }
            sections.push(s);
        }

        if !self.key_references.is_empty() {
            let mut s = String::from("### Key References\n");
            for r in &self.key_references {
                if let Some(line) = r.line {
                    s.push_str(&format!("- {}:{} - {}\n", r.path, line, r.description));
                } else {
                    s.push_str(&format!("- {} - {}\n", r.path, r.description));
                }
            }
            sections.push(s);
        }

        if !self.module_context.is_empty() {
            let mut s = String::from("### Module Context\n");
            for ctx in &self.module_context {
                s.push_str(&format!("- {}\n", ctx));
            }
            sections.push(s);
        }

        if sections.is_empty() {
            "### Internal Knowledge\n- Consult @CLAUDE.md for project-specific constraints\n"
                .to_string()
        } else {
            format!("## Internal Knowledge\n\n{}", sections.join("\n"))
        }
    }

    /// Check if this knowledge is substantial (not empty)
    pub fn is_substantial(&self) -> bool {
        !self.gotchas.is_empty()
            || !self.order_dependencies.is_empty()
            || !self.anti_patterns.is_empty()
            || self.key_references.len() >= 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(ConstraintSeverity::Critical > ConstraintSeverity::High);
        assert!(ConstraintSeverity::High > ConstraintSeverity::Medium);
        assert!(ConstraintSeverity::Medium > ConstraintSeverity::Low);
    }

    #[test]
    fn test_constraint_format() {
        let constraint = EnrichedConstraint {
            description: "Provider must be Arc-shared".to_string(),
            category: ConstraintCategory::Concurrency,
            severity: ConstraintSeverity::Critical,
            file_refs: vec!["@src/ai/provider.rs:42".to_string()],
            related_modules: vec!["ai".to_string()],
        };

        let formatted = constraint.format_for_skill();
        assert!(formatted.contains("CRITICAL"));
        assert!(formatted.contains("Arc-shared"));
        assert!(formatted.contains("@src/ai/provider.rs:42"));
    }

    #[test]
    fn test_agent_knowledge_format() {
        let knowledge = AgentInternalKnowledge {
            gotchas: vec!["Budget uses CAS loop".to_string()],
            order_dependencies: vec!["Init provider before budget".to_string()],
            anti_patterns: vec!["Avoid creating new provider instances".to_string()],
            key_references: vec![KeyReference {
                path: "@src/ai/budget.rs".to_string(),
                line: Some(42),
                description: "CAS loop implementation".to_string(),
            }],
            module_context: vec!["ai: LLM provider management".to_string()],
        };

        let formatted = knowledge.format_as_prompt_section();
        assert!(formatted.contains("## Internal Knowledge"));
        assert!(formatted.contains("### Gotchas"));
        assert!(formatted.contains("Budget uses CAS loop"));
        assert!(formatted.contains("@src/ai/budget.rs:42"));
    }

    #[test]
    fn test_coverage_calculation() {
        let engine = EnrichmentEngine::default();

        let coverage = ConstraintCoverage {
            total_constraints: 10,
            covered_constraints: 8,
            uncovered: vec![],
            coverage_ratio: 0.8,
            artifact_coverage: HashMap::new(),
        };

        assert!(engine.meets_coverage_threshold(&coverage));

        let low_coverage = ConstraintCoverage {
            coverage_ratio: 0.5,
            ..coverage
        };
        assert!(!engine.meets_coverage_threshold(&low_coverage));
    }
}
