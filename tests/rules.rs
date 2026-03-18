//! Integration tests for hierarchical rule generation
//!
//! Validates:
//! - RulesGenerator produces rules with correct priorities
//! - Rule categories are correctly assigned
//! - Rules are sorted by priority

use claudegen::pipeline::generation::rules::{
    DomainRuleGenerator, GroupRuleGenerator, ModuleRuleGenerator, ProjectRuleGenerator,
    RuleGenerationContext, RulesGenerator, TechRuleGenerator,
};
use claudegen::pipeline::phases::constraint_extraction::ExtractedConstraints;
use claudegen::pipeline::phases::convention_inference::{
    ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization, InferredConventions,
    NamingConventions, TestingConvention,
};
use claudegen::pipeline::phases::project_detection::{LanguageInfo, ProjectDetection};
use claudegen::types::module_map::{Convention, DetectedModule, ModuleGroup, TechStack};
use claudegen::types::rule::RuleCategory;

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

mod project_rule {
    use super::*;

    #[test]
    fn generates_project_rule() {
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
        let modules = vec![];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rule = ProjectRuleGenerator::generate(&ctx);
        assert!(rule.is_some());

        let rule = rule.unwrap();
        assert_eq!(rule.category, RuleCategory::Project);
        assert_eq!(rule.priority, 100);
        assert!(rule.always_inject);
    }

    #[test]
    fn project_rule_matches_all_paths() {
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
        let modules = vec![];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rule = ProjectRuleGenerator::generate(&ctx).unwrap();
        let paths = rule.paths.unwrap();
        assert!(paths.contains(&"**/*".to_string()));
    }
}

mod tech_rules {
    use super::*;

    #[test]
    fn generates_rust_tech_rule() {
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
        let modules = vec![];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = TechRuleGenerator::generate(&ctx);
        assert!(!rules.is_empty());

        let rust_rule = rules.iter().find(|r| r.name == "rust");
        assert!(rust_rule.is_some());

        let rust_rule = rust_rule.unwrap();
        assert_eq!(rust_rule.category, RuleCategory::Tech);
        assert_eq!(rust_rule.priority, 90);
    }

    #[test]
    fn tech_rule_has_correct_paths() {
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
        let modules = vec![];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = TechRuleGenerator::generate(&ctx);
        let rust_rule = rules.iter().find(|r| r.name == "rust").unwrap();
        let paths = rust_rule.paths.as_ref().unwrap();
        assert!(paths.iter().any(|p| p.contains(".rs")));
    }

    #[test]
    fn generates_multiple_tech_rules() {
        let detection = ProjectDetection {
            languages: vec![
                LanguageInfo {
                    language: "rust".into(),
                    file_count: 50,
                    percentage: 0.5,
                    primary_manifest: Some("Cargo.toml".into()),
                },
                LanguageInfo {
                    language: "typescript".into(),
                    file_count: 30,
                    percentage: 0.3,
                    primary_manifest: Some("package.json".into()),
                },
            ],
            ..Default::default()
        };
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = TechRuleGenerator::generate(&ctx);
        assert!(rules.len() >= 2);
    }
}

mod module_rules {
    use super::*;

    #[test]
    fn generates_module_rules() {
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
        let modules = vec![
            DetectedModule::new("auth", "Authentication module")
                .paths(vec!["src/auth/".into()])
                .conventions(vec![Convention::new("secure-defaults", "Use secure defaults")]),
        ];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = ModuleRuleGenerator::generate(&ctx);
        assert!(!rules.is_empty());

        let auth_rule = rules.iter().find(|r| r.name == "auth");
        assert!(auth_rule.is_some());

        let auth_rule = auth_rule.unwrap();
        assert_eq!(auth_rule.category, RuleCategory::Module);
        assert_eq!(auth_rule.priority, 80);
    }

    #[test]
    fn module_rule_has_correct_paths() {
        let detection = ProjectDetection::default();
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![DetectedModule::new("auth", "Authentication module")
            .paths(vec!["src/auth/".into()])
            .conventions(vec![Convention::new("secure-defaults", "Use secure defaults")])];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = ModuleRuleGenerator::generate(&ctx);
        let auth_rule = rules.iter().find(|r| r.name == "auth").unwrap();
        let paths = auth_rule.paths.as_ref().unwrap();
        assert!(paths.iter().any(|p| p.contains("src/auth")));
    }
}

mod group_rules {
    use super::*;

    #[test]
    fn generates_group_rules() {
        let detection = ProjectDetection::default();
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![
            DetectedModule::new("auth", "Authentication").paths(vec!["src/auth/".into()]),
            DetectedModule::new("user", "User management").paths(vec!["src/user/".into()]),
        ];
        let groups = vec![ModuleGroup::new(
            "identity",
            "Identity group",
            vec!["auth".into(), "user".into()],
        )
        .with_boundary_rules(vec!["No direct database access".into()])];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = GroupRuleGenerator::generate(&ctx);
        assert!(!rules.is_empty());

        let identity_rule = rules.iter().find(|r| r.name == "identity");
        assert!(identity_rule.is_some());

        let identity_rule = identity_rule.unwrap();
        assert_eq!(identity_rule.category, RuleCategory::Group);
        assert_eq!(identity_rule.priority, 70);
    }

