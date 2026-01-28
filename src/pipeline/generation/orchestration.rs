use crate::pipeline::analysis::cross_synthesis::{
    ArchitectureViolation, CrossModuleConstraint, HiddenDependency, SynthesizedInsights,
};
use crate::pipeline::phases::constraint_extraction::{AntiPattern, ExtractedConstraints, Gotcha};
use crate::pipeline::phases::convention_inference::InferredConventions;
use crate::pipeline::phases::output_router::OrchestrationPlan;
use crate::types::agent::{Agent, AgentModel, PermissionMode};
use crate::types::domain::{DomainAnalysisResult, DomainPolicy, PolicyType};
use crate::types::hook::{Hook, ToolHooks};
use crate::types::module_map::{DetectedModule, ModuleGroup};
use crate::types::skill::{ContextMode, Skill};
use crate::types::Rule;

pub struct OrchestrationGenerator;

impl OrchestrationGenerator {
    pub fn generate(
        plan: &OrchestrationPlan,
        modules: &[DetectedModule],
        groups: &[ModuleGroup],
        cross_insights: Option<&SynthesizedInsights>,
        constraints: &ExtractedConstraints,
        domain_analysis: Option<&DomainAnalysisResult>,
        conventions: &InferredConventions,
    ) -> OrchestrationArtifacts {
        let skills = Self::generate_skills(plan, modules, groups, cross_insights, constraints, domain_analysis, conventions);
        let agents = Self::generate_agents(plan, modules, groups, cross_insights, constraints, domain_analysis, conventions);
        let rules = Self::generate_module_rules(modules);

        OrchestrationArtifacts {
            skills,
            agents,
            rules,
        }
    }

    fn generate_skills(
        plan: &OrchestrationPlan,
        modules: &[DetectedModule],
        groups: &[ModuleGroup],
        cross_insights: Option<&SynthesizedInsights>,
        constraints: &ExtractedConstraints,
        domain_analysis: Option<&DomainAnalysisResult>,
        conventions: &InferredConventions,
    ) -> Vec<Skill> {
        let mut skills = Vec::new();

        for planned in &plan.orchestration_skills {
            let body = build_orchestration_skill_body(
                &planned.name, modules, groups, cross_insights, constraints, domain_analysis, conventions,
            );
            let mut skill = Skill::new(&planned.name, &planned.description, body)
                .with_user_invocable(planned.user_invocable)
                .with_disable_model_invocation(planned.disable_model_invocation);

            if !planned.allowed_tools.is_empty() {
                skill = skill.with_tools(planned.allowed_tools.clone());
            }
            if planned.context.as_deref() == Some("fork") {
                skill = skill.with_context(ContextMode::Fork);
            }
            if let Some(ref agent) = planned.agent {
                skill = skill.with_agent(agent.clone());
            }

            skills.push(skill);
        }

        for planned in &plan.module_skills {
            if let Some(module) = modules.iter().find(|m| m.module_id == planned.module_id) {
                let body = build_module_skill_body(module);
                let skill = Skill::new(&planned.name, &planned.name, body);
                skills.push(skill);
            }
        }

        skills
    }

    fn generate_agents(
        plan: &OrchestrationPlan,
        modules: &[DetectedModule],
        groups: &[ModuleGroup],
        cross_insights: Option<&SynthesizedInsights>,
        constraints: &ExtractedConstraints,
        domain_analysis: Option<&DomainAnalysisResult>,
        conventions: &InferredConventions,
    ) -> Vec<Agent> {
        let mut agents = Vec::new();

        for planned in &plan.orchestration_agents {
            let prompt = build_system_agent_prompt(
                &planned.name, modules, cross_insights, constraints, domain_analysis, conventions,
            );
            let model = parse_model(&planned.model);
            let permission = parse_permission(&planned.permission_mode);

            let mut agent = Agent::new(&planned.name, &planned.description, prompt)
                .with_model(model)
                .with_permission_mode(permission);

            if !planned.tools.is_empty() {
                agent = agent.with_tools(planned.tools.clone());
            }
            if !planned.skills.is_empty() {
                agent = agent.with_skills(planned.skills.clone());
            }

            agents.push(agent);
        }

        for planned in &plan.module_agents {
            if let Some(module) = modules.iter().find(|m| m.module_id == planned.module_id) {
                let prompt = build_module_agent_prompt(
                    module, cross_insights, constraints, domain_analysis,
                );
                let model = parse_model(&planned.model);
                let permission = parse_permission(&planned.permission_mode);

                let mut agent = Agent::new(&planned.name, &module.responsibility, prompt)
                    .with_model(model)
                    .with_permission_mode(permission);

                if !planned.tools.is_empty() {
                    agent = agent.with_tools(planned.tools.clone());
                }

                agent = agent.with_hooks(ToolHooks {
                    pre_tool_use: None,
                    post_tool_use: Some(vec![Hook::new(
                        "Edit|Write",
                        format!(
                            "\"$CLAUDE_PROJECT_DIR\"/.claudegen/hooks/validate-module-scope.sh {}",
                            module.module_id
                        ),
                    )]),
                });

                agents.push(agent);
            }
        }

        // Group orchestrator agents
        for planned in &plan.group_orchestrators {
            let group = groups.iter().find(|g| g.group_id == planned.group_id);
            let prompt = build_group_orchestrator_prompt(
                &planned.group_id,
                group,
                &planned.module_agent_names,
                modules,
            );

            let member_skills = planned.module_agent_names.clone();

            let agent = Agent::new(&planned.name, format!("Group orchestrator for {}", planned.group_id), prompt)
                .with_model(parse_model(&planned.model))
                .with_permission_mode(parse_permission(&planned.permission_mode))
                .with_tools(planned.tools.clone())
                .with_skills(member_skills);

            agents.push(agent);
        }

        agents
    }

