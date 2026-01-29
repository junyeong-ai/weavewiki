//! Integration tests for orchestration and full generation
//!
//! Validates:
//! - OrchestrationGenerator produces combined artifacts
//! - Artifact relationships are consistent
//! - Full generation pipeline produces valid output

use claudegen::pipeline::generation::{
    FixedAgentsGenerator, FixedSkillsGenerator, OrchestrationGenerator, RuleGenerationContext,
    RulesGenerator,
};
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
    fn generates_fixed_skills_and_agents() {
        let artifacts = OrchestrationGenerator::generate();

        assert_eq!(artifacts.skills.len(), 5, "Should have 5 fixed skills");
        assert_eq!(artifacts.agents.len(), 3, "Should have 3 fixed agents");
    }

    #[test]
    fn skills_reference_from_agents() {
        let artifacts = OrchestrationGenerator::generate();

        let skill_names: Vec<&str> = artifacts.skills.iter().map(|s| s.name.as_str()).collect();

        for agent in &artifacts.agents {
            if let Some(agent_skills) = &agent.skills {
                for skill_ref in agent_skills {
                    assert!(
                        skill_names.contains(&skill_ref.as_str()),
                        "Agent {} references unknown skill {}",
                        agent.name,
                        skill_ref
                    );
                }
            }
        }
    }

    #[test]
    fn artifacts_rules_are_empty() {
        let artifacts = OrchestrationGenerator::generate();
        assert!(
            artifacts.rules.is_empty(),
            "OrchestrationGenerator should not generate rules (rules use RulesGenerator)"
        );
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
    fn complete_generation_produces_all_artifacts() {
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
            .with_paths(vec!["src/auth/".into()])
            .with_conventions(vec![Convention::new(
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

        let orchestration = OrchestrationGenerator::generate();
        let rules = RulesGenerator::generate(&ctx);

        assert!(!orchestration.skills.is_empty());
        assert!(!orchestration.agents.is_empty());
        assert!(!rules.is_empty());
    }

    #[test]
    fn artifacts_are_independent() {
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
            .with_paths(vec!["src/auth/".into()])
            .with_conventions(vec![Convention::new("secure", "Be secure")])];
        let modules_b = vec![
            DetectedModule::new("auth", "Authentication")
                .with_paths(vec!["src/auth/".into()])
                .with_conventions(vec![Convention::new("secure", "Be secure")]),
            DetectedModule::new("user", "User management")
                .with_paths(vec!["src/user/".into()])
                .with_conventions(vec![Convention::new("validate", "Validate input")]),
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

        let skills_a = FixedSkillsGenerator::generate();
        let skills_b = FixedSkillsGenerator::generate();
        let agents_a = FixedAgentsGenerator::generate();
        let agents_b = FixedAgentsGenerator::generate();

        assert_eq!(
            skills_a.len(),
            skills_b.len(),
            "Fixed skills should be consistent"
        );
        assert_eq!(
            agents_a.len(),
            agents_b.len(),
            "Fixed agents should be consistent"
        );

        let rules_a = RulesGenerator::generate(&ctx_a);
        let rules_b = RulesGenerator::generate(&ctx_b);

        assert!(
            rules_b.len() >= rules_a.len(),
            "More modules should produce more or equal rules"
        );
    }

    #[test]
    fn all_generated_artifacts_pass_validation() {
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
                .with_paths(vec!["src/auth/".into()])
                .with_conventions(vec![Convention::new("secure", "Be secure")]),
            DetectedModule::new("api", "API layer")
                .with_paths(vec!["src/api/".into()])
                .with_conventions(vec![Convention::new("rest", "RESTful API")]),
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

        let orchestration = OrchestrationGenerator::generate();
        let rules = RulesGenerator::generate(&ctx);

        for skill in &orchestration.skills {
            let issues = skill.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(
                errors.is_empty(),
                "Skill {} has validation errors: {:?}",
                skill.name,
                errors
            );
        }

        for agent in &orchestration.agents {
            let issues = agent.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(
                errors.is_empty(),
                "Agent {} has validation errors: {:?}",
                agent.name,
                errors
            );
        }

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

mod artifact_structure {
    use super::*;

    #[test]
    fn skills_have_consistent_structure() {
        let skills = FixedSkillsGenerator::generate();

        for skill in &skills {
            assert!(!skill.name.is_empty());
            assert!(!skill.description.is_empty());
            assert!(!skill.body.is_empty());
            assert!(skill.allowed_tools.is_some());
        }
    }

    #[test]
    fn agents_have_consistent_structure() {
        let agents = FixedAgentsGenerator::generate();

        for agent in &agents {
            assert!(!agent.name.is_empty());
            assert!(!agent.description.is_empty());
            assert!(!agent.prompt.is_empty());
            assert!(agent.tools.is_some());
            assert!(agent.skills.is_some());
            assert!(agent.color.is_some());
        }
    }

    #[test]
    fn skill_tool_sets_are_valid() {
        let skills = FixedSkillsGenerator::generate();
        let valid_tools = [
            "Read", "Grep", "Glob", "Edit", "Write", "Bash", "Task", "WebSearch", "WebFetch",
        ];

        for skill in &skills {
            if let Some(tools) = &skill.allowed_tools {
                for tool in tools {
                    assert!(
                        valid_tools.contains(&tool.as_str()),
                        "Skill {} has invalid tool: {}",
                        skill.name,
                        tool
                    );
                }
            }
        }
    }

    #[test]
    fn agent_tool_sets_are_valid() {
        let agents = FixedAgentsGenerator::generate();
        let valid_tools = [
            "Read", "Grep", "Glob", "Edit", "Write", "Bash", "Task", "WebSearch", "WebFetch",
        ];

        for agent in &agents {
            if let Some(tools) = &agent.tools {
                for tool in tools {
                    assert!(
                        valid_tools.contains(&tool.as_str()),
                        "Agent {} has invalid tool: {}",
                        agent.name,
                        tool
                    );
                }
            }
        }
    }

    #[test]
    fn read_only_roles_have_no_write_tools() {
        let agents = FixedAgentsGenerator::generate();
        let write_tools = ["Edit", "Write", "Bash"];

        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        let architect = agents.iter().find(|a| a.name == "architect").unwrap();

        for agent in [reviewer, architect] {
            if let Some(tools) = &agent.tools {
                for tool in &write_tools {
                    assert!(
                        !tools.contains(&tool.to_string()),
                        "{} should not have {} tool",
                        agent.name,
                        tool
                    );
                }
            }
        }
    }

    #[test]
    fn coder_has_write_tools() {
        let agents = FixedAgentsGenerator::generate();
        let write_tools = ["Edit", "Write", "Bash"];

        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        let tools = coder.tools.as_ref().unwrap();

        for tool in &write_tools {
            assert!(
                tools.contains(&tool.to_string()),
                "Coder should have {} tool",
                tool
            );
        }
    }
}

mod phase7_integration {
    use super::*;
    use claudegen::types::Plugin;
    use claudegen::types::PluginManifest;
    use std::path::Path;

    #[test]
    fn agents_have_consensus_roles() {
        let agents = FixedAgentsGenerator::generate();

        for agent in &agents {
            assert!(
                agent.consensus.is_some(),
                "Agent {} should have consensus role",
                agent.name
            );
        }
    }

    #[test]
    fn reviewer_has_high_priority_veto() {
        let agents = FixedAgentsGenerator::generate();
        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        let consensus = reviewer.consensus.as_ref().unwrap();

        assert_eq!(consensus.priority, 70);
        assert!(consensus.can_veto, "Reviewer should have veto power");
    }

    #[test]
    fn coder_has_no_veto() {
        let agents = FixedAgentsGenerator::generate();
        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        let consensus = coder.consensus.as_ref().unwrap();

        assert_eq!(consensus.priority, 50);
        assert!(!consensus.can_veto, "Coder should not have veto power");
    }

    #[test]
    fn architect_has_veto() {
        let agents = FixedAgentsGenerator::generate();
        let architect = agents.iter().find(|a| a.name == "architect").unwrap();
        let consensus = architect.consensus.as_ref().unwrap();

        assert_eq!(consensus.priority, 60);
        assert!(consensus.can_veto, "Architect should have veto power");
    }

    #[test]
    fn consensus_serialized_in_markdown() {
        let agents = FixedAgentsGenerator::generate();
        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();

        let md = reviewer.to_markdown();
        assert!(md.contains("consensus:"), "Missing consensus field in frontmatter");
        assert!(md.contains("priority: 70"), "Missing priority in consensus");
        assert!(md.contains("can_veto: true"), "Missing can_veto in consensus");
    }

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

    #[test]
    fn tools_serialized_as_yaml_array() {
        let agents = FixedAgentsGenerator::generate();
        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();

        let md = reviewer.to_markdown();
        // YAML array format uses "- " for items
        assert!(md.contains("- Read"), "Tools should be YAML array");
        assert!(md.contains("- Grep"), "Tools should be YAML array");
    }
}

mod artifact_counts {
    use super::*;

    #[test]
    fn skills_count_is_fixed() {
        for _ in 0..3 {
            let skills = FixedSkillsGenerator::generate();
            assert_eq!(skills.len(), 5, "Skills count should always be 5");
        }
    }

    #[test]
    fn agents_count_is_fixed() {
        for _ in 0..3 {
            let agents = FixedAgentsGenerator::generate();
            assert_eq!(agents.len(), 3, "Agents count should always be 3");
        }
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

        let modules_small = vec![DetectedModule::new("auth", "Auth")
            .with_paths(vec!["src/auth/".into()])
            .with_conventions(vec![Convention::new("c", "c")])];

        let modules_large = vec![
            DetectedModule::new("auth", "Auth")
                .with_paths(vec!["src/auth/".into()])
                .with_conventions(vec![Convention::new("c", "c")]),
            DetectedModule::new("user", "User")
                .with_paths(vec!["src/user/".into()])
                .with_conventions(vec![Convention::new("c", "c")]),
            DetectedModule::new("api", "API")
                .with_paths(vec!["src/api/".into()])
                .with_conventions(vec![Convention::new("c", "c")]),
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