    #[test]
    fn group_rule_has_union_paths() {
        let detection = ProjectDetection::default();
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![
            DetectedModule::new("auth", "Authentication").paths(vec!["src/auth/".into()]),
            DetectedModule::new("user", "User management").paths(vec!["src/user/".into()]),
        ];
        let groups = vec![ModuleGroup::new(
            "identity",
            "Identity group",
            vec!["auth".into(), "user".into()],
        )];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = GroupRuleGenerator::generate(&ctx);
        let identity_rule = rules.iter().find(|r| r.name == "identity").unwrap();
        let paths = identity_rule.paths.as_ref().unwrap();

        assert!(paths.iter().any(|p| p.contains("auth")));
        assert!(paths.iter().any(|p| p.contains("user")));
    }
}

mod domain_rules {
    use super::*;

    #[test]
    fn generates_domain_rules() {
        let detection = ProjectDetection::default();
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = DomainRuleGenerator::generate(&ctx);
        assert!(!rules.is_empty());
    }

    #[test]
    fn domain_rules_have_triggers() {
        let detection = ProjectDetection::default();
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = DomainRuleGenerator::generate(&ctx);
        for rule in &rules {
            assert!(
                rule.triggers.is_some(),
                "Domain rule {} should have triggers",
                rule.name
            );
        }
    }

    #[test]
    fn no_security_rule_without_evidence() {
        let detection = ProjectDetection::default();
        let conventions = default_conventions();
        let constraints = ExtractedConstraints::default();
        let tech_stack = TechStack::new("rust");
        let modules = vec![];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = DomainRuleGenerator::generate(&ctx);
        assert!(
            rules.iter().find(|r| r.name == "security").is_none(),
            "Security rule should not be generated without evidence"
        );
    }

    #[test]
    fn security_rule_has_security_triggers() {
        use claudegen::pipeline::phases::constraint_extraction::{AntiPattern, Gotcha};

        let detection = ProjectDetection::default();
        let conventions = default_conventions();
        let constraints = ExtractedConstraints {
            gotchas: vec![Gotcha {
                title: "Auth token leak".into(),
                description: "Tokens not rotated in auth middleware".into(),
                when: "Using custom auth flow".into(),
                solution: "Use token rotation middleware".into(),
                related_files: vec![],
            }],
            ..Default::default()
        };
        let tech_stack = TechStack::new("rust");
        let modules = vec![];
        let groups = vec![];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = DomainRuleGenerator::generate(&ctx);
        let security = rules
            .iter()
            .find(|r| r.name == "security")
            .expect("Security rule must be generated when security evidence exists");
        let triggers = security.triggers.as_ref().unwrap();
        assert!(
            triggers.iter().any(|t| t.contains("auth") || t.contains("security")),
            "Security rule triggers must include auth or security keywords"
        );
    }
}

mod full_generation {
    use super::*;

    #[test]
    fn rules_are_sorted_by_priority() {
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
            .paths(vec!["src/auth/".into()])];
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

        for window in rules.windows(2) {
            assert!(
                window[0].priority >= window[1].priority,
                "Rules should be sorted by priority (highest first)"
            );
        }
    }

    #[test]
    fn project_rule_is_first() {
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
        let modules = vec![];
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

        let first = &rules[0];
        assert_eq!(first.category, RuleCategory::Project);
    }

    #[test]
    fn all_rules_pass_validation() {
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
            .paths(vec!["src/auth/".into()])];
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

    #[test]
    fn rules_have_valid_output_paths() {
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
            .paths(vec!["src/auth/".into()])];
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
        for rule in &rules {
            let path = rule.output_path();
            assert!(path.ends_with(".md"), "Output path should end with .md");
            assert!(!path.starts_with('/'), "Output path should be relative");
        }
    }

    #[test]
    fn rules_output_paths_use_category_subdirectories() {
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
        let modules = vec![DetectedModule::new("auth", "Authentication")
            .paths(vec!["src/auth/".into()])
            .conventions(vec![Convention::new("secure", "Be secure")])];
        let groups = vec![ModuleGroup::new(
            "backend",
            "Backend",
            vec!["auth".into()],
        ).with_boundary_rules(vec!["No frontend".into()])];

        let ctx = create_test_context(
            &detection,
            &conventions,
            &constraints,
            &tech_stack,
            &modules,
            &groups,
        );

        let rules = RulesGenerator::generate(&ctx);

        // Verify each category uses correct subdirectory
        for rule in &rules {
            let path = rule.output_path();
            match rule.category {
                RuleCategory::Project => assert!(
                    !path.contains('/') || path == "project.md",
                    "Project rule should be at root: {}",
                    path
                ),
                RuleCategory::Tech => assert!(
                    path.starts_with("tech/"),
                    "Tech rule should be in tech/: {}",
                    path
                ),
                RuleCategory::Framework => assert!(
                    path.starts_with("frameworks/"),
                    "Framework rule should be in frameworks/: {}",
                    path
                ),
                RuleCategory::Module => assert!(
                    path.starts_with("modules/"),
                    "Module rule should be in modules/: {}",
                    path
                ),
                RuleCategory::Group => assert!(
                    path.starts_with("groups/"),
                    "Group rule should be in groups/: {}",
                    path
                ),
                RuleCategory::Domain => assert!(
                    path.starts_with("domains/"),
                    "Domain rule should be in domains/: {}",
                    path
                ),
                RuleCategory::CrossCutting => assert!(
                    path.starts_with("cross-cutting/"),
                    "CrossCutting rule should be in cross-cutting/: {}",
                    path
                ),
                RuleCategory::Service => assert!(
                    path.starts_with("services/"),
                    "Service rule should be in services/: {}",
                    path
                ),
                RuleCategory::Custom => assert!(
                    path.starts_with("custom/"),
                    "Custom rule should be in custom/: {}",
                    path
                ),
            }
        }
    }
}
