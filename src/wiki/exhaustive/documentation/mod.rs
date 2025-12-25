//! Dynamic documentation structure discovery and generation
//! AI determines optimal document hierarchy based on project analysis

pub mod blueprint;
pub mod hierarchical_generator;
pub mod structure_agent;

pub use blueprint::*;
pub use hierarchical_generator::*;
pub use structure_agent::*;
