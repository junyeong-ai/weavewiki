//! Configuration Management
//!
//! Unified configuration system with hierarchical resolution:
//! 1. Built-in defaults
//! 2. Global config (~/.config/claudegen/config.toml)
//! 3. Project config (.claudegen/config.toml)
//! 4. Environment variables (CLAUDEGEN_*)
//!
//! All configuration types are defined in `types.rs` and re-exported from this module.

mod loader;
mod types;

pub use loader::ConfigLoader;
pub use types::*;
