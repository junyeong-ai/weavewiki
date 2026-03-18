//! Budget-aware context sections for LLM prompts (3-Tier Progressive Loading).

use crate::ai::context_tracker::{ContextBudget, estimate_tokens};
use super::GenerationContext;

// =========================================================================
// Budgeted Context - 3-Tier Progressive Loading
// =========================================================================

/// Tracks content that was summarized or omitted during budget enforcement.
/// Preserves original reference paths for drill-down capability.
#[derive(Debug, Clone)]
pub struct OmittedReference {
    /// Section that was summarized (e.g., "tier2_modules", "tier3_domain")
    pub section: String,
    /// Summarization level applied
    pub level: SummarizationLevel,
    /// Reference paths to original content that was summarized away
    pub original_paths: Vec<String>,
}

/// How aggressively content was summarized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummarizationLevel {
    /// Full content included
    Full,
    /// Module-level detail preserved
    ModuleLevel,
    /// Aggregated to group-level summaries
    GroupLevel,
    /// Compressed to project-level overview
    ProjectLevel,
    /// Section entirely omitted
    Omitted,
}

/// Pre-computed, budget-aware context sections for LLM prompts.
///
/// Tier 1 (Essential): Always included at full fidelity.
/// Tier 2 (Relevant): Included if budget allows, summarized if tight.
/// Tier 3 (Reference): Summarized or omitted if budget is exhausted.
#[derive(Debug, Clone)]
pub struct BudgetedSections {
    pub system_prompt: String,
    pub tier1: Tier1Sections,
    pub tier2: Tier2Sections,
    pub tier3: Tier3Sections,
    pub budget: ContextBudget,
    /// Sections that were summarized or omitted, with reference paths for drill-down.
    pub omitted: Vec<OmittedReference>,
}

impl BudgetedSections {
    /// Total tokens across all tiers.
    pub fn total_tokens(&self) -> usize {
        self.budget.allocated.values().sum()
    }
}

/// Tier 1: Essential context — always included.
#[derive(Debug, Clone)]
pub struct Tier1Sections {
    pub project_identity: String,
    pub conventions: String,
    pub constraints: String,
}

/// Tier 2: Relevant context — included if budget allows.
#[derive(Debug, Clone)]
pub struct Tier2Sections {
    pub module_summaries: String,
    pub patterns: String,
    pub discovered_insights: String,
}

/// Tier 3: Reference context — summarized or omitted if budget tight.
#[derive(Debug, Clone)]
pub struct Tier3Sections {
    pub domain_knowledge: String,
    pub cross_analysis: String,
}

struct SummarizationLimits {
    modules: usize,
    patterns: usize,
    insights: usize,
}

fn compute_summarization_limits(remaining_budget: usize) -> SummarizationLimits {
    SummarizationLimits {
        modules: (remaining_budget / 500).clamp(3, 30),
        patterns: (remaining_budget / 200).clamp(5, 50),
        insights: (remaining_budget / 300).clamp(3, 20),
    }
}

impl<'a> GenerationContext<'a> {
    /// Plan a token budget and produce pre-formatted sections.
    ///
    /// Allocation order: tier1 (guaranteed) → tier3 (reserved) → tier2 (remaining).
    /// This ensures essential context and reference material are always prioritized
    /// over compressible module-level detail.
    pub fn plan_budget(&self, model_limit: usize) -> BudgetedSections {
        let mut budget = ContextBudget::new(model_limit);
        let mut omitted = Vec::new();

        let system_prompt = self.build_system_prompt();
        budget.allocate("system_prompt", estimate_tokens(&system_prompt));

        // Tier 1: Essential — guaranteed 80% minimum
        let tier1 = self.build_tier1(&mut budget);

        // Tier 3: Reference — reserved space secured before tier2
        let tier3 = self.build_tier3(&mut budget, &mut omitted);

        // Tier 2: Relevant — gets remaining budget, summarized if tight
        let tier2 = self.build_tier2(&mut budget, &mut omitted);

        BudgetedSections {
            system_prompt,
            tier1,
            tier2,
            tier3,
            budget,
            omitted,
        }
    }

    fn build_tier1(&self, budget: &mut ContextBudget) -> Tier1Sections {
        let project_identity = format!(
            "Project: {} ({})\nTech: {}\nFrameworks: {}",
            self.project_name,
            self.detection.primary_type,
            self.tech_stack.primary_language,
            self.detected_frameworks().join(", "),
        );
        budget.allocate_guaranteed("tier1_identity", estimate_tokens(&project_identity));

        let conventions = self.format_conventions();
        budget.allocate_guaranteed("tier1_conventions", estimate_tokens(&conventions));

        let constraints = self.format_constraints();
        budget.allocate_guaranteed("tier1_constraints", estimate_tokens(&constraints));

        Tier1Sections {
            project_identity,
            conventions,
            constraints,
        }
    }

