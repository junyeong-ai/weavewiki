//! Nested CLAUDE.md Generator for Monorepos
//!
//! Generates per-package CLAUDE.md files that:
//! - Reference the parent CLAUDE.md via @import
//! - Include package-specific rules and conventions
//! - Use Claude Code's subdirectory lazy-loading

use crate::pipeline::phases::monorepo_analyzer::{MonorepoAnalysis, SharedPackage, SubprojectInfo};
use crate::types::module_map::DetectedModule;
use crate::types::{ClaudeMdContent, Rule};

/// A per-package CLAUDE.md for a monorepo workspace
#[derive(Debug, Clone)]
pub struct NestedClaudeMd {
    /// Name of the workspace/package
    pub workspace_name: String,
    /// Relative path from project root (e.g. "packages/api")
    pub workspace_path: String,
    /// Generated markdown content
    pub content: String,
    /// @import references to include
    pub imports: Vec<String>,
}

pub struct NestedClaudeMdGenerator;

impl NestedClaudeMdGenerator {
    /// Generate per-package CLAUDE.md files for each subproject in a monorepo.
    ///
    /// Returns an empty Vec if the project is not a monorepo or has no subprojects.
    pub fn generate(
        monorepo: &MonorepoAnalysis,
        rules: &[Rule],
        parent_memory: &ClaudeMdContent,
    ) -> Vec<NestedClaudeMd> {
        if !monorepo.is_monorepo || monorepo.subprojects.is_empty() {
            return Vec::new();
        }

        monorepo
            .subprojects
            .iter()
            .map(|subproject| {
                Self::generate_for_subproject(
                    subproject,
                    rules,
                    parent_memory,
                    &monorepo.shared_packages,
                )
            })
            .collect()
    }

    fn generate_for_subproject(
        subproject: &SubprojectInfo,
        rules: &[Rule],
        _parent_memory: &ClaudeMdContent,
        shared_packages: &[SharedPackage],
    ) -> NestedClaudeMd {
        let parent_import = Self::compute_parent_import(&subproject.path);
        let matching_rules = Self::filter_rules_for_workspace(rules, &subproject.path);
        let consumed_shared = Self::find_consumed_shared_packages(subproject, shared_packages);

        let mut imports = vec![parent_import.clone()];
        let rule_imports: Vec<String> = matching_rules
            .iter()
            .map(|r| {
                let depth = subproject.path.split('/').count();
                let prefix = "../".repeat(depth);
                format!("{prefix}.claude/rules/{}", r.output_path())
            })
            .collect();
        imports.extend(rule_imports.clone());

        let content = Self::format_nested_md_content(
            subproject,
            &parent_import,
            &rule_imports,
            &consumed_shared,
        );

        NestedClaudeMd {
            workspace_name: subproject.name.clone(),
            workspace_path: subproject.path.clone(),
            content,
            imports,
        }
    }

    /// Compute the relative @import path from a workspace to the root CLAUDE.md.
    ///
    /// For "packages/api", returns "../../CLAUDE.md" (2 levels up).
    fn compute_parent_import(workspace_path: &str) -> String {
        let depth = workspace_path
            .trim_end_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .count();
        let prefix = "../".repeat(depth);
        format!("{prefix}CLAUDE.md")
    }

