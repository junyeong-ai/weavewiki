//! Generation Module
//!
//! Artifact generation with context preservation.
//!
//! Key components:
//! - `ClaudeMdGenerator`: Generates CLAUDE.md from OutputPlan
//! - `PathRulesGenerator`: Generates path-based rules
//! - Validators: Artifact quality validation

pub mod artifact;
pub mod path_rules;

pub use artifact::{
    ArtifactValidation, ArtifactValidator, BatchArtifactValidation, BatchValidator,
    GeneratedArtifacts, GenerationStats, ValidationIssue,
};

pub use path_rules::{ClaudeMdGenerator, PathRulesGenerator};
