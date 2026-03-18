//! Integration tests for orchestration and rule generation
//!
//! Validates:
//! - OrchestrationArtifacts has correct structure
//! - RulesGenerator produces rules with correct structure
//! - Rules scale with context
//! - Plugin directory structure

use claudegen::pipeline::generation::{OrchestrationArtifacts, RuleGenerationContext, RulesGenerator};
use claudegen::pipeline::phases::constraint_extraction::ExtractedConstraints;
use claudegen::pipeline::phases::convention_inference::{
    ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization,
    InferredConventions, NamingConventions, TestingConvention,
};
use claudegen::pipeline::phases::project_detection::{LanguageInfo, ProjectDetection};
use claudegen::types::module_map::{Convention, DetectedModule, ModuleGroup, TechStack};

fn default_conventions() -> InferredConventions {
    InferredConventions {
        architecture: ArchitectureConvention::default(),
        naming: NamingConventions::default(),
        file_organization: FileOrganization::default(),
        error_handling: ErrorHandlingPattern::default(),
        async_pattern: AsyncPattern::default(),
        patterns: Vec::new(),
        testing: TestingConvention::default(),
    }
}

mod orchestration_artifacts {
    use super::*;

    #[test]
    fn empty_has_no_artifacts() {
        let artifacts = OrchestrationArtifacts::empty();

        assert!(artifacts.skills.is_empty(), "empty() should have no skills");
        assert!(artifacts.agents.is_empty(), "empty() should have no agents");
        assert!(artifacts.rules.is_empty(), "empty() should have no rules");
    }
}

mod full_generation {
    use super::*;

    fn create_test_context<'a>(
        detection: &'a ProjectDetection,
        conventions: &'a InferredConventions,
        constraints: &'a ExtractedConstraints,
        tech_stack: &'a TechStack,
        modules: &'a [DetectedModule],
        groups: &'a [ModuleGroup],
    ) -> RuleGenerationContext<'a> {
        RuleGenerationContext {
            detection,
            conventions,
            constraints,
            tech_stack,
            modules,
            groups,
            project_name: "test-project",
        }
    }

    #[test]
    fn rules_generated_from_context() {
        let detection = ProjectDetection {
            languages: vec![LanguageInfo {
                language: "rust".into(),
                file_count: 50,
                percentage: 0.8,
                primary_manifest: Some("Cargo.toml".into()),
            }],
            ..Default::default()
        };
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![DetectedModule::new("auth", "Authentication module")
            .paths(vec!["src/auth/".into()])
            .conventions(vec![Convention::new(
                "secure-defaults",
                "Use secure defaults",
            )])];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = RulesGenerator::generate(&ctx);
        assert!(!rules.is_empty());
    }

    #[test]
    fn rules_scale_with_modules() {
        let detection = ProjectDetection {
            languages: vec![LanguageInfo {
                language: "rust".into(),
                file_count: 50,
                percentage: 0.8,
                primary_manifest: Some("Cargo.toml".into()),
            }],
            ..Default::default()
        };
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules_a = vec![DetectedModule::new("auth", "Authentication")
            .paths(vec!["src/auth/".into()])
            .conventions(vec![Convention::new("secure", "Be secure")])];
        let modules_b = vec![
            DetectedModule::new("auth", "Authentication")
                .paths(vec!["src/auth/".into()])
                .conventions(vec![Convention::new("secure", "Be secure")]),
            DetectedModule::new("user", "User management")
                .paths(vec!["src/user/".into()])
                .conventions(vec![Convention::new("validate", "Validate input")]),
        ];
        let groups = vec![];

        let ctx_a = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules_a,
            &groups,
        );
        let ctx_b = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules_b,
            &groups,
        );

        let rules_a = RulesGenerator::generate(&ctx_a);
        let rules_b = RulesGenerator::generate(&ctx_b);

        assert!(
            rules_b.len() >= rules_a.len(),
            "More modules should produce more or equal rules"
        );
    }

    #[test]
    fn all_generated_rules_pass_validation() {
        let detection = ProjectDetection {
            languages: vec![
                LanguageInfo {
                    language: "rust".into(),
                    file_count: 50,
                    percentage: 0.6,
                    primary_manifest: Some("Cargo.toml".into()),
                },
                LanguageInfo {
                    language: "typescript".into(),
                    file_count: 30,
                    percentage: 0.4,
                    primary_manifest: Some("package.json".into()),
                },
            ],
            ..Default::default()
        };
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![
            DetectedModule::new("auth", "Authentication")
                .paths(vec!["src/auth/".into()])
                .conventions(vec![Convention::new("secure", "Be secure")]),
            DetectedModule::new("api", "API layer")
                .paths(vec!["src/api/".into()])
                .conventions(vec![Convention::new("rest", "RESTful API")]),
        ];
        let groups = vec![ModuleGroup::new(
            "backend",
            "Backend services",
            vec!["auth".into(), "api".into()],
        )
        .with_boundary_rules(vec!["No frontend code".into()])];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = RulesGenerator::generate(&ctx);

        for rule in &rules {
            let issues = rule.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(
                errors.is_empty(),
                "Rule {} has validation errors: {:?}",
                rule.name,
                errors
            );
        }
    }
}

