//! Module Rule Generator
//!
//! Generates module-specific rules (priority 80).
//! Contains: responsibility, conventions, known issues, key abstractions.

use super::RuleGenerationContext;
use crate::types::Rule;

pub struct ModuleRuleGenerator;

impl ModuleRuleGenerator {
    pub fn generate(ctx: &RuleGenerationContext<'_>) -> Vec<Rule> {
        ctx.modules
            .iter()
            .filter_map(|module| Self::generate_for_module(ctx, module))
            .collect()
    }

    fn generate_for_module(
        ctx: &RuleGenerationContext<'_>,
        module: &crate::types::module_map::DetectedModule,
    ) -> Option<Rule> {
        let mut content = Vec::new();

        content.push(format!("# Module: {}", module.module_id));
        content.push(String::new());

        // Responsibility
        content.push("## Responsibility".into());
        content.push(String::new());
        content.push("**DO:**".into());
        content.push(format!("- {}", module.responsibility));
        content.push(String::new());

        // Dependencies
        if !module.dependencies.is_empty() || !module.dependents.is_empty() {
            content.push("## Dependencies".into());
            content.push(String::new());
            content.push("| Direction | Module | Type |".into());
            content.push("|-----------|--------|------|".into());
            for dep in &module.dependencies {
                content.push(format!("| Uses | {} | runtime |", dep));
            }
            for dep in &module.dependents {
                content.push(format!("| Used by | {} | runtime |", dep));
            }
            content.push(String::new());
        }

        // Conventions
        if !module.conventions.is_empty() {
            content.push("## Conventions".into());
            content.push(String::new());
            for conv in &module.conventions {
                content.push(format!("### {}", conv.name));
                content.push(conv.pattern.clone());
                if let Some(rationale) = &conv.rationale {
                    content.push(format!("**Rationale**: {rationale}"));
                }
                for ev in &conv.evidence {
                    content.push(format!("(@{}:{})", ev.file, ev.start_line));
                }
                content.push(String::new());
            }
        }

        // Known Issues
        if !module.known_issues.is_empty() {
            content.push("## Known Issues".into());
            content.push(String::new());
            for issue in &module.known_issues {
                content.push(format!("### [{}] {}", issue.severity, issue.id));
                content.push(issue.description.clone());
                if let Some(prevention) = &issue.prevention {
                    content.push(format!("**Prevention**: {prevention}"));
                }
                for ev in &issue.evidence {
                    content.push(format!("(@{}:{})", ev.file, ev.start_line));
                }
                content.push(String::new());
            }
        }

        // Key files
        if !module.key_files.is_empty() {
            content.push("## Key Files".into());
            content.push(String::new());
            for file in &module.key_files {
                content.push(format!("- @{file}"));
            }
            content.push(String::new());
        }

        // API exposure hints (for modules that expose APIs)
        let is_api_module = Self::is_api_module(module);
        if is_api_module {
            content.push("## API Exposure".into());
            content.push(String::new());
            content.push("This module exposes public APIs. Changes may affect:".into());
            content.push("- External consumers".into());
            content.push("- API compatibility".into());
            content.push("- Documentation".into());
            content.push(String::new());
            content.push("**Before modifying:**".into());
            content.push("1. Check for breaking changes".into());
            content.push("2. Update API documentation".into());
            content.push("3. Consider versioning if breaking".into());
            content.push(String::new());
        }

        // Get anti-patterns related to this module
        let module_anti_patterns: Vec<_> = ctx
            .constraints
            .anti_patterns
            .iter()
            .filter(|ap| {
                ap.evidence.iter().any(|ev| {
                    module
                        .paths
                        .iter()
                        .any(|p| ev.file.starts_with(p.trim_end_matches("**")))
                })
            })
            .collect();

        if !module_anti_patterns.is_empty() {
            content.push("## Anti-Patterns".into());
            content.push(String::new());
            for ap in &module_anti_patterns {
                content.push(format!("### {} (DON'T)", ap.name));
                content.push(ap.description.clone());
                content.push(format!("**Instead**: {}", ap.correct_approach));
                content.push(String::new());
            }
        }

        // Skip modules with minimal content
        if module.conventions.is_empty()
            && module.known_issues.is_empty()
            && module_anti_patterns.is_empty()
        {
            return None;
        }

        let paths: Vec<String> = module
            .paths
            .iter()
            .map(|p| {
                if p.ends_with('/') {
                    format!("{}**", p)
                } else if p.ends_with("**") {
                    p.clone()
                } else {
                    format!("{}/**", p)
                }
            })
            .collect();

        Some(Rule::module(module.module_id.clone(), paths, content))
    }

