//! Orchestration Generator
//!
//! Generates fixed skills and agents for orchestration.
//! Rules are generated separately by RulesGenerator.

use super::agents::FixedAgentsGenerator;
use super::skills::FixedSkillsGenerator;
use crate::types::agent::Agent;
use crate::types::skill::Skill;
use crate::types::Rule;

pub struct OrchestrationGenerator;

impl OrchestrationGenerator {
    pub fn generate() -> OrchestrationArtifacts {
        OrchestrationArtifacts {
            skills: FixedSkillsGenerator::generate(),
            agents: FixedAgentsGenerator::generate(),
            rules: Vec::new(),
        }
    }
}

pub struct OrchestrationArtifacts {
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub rules: Vec<Rule>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generates_fixed_skills() {
        let artifacts = OrchestrationGenerator::generate();
        assert_eq!(artifacts.skills.len(), 5);

        let skill_names: Vec<&str> = artifacts.skills.iter().map(|s| s.name.as_str()).collect();
        assert!(skill_names.contains(&"code-review"));
        assert!(skill_names.contains(&"implement"));
        assert!(skill_names.contains(&"plan"));
        assert!(skill_names.contains(&"debug"));
        assert!(skill_names.contains(&"refactor"));
    }

    #[test]
    fn test_generates_fixed_agents() {
        let artifacts = OrchestrationGenerator::generate();
        assert_eq!(artifacts.agents.len(), 3);

        let agent_names: Vec<&str> = artifacts.agents.iter().map(|a| a.name.as_str()).collect();
        assert!(agent_names.contains(&"reviewer"));
        assert!(agent_names.contains(&"coder"));
        assert!(agent_names.contains(&"architect"));
    }

    #[test]
    fn test_rules_are_empty() {
        let artifacts = OrchestrationGenerator::generate();
        assert!(artifacts.rules.is_empty());
    }

    #[test]
    fn test_all_skills_valid() {
        let artifacts = OrchestrationGenerator::generate();
        for skill in &artifacts.skills {
            let issues = skill.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(errors.is_empty(), "Skill {} has errors: {:?}", skill.name, errors);
        }
    }

    #[test]
    fn test_all_agents_valid() {
        let artifacts = OrchestrationGenerator::generate();
        for agent in &artifacts.agents {
            let issues = agent.validate();
            let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
            assert!(errors.is_empty(), "Agent {} has errors: {:?}", agent.name, errors);
        }
    }
}
