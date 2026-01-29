//! Generation Module
//!
//! Artifact generation:
//! - Rules: Domain knowledge (auto-injected by context)
//! - Skills: Methodologies (user-invocable)
//! - Agents: Operational roles
//!
//! Components:
//! - `RulesGenerator`: Hierarchical rules (project/tech/framework/module/group/domain)
//! - `FixedSkillsGenerator`: Skills (code-review, implement, plan, debug, refactor)
//! - `FixedAgentsGenerator`: Agents (reviewer, coder, architect)
//! - `ClaudeMdGenerator`: CLAUDE.md project file
//! - `ModuleMapGenerator`: module_map.json

pub mod agents;
pub mod artifact;
pub mod claude_md;
pub mod module_map_gen;
pub mod orchestration;
pub mod rules;
pub mod skills;

pub use agents::FixedAgentsGenerator;
pub use artifact::{
    ArtifactValidation, ArtifactValidator, BatchArtifactValidation, BatchValidator,
    GeneratedArtifacts, GenerationStats, ValidationIssue,
};
pub use claude_md::{ClaudeMdContext, ClaudeMdGenerator};
pub use module_map_gen::ModuleMapGenerator;
pub use orchestration::{OrchestrationArtifacts, OrchestrationGenerator};
pub use rules::{RuleGenerationContext, RulesGenerator};
pub use skills::FixedSkillsGenerator;
