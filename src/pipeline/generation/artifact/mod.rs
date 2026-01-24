//! Artifact Validation Module
//!
//! Provides validation for generated Claude Code artifacts.

mod validators;

pub use crate::types::validation::ValidationIssue;
pub use validators::{
    ArtifactValidation, ArtifactValidator, BatchArtifactValidation, BatchValidator,
};

use crate::types::{Agent, ProjectMemory, Rule, Skill};

/// Result of artifact generation
#[derive(Debug, Clone, Default)]
pub struct GeneratedArtifacts {
    pub claude_md: Option<ProjectMemory>,
    pub rules: Vec<Rule>,
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub stats: GenerationStats,
}

impl GeneratedArtifacts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn total(&self) -> usize {
        self.skills.len()
            + self.agents.len()
            + self.rules.len()
            + if self.claude_md.is_some() { 1 } else { 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
            && self.agents.is_empty()
            && self.rules.is_empty()
            && self.claude_md.is_none()
    }
}

/// Statistics about generation process
#[derive(Debug, Clone, Default)]
pub struct GenerationStats {
    pub insights_used: usize,
    pub insights_filtered: usize,
    pub claude_md_sections: usize,
    pub rules_generated: usize,
    pub skills_generated: usize,
    pub agents_generated: usize,
}
