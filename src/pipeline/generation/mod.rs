//! Generation Module
//!
//! Artifact generation:
//! - Rules: Domain knowledge (auto-injected by context)
//! - Skills: Methodologies (user-invocable)
//! - Agents: Operational roles
//!
//! Components:
//! - `RulesGenerator`: Hierarchical rules (project/tech/framework/module/group/domain)
//! - `SkillsGenerator`: LLM-first skill discovery
//! - `AgentsGenerator`: Five-layer agent generation
//! - `ClaudeMdGenerator`: CLAUDE.md project file
//! - `ModuleMapGenerator`: module_map.json

pub mod agents;
pub mod artifact;
pub mod artifact_graph;
pub mod claude_md;
pub mod context;
pub mod context_enricher;
pub mod discovery_fmt;
pub mod evidence_gate;
pub mod hooks;
pub mod module_map_gen;
pub mod orchestration;
pub mod rules;
pub mod settings;
pub mod skills;

pub use agents::AgentsGenerator;
pub use artifact_graph::ArtifactGraph;
pub use artifact::{
    ArtifactValidation, ArtifactValidator, BatchArtifactValidation, BatchValidator,
    GeneratedArtifacts, GenerationStats, ValidationIssue,
};
pub use claude_md::{ClaudeMdContext, ClaudeMdGenerator};
pub use hooks::HookGenerator;
pub use module_map_gen::ModuleMapGenerator;
pub use orchestration::OrchestrationArtifacts;
pub use rules::{RuleGenerationContext, RulesGenerator};
pub use settings::{GeneratedSettings, PermissionSettings, SettingsGenerator};
pub use skills::SkillsGenerator;
