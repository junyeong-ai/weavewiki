//! Configuration Management
//!
//! Unified configuration system with hierarchical resolution:
//! 1. Built-in defaults
//! 2. Global config (~/.weavewiki/config.yaml)
//! 3. Project config (.weavewiki/config.yaml)
//! 4. Environment variables (WEAVEWIKI_*)
//! 5. CLI arguments (highest priority)

mod loader;
mod types;

pub use loader::ConfigLoader;
pub use types::*;
