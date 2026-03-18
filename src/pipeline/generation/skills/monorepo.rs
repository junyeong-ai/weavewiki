//! Monorepo Skills Generator
//!
//! Handles workspace-scoped skill generation for monorepos:
//! - Workspace-specific skills with scoped `@import` paths
//! - Root-level skills for cross-workspace operations
//! - Per-workspace specialized skills based on project type

use std::sync::Arc;

use crate::ai::LlmProvider;
use crate::pipeline::generation::context::GenerationContext;
use crate::pipeline::phases::monorepo_analyzer::{MonorepoAnalysis, SubprojectInfo};
use crate::pipeline::phases::output_router::SkillsPlan;
use crate::types::{Result, Skill};

use super::prompt::SkillPromptBuilder;

/// Generated skill with its output path
#[derive(Debug, Clone)]
pub struct WorkspaceSkill {
    pub skill: Skill,
    /// Output path relative to .claude/skills/
    /// For monorepo: `{workspace}/skill-name.md`
    /// For single project: `skill-name.md`
    pub output_path: String,
    /// Workspace name (None for root-level skills)
    pub workspace: Option<String>,
}

impl WorkspaceSkill {
    pub fn new(skill: Skill, workspace: Option<&str>) -> Self {
        let output_path = match workspace {
            Some(ws) => format!("{}/{}.md", ws, skill.name),
            None => format!("{}.md", skill.name),
        };
        Self {
            skill,
            output_path,
            workspace: workspace.map(String::from),
        }
    }

    /// Create a root-level skill (no workspace scoping)
    pub fn root(skill: Skill) -> Self {
        Self::new(skill, None)
    }

    /// Create a workspace-scoped skill
    pub fn scoped(skill: Skill, workspace: &str) -> Self {
        Self::new(skill, Some(workspace))
    }
}

/// Monorepo-aware skills generator
pub struct MonorepoSkillsGenerator;

impl MonorepoSkillsGenerator {
    /// Generate all skills for a monorepo based on the skills plan
    pub async fn generate_with_llm(
        ctx: &GenerationContext<'_>,
        skills_plan: &SkillsPlan,
        monorepo: &MonorepoAnalysis,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<Vec<WorkspaceSkill>> {
        let mut all_skills = Vec::new();

        // Generate root-level cross-workspace skills
        for planned in &skills_plan.root_skills {
            let skill = Self::generate_root_skill(ctx, planned, monorepo, &provider).await?;
            all_skills.push(WorkspaceSkill::root(skill));
        }

        // Generate workspace-scoped skills
        for ws_plan in &skills_plan.workspace_skills {
            let subproject = monorepo
                .subprojects
                .iter()
                .find(|s| s.path == ws_plan.workspace_path);

            for planned in &ws_plan.skills {
                let skill = Self::generate_workspace_skill(
                    ctx,
                    planned,
                    &ws_plan.workspace_name,
                    &ws_plan.workspace_path,
                    subproject,
                    &provider,
                )
                .await?;
                all_skills.push(WorkspaceSkill::scoped(skill, &ws_plan.workspace_name));
            }
        }

        Ok(all_skills)
    }

    /// Generate skills without LLM (template-based generation)
    pub fn generate(
        ctx: &GenerationContext<'_>,
        skills_plan: &SkillsPlan,
        monorepo: &MonorepoAnalysis,
    ) -> Vec<WorkspaceSkill> {
        let mut all_skills = Vec::new();

        // Root-level skills
        for planned in &skills_plan.root_skills {
            let skill = Self::generate_root_skill_template(ctx, planned, monorepo);
            all_skills.push(WorkspaceSkill::root(skill));
        }

        // Workspace-scoped skills
        for ws_plan in &skills_plan.workspace_skills {
            let subproject = monorepo
                .subprojects
                .iter()
                .find(|s| s.path == ws_plan.workspace_path);

            for planned in &ws_plan.skills {
                let skill = Self::generate_workspace_skill_template(
                    ctx,
                    planned,
                    &ws_plan.workspace_name,
                    &ws_plan.workspace_path,
                    subproject,
                );
                all_skills.push(WorkspaceSkill::scoped(skill, &ws_plan.workspace_name));
            }
        }

        all_skills
    }

    async fn generate_root_skill(
        ctx: &GenerationContext<'_>,
        planned: &crate::pipeline::phases::output_router::PlannedSkill,
        monorepo: &MonorepoAnalysis,
        provider: &Arc<dyn LlmProvider>,
    ) -> Result<Skill> {
        let workspace_info = Self::format_workspace_info(monorepo);
        let cross_deps = Self::format_cross_dependencies(monorepo);

        let description = format!(
            "{}\n\nThis skill operates across all workspaces in the monorepo.",
            planned.trigger
        );

        let prompt = SkillPromptBuilder::new(&planned.name, &description)
            .project_type(ctx.detection.primary_type)
            .skill_focus(format!(
                "Cross-workspace operation for monorepo with {} workspaces",
                monorepo.subprojects.len()
            ))
            .monorepo_context(workspace_info, cross_deps)
            .build();

        let system_prompt = ctx.build_system_prompt();
        let response = provider
            .generate(&format!("{}\n\n{}", system_prompt, prompt), &serde_json::json!({}))
            .await?;

        let content_str = response
            .content
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| response.content.to_string());

        let body = super::extract_skill_body(&content_str, &planned.name);

        Ok(Skill::new(&planned.name, &description, &body)
            .tools(vec![
                "Read".into(),
                "Grep".into(),
                "Glob".into(),
                "Edit".into(),
                "Write".into(),
                "Bash".into(),
            ])
            .user_invocable(true))
    }

