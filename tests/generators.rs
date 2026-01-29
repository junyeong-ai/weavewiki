//! Integration tests for fixed generators
//!
//! Validates:
//! - FixedSkillsGenerator produces 5 skills
//! - FixedAgentsGenerator produces 3 agents
//! - All artifacts pass validation

use claudegen::pipeline::generation::{FixedAgentsGenerator, FixedSkillsGenerator};

mod skills {
    use super::*;

    #[test]
    fn generates_exactly_five_skills() {
        let skills = FixedSkillsGenerator::generate();
        assert_eq!(skills.len(), 5);
    }

    #[test]
    fn skill_names_are_correct() {
        let skills = FixedSkillsGenerator::generate();
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();

        assert!(names.contains(&"code-review"));
        assert!(names.contains(&"implement"));
        assert!(names.contains(&"plan"));
        assert!(names.contains(&"debug"));
        assert!(names.contains(&"refactor"));
    }

    #[test]
    fn all_skills_are_user_invocable() {
        let skills = FixedSkillsGenerator::generate();
        for skill in &skills {
            assert_eq!(
                skill.user_invocable,
                Some(true),
                "Skill {} should be user-invocable",
                skill.name
            );
        }
    }

    #[test]
    fn code_review_has_read_only_tools() {
        let skills = FixedSkillsGenerator::generate();
        let code_review = skills.iter().find(|s| s.name == "code-review").unwrap();
        let tools = code_review.allowed_tools.as_ref().unwrap();

        assert!(tools.contains(&"Read".to_string()));
        assert!(tools.contains(&"Grep".to_string()));
        assert!(tools.contains(&"Glob".to_string()));
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn implement_has_edit_tools() {
        let skills = FixedSkillsGenerator::generate();
        let implement = skills.iter().find(|s| s.name == "implement").unwrap();
        let tools = implement.allowed_tools.as_ref().unwrap();

        assert!(tools.contains(&"Edit".to_string()));
        assert!(tools.contains(&"Write".to_string()));
        assert!(tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn refactor_has_edit_tools() {
        let skills = FixedSkillsGenerator::generate();
        let refactor = skills.iter().find(|s| s.name == "refactor").unwrap();
        let tools = refactor.allowed_tools.as_ref().unwrap();

        assert!(tools.contains(&"Edit".to_string()));
        assert!(tools.contains(&"Write".to_string()));
    }

    #[test]
    fn debug_has_bash_tool() {
        let skills = FixedSkillsGenerator::generate();
        let debug = skills.iter().find(|s| s.name == "debug").unwrap();
        let tools = debug.allowed_tools.as_ref().unwrap();

        assert!(tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn plan_is_read_only() {
        let skills = FixedSkillsGenerator::generate();
        let plan = skills.iter().find(|s| s.name == "plan").unwrap();
        let tools = plan.allowed_tools.as_ref().unwrap();

        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn skills_with_arguments_have_hints() {
        let skills = FixedSkillsGenerator::generate();

        let code_review = skills.iter().find(|s| s.name == "code-review").unwrap();
        assert!(code_review.argument_hint.is_none());

        let implement = skills.iter().find(|s| s.name == "implement").unwrap();
        assert!(implement.argument_hint.is_some());

        let plan = skills.iter().find(|s| s.name == "plan").unwrap();
        assert!(plan.argument_hint.is_some());

        let debug = skills.iter().find(|s| s.name == "debug").unwrap();
        assert!(debug.argument_hint.is_some());

        let refactor = skills.iter().find(|s| s.name == "refactor").unwrap();
        assert!(refactor.argument_hint.is_some());
    }

    #[test]
    fn all_skills_pass_validation() {
        let skills = FixedSkillsGenerator::generate();
        for skill in &skills {
            let issues = skill.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(
                errors.is_empty(),
                "Skill {} has validation errors: {:?}",
                skill.name,
                errors
            );
        }
    }

    #[test]
    fn skills_have_non_empty_body() {
        let skills = FixedSkillsGenerator::generate();
        for skill in &skills {
            assert!(
                !skill.body.is_empty(),
                "Skill {} should have non-empty body",
                skill.name
            );
            assert!(
                skill.body.len() > 100,
                "Skill {} body seems too short",
                skill.name
            );
        }
    }

    #[test]
    fn skills_have_descriptions() {
        let skills = FixedSkillsGenerator::generate();
        for skill in &skills {
            assert!(
                !skill.description.is_empty(),
                "Skill {} should have description",
                skill.name
            );
        }
    }
}

mod agents {
    use super::*;

    #[test]
    fn generates_exactly_three_agents() {
        let agents = FixedAgentsGenerator::generate();
        assert_eq!(agents.len(), 3);
    }

    #[test]
    fn agent_names_are_correct() {
        let agents = FixedAgentsGenerator::generate();
        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();

        assert!(names.contains(&"reviewer"));
        assert!(names.contains(&"coder"));
        assert!(names.contains(&"architect"));
    }

    #[test]
    fn reviewer_is_read_only() {
        let agents = FixedAgentsGenerator::generate();
        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        let tools = reviewer.tools.as_ref().unwrap();

        assert!(tools.contains(&"Read".to_string()));
        assert!(tools.contains(&"Grep".to_string()));
        assert!(tools.contains(&"Glob".to_string()));
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn coder_has_full_tools() {
        let agents = FixedAgentsGenerator::generate();
        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        let tools = coder.tools.as_ref().unwrap();

        assert!(tools.contains(&"Read".to_string()));
        assert!(tools.contains(&"Edit".to_string()));
        assert!(tools.contains(&"Write".to_string()));
        assert!(tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn architect_is_read_only() {
        let agents = FixedAgentsGenerator::generate();
        let architect = agents.iter().find(|a| a.name == "architect").unwrap();
        let tools = architect.tools.as_ref().unwrap();

        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn reviewer_has_code_review_skill() {
        let agents = FixedAgentsGenerator::generate();
        let reviewer = agents.iter().find(|a| a.name == "reviewer").unwrap();
        let skills = reviewer.skills.as_ref().unwrap();

        assert!(skills.contains(&"code-review".to_string()));
    }

    #[test]
    fn coder_has_implementation_skills() {
        let agents = FixedAgentsGenerator::generate();
        let coder = agents.iter().find(|a| a.name == "coder").unwrap();
        let skills = coder.skills.as_ref().unwrap();

        assert!(skills.contains(&"implement".to_string()));
        assert!(skills.contains(&"debug".to_string()));
        assert!(skills.contains(&"refactor".to_string()));
    }

    #[test]
    fn architect_has_plan_skill() {
        let agents = FixedAgentsGenerator::generate();
        let architect = agents.iter().find(|a| a.name == "architect").unwrap();
        let skills = architect.skills.as_ref().unwrap();

        assert!(skills.contains(&"plan".to_string()));
    }

    #[test]
    fn all_agents_have_colors() {
        let agents = FixedAgentsGenerator::generate();
        for agent in &agents {
            assert!(
                agent.color.is_some(),
                "Agent {} should have a color",
                agent.name
            );
        }
    }

    #[test]
    fn all_agents_pass_validation() {
        let agents = FixedAgentsGenerator::generate();
        for agent in &agents {
            let issues = agent.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(
                errors.is_empty(),
                "Agent {} has validation errors: {:?}",
                agent.name,
                errors
            );
        }
    }

    #[test]
    fn agents_have_descriptions() {
        let agents = FixedAgentsGenerator::generate();
        for agent in &agents {
            assert!(
                !agent.description.is_empty(),
                "Agent {} should have description",
                agent.name
            );
        }
    }

    #[test]
    fn agents_have_prompts() {
        let agents = FixedAgentsGenerator::generate();
        for agent in &agents {
            assert!(
                !agent.prompt.is_empty(),
                "Agent {} should have prompt",
                agent.name
            );
            assert!(
                agent.prompt.len() > 200,
                "Agent {} prompt seems too short",
                agent.name
            );
        }
    }
}
