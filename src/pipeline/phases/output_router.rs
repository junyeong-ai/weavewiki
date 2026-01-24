//! Output Strategy Router
//!
//! Determines the optimal output strategy based on project structure:
//! - Single project: Unified CLAUDE.md
//! - Monorepo: CLAUDE.md (common) + Path-based Rules

use serde::{Deserialize, Serialize};

use crate::config::ProjectType;
use crate::types::Result;

use super::OutputStrategy;
use super::constraint_extraction::ExtractedConstraints;
use super::convention_inference::InferredConventions;
use super::monorepo_analyzer::MonorepoAnalysis;
use super::project_detection::ProjectDetection;
use crate::pipeline::analysis::SynthesizedAnalysis;

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
        let strategy = Self::determine_strategy(detection, monorepo);
        let claude_md_plan = Self::plan_claude_md(&strategy, detection, conventions, constraints);
        let rules_plan = Self::plan_rules(&strategy, monorepo, conventions, constraints);
        let skills_plan = Self::plan_skills(&strategy, constraints, monorepo, synthesis);
        let agents_plan = Self::plan_agents(&strategy, detection, monorepo, synthesis);

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
        }

        if detection.is_monorepo {
            sections.push(PlannedSection {
                name: "Workspace".to_string(),
                priority: SectionPriority::Required,
                source: SectionSource::Detection,
            });
        }

        ClaudeMdPlan {
            content_scope,
            sections,
            include_architecture: true,
            include_conventions: content_scope == ContentScope::Full,
            include_constraints: content_scope == ContentScope::Full
                && !constraints.anti_patterns.is_empty(),
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
    ) -> SkillsPlan {
        let mut planned_skills = Vec::new();

        for workflow in &constraints.complex_workflows {
            if workflow.automation_potential >= 0.5 {
                planned_skills.push(PlannedSkill {
                    name: to_kebab_case(&workflow.name),
                    trigger: workflow.trigger.clone(),
                    source: SkillSource::ComplexWorkflow,
                    project_scope: None,
                });
            }
        }

        // Use synthesis to identify additional skills from deep analysis
        if let Some(synth) = synthesis {
            // Create skills for modules with many patterns (likely complex)
            for module in &synth.modules {
                if module.patterns.len() >= 3 && module.key_files.len() >= 2 {
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

    /// Dynamic agent planning based on synthesis analysis
    /// Instead of hardcoded project-type agents, derive agents from actual codebase insights
    fn plan_agents(
        strategy: &OutputStrategy,
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> AgentsPlan {
        let mut planned_agents = Vec::new();

        // Primary: Derive agents from synthesis analysis (AI-driven insights)
        if let Some(synth) = synthesis {
            // Create domain expert agents for modules with significant complexity
            for module in &synth.modules {
                let complexity_score = module.constraints.len() as f32 * 0.4
                    + module.patterns.len() as f32 * 0.3
                    + module.key_files.len() as f32 * 0.3;

                // Only create agents for truly complex modules
                if complexity_score >= 2.0 && !module.responsibility.is_empty() {
                    let agent_name = format!("{}-specialist", to_kebab_case(&module.name));

                    // Skip generic names
                    if !is_generic_agent_name(&agent_name) {
                        planned_agents.push(PlannedAgent {
                            name: agent_name,
                            role: format!(
                                "{} domain expert with knowledge of: {}",
                                module.name,
                                module
                                    .constraints
                                    .iter()
                                    .take(3)
                                    .map(|c| c.as_str())
                                    .collect::<Vec<_>>()
                                    .join("; ")
                            ),
                            scope: AgentScope::Domain(module.path.clone()),
                        });
                    }
                }
            }

            // Create constraint-based agents for areas with many gotchas
            let constraint_areas = synth
                .deep
                .patterns
                .iter()
                .filter(|p| p.locations.len() >= 2)
                .take(2);

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

        // Secondary: Add cross-project coordinator for monorepos
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

        // Tertiary: Add polyglot coordinator only if truly multi-language
        if detection.languages.len() > 1 {
            let primary_langs: Vec<_> = detection
                .languages
                .iter()
                .filter(|l| l.percentage > 10.0)
                .map(|l| l.language.as_str())
                .collect();

            if primary_langs.len() > 1 {
                planned_agents.push(PlannedAgent {
                    name: "polyglot-integrator".to_string(),
                    role: format!(
                        "Integrate changes across {} codebases with cross-language constraints",
                        primary_langs.join("/")
                    ),
                    scope: AgentScope::Global,
                });
            }
        }

        // Limit to most valuable agents (avoid agent sprawl)
        planned_agents.truncate(5);

        AgentsPlan {
            generate_agents: !planned_agents.is_empty(),
            planned_agents,
        }
    }
}

/// Check if agent name is too generic (Tier 1)
fn is_generic_agent_name(name: &str) -> bool {
    const GENERIC_PATTERNS: &[&str] = &[
        "code-reviewer",
        "test-writer",
        "bug-fixer",
        "feature-developer",
        "code-assistant",
        "helper",
        "navigator",
        "guide",
        "checker",
        "general",
        "generic",
        "default",
        "main",
        "core",
        "basic",
    ];
    let name_lower = name.to_lowercase();
    GENERIC_PATTERNS.iter().any(|p| name_lower.contains(p))
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