    fn build_tier2(
        &self,
        budget: &mut ContextBudget,
        omitted: &mut Vec<OmittedReference>,
    ) -> Tier2Sections {
        let limits = compute_summarization_limits(budget.remaining());
        let summaries = self.module_summaries();

        let full_modules = self.format_module_summaries(summaries.len());
        let full_tokens = estimate_tokens(&full_modules);

        // Tier 2 overflow: ModuleLevel → GroupLevel (never below GroupLevel for modules)
        let module_summaries = if budget.can_fit(full_tokens) {
            let (actual, _) = budget.allocate_flexible("tier2_modules", full_tokens);
            if actual == full_tokens {
                full_modules
            } else {
                // Partial allocation - fall through to summarization
                self.summarize_tier2_modules(budget, omitted, &summaries, &limits)
            }
        } else {
            self.summarize_tier2_modules(budget, omitted, &summaries, &limits)
        };

        let all_patterns = self.all_patterns();
        let full_patterns = self.format_patterns(&all_patterns);
        let pattern_tokens = estimate_tokens(&full_patterns);

        let patterns = {
            let (_actual, needs_summary) =
                budget.allocate_flexible("tier2_patterns", pattern_tokens);
            if !needs_summary {
                full_patterns
            } else {
                let limited: Vec<_> = all_patterns.into_iter().take(limits.patterns).collect();
                let summarized = self.format_patterns(&limited);
                // Re-allocate with actual summarized size (overwrite previous)
                budget.allocate("tier2_patterns", estimate_tokens(&summarized));
                summarized
            }
        };

        let full_insights = self.format_discovered_insights();
        let insight_tokens = estimate_tokens(&full_insights);

        let discovered_insights = {
            let (_actual, needs_summary) =
                budget.allocate_flexible("tier2_insights", insight_tokens);
            if !needs_summary {
                full_insights
            } else {
                let summarized = self.format_discovered_insights_limited(limits.insights);
                budget.allocate("tier2_insights", estimate_tokens(&summarized));
                summarized
            }
        };

        Tier2Sections {
            module_summaries,
            patterns,
            discovered_insights,
        }
    }

    /// Summarize tier2 modules progressively: ModuleLevel → GroupLevel.
    /// Never summarizes below GroupLevel for tier2.
    fn summarize_tier2_modules(
        &self,
        budget: &mut ContextBudget,
        omitted: &mut Vec<OmittedReference>,
        summaries: &[crate::pipeline::analysis::ModuleSummary],
        limits: &SummarizationLimits,
    ) -> String {
        // Try truncated module-level first (preserve module detail)
        let summarized = self.format_module_summaries(limits.modules);
        let summarized_tokens = estimate_tokens(&summarized);

        if budget.can_fit(summarized_tokens) {
            budget.allocate("tier2_modules", summarized_tokens);
            let omitted_count = summaries.len().saturating_sub(limits.modules);
            if omitted_count > 0 {
                let omitted_names: Vec<_> = summaries
                    .iter()
                    .skip(limits.modules)
                    .map(|s| s.module_path.clone())
                    .collect();
                let annotation = if omitted_names.len() <= 10 {
                    format!(
                        "({} additional modules: {})",
                        omitted_count,
                        omitted_names.join(", ")
                    )
                } else {
                    format!(
                        "({} additional modules: {}, ...)",
                        omitted_count,
                        omitted_names[..10].join(", ")
                    )
                };
                omitted.push(OmittedReference {
                    section: "tier2_modules".into(),
                    level: SummarizationLevel::ModuleLevel,
                    original_paths: omitted_names,
                });
                return format!("{}\n\n{}", summarized, annotation);
            }
            return summarized;
        }

        // Fall back to group-level (minimum detail for tier2)
        let group_summary = self.format_group_summaries();
        let group_tokens = estimate_tokens(&group_summary);

        if budget.can_fit(group_tokens) {
            budget.allocate("tier2_modules", group_tokens);
            let omitted_paths: Vec<_> = summaries.iter().map(|s| s.module_path.clone()).collect();
            omitted.push(OmittedReference {
                section: "tier2_modules".into(),
                level: SummarizationLevel::GroupLevel,
                original_paths: omitted_paths,
            });
            return group_summary;
        }

        // Extreme budget pressure: project-level summary
        let project_summary = self.format_project_summary();
        let project_tokens = estimate_tokens(&project_summary);
        budget.allocate("tier2_modules", project_tokens.min(budget.remaining()));
        let omitted_paths: Vec<_> = summaries.iter().map(|s| s.module_path.clone()).collect();
        omitted.push(OmittedReference {
            section: "tier2_modules".into(),
            level: SummarizationLevel::ProjectLevel,
            original_paths: omitted_paths,
        });
        project_summary
    }