    fn generate_module_rules(modules: &[DetectedModule]) -> Vec<Rule> {
        modules
            .iter()
            .filter(|m| !m.conventions.is_empty() || !m.known_issues.is_empty())
            .map(|module| {
                let mut content = Vec::new();

                if !module.conventions.is_empty() {
                    content.push(format!("## {} Conventions", module.module_id));
                    for conv in &module.conventions {
                        content.push(format!("- {}", conv));
                    }
                }

                if !module.known_issues.is_empty() {
                    content.push("\n## Known Issues".to_string());
                    for issue in &module.known_issues {
                        content.push(format!("- {}", issue));
                    }
                }

                if !module.dependencies.is_empty() {
                    content.push("\n## Dependencies".to_string());
                    for dep in &module.dependencies {
                        content.push(format!("- {}", dep));
                    }
                }

                for evidence in &module.evidence {
                    if !evidence.file.is_empty() {
                        content.push(format!(
                            "\nKey reference: @{}:{}",
                            evidence.file, evidence.start_line
                        ));
                    }
                }

                Rule::new(format!("module-{}", module.module_id), content)
                    .with_paths(module.paths.clone())
            })
            .collect()
    }
}

// --- Helper parsers ---

fn parse_model(s: &str) -> AgentModel {
    match s {
        "opus" => AgentModel::Opus,
        "haiku" => AgentModel::Haiku,
        _ => AgentModel::Sonnet,
    }
}

fn parse_permission(s: &str) -> PermissionMode {
    match s {
        "acceptEdits" => PermissionMode::AcceptEdits,
        "dontAsk" => PermissionMode::DontAsk,
        _ => PermissionMode::Default,
    }
}

const RISK_SUMMARY_THRESHOLD: f32 = 0.3;

// --- Shared formatters ---

fn format_workflow_steps(steps: &[crate::types::domain::WorkflowStep]) -> String {
    steps
        .iter()
        .map(|s| format!("  {}. {}", s.order, s.action))
        .collect::<Vec<_>>()
        .join("\n")
}

// --- Section builders ---

