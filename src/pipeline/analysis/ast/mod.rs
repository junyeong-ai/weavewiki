//! AST Analysis Module
//!
//! Ground-truth extraction using tree-sitter parsers.
//! Provides verified facts for LLM analysis validation.

mod analyzer;
mod dependencies;
mod structure;

pub use analyzer::{AstAnalysisResult, AstAnalyzerAgent, FileAstAnalysis};
pub use dependencies::{DependencyEdge, DependencyGraph, DependencyType};
pub use structure::{
    AstProjectStructure, ComplexityMetrics, ExportInfo, FunctionInfo, ImportInfo, PublicApiSurface,
    TypeInfo,
};
