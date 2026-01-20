//! Generation Module
//!
//! Handles output generation for the adaptive pipeline.

pub mod artifact;
pub mod insight_driven;
pub mod path_rules;

pub use artifact::{
    generate_all_artifacts, AgentsGenerator, ArtifactGenerator, GeneratedArtifacts,
    GenerationStats, RulesGenerator, SkillsGenerator,
};
pub use artifact::ClaudeMdGenerator as InsightClaudeMdGenerator;
pub use insight_driven::{GenerationContext, InsightDrivenGenerator, ValueAssessment};
pub use path_rules::{ClaudeMdGenerator, PathRulesGenerator};