fn build_module_map_section(modules: &[DetectedModule]) -> String {
    let list: String = modules
        .iter()
        .map(|m| {
            format!(
                "- **{}**: {} (paths: {}, deps: {})",
                m.module_id,
                m.responsibility,
                m.paths.join(", "),
                m.dependencies.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("### Module Map\n{list}")
}

fn build_key_refs_section(modules: &[DetectedModule]) -> String {
    let refs: String = modules
        .iter()
        .flat_map(|m| m.key_files.iter().take(2))
        .map(|f| format!("- @{}", f))
        .collect::<Vec<_>>()
        .join("\n");
    if refs.is_empty() {
        String::new()
    } else {
        format!("### Key References\n{refs}")
    }
}

fn build_dependency_graph_section(modules: &[DetectedModule]) -> String {
    let edges: Vec<String> = modules
        .iter()
        .filter(|m| !m.dependencies.is_empty())
        .map(|m| {
            format!(
                "- {} → {}",
                m.module_id,
                m.dependencies.join(", ")
            )
        })
        .collect();
    if edges.is_empty() {
        return String::new();
    }
    format!("### Dependency Graph\n{}", edges.join("\n"))
}

fn build_cross_constraints_section(
    constraints: &[CrossModuleConstraint],
) -> String {
    if constraints.is_empty() {
        return String::new();
    }
    let items: Vec<String> = constraints
        .iter()
        .map(|c| {
            format!(
                "- **{}**: {} (modules: {})",
                c.name,
                c.description,
                c.affected_modules.join(", ")
            )
        })
        .collect();
    format!("### Cross-Module Constraints\n{}", items.join("\n"))
}

fn build_violations_section(
    violations: &[ArchitectureViolation],
) -> String {
    if violations.is_empty() {
        return String::new();
    }
    let items: Vec<String> = violations
        .iter()
        .map(|v| {
            format!(
                "- {} → {}: {} (fix: {})",
                v.from_layer, v.to_layer, v.description, v.suggested_fix
            )
        })
        .collect();
    format!("### Architecture Violations\n{}", items.join("\n"))
}

fn build_hidden_deps_section(
    deps: &[HiddenDependency],
) -> String {
    if deps.is_empty() {
        return String::new();
    }
    let items: Vec<String> = deps
        .iter()
        .map(|d| {
            format!(
                "- {} → {}: {} (impact: {})",
                d.from_module, d.to_module, d.description, d.impact
            )
        })
        .collect();
    format!("### Hidden Dependencies\n{}", items.join("\n"))
}

fn build_architecture_section(conventions: &InferredConventions) -> String {
    let mut lines = Vec::new();
    if !conventions.architecture.pattern_name.is_empty() {
        lines.push(format!("Pattern: {}", conventions.architecture.pattern_name));
    }
    if !conventions.architecture.layers.is_empty() {
        let layer_names: Vec<&str> = conventions
            .architecture
            .layers
            .iter()
            .map(|l| l.name.as_str())
            .collect();
        lines.push(format!("Layers: {}", layer_names.join(" → ")));
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("### Architecture\n{}", lines.join("\n"))
}

fn build_gotchas_section(gotchas: &[Gotcha]) -> String {
    if gotchas.is_empty() {
        return String::new();
    }
    let items: Vec<String> = gotchas
        .iter()
        .map(|g| format!("- **{}**: {} → {}", g.title, g.description, g.solution))
        .collect();
    format!("### Gotchas\n{}", items.join("\n"))
}

fn build_antipatterns_section(
    anti_patterns: &[AntiPattern],
) -> String {
    if anti_patterns.is_empty() {
        return String::new();
    }
    let items: Vec<String> = anti_patterns
        .iter()
        .map(|a| format!("- **{}**: {} → {}", a.name, a.why_bad, a.correct_approach))
        .collect();
    format!("### Anti-Patterns\n{}", items.join("\n"))
}

fn build_risk_summary(modules: &[DetectedModule]) -> String {
    let risky: Vec<String> = modules
        .iter()
        .filter(|m| m.risk_score > RISK_SUMMARY_THRESHOLD)
        .map(|m| format!("- **{}**: risk={:.1}, value={:.1}", m.module_id, m.risk_score, m.value_score))
        .collect();
    if risky.is_empty() {
        return String::new();
    }
    format!("### Risk Summary\n{}", risky.join("\n"))
}

fn build_security_policies_section(policies: &[DomainPolicy]) -> String {
    let security: Vec<String> = policies
        .iter()
        .filter(|p| {
            matches!(
                p.policy_type,
                PolicyType::Authorization
                    | PolicyType::Validation
                    | PolicyType::DataIntegrity
            )
        })
        .map(|p| {
            format!(
                "- **{}** ({:?}, {:?}): {}",
                p.name, p.policy_type, p.enforcement, p.description
            )
        })
        .collect();
    if security.is_empty() {
        return String::new();
    }
    format!("### Security Policies\n{}", security.join("\n"))
}

fn build_security_constraints_section(constraints: &ExtractedConstraints) -> String {
    use crate::pipeline::phases::output_router::has_security_keywords;
    let security_gotchas: Vec<String> = constraints
        .gotchas
        .iter()
        .filter(|g| has_security_keywords(&g.title))
        .map(|g| format!("- **{}**: {}", g.title, g.description))
        .collect();
    if security_gotchas.is_empty() {
        return String::new();
    }
    format!("### Security Constraints\n{}", security_gotchas.join("\n"))
}

fn build_workflows_summary(workflows: &[crate::types::domain::BusinessWorkflow]) -> String {
    if workflows.is_empty() {
        return String::new();
    }
    let items: Vec<String> = workflows
        .iter()
        .map(|w| format!("- **{}**: {}\n{}", w.name, w.description, format_workflow_steps(&w.steps)))
        .collect();
    format!("### Business Workflows\n{}", items.join("\n"))
}

fn build_policies_summary(policies: &[DomainPolicy]) -> String {
    if policies.is_empty() {
        return String::new();
    }
    let items: Vec<String> = policies
        .iter()
        .map(|p| format!("- **{}** ({:?}): {}", p.name, p.policy_type, p.description))
        .collect();
    format!("### Domain Policies\n{}", items.join("\n"))
}

// --- System agent prompt assembler ---

fn role_description(name: &str) -> &str {
    match name {
        "architect" => "Architecture reviewer ensuring module boundaries, dependency rules, and design patterns are respected.",
        "qa-reviewer" => "Quality assurance reviewer checking quality and consistency across module boundaries.",
        "security-reviewer" => "Security reviewer analyzing changes for vulnerabilities with module-scoped threat awareness.",
        _ => "Specialized agent with project module knowledge.",
    }
}

fn agent_workflow(name: &str, _modules: &[DetectedModule]) -> String {
    match name {
        "architect" => "\
            ## Workflow\n\
            1. Review the proposed changes against the module dependency graph\n\
            2. Check for architecture violations: forbidden layer crossings, circular dependencies\n\
            3. Verify hidden dependencies are not broken by the change\n\
            4. Report violations with specific fix suggestions referencing the correct module agent\n\
            5. Approve or request changes with module-scoped justification"
            .to_string(),
        "qa-reviewer" => "\
            ## Workflow\n\
            1. For each affected module, verify changes against its known gotchas and anti-patterns\n\
            2. Run static analysis using preloaded qa-static-analysis knowledge (linters, formatters via Bash)\n\
            3. Run test suites using preloaded qa-test-runner knowledge scoped to affected modules\n\
            4. Check cross-module consistency: do changes in one module break assumptions of another?\n\
            5. Report issues grouped by module with severity and remediation steps\n\
            6. Return findings to the orchestrator (claude-pilot) for routing fixes to module agents"
            .to_string(),
        "security-reviewer" => "\
            ## Workflow\n\
            1. Review changes against security policies and constraints\n\
            2. Check for OWASP Top 10 vulnerabilities in the context of this project's specific threat surface\n\
            3. Verify authorization and validation policies are maintained\n\
            4. Report findings with evidence references and specific remediation steps"
            .to_string(),
        _ => String::new(),
    }
}

fn module_agent_collaboration_protocol(module: &DetectedModule) -> String {
    format!(
        "## Collaboration Protocol\n\
        You are part of a multi-agent system. The orchestrator (claude-pilot) delegates tasks to you via Task tool.\n\n\
        ### Receiving Requests\n\
        The orchestrator sends structured queries:\n\
        ```\n\
        ACTION: <constraint-check | impact-analysis | implement | review-fix>\n\
        SCOPE: <description>\n\
        CONTEXT: <decisions from other agents>\n\
        QUESTION: <specific question or task>\n\
        ```\n\n\
        ### Response Format\n\
        Always respond with:\n\
        ```\n\
        STATUS: <ok | conflict | needs-other-module | out-of-scope>\n\
        CONSTRAINTS: <constraints this module imposes on the change>\n\
        DEPENDENCIES: <other modules that would be affected>\n\
        CONCERNS: <risks or gotchas>\n\
        PROPOSAL: <your suggested approach>\n\
        ```\n\n\
        ### Rules\n\
        - Your scope: {paths}\n\
        - If `ACTION: constraint-check`, analyze the proposed change against your module's constraints and respond with STATUS.\n\
        - If `ACTION: impact-analysis`, analyze how the change affects your module and what adaptations are needed.\n\
        - If `ACTION: implement`, execute the task within your scope. Report what was changed.\n\
        - If `ACTION: review-fix`, fix the specific QA issue within your scope.\n\
        - If the task requires changes outside your scope, set `STATUS: needs-other-module` and specify which module in DEPENDENCIES.\n\
        - If you detect a conflict with the CONTEXT from other agents, set `STATUS: conflict` and explain in CONSTRAINTS.\n\
        - Read CONTEXT carefully — it contains decisions from other module agents that you must respect or explicitly conflict with.",
        paths = module.paths.join(", ")
    )
}

fn build_system_agent_prompt(
    name: &str,
    modules: &[DetectedModule],
    cross_insights: Option<&SynthesizedInsights>,
    constraints: &ExtractedConstraints,
    domain_analysis: Option<&DomainAnalysisResult>,
    conventions: &InferredConventions,
) -> String {
    let mut sections = vec![
        format!("## Description\n{}", role_description(name)),
    ];

    // Operational workflow — what this agent does, step by step
    sections.push(agent_workflow(name, modules));

    // Internal knowledge — what this agent knows
    sections.push(format!("## Internal Knowledge\n{}", build_module_map_section(modules)));

    match name {
        "architect" => {
            sections.push(build_architecture_section(conventions));
            if let Some(insights) = cross_insights {
                sections.push(build_violations_section(&insights.architecture_violations));
                sections.push(build_hidden_deps_section(&insights.hidden_dependencies));
            }
        }
        "qa-reviewer" => {
            sections.push(build_gotchas_section(&constraints.gotchas));
            sections.push(build_antipatterns_section(&constraints.anti_patterns));
            sections.push(build_risk_summary(modules));
        }
        "security-reviewer" => {
            if let Some(domain) = domain_analysis {
                sections.push(build_security_policies_section(&domain.policies));
            }
            sections.push(build_security_constraints_section(constraints));
        }
        _ => {}
    }

    sections.push(build_key_refs_section(modules));

    sections
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

// --- Orchestration skill body assembler ---

fn build_orchestration_skill_body(
    name: &str,
    modules: &[DetectedModule],
    groups: &[ModuleGroup],
    cross_insights: Option<&SynthesizedInsights>,
    constraints: &ExtractedConstraints,
    domain_analysis: Option<&DomainAnalysisResult>,
    conventions: &InferredConventions,
) -> String {
    let module_map = build_module_map_section(modules);
    let key_refs = build_key_refs_section(modules);

    let mut sections = vec![format!("## {}", skill_title(name))];

    // Operational workflow for this skill
    sections.push(skill_workflow(name, modules, groups));

    sections.push(module_map);

    match name {
        "claude-pilot" | "consensus-planning" => {
            sections.push(build_dependency_graph_section(modules));
            if let Some(insights) = cross_insights {
                sections.push(build_cross_constraints_section(&insights.cross_constraints));
                sections.push(build_hidden_deps_section(&insights.hidden_dependencies));
            }
            if let Some(domain) = domain_analysis {
                sections.push(build_workflows_summary(&domain.workflows));
                sections.push(build_policies_summary(&domain.policies));
            }
            sections.push(build_risk_summary(modules));
        }
        "qa-review" => {
            sections.push(build_gotchas_section(&constraints.gotchas));
            sections.push(build_risk_summary(modules));
            if let Some(domain) = domain_analysis {
                sections.push(build_policies_summary(&domain.policies));
            }
        }
        "architecture-review" => {
            sections.push(build_architecture_section(conventions));
            if let Some(insights) = cross_insights {
                sections.push(build_violations_section(&insights.architecture_violations));
            }
        }
        "qa-static-analysis" | "qa-test-runner" => {}
        _ => {}
    }

    sections.push(key_refs);

    sections
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn skill_workflow(name: &str, modules: &[DetectedModule], groups: &[ModuleGroup]) -> String {
    let module_agents: String = modules
        .iter()
        .map(|m| format!("module-{}", m.module_id))
        .collect::<Vec<_>>()
        .join(", ");

    let group_agents: String = groups
        .iter()
        .map(|g| format!("group-{}", g.group_id))
        .collect::<Vec<_>>()
        .join(", ");

    let delegate_targets = if groups.is_empty() {
        format!("Available module agents: {module_agents}")
    } else {
        format!(
            "Available group orchestrators: {group_agents}\n\
            Available module agents (for ungrouped or direct access): {module_agents}\n\
            For grouped modules, delegate to the group orchestrator instead of individual module agents."
        )
    };

    match name {
        "claude-pilot" => format!(
            "### Workflow\n\
            1. Analyze the user's request to determine scope and affected modules\n\
            2. {delegate_targets}\n\
            3. For cross-module changes, invoke `consensus-planning` first\n\
            4. Decompose into module-scoped subtasks\n\
            5. Delegate each subtask to the appropriate module agent via Task tool\n\
            6. After module agents complete, invoke `qa-review` for verification\n\
            7. If QA finds issues, route fixes back to specific module agents and re-verify\n\n\
            ### Inter-Agent Communication Protocol\n\
            When delegating to a module agent via Task tool, use this structured format:\n\n\
            **Query format** (orchestrator → module agent):\n\
            ```\n\
            ACTION: <constraint-check | impact-analysis | implement | review-fix>\n\
            SCOPE: <description of the change>\n\
            CONTEXT: <relevant decisions from other agents, if any>\n\
            QUESTION: <specific question or task>\n\
            ```\n\n\
            **Expected response format** (module agent → orchestrator):\n\
            ```\n\
            STATUS: <ok | conflict | needs-other-module | out-of-scope>\n\
            CONSTRAINTS: <list of constraints this module imposes>\n\
            DEPENDENCIES: <other modules affected by this change>\n\
            CONCERNS: <risks or gotchas discovered>\n\
            PROPOSAL: <suggested approach within this module>\n\
            ```\n\n\
            ### Consensus Protocol\n\
            1. **Discovery round**: Send `ACTION: constraint-check` to each affected module agent. Collect all CONSTRAINTS and DEPENDENCIES.\n\
            2. **Conflict detection**: If any agent returns `STATUS: conflict` or constraints contradict, invoke `consensus-planning` with all collected responses.\n\
            3. **Agent recruitment**: If any agent returns `STATUS: needs-other-module` or DEPENDENCIES lists a module not yet consulted, send `ACTION: impact-analysis` to that module agent before proceeding.\n\
            4. **Resolution round**: After consensus-planning produces a resolution, send the resolution to each affected agent with `ACTION: constraint-check` to confirm acceptance.\n\
            5. **Convergence**: Consensus is reached when ALL agents return `STATUS: ok`. If any agent still returns `conflict` after 2 resolution rounds, escalate to the user with the unresolved conflict details.\n\
            6. **Execution**: Send `ACTION: implement` with the agreed plan to each module agent.\n\
            7. **Verification**: After all agents complete, invoke `qa-review`. If issues found, send `ACTION: review-fix` to the specific module agent with the QA findings.\n\n\
            ### Cross-Agent Result Sharing\n\
            When one module agent's output affects another:\n\
            - Include the relevant agent's PROPOSAL in the CONTEXT field of the next query\n\
            - Example: When module-api changes an interface, pass `CONTEXT: module-api will change OrderDTO to include cancelledAt field` to module-services"
        ),
        "consensus-planning" => format!(
            "### Workflow\n\
            1. Receive the change scope and all module agent responses from the orchestrator\n\
            2. Parse each agent's response for CONSTRAINTS, DEPENDENCIES, and CONCERNS\n\
            3. Build a constraint matrix: for each constraint, which modules are affected\n\
            4. Detect conflicts: constraints that contradict each other across modules\n\
            5. For each conflict, propose resolution options ranked by impact\n\
            6. If resolution requires a module not yet consulted, return `NEEDS_AGENT: <module-id>` so the orchestrator can recruit it\n\
            7. Send proposed resolution back to conflicting agents via Task tool with `ACTION: constraint-check`\n\
            8. Repeat steps 3-7 until all agents return `STATUS: ok` (max 3 rounds)\n\
            9. Output the agreed plan:\n\
            ```\n\
            CONSENSUS: <reached | partial | escalate>\n\
            PLAN:\n\
              - MODULE: <module-id>\n\
                TASK: <description>\n\
                CONSTRAINTS_APPLIED: <list>\n\
                DEPENDS_ON: <other module tasks that must complete first>\n\
            UNRESOLVED: <any remaining conflicts, if CONSENSUS is partial or escalate>\n\
            ```\n\n\
            ### Available module agents\n\
            {module_agents}\n\n\
            ### Conflict Resolution Strategy\n\
            When constraints conflict:\n\
            1. Check if the conflict is about ordering (can be resolved by sequencing tasks)\n\
            2. Check if the conflict is about interface (can be resolved by adapter/abstraction)\n\
            3. Check if the conflict is about data format (can be resolved by transformation)\n\
            4. If none of the above, present options to the orchestrator with trade-offs for each"
        ),
        "qa-review" => "\
            ### Workflow\n\
            1. Receive the set of changes from the orchestrator\n\
            2. For each affected module, check changes against its gotchas and anti-patterns\n\
            3. Invoke `qa-static-analysis` for automated checks\n\
            4. Invoke `qa-test-runner` scoped to affected modules\n\
            5. Verify cross-module consistency: changes in one module must not violate constraints of another\n\
            6. Report issues grouped by module with severity\n\
            7. If issues found, the orchestrator routes fixes to module agents; re-verify after fixes (multi-turn)"
            .to_string(),
        "architecture-review" => "\
            ### Workflow\n\
            1. Review proposed changes against the architecture pattern and layer rules\n\
            2. Check module boundary violations and dependency direction\n\
            3. Identify hidden dependency impacts\n\
            4. Report violations with fix suggestions referencing specific modules"
            .to_string(),
        "qa-static-analysis" => "\
            ### Workflow\n\
            1. Identify the project's linters, formatters, and static analysis tools\n\
            2. Run them via Bash scoped to affected paths\n\
            3. Parse and report findings"
            .to_string(),
        "qa-test-runner" => "\
            ### Workflow\n\
            1. Identify test framework and test locations for affected modules\n\
            2. Run tests via Bash scoped to affected module paths\n\
            3. Report pass/fail with failure details"
            .to_string(),
        _ => String::new(),
    }
}

fn skill_title(name: &str) -> &str {
    match name {
        "claude-pilot" => "Multi-Agent Orchestration",
        "consensus-planning" => "Consensus Planning",
        "qa-review" => "QA Review",
        "qa-static-analysis" => "Static Analysis",
        "qa-test-runner" => "Test Runner",
        "architecture-review" => "Architecture Review",
        _ => name,
    }
}

// --- Module skill body ---

fn build_module_skill_body(module: &DetectedModule) -> String {
    let mut body = format!(
        "## Module: {}\n\n{}\n\n### Scope\nPaths: {}\n",
        module.module_id,
        module.responsibility,
        module.paths.join(", ")
    );

    if !module.conventions.is_empty() {
        body.push_str("\n### Conventions\n");
        for conv in &module.conventions {
            body.push_str(&format!("- {conv}\n"));
        }
    }

    if !module.known_issues.is_empty() {
        body.push_str("\n### Known Issues\n");
        for issue in &module.known_issues {
            body.push_str(&format!("- {issue}\n"));
        }
    }

    if !module.key_files.is_empty() {
        body.push_str("\n### Key References\n");
        for f in &module.key_files {
            body.push_str(&format!("- @{f}\n"));
        }
    }

    body
}

// --- Module agent prompt with enriched analysis data ---

fn build_module_agent_prompt(
    module: &DetectedModule,
    cross_insights: Option<&SynthesizedInsights>,
    constraints: &ExtractedConstraints,
    domain_analysis: Option<&DomainAnalysisResult>,
) -> String {
    let mut sections = Vec::new();

    // Core module info
    sections.push(format!(
        "## Description\nSpecialist agent for the {} module. {}\n\n## Scope\nPaths: {}",
        module.module_id,
        module.responsibility,
        module.paths.join(", ")
    ));

    if !module.dependencies.is_empty() {
        sections.push(format!(
            "## Dependencies\n{}",
            module.dependencies.iter().map(|d| format!("- {d}")).collect::<Vec<_>>().join("\n")
        ));
    }

    if !module.conventions.is_empty() {
        sections.push(format!(
            "## Conventions\n{}",
            module.conventions.iter().map(|c| format!("- {c}")).collect::<Vec<_>>().join("\n")
        ));
    }

    if !module.known_issues.is_empty() {
        sections.push(format!(
            "## Known Issues\n{}",
            module.known_issues.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n")
        ));
    }

    if !module.key_files.is_empty() {
        sections.push(format!(
            "## Key References\n{}",
            module.key_files.iter().map(|f| format!("- @{f}")).collect::<Vec<_>>().join("\n")
        ));
    }

    // --- Enriched analysis data filtered to this module ---

    if let Some(insights) = cross_insights {
        // Tier3 insights related to this module
        let module_tier3: Vec<_> = insights
            .tier3_insights
            .iter()
            .filter(|i| {
                i.evidence.iter().any(|e| {
                    module.paths.iter().any(|p| e.file.starts_with(p.trim_end_matches('/')))
                })
            })
            .collect();
        if !module_tier3.is_empty() {
            let items: Vec<String> = module_tier3
                .iter()
                .map(|i| format!("- **{}**: {} → {}", i.title, i.description, i.prevention_guidance))
                .collect();
            sections.push(format!("## Critical Gotchas\n{}", items.join("\n")));
        }

        // Hidden dependencies involving this module
        let module_deps: Vec<_> = insights
            .hidden_dependencies
            .iter()
            .filter(|d| d.from_module == module.module_id || d.to_module == module.module_id)
            .collect();
        if !module_deps.is_empty() {
            let items: Vec<String> = module_deps
                .iter()
                .map(|d| format!("- {} → {}: {} (impact: {})", d.from_module, d.to_module, d.description, d.impact))
                .collect();
            sections.push(format!("## Hidden Dependencies\n{}", items.join("\n")));
        }

        // Cross-module constraints affecting this module
        let module_constraints: Vec<_> = insights
            .cross_constraints
            .iter()
            .filter(|c| c.affected_modules.contains(&module.module_id))
            .collect();
        if !module_constraints.is_empty() {
            let items: Vec<String> = module_constraints
                .iter()
                .map(|c| format!("- **{}**: {}", c.name, c.description))
                .collect();
            sections.push(format!("## Cross-Module Constraints\n{}", items.join("\n")));
        }
    }

    // Domain policies related to this module
    if let Some(domain) = domain_analysis {
        let module_policies: Vec<_> = domain
            .policies
            .iter()
            .filter(|p| {
                p.related_modules.iter().any(|rm| {
                    module.paths.iter().any(|path| rm.contains(path.trim_end_matches('/')) || path.contains(rm))
                }) || p.related_modules.contains(&module.module_id)
            })
            .collect();
        if !module_policies.is_empty() {
            let items: Vec<String> = module_policies
                .iter()
                .map(|p| format!("- **{}** ({:?}): {}", p.name, p.policy_type, p.description))
                .collect();
            sections.push(format!("## Domain Policies\n{}", items.join("\n")));
        }

        // Workflows involving this module
        let module_workflows: Vec<_> = domain
            .workflows
            .iter()
            .filter(|w| w.involved_modules.iter().any(|m| m.contains(&module.module_id)))
            .collect();
        if !module_workflows.is_empty() {
            let items: Vec<String> = module_workflows
                .iter()
                .map(|w| format!("### {}\n{}\n{}", w.name, w.description, format_workflow_steps(&w.steps)))
                .collect();
            sections.push(format!("## Business Workflows\n{}", items.join("\n\n")));
        }
    }

    // Gotchas filtered to this module's paths
    let module_gotchas: Vec<_> = constraints
        .gotchas
        .iter()
        .filter(|g| {
            g.related_files.iter().any(|f| {
                module.paths.iter().any(|p| f.starts_with(p.trim_end_matches('/')))
            })
        })
        .collect();
    if !module_gotchas.is_empty() {
        let items: Vec<String> = module_gotchas
            .iter()
            .map(|g| format!("- **{}**: {} → {}", g.title, g.description, g.solution))
            .collect();
        sections.push(format!("## Module Gotchas\n{}", items.join("\n")));
    }

    // Collaboration protocol for multi-agent system
    sections.push(module_agent_collaboration_protocol(module));

    // QA Checklist embedded in module agent
    let mut qa_items = Vec::new();
    if !module.conventions.is_empty() {
        qa_items.push("- Verify conventions are followed".to_string());
    }
    if !module.known_issues.is_empty() {
        qa_items.push("- Check against known issues".to_string());
    }
    if !module_gotchas.is_empty() {
        qa_items.push("- Verify gotcha mitigations in place".to_string());
    }
    if !module.dependencies.is_empty() {
        qa_items.push("- Ensure dependency contracts respected".to_string());
    }
    if !qa_items.is_empty() {
        sections.push(format!("## QA Checklist\n{}", qa_items.join("\n")));
    }

    sections
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn build_group_orchestrator_prompt(
    group_id: &str,
    group: Option<&ModuleGroup>,
    member_agent_names: &[String],
    modules: &[DetectedModule],
) -> String {
    let mut sections = Vec::new();

    let responsibility = group
        .map(|g| g.responsibility.as_str())
        .unwrap_or("Coordinate member modules");

    sections.push(format!(
        "## Description\nSub-orchestrator for the {} group. {}\n\n## Member Modules\n{}",
        group_id,
        responsibility,
        member_agent_names
            .iter()
            .map(|n| format!("- {}", n))
            .collect::<Vec<_>>()
            .join("\n")
    ));

    // Cross-module constraints within the group
    if let Some(g) = group {
        let member_modules: Vec<&DetectedModule> = modules
            .iter()
            .filter(|m| g.module_ids.contains(&m.module_id))
            .collect();

        if !member_modules.is_empty() {
            let mut constraints_section = String::from("## Intra-Group Constraints\n");
            for m in &member_modules {
                let intra_deps: Vec<&str> = m
                    .dependencies
                    .iter()
                    .filter(|d| g.module_ids.contains(d))
                    .map(|d| d.as_str())
                    .collect();
                if !intra_deps.is_empty() {
                    constraints_section.push_str(&format!(
                        "- {} depends on: {}\n",
                        m.module_id,
                        intra_deps.join(", ")
                    ));
                }
            }
            sections.push(constraints_section);
        }

        if !g.external_dependencies.is_empty() {
            sections.push(format!(
                "## External Dependencies\n{}",
                g.external_dependencies
                    .iter()
                    .map(|d| format!("- {}", d))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }

    sections.push(
        "## Consensus Protocol\n\
        1. Receive task from claude-pilot\n\
        2. Decompose into member module subtasks\n\
        3. For cross-module changes within this group, query each member with `ACTION: constraint-check`\n\
        4. Resolve intra-group conflicts before reporting back\n\
        5. Delegate `ACTION: implement` to member agents\n\
        6. Report consolidated result to claude-pilot\n\n\
        ## Response Format\n\
        ```\n\
        GROUP_STATUS: <ok | conflict | needs-external>\n\
        MEMBER_RESULTS:\n\
          - MODULE: <module-id>\n\
            STATUS: <ok | conflict>\n\
            CHANGES: <summary>\n\
        EXTERNAL_NEEDS: <modules outside this group that are affected>\n\
        ```"
        .to_string()
    );

    sections
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub struct OrchestrationArtifacts {
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub rules: Vec<Rule>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::output_router::*;

    fn sample_modules() -> Vec<DetectedModule> {
        vec![
            DetectedModule {
                module_id: "pipeline".to_string(),
                paths: vec!["src/pipeline/".to_string()],
                key_files: vec!["src/pipeline/adaptive.rs".to_string()],
                dependencies: vec!["types".to_string()],
                responsibility: "Generation pipeline orchestration".to_string(),
                coverage_ratio: 0.6,
                value_score: 0.9,
                risk_score: 0.4,
                conventions: vec!["Phase-based execution".to_string()],
                known_issues: vec!["Timeout handling required".to_string()],
                evidence: vec![],
            },
            DetectedModule {
                module_id: "types".to_string(),
                paths: vec!["src/types/".to_string()],
                key_files: vec!["src/types/mod.rs".to_string()],
                dependencies: vec![],
                responsibility: "Domain type definitions".to_string(),
                coverage_ratio: 0.2,
                value_score: 0.7,
                risk_score: 0.2,
                conventions: vec!["Builder pattern".to_string()],
                known_issues: vec![],
                evidence: vec![],
            },
        ]
    }

    fn sample_plan(modules: &[DetectedModule]) -> OrchestrationPlan {
        OrchestrationPlan {
            orchestration_skills: vec![
                PlannedOrchestrationSkill {
                    name: "claude-pilot".to_string(),
                    description: "Multi-agent orchestration".to_string(),
                    user_invocable: true,
                    disable_model_invocation: false,
                    context: None,
                    agent: None,
                    allowed_tools: vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string(), "Task".to_string(), "Skill".to_string()],
                },
                PlannedOrchestrationSkill {
                    name: "consensus-planning".to_string(),
                    description: "Cross-module consensus planning".to_string(),
                    user_invocable: false,
                    disable_model_invocation: false,
                    context: None,
                    agent: None,
                    allowed_tools: vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string(), "Task".to_string(), "Skill".to_string()],
                },
            ],
            module_skills: modules
                .iter()
                .map(|m| PlannedModuleSkill {
                    name: format!("module-{}", m.module_id),
                    module_id: m.module_id.clone(),
                })
                .collect(),
            orchestration_agents: vec![PlannedOrchestrationAgent {
                name: "architect".to_string(),
                description: "Architecture reviewer".to_string(),
                tools: vec!["Read".to_string()],
                model: "sonnet".to_string(),
                permission_mode: "default".to_string(),
                skills: vec![],
            }],
            module_agents: vec![
                PlannedModuleAgent {
                    name: "module-pipeline".to_string(),
                    module_id: "pipeline".to_string(),
                    tools: vec!["Read".to_string(), "Edit".to_string()],
                    model: "sonnet".to_string(),
                    permission_mode: "acceptEdits".to_string(),
                },
                PlannedModuleAgent {
                    name: "module-types".to_string(),
                    module_id: "types".to_string(),
                    tools: vec!["Read".to_string(), "Edit".to_string()],
                    model: "sonnet".to_string(),
                    permission_mode: "acceptEdits".to_string(),
                },
            ],
            group_orchestrators: vec![],
        }
    }

    #[test]
    fn test_generate_orchestration_artifacts() {
        let modules = sample_modules();
        let plan = sample_plan(&modules);
        let constraints = ExtractedConstraints::default();
        let conventions = InferredConventions::default();
        let artifacts = OrchestrationGenerator::generate(
            &plan, &modules, &[], None, &constraints, None, &conventions,
        );

        // 2 orchestration skills (claude-pilot, consensus-planning) + 2 module skills
        assert_eq!(artifacts.skills.len(), 4);
        // 1 orchestration agent (architect) + 2 module agents
        assert_eq!(artifacts.agents.len(), 3);
        // module rules for modules with conventions/issues
        assert!(!artifacts.rules.is_empty());
    }

    #[test]
    fn test_module_rules_have_paths() {
        let modules = sample_modules();
        let plan = sample_plan(&modules);
        let constraints = ExtractedConstraints::default();
        let conventions = InferredConventions::default();
        let artifacts = OrchestrationGenerator::generate(
            &plan, &modules, &[], None, &constraints, None, &conventions,
        );

        for rule in &artifacts.rules {
            assert!(rule.name.starts_with("module-"));
        }
    }
}