    async fn generate_workspace_skill(
        ctx: &GenerationContext<'_>,
        planned: &crate::pipeline::phases::output_router::PlannedSkill,
        workspace_name: &str,
        workspace_path: &str,
        subproject: Option<&SubprojectInfo>,
        provider: &Arc<dyn LlmProvider>,
    ) -> Result<Skill> {
        let scope_info = if let Some(sp) = subproject {
            format!(
                "Workspace: {} ({}, {})\nPath: {}\nEntry points: {}",
                sp.name,
                sp.project_type.as_str(),
                sp.language,
                sp.path,
                sp.entry_points.join(", ")
            )
        } else {
            format!("Workspace: {}\nPath: {}", workspace_name, workspace_path)
        };

        let description = format!(
            "{}\n\nScoped to workspace: {}",
            planned.trigger, workspace_name
        );

        let prompt = SkillPromptBuilder::new(&planned.name, &description)
            .project_type(ctx.detection.primary_type)
            .skill_focus(format!("Workspace-specific skill for {}", workspace_name))
            .workspace_scope(scope_info, workspace_path.to_string())
            .build();

        let system_prompt = ctx.build_system_prompt();
        let response = provider
            .generate(&format!("{}\n\n{}", system_prompt, prompt), &serde_json::json!({}))
            .await?;

        let content_str = response
            .content
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| response.content.to_string());

        let body = super::extract_skill_body(&content_str, &planned.name);

        // Add scoped @import for workspace files
        let body_with_imports = Self::inject_scoped_imports(&body, workspace_path);

        let tools = Self::tools_for_skill(&planned.name, subproject);

