//! Enrichment Engine - Maps synthesis findings to artifact content

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
    /// Key abstractions from deep analysis
    pub key_abstractions: Vec<EnrichedAbstraction>,
    /// File insights with gotchas
    pub file_insights: Vec<EnrichedFileInsight>,
}

/// Key abstraction enriched with context
#[derive(Debug, Clone)]
pub struct EnrichedAbstraction {
    pub name: String,
    pub kind: String,
    pub file_ref: String,
    pub description: String,
    pub usage_notes: Vec<String>,
}

/// File insight enriched with context
#[derive(Debug, Clone)]
pub struct EnrichedFileInsight {
    pub file: String,
    pub purpose: String,
    pub gotchas: Vec<String>,
    pub key_exports: Vec<String>,
}

/// A constraint enriched with context for injection into artifacts
#[derive(Debug, Clone)]
pub struct EnrichedConstraint {
    /// The constraint text
    pub description: String,
    /// Category (gotcha, hidden_dependency, anti_pattern, order_dependency)
    pub category: ConstraintCategory,
    /// Severity level
    pub severity: Severity,
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
    pub severity: Severity,
    pub file_refs: Vec<String>,
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

impl std::fmt::Display for ConstraintCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gotcha => write!(f, "Gotcha"),
            Self::HiddenDependency => write!(f, "Hidden Dependency"),
            Self::AntiPattern => write!(f, "Anti-Pattern"),
            Self::OrderDependency => write!(f, "Order Dependency"),
            Self::Concurrency => write!(f, "Concurrency"),
            Self::ResourceManagement => write!(f, "Resource Management"),
            Self::InitializationOrder => write!(f, "Initialization Order"),
            Self::SecurityConstraint => write!(f, "Security"),
        }
    }
}

use crate::types::Severity;

/// Enrichment Engine that bridges synthesis to generation
pub struct EnrichmentEngine {
    min_coverage_ratio: f32,
    max_constraints_per_skill: usize,
    max_knowledge_items_per_agent: usize,
    max_anti_patterns_per_agent: usize,
    max_key_references_per_agent: usize,
    /// Maximum evidence locations per pattern (0 = unlimited)
    max_locations_per_pattern: usize,
    /// Maximum evidence references per constraint (0 = unlimited)
    max_evidence_per_constraint: usize,
}

