//! Generation Module
//!
//! Artifact generation with context preservation.
//!
//! Key components:
//! - `GenerationContextBuilder`: Builds rich context for generation
//! - `ClaudeMdGenerator`: Generates CLAUDE.md from OutputPlan
//! - `PathRulesGenerator`: Generates path-based rules
//! - Prompt builders: LLM prompts for refinement

pub mod artifact;
pub mod context;
pub mod path_rules;
pub mod prompts;
pub mod types;

pub use context::GenerationContextBuilder;
pub use prompts::{
    AgentPromptBuilder, ClaudeMdPromptBuilder, RulePromptBuilder, SkillPromptBuilder,
};
pub use types::{GenerationContext, PlannedArtifact, SynthesisSlice};

pub use artifact::{
    ArtifactValidation, ArtifactValidator, BatchArtifactValidation, BatchValidator,
    GeneratedArtifacts, GenerationStats, ValidationIssue,
};

pub use path_rules::{ClaudeMdGenerator, PathRulesGenerator};
