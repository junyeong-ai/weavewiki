//! Output Strategy Router
//!
//! Determines the optimal output strategy based on project structure:
//! - Single project: Unified CLAUDE.md
//! - Monorepo: CLAUDE.md (common) + Path-based Rules

use serde::{Deserialize, Serialize};

use crate::config::ProjectType;
use crate::pipeline::analysis::{SynthesizedAnalysis, SynthesizedInsights};
use crate::types::Result;
use crate::types::domain::{DomainAnalysisResult, PolicyType};
use crate::types::module_map::{DetectedModule, ModuleGroup};

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
    pub orchestration_plan: Option<OrchestrationPlan>,
    pub module_map_plan: Option<ModuleMapPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedGroupOrchestrator {
    pub name: String,
    pub group_id: String,
    pub module_agent_names: Vec<String>,
    pub tools: Vec<String>,
    pub model: String,
    pub permission_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationPlan {
    pub orchestration_skills: Vec<PlannedOrchestrationSkill>,
    pub module_skills: Vec<PlannedModuleSkill>,
    pub orchestration_agents: Vec<PlannedOrchestrationAgent>,
    pub module_agents: Vec<PlannedModuleAgent>,
    pub group_orchestrators: Vec<PlannedGroupOrchestrator>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedOrchestrationSkill {
    pub name: String,
    pub description: String,
    pub user_invocable: bool,
    pub disable_model_invocation: bool,
    pub context: Option<String>,
    pub agent: Option<String>,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedModuleSkill {
    pub name: String,
    pub module_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedOrchestrationAgent {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub model: String,
    pub permission_mode: String,
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedModuleAgent {
    pub name: String,
    pub module_id: String,
    pub tools: Vec<String>,
    pub model: String,
    pub permission_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMapPlan {
    pub output_path: String,
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
const MAX_PLANNED_AGENTS: usize = 10;

// --- Orchestration generation thresholds ---
const MIN_MODULES_FOR_GROUPING: usize = 6;
const MIN_MODULES_FOR_ORCHESTRATOR: usize = 2;
const MIN_MODULES_FOR_ARCHITECT: usize = 3;
const QA_RISK_THRESHOLD: f32 = 0.5;
const MIN_GOTCHAS_FOR_QA: usize = 3;
const HIGH_RISK_THRESHOLD: f32 = 0.7;
const HIGH_VALUE_THRESHOLD: f32 = 0.8;

pub struct OutputRouter;

impl OutputRouter {
    #[allow(clippy::too_many_arguments)]
    pub fn plan(
        detection: &ProjectDetection,
        monorepo: Option<&MonorepoAnalysis>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        synthesis: Option<&SynthesizedAnalysis>,
        domain_analysis: Option<&DomainAnalysisResult>,
        cross_insights: Option<&SynthesizedInsights>,
        detected_modules: Option<&[DetectedModule]>,
        groups: &[ModuleGroup],
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

        let orchestration_plan = detected_modules
            .filter(|modules| !modules.is_empty())
            .map(|modules| Self::plan_orchestration(
                modules,
                constraints,
                cross_insights,
                domain_analysis,
                groups,
            ));
        let module_map_plan = detected_modules
            .filter(|modules| !modules.is_empty())
            .map(|_| ModuleMapPlan {
                output_path: ".claudegen/module_map.json".to_string(),
            });

        let plan = OutputPlan {
            strategy,
            claude_md_plan,
            rules_plan,
            skills_plan,
            agents_plan,
            orchestration_plan,
            module_map_plan,
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

            // Constraint-based agents for areas with patterns - no arbitrary limit
            let constraint_areas = synth
                .deep
                .patterns
                .iter()
                .filter(|p| p.locations.len() >= 2);

            for pattern in constraint_areas {
                let area_name = pattern.name.replace(' ', "-").to_lowercase();
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

        // Domain expert agents from core domain logic - include all
        if let Some(domain) = domain_analysis {
            for logic in &domain.core_logic {
                let agent_name = format!("{}-expert", to_kebab_case(&logic.name));
                if !planned_agents.iter().any(|a| a.name == agent_name) {
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
                if !planned_agents.iter().any(|a| a.name == agent_name) {
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
                if !planned_agents.iter().any(|a| a.name == agent_name) {
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

    fn plan_orchestration(
        modules: &[DetectedModule],
        constraints: &ExtractedConstraints,
        cross_insights: Option<&SynthesizedInsights>,
        domain_analysis: Option<&DomainAnalysisResult>,
        groups: &[ModuleGroup],
    ) -> OrchestrationPlan {
        let read_tools = vec![
            "Read".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
            "Task".to_string(),
        ];
        let read_only_tools = vec![
            "Read".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
        ];
        let tool_runner_tools = vec![
            "Read".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
            "Bash".to_string(),
        ];
        let edit_tools = vec![
            "Read".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
            "Edit".to_string(),
            "Write".to_string(),
            "Bash".to_string(),
        ];

        // --- Conditional system agents ---
        let mut orchestration_agents = Vec::new();
        let mut orchestration_skills = Vec::new();

        let has_orchestrator = modules.len() >= MIN_MODULES_FOR_ORCHESTRATOR;

        let has_violations = cross_insights
            .map(|i| !i.architecture_violations.is_empty())
            .unwrap_or(false);
        let has_architect = modules.len() >= MIN_MODULES_FOR_ARCHITECT || has_violations;
        if has_architect {
            orchestration_agents.push(PlannedOrchestrationAgent {
                name: "architect".to_string(),
                description: "Architecture reviewer ensuring module boundaries, dependency rules, and design patterns are respected.".to_string(),
                tools: read_only_tools.clone(),
                model: "sonnet".to_string(),
                permission_mode: "default".to_string(),
                skills: vec![],
            });
        }

        let has_risky_modules = modules.iter().any(|m| m.risk_score > QA_RISK_THRESHOLD);
        let has_qa = has_risky_modules || constraints.gotchas.len() >= MIN_GOTCHAS_FOR_QA;
        if has_qa {
            orchestration_agents.push(PlannedOrchestrationAgent {
                name: "qa-reviewer".to_string(),
                description: "Quality assurance reviewer checking consistency across module boundaries. Use proactively after code changes.".to_string(),
                tools: tool_runner_tools.clone(),
                model: "sonnet".to_string(),
                permission_mode: "default".to_string(),
                skills: vec![
                    "qa-static-analysis".to_string(),
                    "qa-test-runner".to_string(),
                ],
            });
        }

        let has_security_constraints = constraints.gotchas.iter().any(|g| has_security_keywords(&g.title))
            || constraints.anti_patterns.iter().any(|a| has_security_keywords(&a.name));
        let has_security_policies = domain_analysis
            .map(|d| {
                d.policies.iter().any(|p| {
                    matches!(p.policy_type, PolicyType::Authorization)
                })
            })
            .unwrap_or(false);
        let has_security = has_security_constraints || has_security_policies;
        if has_security {
            orchestration_agents.push(PlannedOrchestrationAgent {
                name: "security-reviewer".to_string(),
                description: "Security reviewer analyzing changes for vulnerabilities with module-scoped threat awareness.".to_string(),
                tools: read_only_tools.clone(),
                model: "sonnet".to_string(),
                permission_mode: "default".to_string(),
                skills: vec![],
            });
        }

        // --- Conditional skills (tied to agent existence) ---
        if has_orchestrator {
            let orchestrator_tools = vec![
                "Read".to_string(),
                "Grep".to_string(),
                "Glob".to_string(),
                "Task".to_string(),
                "Skill".to_string(),
            ];
            orchestration_skills.push(PlannedOrchestrationSkill {
                name: "claude-pilot".to_string(),
                description: "Multi-agent orchestration. Decomposes tasks, coordinates module agents via consensus planning. Use for cross-module changes.".to_string(),
                user_invocable: true,
                disable_model_invocation: false,
                context: None,
                agent: None,
                allowed_tools: orchestrator_tools.clone(),
            });
            orchestration_skills.push(PlannedOrchestrationSkill {
                name: "consensus-planning".to_string(),
                description: "Cross-module consensus planning. Queries module agents for constraints and resolves conflicts.".to_string(),
                user_invocable: false,
                disable_model_invocation: false,
                context: None,
                agent: None,
                allowed_tools: orchestrator_tools,
            });
        }

        if has_architect {
            orchestration_skills.push(PlannedOrchestrationSkill {
                name: "architecture-review".to_string(),
                description: "Review changes against architecture patterns, layer rules, and module boundaries.".to_string(),
                user_invocable: false,
                disable_model_invocation: false,
                context: Some("fork".to_string()),
                agent: Some("architect".to_string()),
                allowed_tools: read_only_tools,
            });
        }

        if has_qa {
            orchestration_skills.push(PlannedOrchestrationSkill {
                name: "qa-review".to_string(),
                description: "Verify changes against gotchas, anti-patterns, and cross-module consistency.".to_string(),
                user_invocable: false,
                disable_model_invocation: false,
                context: Some("fork".to_string()),
                agent: Some("qa-reviewer".to_string()),
                allowed_tools: read_tools,
            });
        }

        orchestration_skills.push(PlannedOrchestrationSkill {
            name: "qa-static-analysis".to_string(),
            description: "Run project linters, formatters, and static analysis tools on affected paths.".to_string(),
            user_invocable: false,
            disable_model_invocation: true,
            context: None,
            agent: None,
            allowed_tools: tool_runner_tools.clone(),
        });
        orchestration_skills.push(PlannedOrchestrationSkill {
            name: "qa-test-runner".to_string(),
            description: "Run test suites scoped to affected module paths and report results.".to_string(),
            user_invocable: false,
            disable_model_invocation: true,
            context: None,
            agent: None,
            allowed_tools: tool_runner_tools,
        });

        // --- Module skills ---
        let module_skills: Vec<PlannedModuleSkill> = modules
            .iter()
            .map(|m| PlannedModuleSkill {
                name: format!("module-{}", m.module_id),
                module_id: m.module_id.clone(),
            })
            .collect();

        // --- Module agents with differentiated settings ---
        let module_agents: Vec<PlannedModuleAgent> = modules
            .iter()
            .map(|m| {
                let (model, permission_mode) =
                    if m.risk_score > HIGH_RISK_THRESHOLD || m.value_score > HIGH_VALUE_THRESHOLD {
                        ("sonnet", "default")
                    } else {
                        ("haiku", "acceptEdits")
                    };
                PlannedModuleAgent {
                    name: format!("module-{}", m.module_id),
                    module_id: m.module_id.clone(),
                    tools: edit_tools.clone(),
                    model: model.to_string(),
                    permission_mode: permission_mode.to_string(),
                }
            })
            .collect();

        let group_orchestrators = Self::plan_group_orchestrators(groups, modules);

        OrchestrationPlan {
            orchestration_skills,
            module_skills,
            orchestration_agents,
            module_agents,
            group_orchestrators,
        }
    }

    fn plan_group_orchestrators(
        groups: &[ModuleGroup],
        modules: &[DetectedModule],
    ) -> Vec<PlannedGroupOrchestrator> {
        if groups.is_empty() || modules.len() < MIN_MODULES_FOR_GROUPING {
            return Vec::new();
        }

        groups
            .iter()
            .map(|group| {
                let module_agent_names: Vec<String> = group
                    .module_ids
                    .iter()
                    .map(|id| format!("module-{}", id))
                    .collect();

                PlannedGroupOrchestrator {
                    name: format!("group-{}", group.group_id),
                    group_id: group.group_id.clone(),
                    module_agent_names,
                    tools: vec![
                        "Read".to_string(),
                        "Grep".to_string(),
                        "Glob".to_string(),
                        "Task".to_string(),
                        "Skill".to_string(),
                    ],
                    model: "sonnet".to_string(),
                    permission_mode: "default".to_string(),
                }
            })
            .collect()
    }
}

/// Check if a string contains security-related keywords.
/// Used as a fast-path heuristic for conditional agent generation.
/// Over-matching is acceptable (creates an unused reviewer) — under-matching is not.
pub(crate) fn has_security_keywords(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("security")
        || lower.contains("auth")
        || lower.contains("permission")
        || lower.contains("injection")
        || lower.contains("xss")
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
