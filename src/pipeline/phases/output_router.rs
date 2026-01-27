//! Output Strategy Router
//!
//! Determines the optimal output strategy based on project structure:
//! - Single project: Unified CLAUDE.md
//! - Monorepo: CLAUDE.md (common) + Path-based Rules

use serde::{Deserialize, Serialize};

use crate::config::ProjectType;
use crate::pipeline::analysis::{SynthesizedAnalysis, SynthesizedInsights};
use crate::types::Result;
use crate::types::domain::DomainAnalysisResult;

use super::OutputStrategy;
use super::constraint_extraction::ExtractedConstraints;
use super::convention_inference::InferredConventions;
use super::monorepo_analyzer::MonorepoAnalysis;
use super::project_detection::ProjectDetection;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPlan {
    pub strategy: OutputStrategy,
    pub claude_md_plan: ClaudeMdPlan,
    pub rules_plan: RulesPlan,
    pub skills_plan: SkillsPlan,
    pub agents_plan: AgentsPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMdPlan {
    pub content_scope: ContentScope,
    pub sections: Vec<PlannedSection>,
    pub include_architecture: bool,
    pub include_conventions: bool,
    pub include_constraints: bool,
    pub include_domain_knowledge: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContentScope {
    Full,
    CommonOnly,
    OverviewOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedSection {
    pub name: String,
    pub priority: SectionPriority,
    pub source: SectionSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectionPriority {
    Required,
    Recommended,
    Optional,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SectionSource {
    Detection,
    Conventions,
    Constraints,
    DomainAnalysis,
    CrossInsights,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesPlan {
    pub generate_path_rules: bool,
    pub rule_groups: Vec<PlannedRuleGroup>,
    pub location: RulesLocation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RulesLocation {
    DotClaude,
    ClaudeMdInline,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedRuleGroup {
    pub name: String,
    pub paths: Vec<String>,
    pub project_types: Vec<ProjectType>,
    pub languages: Vec<String>,
    pub content_sources: Vec<RuleContentSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuleContentSource {
    Conventions,
    AntiPatterns,
    HiddenDependencies,
    Gotchas,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsPlan {
    pub generate_skills: bool,
    pub planned_skills: Vec<PlannedSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedSkill {
    pub name: String,
    pub trigger: String,
    pub source: SkillSource,
    pub project_scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkillSource {
    ComplexWorkflow,
    CommonTask,
    CrossProjectOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsPlan {
    pub generate_agents: bool,
    pub planned_agents: Vec<PlannedAgent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedAgent {
    pub name: String,
    pub role: String,
    pub scope: AgentScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentScope {
    Global,
    Subproject(String),
    Language(String),
    Domain(String),
}

/// Maximum number of agents to plan (prevents agent sprawl)
/// Set to 0 for unlimited. LLM can generate fewer based on project needs.
const MAX_PLANNED_AGENTS: usize = 10; // Increased from hardcoded 5

pub struct OutputRouter;

impl OutputRouter {
    pub fn plan(
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
    ) -> Result<OutputPlan> {
        Self::plan_with_synthesis(detection, monorepo, conventions, constraints, None)
    }

    /// Plan output with optional synthesis data for enhanced skill/agent generation
    pub fn plan_with_synthesis(
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Result<OutputPlan> {
        Self::plan_full(
            detection,
            monorepo,
            conventions,
            constraints,
            synthesis,
            None,
            None,
        )
    }

    /// Full planning with all analysis data including domain and cross-reference insights
    pub fn plan_full(
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        synthesis: Option<&SynthesizedAnalysis>,
        domain_analysis: Option<&DomainAnalysisResult>,
        cross_insights: Option<&SynthesizedInsights>,
    ) -> Result<OutputPlan> {
        let strategy = Self::determine_strategy(detection, monorepo);
        let claude_md_plan =
            Self::plan_claude_md(&strategy, detection, conventions, constraints, domain_analysis);
        let rules_plan = Self::plan_rules(&strategy, monorepo, conventions, constraints);
        let skills_plan =
            Self::plan_skills(&strategy, constraints, monorepo, synthesis, domain_analysis);
        let agents_plan = Self::plan_agents(
            &strategy,
            detection,
            monorepo,
            synthesis,
            domain_analysis,
            cross_insights,
        );

        let plan = OutputPlan {
            strategy,
            claude_md_plan,
            rules_plan,
            skills_plan,
            agents_plan,
        };

        tracing::info!(
            strategy = ?plan.strategy,
            claude_md_scope = ?plan.claude_md_plan.content_scope,
            rule_groups = plan.rules_plan.rule_groups.len(),
            skills = plan.skills_plan.planned_skills.len(),
            agents = plan.agents_plan.planned_agents.len(),
            "Output plan created"
        );

        Ok(plan)
    }

    fn determine_strategy(
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
    ) -> OutputStrategy {
        if !detection.is_monorepo {
            return OutputStrategy::Unified;
        }

        monorepo
            .map(|m| m.output_strategy)
            .unwrap_or(OutputStrategy::SplitByProject)
    }

    fn plan_claude_md(
        strategy: &OutputStrategy,
        detection: &ProjectDetection,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        domain_analysis: Option<&DomainAnalysisResult>,
    ) -> ClaudeMdPlan {
        let content_scope = match strategy {
            OutputStrategy::Unified => ContentScope::Full,
            OutputStrategy::SplitByProject | OutputStrategy::Hierarchical => {
                ContentScope::CommonOnly
            }
            OutputStrategy::SplitByLanguage => ContentScope::OverviewOnly,
        };

        let mut sections = vec![PlannedSection {
            name: "Overview".to_string(),
            priority: SectionPriority::Required,
            source: SectionSource::Detection,
        }];

        if !conventions.architecture.pattern_name.is_empty() {
            sections.push(PlannedSection {
                name: "Architecture".to_string(),
                priority: SectionPriority::Required,
                source: SectionSource::Conventions,
            });
        }

        sections.push(PlannedSection {
            name: "Commands".to_string(),
            priority: SectionPriority::Required,
            source: SectionSource::Detection,
        });

        if content_scope == ContentScope::Full {
            sections.push(PlannedSection {
                name: "Conventions".to_string(),
                priority: SectionPriority::Recommended,
                source: SectionSource::Conventions,
            });

            if !constraints.anti_patterns.is_empty() || !constraints.gotchas.is_empty() {
                sections.push(PlannedSection {
                    name: "Constraints".to_string(),
                    priority: SectionPriority::Recommended,
                    source: SectionSource::Constraints,
                });
            }

            // Add domain knowledge section if domain analysis has content
            if let Some(domain) = domain_analysis {
                let has_domain_content = !domain.policies.is_empty()
                    || !domain.core_logic.is_empty()
                    || !domain.glossary.terms.is_empty()
                    || !domain.workflows.is_empty();

                if has_domain_content {
                    sections.push(PlannedSection {
                        name: "Domain Knowledge".to_string(),
                        priority: SectionPriority::Recommended,
                        source: SectionSource::DomainAnalysis,
                    });
                }
            }
        }

        if detection.is_monorepo {
            sections.push(PlannedSection {
                name: "Workspace".to_string(),
                priority: SectionPriority::Required,
                source: SectionSource::Detection,
            });
        }

        let has_domain_content = domain_analysis
            .map(|d| {
                !d.policies.is_empty()
                    || !d.core_logic.is_empty()
                    || !d.glossary.terms.is_empty()
                    || !d.workflows.is_empty()
            })
            .unwrap_or(false);

        ClaudeMdPlan {
            content_scope,
            sections,
            include_architecture: true,
            include_conventions: content_scope == ContentScope::Full,
            include_constraints: content_scope == ContentScope::Full
                && !constraints.anti_patterns.is_empty(),
            include_domain_knowledge: content_scope == ContentScope::Full && has_domain_content,
        }
    }

    fn plan_rules(
        strategy: &OutputStrategy,
        monorepo: Option<&MonorepoAnalysis>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
    ) -> RulesPlan {
        let generate_path_rules = strategy.requires_path_rules();

        let location = if generate_path_rules {
            RulesLocation::DotClaude
        } else {
            RulesLocation::ClaudeMdInline
        };

        let mut rule_groups = Vec::new();

        if let Some(mono) = monorepo {
            for group in &mono.rules_grouping {
                rule_groups.push(PlannedRuleGroup {
                    name: group.name.clone(),
                    paths: group.paths.clone(),
                    project_types: group.project_types.clone(),
                    languages: group.languages.clone(),
                    content_sources: vec![
                        RuleContentSource::Conventions,
                        RuleContentSource::AntiPatterns,
                    ],
                });
            }
        }

        // Add project-conventions if there's valuable convention data
        let has_valuable_conventions = !conventions.patterns.is_empty()
            || !conventions.architecture.pattern_name.is_empty()
            || !conventions.architecture.layers.is_empty();

        if rule_groups.is_empty() && has_valuable_conventions {
            rule_groups.push(PlannedRuleGroup {
                name: "project-conventions".to_string(),
                paths: vec!["**/*".to_string()],
                project_types: vec![],
                languages: vec![],
                content_sources: vec![RuleContentSource::Conventions],
            });
        }

        if !constraints.anti_patterns.is_empty() {
            let has_anti_pattern_group = rule_groups
                .iter()
                .any(|g| g.content_sources.contains(&RuleContentSource::AntiPatterns));

            if !has_anti_pattern_group {
                rule_groups.push(PlannedRuleGroup {
                    name: "anti-patterns".to_string(),
                    paths: vec!["**/*".to_string()],
                    project_types: vec![],
                    languages: vec![],
                    content_sources: vec![RuleContentSource::AntiPatterns],
                });
            }
        }

        RulesPlan {
            generate_path_rules,
            rule_groups,
            location,
        }
    }

    fn plan_skills(
        strategy: &OutputStrategy,
        constraints: &ExtractedConstraints,
        monorepo: Option<&MonorepoAnalysis>,
        synthesis: Option<&SynthesizedAnalysis>,
        domain_analysis: Option<&DomainAnalysisResult>,
    ) -> SkillsPlan {
        let mut planned_skills = Vec::new();

        for workflow in &constraints.complex_workflows {
            if !workflow.steps.is_empty() {
                planned_skills.push(PlannedSkill {
                    name: to_kebab_case(&workflow.name),
                    trigger: workflow.trigger.clone(),
                    source: SkillSource::ComplexWorkflow,
                    project_scope: None,
                });
            }
        }

        if let Some(synth) = synthesis {
            for module in &synth.modules {
                if !module.patterns.is_empty() && !module.internal_deps.is_empty() {
                    let skill_name = format!("{}-workflow", to_kebab_case(&module.name));
                    if !planned_skills.iter().any(|s| s.name == skill_name) {
                        planned_skills.push(PlannedSkill {
                            name: skill_name,
                            trigger: format!("Working with {} module", module.name),
                            source: SkillSource::CommonTask,
                            project_scope: Some(module.path.clone()),
                        });
                    }
                }
            }
        }

        // Skills from domain business workflows
        if let Some(domain) = domain_analysis {
            for workflow in &domain.workflows {
                let skill_name = to_kebab_case(&workflow.name);
                if !planned_skills.iter().any(|s| s.name == skill_name) {
                    planned_skills.push(PlannedSkill {
                        name: skill_name,
                        trigger: workflow.description.clone(),
                        source: SkillSource::ComplexWorkflow,
                        project_scope: None,
                    });
                }
            }
        }

        // Cross-project skill for monorepos
        if let Some(mono) = monorepo
            && strategy.requires_subproject_agents()
            && mono.subprojects.len() > 1
        {
            planned_skills.push(PlannedSkill {
                name: "cross-project-update".to_string(),
                trigger: "Coordinated update across projects".to_string(),
                source: SkillSource::CrossProjectOperation,
                project_scope: None,
            });
        }

        SkillsPlan {
            generate_skills: !planned_skills.is_empty(),
            planned_skills,
        }
    }

    /// Dynamic agent planning based on synthesis, domain analysis, and cross insights
    /// Derives agents from actual codebase insights rather than hardcoded patterns
    fn plan_agents(
        strategy: &OutputStrategy,
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
        synthesis: Option<&SynthesizedAnalysis>,
        domain_analysis: Option<&DomainAnalysisResult>,
        cross_insights: Option<&SynthesizedInsights>,
    ) -> AgentsPlan {
        let mut planned_agents = Vec::new();

        if let Some(synth) = synthesis {
            for module in &synth.modules {
                let has_complexity = !module.constraints.is_empty()
                    || module.patterns.len() >= 2
                    || module.internal_deps.len() >= 2;

                if has_complexity && !module.responsibility.is_empty() {
                    let agent_name = format!("{}-specialist", to_kebab_case(&module.name));

                    if !is_generic_agent_name(&agent_name) {
                        planned_agents.push(PlannedAgent {
                            name: agent_name,
                            role: format!(
                                "{} domain expert with knowledge of: {}",
                                module.name,
                                module
                                    .constraints
                                    .iter()
                                    .map(|c| c.as_str())
                                    .collect::<Vec<_>>()
                                    .join("; ")
                            ),
                            scope: AgentScope::Domain(module.path.clone()),
                        });
                    }
                }
            }

            // Constraint-based agents for areas with patterns - no arbitrary limit
            let constraint_areas = synth
                .deep
                .patterns
                .iter()
                .filter(|p| p.locations.len() >= 2);

            for pattern in constraint_areas {
                let area_name = pattern.name.replace(' ', "-").to_lowercase();
                if !is_generic_agent_name(&area_name) {
                    planned_agents.push(PlannedAgent {
                        name: format!("{}-debugger", area_name),
                        role: format!(
                            "Debug {} related issues using internal knowledge",
                            pattern.name
                        ),
                        scope: AgentScope::Global,
                    });
                }
            }
        }

        // Domain expert agents from core domain logic - include all
        if let Some(domain) = domain_analysis {
            for logic in &domain.core_logic {
                let agent_name = format!("{}-expert", to_kebab_case(&logic.name));
                if !planned_agents.iter().any(|a| a.name == agent_name)
                    && !is_generic_agent_name(&agent_name)
                {
                    planned_agents.push(PlannedAgent {
                        name: agent_name,
                        role: format!(
                            "Expert in {} ({:?}): {}",
                            logic.name, logic.logic_type, logic.description
                        ),
                        scope: AgentScope::Domain(logic.location.file.clone()),
                    });
                }
            }

            // Policy enforcement agents for strict policies
            for policy in domain.policies.iter().filter(|p| {
                matches!(
                    p.enforcement,
                    crate::types::domain::EnforcementLevel::Strict
                )
            }) {
                let agent_name = format!("{}-enforcer", to_kebab_case(&policy.name));
                if !planned_agents.iter().any(|a| a.name == agent_name)
                    && !is_generic_agent_name(&agent_name)
                {
                    planned_agents.push(PlannedAgent {
                        name: agent_name,
                        role: format!(
                            "Enforce {} policy ({:?}): {}",
                            policy.name, policy.policy_type, policy.description
                        ),
                        scope: AgentScope::Global,
                    });
                }
            }
        }

        // Agents for critical cross-module concerns from tier3 insights - include all
        if let Some(insights) = cross_insights {
            for insight in &insights.tier3_insights {
                let agent_name = format!("{}-guardian", to_kebab_case(&insight.title));
                if !planned_agents.iter().any(|a| a.name == agent_name)
                    && !is_generic_agent_name(&agent_name)
                {
                    planned_agents.push(PlannedAgent {
                        name: agent_name,
                        role: format!(
                            "Guard against: {} → {}",
                            insight.description, insight.prevention_guidance
                        ),
                        scope: AgentScope::Global,
                    });
                }
            }
        }

        // Cross-project coordinator for monorepos
        if strategy.requires_subproject_agents()
            && let Some(mono) = monorepo
            && mono.subprojects.len() >= 2
        {
            planned_agents.push(PlannedAgent {
                name: "cross-package-coordinator".to_string(),
                role: format!(
                    "Coordinate changes across {} packages with dependency awareness",
                    mono.subprojects.len()
                ),
                scope: AgentScope::Global,
            });
        }

        if detection.languages.len() > 1 {
            let significant_langs: Vec<_> = detection
                .languages
                .iter()
                .filter(|l| l.file_count >= 5)
                .map(|l| l.language.as_str())
                .collect();

            if significant_langs.len() > 1 {
                planned_agents.push(PlannedAgent {
                    name: "polyglot-integrator".to_string(),
                    role: format!(
                        "Integrate changes across {} codebases with cross-language constraints",
                        significant_langs.join("/")
                    ),
                    scope: AgentScope::Global,
                });
            }
        }

        if MAX_PLANNED_AGENTS > 0 {
            planned_agents.truncate(MAX_PLANNED_AGENTS);
        }

        AgentsPlan {
            generate_agents: !planned_agents.is_empty(),
            planned_agents,
        }
    }
}

/// Agent name filtering is delegated to LLM during generation.
/// Programmatic filtering with hardcoded patterns causes:
/// - False positives ("main-handler", "core-service" are legitimate)
/// - English-only bias
/// - No override mechanism for context-specific names
fn is_generic_agent_name(_name: &str) -> bool {
    false // LLM determines agent name quality, not pattern matching
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

pub fn plan(
    detection: &ProjectDetection,
    monorepo: Option<&MonorepoAnalysis>,
    conventions: &InferredConventions,
    constraints: &ExtractedConstraints,
) -> Result<OutputPlan> {
    OutputRouter::plan(detection, monorepo, conventions, constraints)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_strategy() {
        assert!(!OutputStrategy::Unified.requires_path_rules());
        assert!(OutputStrategy::SplitByProject.requires_path_rules());
    }

    #[test]
    fn test_to_kebab_case() {
        assert_eq!(to_kebab_case("Hello World"), "hello-world");
        assert_eq!(to_kebab_case("API_Client"), "api-client");
        assert_eq!(to_kebab_case("alreadyKebab"), "alreadykebab");
    }

    #[test]
    fn test_section_priority_ordering() {
        assert!(SectionPriority::Required < SectionPriority::Recommended);
        assert!(SectionPriority::Recommended < SectionPriority::Optional);
    }
}
