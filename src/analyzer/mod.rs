//! Code Analyzer Module
//!
//! Provides language-agnostic code analysis capabilities:
//! - Multi-language parsing (AST extraction)
//! - File scanning with gitignore support
//! - Universal structure analysis

pub mod parser;
pub mod scanner;
pub mod structure;

pub use structure::{StructureAnalysis, StructureAnalyzer};
