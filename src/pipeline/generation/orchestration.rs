//! Orchestration Artifacts
//!
//! Container for generated artifact collections.
//! Skills and agents are produced by their respective generators;
//! rules are produced by RulesGenerator.

use crate::types::agent::Agent;
use crate::types::skill::Skill;
use crate::types::Rule;

pub struct OrchestrationArtifacts {
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub rules: Vec<Rule>,
}

impl OrchestrationArtifacts {
    pub fn empty() -> Self {
        Self {
            skills: Vec::new(),
            agents: Vec::new(),
            rules: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_artifacts() {
        let artifacts = OrchestrationArtifacts::empty();
        assert!(artifacts.skills.is_empty());
        assert!(artifacts.agents.is_empty());
        assert!(artifacts.rules.is_empty());
    }
}