    /// Detect if a module exposes public APIs based on naming/path patterns
    fn is_api_module(module: &crate::types::module_map::DetectedModule) -> bool {
        let api_indicators = [
            "handler", "handlers", "route", "routes", "api", "controller",
            "controllers", "endpoint", "endpoints", "rest", "graphql",
        ];

        // Check module name
        let module_lower = module.module_id.to_lowercase();
        if api_indicators.iter().any(|i| module_lower.contains(i)) {
            return true;
        }

        // Check paths
        for path in &module.paths {
            let path_lower = path.to_lowercase();
            if api_indicators.iter().any(|i| path_lower.contains(i)) {
                return true;
            }
        }

        // Check key files
        for file in &module.key_files {
            let file_lower = file.to_lowercase();
            if api_indicators.iter().any(|i| file_lower.contains(i)) {
                return true;
            }
        }

        false
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
    use crate::types::module_map::{
        Convention, DetectedModule, IssueCategory, IssueSeverity, KnownIssue, TechStack,
    };

    #[test]
    fn test_module_rule_generation() {
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
            DetectedModule::new("auth", "User authentication and authorization")
                .paths(vec!["src/auth/".into()])
                .key_files(vec!["src/auth/mod.rs".into()])
                .dependencies(vec!["types".into()])
                .conventions(vec![Convention::new(
                    "secure-defaults",
                    "Always use secure defaults for auth settings",
                )])
                .known_issues(vec![KnownIssue::new(
                    "session-timeout",
                    "Sessions may not timeout properly under load",
                    IssueSeverity::Medium,
                    IssueCategory::Security,
                )
                .with_prevention("Use atomic session management")]),
        ];
        let groups = vec![];

        let ctx = RuleGenerationContext {
            detection: &detection,
            conventions: &conventions,
            constraints: &constraints,
            tech_stack: &tech_stack,
            modules: &modules,
            groups: &groups,
            project_name: "test-project",
        };

        let rules = ModuleRuleGenerator::generate(&ctx);
        assert_eq!(rules.len(), 1);

        let rule = &rules[0];
        assert_eq!(rule.name, "auth");
        assert_eq!(rule.priority, 80);
        assert!(rule.content.iter().any(|c| c.contains("secure-defaults")));
        assert!(rule.content.iter().any(|c| c.contains("session-timeout")));
    }

    #[test]
    fn test_module_without_conventions_skipped() {
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
            DetectedModule::new("empty", "Empty module").paths(vec!["src/empty/".into()])
        ];
        let groups = vec![];

        let ctx = RuleGenerationContext {
            detection: &detection,
            conventions: &conventions,
            constraints: &constraints,
            tech_stack: &tech_stack,
            modules: &modules,
            groups: &groups,
            project_name: "test-project",
        };

        let rules = ModuleRuleGenerator::generate(&ctx);
        assert!(rules.is_empty(), "Module without conventions should be skipped");
    }

    #[test]
    fn test_api_module_detection() {
        // Test module with "handler" in name
        let handler_module = DetectedModule::new("handlers", "HTTP handlers")
            .paths(vec!["src/handlers/".into()]);
        assert!(ModuleRuleGenerator::is_api_module(&handler_module));

        // Test module with "api" in path
        let api_module = DetectedModule::new("v1", "API version 1")
            .paths(vec!["src/api/v1/".into()]);
        assert!(ModuleRuleGenerator::is_api_module(&api_module));

        // Test module with "controller" in key_files
        let controller_module = DetectedModule::new("users", "User management")
            .paths(vec!["src/users/".into()])
            .key_files(vec!["src/users/user_controller.rs".into()]);
        assert!(ModuleRuleGenerator::is_api_module(&controller_module));

        // Test non-API module
        let internal_module = DetectedModule::new("utils", "Utility functions")
            .paths(vec!["src/utils/".into()]);
        assert!(!ModuleRuleGenerator::is_api_module(&internal_module));
    }

    #[test]
    fn test_api_module_includes_exposure_section() {
        let detection = ProjectDetection::default();
        let conventions = InferredConventions::default();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![
            DetectedModule::new("handlers", "HTTP request handlers")
                .paths(vec!["src/handlers/".into()])
                .conventions(vec![Convention::new(
                    "json-responses",
                    "Always return JSON responses",
                )])
        ];
        let groups = vec![];

        let ctx = RuleGenerationContext {
            detection: &detection,
            conventions: &conventions,
            constraints: &constraints,
            tech_stack: &tech_stack,
            modules: &modules,
            groups: &groups,
            project_name: "test-project",
        };

        let rules = ModuleRuleGenerator::generate(&ctx);
        assert_eq!(rules.len(), 1);

        let rule = &rules[0];
        assert!(rule.content.iter().any(|c| c.contains("API Exposure")));
        assert!(rule.content.iter().any(|c| c.contains("breaking changes")));
    }
}