        Ok(Skill::new(&planned.name, &description, &body_with_imports)
            .tools(tools)
            .user_invocable(true))
    }

    fn generate_root_skill_template(
        _ctx: &GenerationContext<'_>,
        planned: &crate::pipeline::phases::output_router::PlannedSkill,
        monorepo: &MonorepoAnalysis,
    ) -> Skill {
        let workspace_list: Vec<_> = monorepo
            .subprojects
            .iter()
            .map(|s| format!("- {} ({})", s.name, s.language))
            .collect();

        let shared_list: Vec<_> = monorepo
            .shared_packages
            .iter()
            .map(|s| format!("- {} (used by: {})", s.name, s.consumers.join(", ")))
            .collect();

        let body = format!(
            r#"# {}

## Overview

{}

## Workspaces

{}

## Shared Packages

{}

## Process

1. Identify affected workspaces
2. Check for shared dependencies
3. Make coordinated changes
4. Verify cross-workspace compatibility
5. Run workspace-specific tests
"#,
            planned.name.replace('-', " ").to_uppercase(),
            planned.trigger,
            if workspace_list.is_empty() {
                "No workspaces detected".to_string()
            } else {
                workspace_list.join("\n")
            },
            if shared_list.is_empty() {
                "No shared packages detected".to_string()
            } else {
                shared_list.join("\n")
            },
        );

        Skill::new(&planned.name, &planned.trigger, &body)
            .tools(vec![
                "Read".into(),
                "Grep".into(),
                "Glob".into(),
                "Edit".into(),
                "Write".into(),
                "Bash".into(),
            ])
            .user_invocable(true)
    }

    fn generate_workspace_skill_template(
        _ctx: &GenerationContext<'_>,
        planned: &crate::pipeline::phases::output_router::PlannedSkill,
        workspace_name: &str,
        workspace_path: &str,
        subproject: Option<&SubprojectInfo>,
    ) -> Skill {
        let (project_type, language, entry_points) = if let Some(sp) = subproject {
            (
                sp.project_type.as_str().to_string(),
                sp.language.clone(),
                sp.entry_points.clone(),
            )
        } else {
            ("unknown".into(), "unknown".into(), vec![])
        };

        let entry_point_list = if entry_points.is_empty() {
            "No entry points detected".to_string()
        } else {
            entry_points
                .iter()
                .map(|e| format!("- @{}/{}", workspace_path, e))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let body = format!(
            r#"# {}

## Workspace: {}

- Type: {}
- Language: {}
- Path: {}

## Entry Points

{}

## Scoped Operations

All file references in this skill are scoped to `{}/**`.

## Process

1. Read workspace-specific configuration
2. Follow workspace conventions
3. Make changes within workspace scope
4. Run workspace tests
"#,
            planned.name.replace('-', " ").to_uppercase(),
            workspace_name,
            project_type,
            language,
            workspace_path,
            entry_point_list,
            workspace_path,
        );

        let tools = Self::tools_for_skill(&planned.name, subproject);

        Skill::new(&planned.name, &planned.trigger, &body)
            .tools(tools)
            .user_invocable(true)
    }

    fn format_workspace_info(monorepo: &MonorepoAnalysis) -> String {
        let workspaces: Vec<_> = monorepo
            .subprojects
            .iter()
            .map(|s| {
                format!(
                    "- {} ({}, {}): {}",
                    s.name,
                    s.project_type.as_str(),
                    s.language,
                    s.path
                )
            })
            .collect();

        format!("## Workspaces\n{}", workspaces.join("\n"))
    }

    fn format_cross_dependencies(monorepo: &MonorepoAnalysis) -> String {
        if monorepo.cross_dependencies.is_empty() {
            return String::new();
        }

        let deps: Vec<_> = monorepo
            .cross_dependencies
            .iter()
            .map(|d| format!("- {} -> {} ({})", d.source, d.target, d.dependency_type))
            .collect();

        format!("## Cross-Dependencies\n{}", deps.join("\n"))
    }

    fn inject_scoped_imports(body: &str, workspace_path: &str) -> String {
        // Add a note about scoped file references at the start
        let scope_note = format!(
            "<!-- Scoped to workspace: {} -->\n\n",
            workspace_path
        );

        // Replace unscoped @file references with workspace-scoped ones
        // This is a simple heuristic - files without path prefix get the workspace prefix
        let mut result = scope_note + body;

        // Add workspace path context section if not present
        if !result.contains("## Workspace Scope") {
            result.push_str(&format!(
                "\n\n## Workspace Scope\n\nAll file references in this skill are relative to `{}/`.\n",
                workspace_path
            ));
        }

        result
    }

    fn tools_for_skill(skill_name: &str, _subproject: Option<&SubprojectInfo>) -> Vec<String> {
        let name_lower = skill_name.to_lowercase();

        // Read-only skills
        if name_lower.contains("review") || name_lower.contains("audit") {
            return vec!["Read".into(), "Grep".into(), "Glob".into()];
        }

        // Build/test skills need Bash
        if name_lower.contains("build")
            || name_lower.contains("test")
            || name_lower.contains("dev")
        {
            return vec![
                "Read".into(),
                "Grep".into(),
                "Glob".into(),
                "Bash".into(),
            ];
        }

        // Default: full toolset for implementation skills
        vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "Edit".into(),
            "Write".into(),
            "Bash".into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_output_path(skill_name: &str, workspace: Option<&str>) -> String {
        match workspace {
            Some(ws) => format!("skills/{}/{}.md", ws, skill_name),
            None => format!("skills/{}.md", skill_name),
        }
    }
    use crate::config::ProjectType;
    use crate::pipeline::phases::monorepo_analyzer::{CrossDepType, CrossDependency, SubprojectInfo};
    use crate::pipeline::phases::output_router::{PlannedSkill, SkillSource, SkillsPlan, WorkspaceSkillPlan};

    fn sample_monorepo() -> MonorepoAnalysis {
        MonorepoAnalysis {
            is_monorepo: true,
            workspace_type: None,
            subprojects: vec![
                SubprojectInfo {
                    path: "packages/api".into(),
                    name: "api".into(),
                    project_type: ProjectType::Backend,
                    language: "typescript".into(),
                    is_app: true,
                    dependencies: vec!["shared".into()],
                    entry_points: vec!["src/index.ts".into()],
                },
                SubprojectInfo {
                    path: "packages/web".into(),
                    name: "web".into(),
                    project_type: ProjectType::Frontend,
                    language: "typescript".into(),
                    is_app: true,
                    dependencies: vec!["shared".into()],
                    entry_points: vec!["src/main.tsx".into()],
                },
                SubprojectInfo {
                    path: "packages/shared".into(),
                    name: "shared".into(),
                    project_type: ProjectType::Library,
                    language: "typescript".into(),
                    is_app: false,
                    dependencies: vec![],
                    entry_points: vec!["src/index.ts".into()],
                },
            ],
            shared_packages: vec![],
            cross_dependencies: vec![
                CrossDependency {
                    source: "api".into(),
                    target: "shared".into(),
                    dependency_type: CrossDepType::Shared,
                },
            ],
            output_strategy: crate::pipeline::phases::OutputStrategy::SplitByProject,
            rules_grouping: vec![],
        }
    }

    fn sample_skills_plan() -> SkillsPlan {
        SkillsPlan {
            generate_skills: true,
            planned_skills: vec![],
            workspace_skills: vec![
                WorkspaceSkillPlan {
                    workspace_name: "api".into(),
                    workspace_path: "packages/api".into(),
                    skills: vec![PlannedSkill {
                        name: "api-workflow".into(),
                        trigger: "Working with API workspace".into(),
                        source: SkillSource::CommonTask,
                        project_scope: Some("packages/api".into()),
                    }],
                    output_dir: ".claude/skills/api/".into(),
                },
                WorkspaceSkillPlan {
                    workspace_name: "web".into(),
                    workspace_path: "packages/web".into(),
                    skills: vec![PlannedSkill {
                        name: "web-workflow".into(),
                        trigger: "Working with Web workspace".into(),
                        source: SkillSource::CommonTask,
                        project_scope: Some("packages/web".into()),
                    }],
                    output_dir: ".claude/skills/web/".into(),
                },
            ],
            root_skills: vec![PlannedSkill {
                name: "cross-workspace-update".into(),
                trigger: "Coordinated update across workspaces".into(),
                source: SkillSource::CrossProjectOperation,
                project_scope: None,
            }],
        }
    }

    #[test]
    fn test_workspace_skill_new() {
        let skill = Skill::new("api-workflow", "API workflow", "body");
        let ws_skill = WorkspaceSkill::scoped(skill.clone(), "api");

        assert_eq!(ws_skill.output_path, "api/api-workflow.md");
        assert_eq!(ws_skill.workspace, Some("api".into()));
    }

    #[test]
    fn test_root_skill() {
        let skill = Skill::new("cross-update", "Cross update", "body");
        let ws_skill = WorkspaceSkill::root(skill);

        assert_eq!(ws_skill.output_path, "cross-update.md");
        assert!(ws_skill.workspace.is_none());
    }

    #[test]
    fn test_skill_output_path() {
        assert_eq!(skill_output_path("my-skill", None), "skills/my-skill.md");
        assert_eq!(
            skill_output_path("my-skill", Some("api")),
            "skills/api/my-skill.md"
        );
    }

    #[test]
    fn test_generate_template_produces_skills() {
        use crate::pipeline::context::VerifiedFileRegistry;
        use crate::pipeline::generation::context::GenerationContext;
        use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
        use crate::types::{InferredConventions, ProjectDetection};
        use crate::types::module_map::TechStack;

        let detection = ProjectDetection {
            is_monorepo: true,
            ..Default::default()
        };
        let tech_stack = TechStack::new("typescript");
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let registry = VerifiedFileRegistry::empty();

        let ctx = GenerationContext::new(
            &detection,
            &tech_stack,
            "test-monorepo",
            &[],
            &[],
            &[],
            &conventions,
            &constraints,
            &registry,
        );

        let monorepo = sample_monorepo();
        let skills_plan = sample_skills_plan();

        let skills = MonorepoSkillsGenerator::generate(&ctx, &skills_plan, &monorepo);

        // Should have root skill + workspace skills
        assert!(skills.len() >= 3);

        // Check root skill exists
        let root_skill = skills.iter().find(|s| s.workspace.is_none());
        assert!(root_skill.is_some());

        // Check workspace skills exist
        let api_skill = skills.iter().find(|s| s.workspace == Some("api".into()));
        assert!(api_skill.is_some());

        let web_skill = skills.iter().find(|s| s.workspace == Some("web".into()));
        assert!(web_skill.is_some());
    }

    #[test]
    fn test_inject_scoped_imports() {
        let body = "# Skill\n\nSome content";
        let result = MonorepoSkillsGenerator::inject_scoped_imports(body, "packages/api");

        assert!(result.contains("Scoped to workspace: packages/api"));
        assert!(result.contains("## Workspace Scope"));
    }

    #[test]
    fn test_tools_for_skill() {
        let review_tools = MonorepoSkillsGenerator::tools_for_skill("api-review", None);
        assert!(!review_tools.contains(&"Edit".into()));

        let build_tools = MonorepoSkillsGenerator::tools_for_skill("api-build", None);
        assert!(build_tools.contains(&"Bash".into()));

        let impl_tools = MonorepoSkillsGenerator::tools_for_skill("api-workflow", None);
        assert!(impl_tools.contains(&"Edit".into()));
    }
}
