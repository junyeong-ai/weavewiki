//! Generation Module
//!
//! Artifact generation with context preservation.
//!
//! Key components:
//! - `ClaudeMdGenerator`: Generates CLAUDE.md from OutputPlan
//! - `PathRulesGenerator`: Generates path-based rules
//! - `OrchestrationGenerator`: Generates multi-agent orchestration artifacts
//! - `ModuleMapGenerator`: Generates module_map.json
//! - `HookScriptGenerator`: Generates hook scripts
//! - Validators: Artifact quality validation

pub mod artifact;
pub mod hooks;
pub mod module_map_gen;
pub mod orchestration;
pub mod path_rules;

pub use artifact::{
    ArtifactValidation, ArtifactValidator, BatchArtifactValidation, BatchValidator,
    GeneratedArtifacts, GenerationStats, ValidationIssue,
};
pub use hooks::HookScriptGenerator;
pub use module_map_gen::ModuleMapGenerator;
pub use orchestration::{OrchestrationArtifacts, OrchestrationGenerator};
pub use path_rules::{ClaudeMdGenerator, PathRulesGenerator};