mod plugin_structure {
    use claudegen::types::Plugin;
    use claudegen::types::PluginManifest;
    use std::path::Path;

    #[test]
    fn plugin_dir_uses_new_structure() {
        let manifest = PluginManifest::new("test-project");
        let plugin = Plugin::new(manifest);
        let base = Path::new("/project");

        let plugin_dir = plugin.plugin_dir(base);
        assert_eq!(
            plugin_dir.to_str().unwrap(),
            "/project/.claude/plugins/test-project"
        );
    }

    #[test]
    fn rules_dir_under_plugin() {
        let manifest = PluginManifest::new("test-project");
        let plugin = Plugin::new(manifest);
        let base = Path::new("/project");

        let rules_dir = plugin.rules_dir(base);
        assert_eq!(
            rules_dir.to_str().unwrap(),
            "/project/.claude/plugins/test-project/rules"
        );
    }
}

mod artifact_counts {
    use super::*;

    #[test]
    fn rules_scale_with_modules() {
        let detection = ProjectDetection {
            languages: vec![LanguageInfo {
                language: "rust".into(),
                file_count: 50,
                percentage: 0.8,
                primary_manifest: Some("Cargo.toml".into()),
            }],
            ..Default::default()
        };
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");

        let modules_small = vec![DetectedModule::new("auth", "Auth")
            .paths(vec!["src/auth/".into()])
            .conventions(vec![Convention::new("c", "c")])];

        let modules_large = vec![
            DetectedModule::new("auth", "Auth")
                .paths(vec!["src/auth/".into()])
                .conventions(vec![Convention::new("c", "c")]),
            DetectedModule::new("user", "User")
                .paths(vec!["src/user/".into()])
                .conventions(vec![Convention::new("c", "c")]),
            DetectedModule::new("api", "API")
                .paths(vec!["src/api/".into()])
                .conventions(vec![Convention::new("c", "c")]),
        ];

        let ctx_small = RuleGenerationContext {
            detection: &detection,
            conventions: &conventions,
            constraints: &constraints,
            tech_stack: &tech_stack,
            modules: &modules_small,
            groups: &[],
            project_name: "test",
        };

        let ctx_large = RuleGenerationContext {
            detection: &detection,
            conventions: &conventions,
            constraints: &constraints,
            tech_stack: &tech_stack,
            modules: &modules_large,
            groups: &[],
            project_name: "test",
        };

        let rules_small = RulesGenerator::generate(&ctx_small);
        let rules_large = RulesGenerator::generate(&ctx_large);

        assert!(
            rules_large.len() >= rules_small.len(),
            "More modules should produce more rules"
        );
    }
}