    /// Filter rules to those whose path patterns overlap with the workspace path.
    ///
    /// A rule matches if any of its `paths` patterns share a common prefix with
    /// the workspace. The glob portion (after the first `*`) is stripped before
    /// comparison.
    fn filter_rules_for_workspace<'a>(rules: &'a [Rule], workspace_path: &str) -> Vec<&'a Rule> {
        let normalized = workspace_path.trim_end_matches('/');
        rules
            .iter()
            .filter(|rule| {
                if let Some(paths) = &rule.paths {
                    paths.iter().any(|pattern| {
                        let prefix = pattern_prefix(pattern);
                        let prefix = prefix.trim_end_matches('/');
                        // Rule path is inside the workspace, or workspace is inside the rule path
                        prefix.starts_with(normalized)
                            || normalized.starts_with(prefix)
                                && !prefix.is_empty()
                                && prefix != "**"
                                && prefix != "*"
                    })
                } else {
                    false
                }
            })
            // Exclude catch-all rules (paths: ["**/*"]) - those belong to the parent
            .filter(|rule| {
                if let Some(paths) = &rule.paths {
                    !paths.iter().all(|p| p == "**/*" || p == "**" || p == "*")
                } else {
                    true
                }
            })
            .collect()
    }

    /// Find shared packages consumed by this subproject.
    fn find_consumed_shared_packages<'a>(
        subproject: &SubprojectInfo,
        shared_packages: &'a [SharedPackage],
    ) -> Vec<&'a SharedPackage> {
        shared_packages
            .iter()
            .filter(|sp| sp.consumers.contains(&subproject.name))
            .collect()
    }

    /// Format the nested CLAUDE.md content.
    fn format_nested_md_content(
        subproject: &SubprojectInfo,
        parent_import: &str,
        rule_imports: &[String],
        consumed_shared: &[&SharedPackage],
    ) -> String {
        let mut sections = Vec::new();

        // Parent import directive
        sections.push(format!("@import {parent_import}"));

        // Header
        sections.push(format!("# {}", subproject.name));

        // Overview
        let entry_points_str = if subproject.entry_points.is_empty() {
            String::new()
        } else {
            format!(
                "\n\nEntry points: {}",
                subproject
                    .entry_points
                    .iter()
                    .map(|e| format!("`{e}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        sections.push(format!(
            "## Overview\n\n{} ({}) workspace at `{}/`{entry_points_str}",
            subproject.project_type.as_str(),
            subproject.language,
            subproject.path,
        ));

        // Dependencies
        if !subproject.dependencies.is_empty() {
            let deps = subproject
                .dependencies
                .iter()
                .map(|d| format!("- {d}"))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## Dependencies\n\n{deps}"));
        }

        // Shared packages
        if !consumed_shared.is_empty() {
            let shared = consumed_shared
                .iter()
                .map(|sp| format!("- `{}` (`{}`)", sp.name, sp.path))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## Shared Packages\n\n{shared}"));
        }

        // Rules
        if !rule_imports.is_empty() {
            let imports = rule_imports
                .iter()
                .map(|i| format!("@import {i}"))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## Rules\n\n{imports}"));
        }

        sections.join("\n\n")
    }

    /// Format a `NestedClaudeMd` into its final markdown string.
    pub fn format_nested_md(workspace: &NestedClaudeMd) -> String {
        workspace.content.clone()
    }

    /// Minimum number of files for a module to warrant its own CLAUDE.md.
    const MODULE_FILE_THRESHOLD: usize = 10;

    /// Generate per-module CLAUDE.md files for deep module hierarchies.
    ///
    /// Modules with 10+ files and unique conventions get their own CLAUDE.md
    /// with parent import, module-specific conventions, and related rule references.
    ///
    /// Returns an empty Vec if no modules qualify.
    pub fn generate_for_modules(
        modules: &[DetectedModule],
        rules: &[Rule],
    ) -> Vec<NestedClaudeMd> {
        modules
            .iter()
            .filter(|m| Self::should_generate_module_md(m))
            .map(|m| Self::generate_module_md(m, rules))
            .collect()
    }

    fn should_generate_module_md(module: &DetectedModule) -> bool {
        let file_count = module.key_files.len() + module.paths.len();
        file_count >= Self::MODULE_FILE_THRESHOLD
            && !module.conventions.is_empty()
    }

    fn generate_module_md(module: &DetectedModule, rules: &[Rule]) -> NestedClaudeMd {
        let module_path = module.paths.first().map(|p| p.as_str()).unwrap_or(&module.module_id);
        let parent_import = Self::compute_parent_import(module_path);
        let matching_rules = Self::filter_rules_for_workspace(rules, module_path);

        let mut imports = vec![parent_import.clone()];
        let rule_imports: Vec<String> = matching_rules
            .iter()
            .map(|r| {
                let depth = module_path.split('/').count();
                let prefix = "../".repeat(depth);
                format!("{prefix}.claude/rules/{}", r.output_path())
            })
            .collect();
        imports.extend(rule_imports.clone());

        let content = Self::format_module_md_content(
            module,
            &parent_import,
            &rule_imports,
        );

        NestedClaudeMd {
            workspace_name: module.module_id.clone(),
            workspace_path: module_path.to_string(),
            content,
            imports,
        }
    }

    fn format_module_md_content(
        module: &DetectedModule,
        parent_import: &str,
        rule_imports: &[String],
    ) -> String {
        let mut sections = Vec::new();

        // Parent import directive
        sections.push(format!("@import {parent_import}"));

        // Header
        sections.push(format!("# {} Module", module.module_id));

        // Overview
        let paths = module.paths.join(", ");
        sections.push(format!(
            "## Overview\n\n{}\n\nPaths: {}",
            module.responsibility, paths,
        ));

        // Module conventions
        if !module.conventions.is_empty() {
            let convs: Vec<String> = module.conventions.iter().map(|c| {
                let evidence_str = c.evidence.iter()
                    .take(2)
                    .map(|e| {
                        if e.is_file_level() {
                            format!("@{}", e.file)
                        } else {
                            format!("@{}:{}", e.file, e.start_line)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let rationale = c.rationale.as_deref().unwrap_or(&c.pattern);
                if evidence_str.is_empty() {
                    format!("- **{}**: {}", c.name, rationale)
                } else {
                    format!("- **{}**: {} ({})", c.name, rationale, evidence_str)
                }
            }).collect();
            sections.push(format!("## Conventions\n\n{}", convs.join("\n")));
        }

        // Known issues
        if !module.known_issues.is_empty() {
            let issues: Vec<String> = module.known_issues.iter().map(|i| {
                format!("- **[{}]** {}: {}", i.severity, i.id, i.description)
            }).collect();
            sections.push(format!("## Known Issues\n\n{}", issues.join("\n")));
        }

        // Related rules
        if !rule_imports.is_empty() {
            let imports = rule_imports
                .iter()
                .map(|i| format!("@import {i}"))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## Related Rules\n\n{imports}"));
        }

        sections.join("\n\n")
    }
}

/// Extract the directory prefix from a glob pattern (everything before the first `*`).
fn pattern_prefix(pattern: &str) -> &str {
    match pattern.find('*') {
        Some(idx) => {
            let prefix = &pattern[..idx];
            prefix.trim_end_matches('/')
        }
        None => pattern,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectType;
    use crate::pipeline::phases::monorepo_analyzer::{
        MonorepoAnalysis, SharedPackage, SubprojectInfo,
    };
    use crate::pipeline::phases::OutputStrategy;

    fn make_monorepo(subprojects: Vec<SubprojectInfo>) -> MonorepoAnalysis {
        MonorepoAnalysis {
            is_monorepo: true,
            workspace_type: None,
            subprojects,
            shared_packages: Vec::new(),
            cross_dependencies: Vec::new(),
            output_strategy: OutputStrategy::SplitByProject,
            rules_grouping: Vec::new(),
        }
    }

    fn make_subproject(name: &str, path: &str, lang: &str) -> SubprojectInfo {
        SubprojectInfo {
            path: path.to_string(),
            name: name.to_string(),
            project_type: ProjectType::Library,
            language: lang.to_string(),
            is_app: false,
            dependencies: Vec::new(),
            entry_points: vec!["src/index.ts".to_string()],
        }
    }

    fn make_parent_memory() -> ClaudeMdContent {
        ClaudeMdContent::new("Test monorepo project")
    }

    #[test]
    fn test_generates_nested_for_each_subproject() {
        let monorepo = make_monorepo(vec![
            make_subproject("api", "packages/api", "typescript"),
            make_subproject("web", "packages/web", "typescript"),
            make_subproject("shared", "packages/shared", "typescript"),
        ]);

        let result = NestedClaudeMdGenerator::generate(&monorepo, &[], &make_parent_memory());

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].workspace_name, "api");
        assert_eq!(result[0].workspace_path, "packages/api");
        assert_eq!(result[1].workspace_name, "web");
        assert_eq!(result[2].workspace_name, "shared");
    }

    #[test]
    fn test_parent_import_included() {
        let monorepo = make_monorepo(vec![
            make_subproject("api", "packages/api", "typescript"),
        ]);

        let result = NestedClaudeMdGenerator::generate(&monorepo, &[], &make_parent_memory());

        assert_eq!(result.len(), 1);
        assert!(result[0].content.contains("@import ../../CLAUDE.md"));
        assert!(result[0].imports.contains(&"../../CLAUDE.md".to_string()));
    }

    #[test]
    fn test_parent_import_depth() {
        // Single level deep
        let monorepo = make_monorepo(vec![
            make_subproject("api", "api", "typescript"),
        ]);
        let result = NestedClaudeMdGenerator::generate(&monorepo, &[], &make_parent_memory());
        assert!(result[0].content.contains("@import ../CLAUDE.md"));

        // Three levels deep
        let monorepo = make_monorepo(vec![
            make_subproject("core", "packages/libs/core", "typescript"),
        ]);
        let result = NestedClaudeMdGenerator::generate(&monorepo, &[], &make_parent_memory());
        assert!(result[0].content.contains("@import ../../../CLAUDE.md"));
    }

    #[test]
    fn test_rules_filtered_by_workspace() {
        let monorepo = make_monorepo(vec![
            make_subproject("api", "packages/api", "typescript"),
            make_subproject("web", "packages/web", "typescript"),
        ]);

        let rules = vec![
            Rule::tech(
                "api-typescript",
                vec!["packages/api/**/*.ts".to_string()],
                vec!["# API TypeScript Rules".to_string()],
            ),
            Rule::tech(
                "web-react",
                vec!["packages/web/**/*.tsx".to_string()],
                vec!["# Web React Rules".to_string()],
            ),
            Rule::project(
                "project-wide",
                vec!["# Project Wide Rules".to_string()],
            ),
        ];

        let result = NestedClaudeMdGenerator::generate(&monorepo, &rules, &make_parent_memory());

        // API should get api-typescript rule but not web-react
        let api = &result[0];
        assert!(api.content.contains("api-typescript"));
        assert!(!api.content.contains("web-react"));

        // Web should get web-react rule but not api-typescript
        let web = &result[1];
        assert!(web.content.contains("web-react"));
        assert!(!web.content.contains("api-typescript"));

        // Neither should get the project-wide rule (it's catch-all)
        assert!(!api.content.contains("project-wide"));
        assert!(!web.content.contains("project-wide"));
    }

    #[test]
    fn test_no_nested_for_single_project() {
        let monorepo = MonorepoAnalysis::default(); // is_monorepo: false

        let result = NestedClaudeMdGenerator::generate(&monorepo, &[], &make_parent_memory());

        assert!(result.is_empty());
    }

    #[test]
    fn test_no_nested_for_empty_subprojects() {
        let monorepo = MonorepoAnalysis {
            is_monorepo: true,
            subprojects: Vec::new(),
            ..MonorepoAnalysis::default()
        };

        let result = NestedClaudeMdGenerator::generate(&monorepo, &[], &make_parent_memory());

        assert!(result.is_empty());
    }

    #[test]
    fn test_format_nested_md() {
        let nested = NestedClaudeMd {
            workspace_name: "api".to_string(),
            workspace_path: "packages/api".to_string(),
            content: "@import ../../CLAUDE.md\n\n# api".to_string(),
            imports: vec!["../../CLAUDE.md".to_string()],
        };

        let formatted = NestedClaudeMdGenerator::format_nested_md(&nested);
        assert_eq!(formatted, nested.content);
    }

    #[test]
    fn test_shared_packages_included() {
        let mut monorepo = make_monorepo(vec![
            make_subproject("api", "packages/api", "typescript"),
            make_subproject("utils", "packages/utils", "typescript"),
        ]);
        monorepo.shared_packages = vec![SharedPackage {
            path: "packages/utils".to_string(),
            name: "utils".to_string(),
            consumers: vec!["api".to_string()],
            is_internal: true,
        }];

        let result = NestedClaudeMdGenerator::generate(&monorepo, &[], &make_parent_memory());

        let api = &result[0];
        assert!(api.content.contains("## Shared Packages"));
        assert!(api.content.contains("`utils`"));

        // utils itself doesn't consume itself
        let utils = &result[1];
        assert!(!utils.content.contains("## Shared Packages"));
    }

    #[test]
    fn test_dependencies_section() {
        let mut subproject = make_subproject("api", "packages/api", "typescript");
        subproject.dependencies = vec!["express".to_string(), "prisma".to_string()];

        let monorepo = make_monorepo(vec![subproject]);
        let result = NestedClaudeMdGenerator::generate(&monorepo, &[], &make_parent_memory());

        let api = &result[0];
        assert!(api.content.contains("## Dependencies"));
        assert!(api.content.contains("- express"));
        assert!(api.content.contains("- prisma"));
    }

    #[test]
    fn test_pattern_prefix_extraction() {
        assert_eq!(pattern_prefix("packages/api/**/*.ts"), "packages/api");
        assert_eq!(pattern_prefix("src/*.rs"), "src");
        assert_eq!(pattern_prefix("**/*"), "");
        assert_eq!(pattern_prefix("src/main.rs"), "src/main.rs");
        assert_eq!(pattern_prefix("*"), "");
    }

    #[test]
    fn test_overview_includes_project_info() {
        let monorepo = make_monorepo(vec![
            make_subproject("api", "packages/api", "typescript"),
        ]);

        let result = NestedClaudeMdGenerator::generate(&monorepo, &[], &make_parent_memory());

        let api = &result[0];
        assert!(api.content.contains("## Overview"));
        assert!(api.content.contains("library"));
        assert!(api.content.contains("typescript"));
        assert!(api.content.contains("`packages/api/`"));
        assert!(api.content.contains("`src/index.ts`"));
    }

    // =========================================================================
    // Per-module CLAUDE.md tests
    // =========================================================================

    use modmap::{Convention, EvidenceLocation, IssueCategory, IssueSeverity, KnownIssue};

    fn make_deep_module(id: &str, file_count: usize, with_conventions: bool) -> DetectedModule {
        let key_files: Vec<String> = (0..file_count)
            .map(|i| format!("src/{}/file_{}.rs", id, i))
            .collect();
        let mut module = DetectedModule {
            module_id: id.into(),
            paths: vec![format!("src/{}", id)],
            key_files,
            dependencies: vec![],
            dependents: vec![],
            responsibility: format!("{} module logic", id),
            conventions: vec![],
            known_issues: vec![],
            value_score: 0.9,
            risk_score: 0.0,
            coverage_ratio: 0.1,
            evidence: vec![],
            primary_language: None,
        };
        if with_conventions {
            module.conventions = vec![
                Convention::new("naming", "snake_case everywhere")
                    .with_evidence(vec![EvidenceLocation::new(
                        format!("src/{}/mod.rs", id),
                        10,
                    )]),
                Convention::new("error-handling", "Result<T, Error> pattern"),
            ];
        }
        module
    }

    #[test]
    fn test_module_md_not_generated_for_small_modules() {
        let modules = vec![make_deep_module("small", 3, true)];
        let result = NestedClaudeMdGenerator::generate_for_modules(&modules, &[]);
        assert!(result.is_empty(), "Small modules should not get their own CLAUDE.md");
    }

    #[test]
    fn test_module_md_not_generated_without_conventions() {
        let modules = vec![make_deep_module("big", 15, false)];
        let result = NestedClaudeMdGenerator::generate_for_modules(&modules, &[]);
        assert!(result.is_empty(), "Modules without conventions should not get CLAUDE.md");
    }

    #[test]
    fn test_module_md_generated_for_deep_modules() {
        let modules = vec![make_deep_module("pipeline", 12, true)];
        let result = NestedClaudeMdGenerator::generate_for_modules(&modules, &[]);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].workspace_name, "pipeline");
        assert_eq!(result[0].workspace_path, "src/pipeline");
    }

    #[test]
    fn test_module_md_has_parent_import() {
        let modules = vec![make_deep_module("pipeline", 12, true)];
        let result = NestedClaudeMdGenerator::generate_for_modules(&modules, &[]);

        // src/pipeline is 2 levels deep
        assert!(result[0].content.contains("@import ../../CLAUDE.md"));
    }

    #[test]
    fn test_module_md_has_conventions() {
        let modules = vec![make_deep_module("pipeline", 12, true)];
        let result = NestedClaudeMdGenerator::generate_for_modules(&modules, &[]);

        assert!(result[0].content.contains("## Conventions"));
        assert!(result[0].content.contains("**naming**"));
        assert!(result[0].content.contains("snake_case everywhere"));
        assert!(result[0].content.contains("**error-handling**"));
    }

    #[test]
    fn test_module_md_has_rule_cross_references() {
        let modules = vec![make_deep_module("pipeline", 12, true)];
        let rules = vec![Rule::module(
            "pipeline",
            vec!["src/pipeline/**/*.rs".to_string()],
            vec!["# Pipeline rules".to_string()],
        )];

        let result = NestedClaudeMdGenerator::generate_for_modules(&modules, &rules);

        assert!(result[0].content.contains("## Related Rules"));
        assert!(result[0].content.contains("pipeline"));
    }

    #[test]
    fn test_module_md_has_known_issues() {
        let mut module = make_deep_module("auth", 12, true);
        module.known_issues = vec![KnownIssue::new(
            "race-condition",
            "Token refresh race",
            IssueSeverity::High,
            IssueCategory::Concurrency,
        )];

        let result = NestedClaudeMdGenerator::generate_for_modules(&[module], &[]);

        assert!(result[0].content.contains("## Known Issues"));
        assert!(result[0].content.contains("race-condition"));
        assert!(result[0].content.contains("Token refresh race"));
    }
}