impl Default for EnrichmentEngine {
    fn default() -> Self {
        Self {
            min_coverage_ratio: 0.8,
            max_constraints_per_skill: 5,
            max_knowledge_items_per_agent: 10,
            max_anti_patterns_per_agent: 10,
            max_key_references_per_agent: 10,
            max_locations_per_pattern: 5,   // Increased from hardcoded 2
            max_evidence_per_constraint: 5, // Increased from hardcoded 2
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

        // Map constraints to skills using file-based matching
        for planned_skill in &plan.skills_plan.planned_skills {
            let relevant = self.find_relevant_constraints(
                &planned_skill.name,
                &planned_skill.trigger,
                &all_constraints,
                synthesis,
                &constraints.complex_workflows,
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
                file_refs: c.file_refs.clone(),
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

        // Extract key abstractions (from deep analysis)
        let key_abstractions = self.extract_key_abstractions(synthesis);

        // Extract file insights (from deep analysis)
        let file_insights = self.extract_file_insights(synthesis);

        EnrichedPlan {
            plan,
            skill_constraints,
            agent_knowledge,
            coverage,
            key_abstractions,
            file_insights,
        }
    }

    fn extract_key_abstractions(
        &self,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Vec<EnrichedAbstraction> {
        let Some(synth) = synthesis else {
            return Vec::new();
        };

        synth
            .deep
            .key_abstractions
            .iter()
            .map(|a| EnrichedAbstraction {
                name: a.name.clone(),
                kind: format!("{:?}", a.kind),
                file_ref: format!("@{}:{}", a.file, a.line),
                description: a.description.clone(),
                usage_notes: a.usage_notes.clone(),
            })
            .collect()
    }

    fn extract_file_insights(
        &self,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Vec<EnrichedFileInsight> {
        let Some(synth) = synthesis else {
            return Vec::new();
        };

        synth
            .deep
            .insights
            .iter()
            .filter(|i| !i.gotchas.is_empty())
            .map(|i| EnrichedFileInsight {
                file: i.file.clone(),
                purpose: i.purpose.clone(),
                gotchas: i.gotchas.clone(),
                key_exports: i.key_exports.clone(),
            })
            .collect()
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
                severity: Severity::Medium, // Gotcha doesn't have severity field
                file_refs: gotcha.related_files.clone(),
                related_modules: Vec::new(),
            });
        }

        // From ExtractedConstraints - Anti-patterns
        for anti in &constraints.anti_patterns {
            let severity = match anti.severity {
                crate::types::Severity::Low => Severity::Low,
                crate::types::Severity::Medium => Severity::Medium,
                crate::types::Severity::High => Severity::High,
                crate::types::Severity::Critical => Severity::Critical,
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
                severity: Severity::High,
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
                    let severity = constraint.severity;

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
                                severity: Severity::Medium,
                                file_refs: vec![format!("@{}", module.path)],
                                related_modules: vec![module.name.clone()],
                            });
                        }
                    }
                }
            }

            // File insights with gotchas (from deep analysis)
            for insight in &synth.deep.insights {
                for gotcha in &insight.gotchas {
                    if !result.iter().any(|c| c.description == *gotcha) {
                        result.push(EnrichedConstraint {
                            description: gotcha.clone(),
                            category: ConstraintCategory::Gotcha,
                            severity: Severity::High,
                            file_refs: vec![format!("@{}", insight.file)],
                            related_modules: Vec::new(),
                        });
                    }
                }
            }
        }

        result
    }

    /// Find constraints relevant to a specific skill using structural matching
    fn find_relevant_constraints(
        &self,
        skill_name: &str,
        trigger: &str,
        all_constraints: &[EnrichedConstraint],
        synthesis: Option<&SynthesizedAnalysis>,
        workflows: &[super::phases::constraint_extraction::ComplexWorkflow],
    ) -> Vec<EnrichedConstraint> {
        // 1. Get files involved in this skill's workflow
        let skill_files: HashSet<String> = workflows
            .iter()
            .find(|w| Self::to_kebab_case(&w.name) == skill_name)
            .map(|w| {
                w.steps
                    .iter()
                    .flat_map(|s| s.files_involved.iter().cloned())
                    .collect()
            })
            .unwrap_or_default();

        // 2. Extract module prefixes from files
        let skill_modules: HashSet<String> = skill_files
            .iter()
            .filter_map(|f| Self::extract_module_prefix(f))
            .collect();

        // 3. Get synthesis modules if available
        let synthesis_modules: HashSet<String> = synthesis
            .map(|s| {
                s.modules
                    .iter()
                    .map(|m| Self::extract_module_prefix(&m.path).unwrap_or_default())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        // 4. Combine all known modules for this skill
        let all_skill_modules: HashSet<_> =
            skill_modules.union(&synthesis_modules).cloned().collect();

        // 5. Match constraints by file/module overlap
        let mut relevant: Vec<_> = all_constraints
            .iter()
            .filter(|c| {
                // Check if constraint's file refs overlap with skill's modules
                let file_match = c.file_refs.iter().any(|ref_path| {
                    let ref_module = Self::extract_module_prefix(ref_path).unwrap_or_default();
                    all_skill_modules.contains(&ref_module)
                        || skill_files.iter().any(|sf| {
                            let sf_norm = sf.strip_prefix('@').unwrap_or(sf);
                            let ref_norm = ref_path.strip_prefix('@').unwrap_or(ref_path);
                            sf_norm == ref_norm
                        })
                });

                // Check if constraint's related_modules overlap
                let module_match = c.related_modules.iter().any(|m| {
                    let m_lower = m.to_lowercase();
                    all_skill_modules.iter().any(|sm| {
                        sm.to_lowercase().contains(&m_lower) || m_lower.contains(&sm.to_lowercase())
                    })
                });

                // High severity common constraints (fallback)
                let severity_match =
                    c.severity >= Severity::Critical && self.is_common_constraint(c);

                file_match || module_match || severity_match
            })
            .cloned()
            .collect();

        // Keyword matching when structural analysis has no overlap
        if relevant.is_empty() {
            relevant = self.keyword_matching(skill_name, trigger, all_constraints);
        }

        // Sort by severity (highest first) and take top N
        relevant.sort_by(|a, b| b.severity.cmp(&a.severity));
        relevant.truncate(self.max_constraints_per_skill);

        relevant
    }

    fn keyword_matching(
        &self,
        _skill_name: &str,
        _trigger: &str,
        _all_constraints: &[EnrichedConstraint],
    ) -> Vec<EnrichedConstraint> {
        Vec::new()
    }

    fn to_kebab_case(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_whitespace() || c == '_' {
                    '-'
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect::<String>()
            .replace("--", "-")
            .trim_matches('-')
            .to_string()
    }

    fn extract_module_prefix(path: &str) -> Option<String> {
        let path = path.strip_prefix('@').unwrap_or(path);
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            let start = if parts[0] == "src" { 1 } else { 0 };
            // Need at least one directory between start and the filename
            if parts.len() > start + 1 {
                Some(parts[start..parts.len() - 1].join("/"))
            } else {
                None // File directly in root (like src/lib.rs) has no module
            }
        } else {
            None
        }
    }

    /// Build internal knowledge for an agent from synthesis
    fn build_agent_knowledge(
        &self,
        agent_name: &str,
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
                    let take_count = if self.max_locations_per_pattern == 0 {
                        pattern.locations.len()
                    } else {
                        self.max_locations_per_pattern
                    };
                    for location in pattern.locations.iter().take(take_count) {
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
                            anti_patterns
                                .push(format!("{}: {}", constraint.title, constraint.description));
                        }
                        ConstraintKind::HiddenDependency | ConstraintKind::Invariant => {
                            gotchas.push(format!(
                                "{}: {} ({})",
                                constraint.title, constraint.description, constraint.rationale
                            ));
                        }
                        ConstraintKind::WorkflowRequirement | ConstraintKind::NamingConvention => {
                            order_dependencies
                                .push(format!("{}: {}", constraint.title, constraint.description));
                        }
                    }

                    // Add evidence as key references
                    let evidence_take = if self.max_evidence_per_constraint == 0 {
                        constraint.evidence.len()
                    } else {
                        self.max_evidence_per_constraint
                    };
                    for evidence in constraint.evidence.iter().take(evidence_take) {
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

        // Deduplicate and limit with logging when truncation occurs
        gotchas.sort();
        gotchas.dedup();
        if gotchas.len() > self.max_knowledge_items_per_agent {
            tracing::debug!(
                agent = %agent_name,
                original = gotchas.len(),
                limit = self.max_knowledge_items_per_agent,
                "Truncating gotchas for agent"
            );
        }
        gotchas.truncate(self.max_knowledge_items_per_agent);

        anti_patterns.sort();
        anti_patterns.dedup();
        if anti_patterns.len() > self.max_anti_patterns_per_agent {
            tracing::debug!(
                agent = %agent_name,
                original = anti_patterns.len(),
                limit = self.max_anti_patterns_per_agent,
                "Truncating anti-patterns for agent"
            );
        }
        anti_patterns.truncate(self.max_anti_patterns_per_agent);

        if key_references.len() > self.max_key_references_per_agent {
            tracing::debug!(
                agent = %agent_name,
                original = key_references.len(),
                limit = self.max_key_references_per_agent,
                "Truncating key references for agent"
            );
        }
        key_references.truncate(self.max_key_references_per_agent);

        AgentInternalKnowledge {
            gotchas,
            order_dependencies,
            anti_patterns,
            key_references,
            module_context,
        }
    }

    fn is_relevant_to_role(&self, text: &str, role_lower: &str) -> bool {
        let text_lower = text.to_lowercase();
        // Allow short keywords (2+ chars) to match valid role names like "api", "db", "qa", "ui"
        let role_keywords: Vec<&str> = role_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() >= 2)
            .collect();

        role_keywords.iter().any(|kw| text_lower.contains(kw))
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

    /// Check if coverage meets minimum threshold
    pub fn meets_coverage_threshold(&self, coverage: &ConstraintCoverage) -> bool {
        coverage.coverage_ratio >= self.min_coverage_ratio
    }
}

/// Format enriched constraint for inclusion in skill body
impl EnrichedConstraint {
    pub fn format_for_skill(&self) -> String {
        let severity_marker = match self.severity {
            Severity::Critical => "⚠️ CRITICAL:",
            Severity::High => "⚠️",
            Severity::Medium => "📌",
            Severity::Low => "💡",
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
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        assert!(Severity::Medium > Severity::Low);
    }

    #[test]
    fn test_constraint_format() {
        let constraint = EnrichedConstraint {
            description: "Provider must be Arc-shared".to_string(),
            category: ConstraintCategory::Concurrency,
            severity: Severity::Critical,
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

    #[test]
    fn test_extract_module_prefix() {
        assert_eq!(
            EnrichmentEngine::extract_module_prefix("src/ai/provider.rs"),
            Some("ai".to_string())
        );
        assert_eq!(
            EnrichmentEngine::extract_module_prefix("@src/pipeline/phases/detection.rs"),
            Some("pipeline/phases".to_string())
        );
        assert_eq!(EnrichmentEngine::extract_module_prefix("src/lib.rs"), None);
        assert_eq!(
            EnrichmentEngine::extract_module_prefix("config/types.rs"),
            Some("config".to_string())
        );
    }

    #[test]
    fn test_to_kebab_case() {
        assert_eq!(
            EnrichmentEngine::to_kebab_case("Hello World"),
            "hello-world"
        );
        assert_eq!(EnrichmentEngine::to_kebab_case("API_Client"), "api-client");
        assert_eq!(
            EnrichmentEngine::to_kebab_case("Provider Initialization"),
            "provider-initialization"
        );
    }
}
