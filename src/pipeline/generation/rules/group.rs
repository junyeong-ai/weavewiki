//! Group Rule Generator
//!
//! Generates cross-module group rules (priority 70).
//! Contains: member modules, cross-module constraints, boundary rules.

use super::RuleGenerationContext;
use crate::types::module_map::ModuleGroup;
use crate::types::Rule;

pub struct GroupRuleGenerator;

impl GroupRuleGenerator {
    pub fn generate(ctx: &RuleGenerationContext<'_>) -> Vec<Rule> {
        ctx.groups
            .iter()
            .filter_map(|group| Self::generate_for_group(ctx, group))
            .collect()
    }

    fn generate_for_group(ctx: &RuleGenerationContext<'_>, group: &ModuleGroup) -> Option<Rule> {
        let mut content = Vec::new();

        content.push(format!("# Group: {}", group.name));
        content.push(String::new());

        // Responsibility
        if !group.responsibility.is_empty() {
            content.push(group.responsibility.clone());
            content.push(String::new());
        }

        // Member modules
        content.push("## Member Modules".into());
        content.push(String::new());
        content.push("| Module | Responsibility |".into());
        content.push("|--------|---------------|".into());

        for module_id in &group.module_ids {
            let responsibility = ctx
                .modules
                .iter()
                .find(|m| &m.module_id == module_id)
                .map(|m| m.responsibility.as_str())
                .unwrap_or("-");
            content.push(format!("| {} | {} |", module_id, responsibility));
        }
        content.push(String::new());

        // Cross-module constraints
        let member_set: std::collections::HashSet<&str> =
            group.module_ids.iter().map(|s| s.as_str()).collect();

        let cross_constraints: Vec<_> = ctx
            .constraints
            .hidden_dependencies
            .iter()
            .filter(|dep| {
                let source_module = Self::extract_module_from_path(&dep.source, ctx);
                let target_module = Self::extract_module_from_path(&dep.target, ctx);
                source_module
                    .map(|s| member_set.contains(s))
                    .unwrap_or(false)
                    && target_module
                        .map(|t| member_set.contains(t))
                        .unwrap_or(false)
            })
            .collect();

        if !cross_constraints.is_empty() {
            content.push("## Cross-Module Constraints".into());
            content.push(String::new());
            for dep in cross_constraints {
                content.push(format!("### {} → {}", dep.source, dep.target));
                content.push(dep.description.clone());
                content.push(format!("**Impact**: {}", dep.impact));
                content.push(String::new());
            }
        }

        // Boundary rules
        if !group.boundary_rules.is_empty() {
            content.push("## Boundary Rules".into());
            content.push(String::new());
            for rule in &group.boundary_rules {
                content.push(format!("- {rule}"));
            }
            content.push(String::new());
        }

        // Collect all paths from member modules
        let paths: Vec<String> = group
            .module_ids
            .iter()
            .flat_map(|id| {
                ctx.modules
                    .iter()
                    .find(|m| &m.module_id == id)
                    .map(|m| m.paths.clone())
                    .unwrap_or_default()
            })
            .map(|p| {
                if p.ends_with('/') {
                    format!("{}**", p)
                } else if p.ends_with("**") {
                    p
                } else {
                    format!("{}/**", p)
                }
            })
            .collect();

        if paths.is_empty() {
            return None;
        }

        Some(Rule::group(group.id.clone(), paths, content))
    }

    fn extract_module_from_path<'a>(
        path: &str,
        ctx: &'a RuleGenerationContext<'_>,
    ) -> Option<&'a str> {
        ctx.modules.iter().find_map(|m| {
            if m.paths
                .iter()
                .any(|p| path.starts_with(p.trim_end_matches("**").trim_end_matches('/')))
            {
                Some(m.module_id.as_str())
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization,
        InferredConventions, NamingConventions, TestingConvention,
    };
    use crate::pipeline::phases::project_detection::ProjectDetection;
    use crate::types::module_map::{DetectedModule, TechStack};

    #[test]
    fn test_group_rule_generation() {
        let detection = ProjectDetection::default();
        let conventions = InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        };
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![
            DetectedModule::new("auth", "Authentication").with_paths(vec!["src/auth/".into()]),
            DetectedModule::new("users", "User management").with_paths(vec!["src/users/".into()]),
        ];
        let groups = vec![ModuleGroup::new(
            "identity",
            "Identity Management",
            vec!["auth".into(), "users".into()],
        )
        .with_responsibility("Handles all identity-related operations")
        .with_boundary_rules(vec!["External systems must go through API gateway".into()])];

        let ctx = RuleGenerationContext {
            detection: &detection,
            conventions: &conventions,
            constraints: &constraints,
            tech_stack: &tech_stack,
            modules: &modules,
            groups: &groups,
            project_name: "test-project",
        };

        let rules = GroupRuleGenerator::generate(&ctx);
        assert_eq!(rules.len(), 1);

        let rule = &rules[0];
        assert_eq!(rule.name, "identity");
        assert_eq!(rule.priority, 70);
        assert!(rule.content.iter().any(|c| c.contains("auth")));
        assert!(rule.content.iter().any(|c| c.contains("users")));
        assert!(rule.content.iter().any(|c| c.contains("Boundary Rules")));
    }
}
