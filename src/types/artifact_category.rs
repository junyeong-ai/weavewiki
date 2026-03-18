//! Artifact Category System
//!
//! Centralized definition of artifact categories for organizational purposes.
//! Quality decisions are delegated to LLM and fact validation.
//!
//! LLM-Trust Architecture:
//! - All skills are discovered by LLM - no fixed "core" skills
//! - All skills are project-specific by nature
//! - Base agents provide methodology patterns

/// Base agent name constants
pub const AGENT_REVIEWER: &str = "reviewer";
pub const AGENT_CODER: &str = "coder";
pub const AGENT_ARCHITECT: &str = "architect";

/// Base agent names - these provide methodology templates
pub const BASE_AGENTS: &[&str] = &[AGENT_REVIEWER, AGENT_CODER, AGENT_ARCHITECT];

/// Minimum required evidence references for project-specific artifacts
pub const MIN_PROJECT_SPECIFIC_REFS: usize = 2;

/// Artifact category for organizational purposes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactCategory {
    /// Base Agents - methodology templates
    Methodology,
    /// Skills, Rules, Module/Domain Agents - project-specific
    ProjectSpecific,
}

impl ArtifactCategory {
    /// Minimum recommended @file:line references
    #[inline]
    pub const fn min_evidence_refs(self) -> usize {
        match self {
            Self::Methodology => 0,
            Self::ProjectSpecific => MIN_PROJECT_SPECIFIC_REFS,
        }
    }

    /// All skills are project-specific (LLM-discovered)
    #[inline]
    pub const fn for_skill() -> Self {
        Self::ProjectSpecific
    }

    /// Determine category for an agent by name
    #[inline]
    pub fn for_agent(name: &str) -> Self {
        if BASE_AGENTS.contains(&name) {
            Self::Methodology
        } else {
            Self::ProjectSpecific
        }
    }

    /// Rules are always ProjectSpecific
    #[inline]
    pub const fn for_rule() -> Self {
        Self::ProjectSpecific
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_skills_are_project_specific() {
        assert_eq!(ArtifactCategory::for_skill(), ArtifactCategory::ProjectSpecific);
    }

    #[test]
    fn test_base_agents_are_methodology() {
        for name in BASE_AGENTS {
            assert_eq!(
                ArtifactCategory::for_agent(name),
                ArtifactCategory::Methodology
            );
        }
    }

    #[test]
    fn test_module_agents_are_project_specific() {
        assert_eq!(
            ArtifactCategory::for_agent("auth-specialist"),
            ArtifactCategory::ProjectSpecific
        );
    }

    #[test]
    fn test_rules_always_project_specific() {
        assert_eq!(
            ArtifactCategory::for_rule(),
            ArtifactCategory::ProjectSpecific
        );
    }
}