    fn build_tier3(
        &self,
        budget: &mut ContextBudget,
        omitted: &mut Vec<OmittedReference>,
    ) -> Tier3Sections {
        if budget.remaining_total() < 500 {
            // Track that entire tier3 was omitted
            if self.domain_analysis.is_some() {
                omitted.push(OmittedReference {
                    section: "tier3_domain".into(),
                    level: SummarizationLevel::Omitted,
                    original_paths: vec!["domain_analysis".into()],
                });
            }
            if self.cross_insights.is_some() {
                omitted.push(OmittedReference {
                    section: "tier3_cross".into(),
                    level: SummarizationLevel::Omitted,
                    original_paths: vec!["cross_analysis".into()],
                });
            }
            return Tier3Sections {
                domain_knowledge: String::new(),
                cross_analysis: String::new(),
            };
        }

        let domain = self.format_enriched_domain();
        let domain_tokens = estimate_tokens(&domain);

        let (_domain_alloc, domain_needs_summary) =
            budget.allocate_flexible("tier3_domain", domain_tokens);

        let domain_knowledge = if !domain_needs_summary {
            domain
        } else {
            // Tier3 minimum: ModuleLevel (simplified domain with policies only)
            let simple = self
                .domain_knowledge()
                .map(|d| self.format_domain(&Some(d)))
                .unwrap_or_default();
            let simple_tokens = estimate_tokens(&simple);
            if budget.can_fit(simple_tokens) {
                budget.allocate("tier3_domain", simple_tokens);
                omitted.push(OmittedReference {
                    section: "tier3_domain".into(),
                    level: SummarizationLevel::ModuleLevel,
                    original_paths: vec![
                        "domain_workflows".into(),
                        "domain_core_logic".into(),
                        "domain_terminology".into(),
                    ],
                });
                simple
            } else {
                omitted.push(OmittedReference {
                    section: "tier3_domain".into(),
                    level: SummarizationLevel::Omitted,
                    original_paths: vec!["domain_analysis".into()],
                });
                String::new()
            }
        };

        let violations = self.format_architecture_violations();
        let cross_constraints = self.format_cross_constraints();
        let cross_analysis = if !violations.is_empty() || !cross_constraints.is_empty() {
            let combined = format!("{}\n\n{}", violations, cross_constraints);
            let tokens = estimate_tokens(&combined);
            if budget.can_fit(tokens) {
                budget.allocate("tier3_cross", tokens);
                combined
            } else if !violations.is_empty() {
                let vt = estimate_tokens(&violations);
                if budget.can_fit(vt) {
                    budget.allocate("tier3_cross", vt);
                    omitted.push(OmittedReference {
                        section: "tier3_cross".into(),
                        level: SummarizationLevel::ModuleLevel,
                        original_paths: vec!["cross_constraints".into()],
                    });
                    violations
                } else {
                    omitted.push(OmittedReference {
                        section: "tier3_cross".into(),
                        level: SummarizationLevel::Omitted,
                        original_paths: vec![
                            "architecture_violations".into(),
                            "cross_constraints".into(),
                        ],
                    });
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        Tier3Sections {
            domain_knowledge,
            cross_analysis,
        }
    }

    /// Estimate total token cost of all context sections (without budgeting).
    pub fn estimate_total_tokens(&self) -> usize {
        let system = estimate_tokens(&self.build_system_prompt());
        let conventions = estimate_tokens(&self.format_conventions());
        let constraints = estimate_tokens(&self.format_constraints());
        let modules = estimate_tokens(&self.format_module_summaries(usize::MAX));
        let patterns = estimate_tokens(&self.format_patterns(&self.all_patterns()));
        let insights = estimate_tokens(&self.format_discovered_insights());
        let domain = estimate_tokens(&self.format_enriched_domain());
        let violations = estimate_tokens(&self.format_architecture_violations());
        let cross = estimate_tokens(&self.format_cross_constraints());

        system + conventions + constraints + modules + patterns + insights + domain + violations + cross
    }
}
