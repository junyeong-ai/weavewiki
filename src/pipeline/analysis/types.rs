//! Analysis Types
//!
//! Summary-level types for module analysis results.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::Severity;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PatternSummary {
    pub name: String,
    pub category: String,
    pub description: String,
    pub locations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConstraintSummary {
    pub title: String,
    pub description: String,
    pub severity: Severity,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GotchaSummary {
    pub description: String,
    pub severity: Severity,
    pub file: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ModuleSummary {
    pub module_path: String,
    pub responsibility: String,
    pub file_count: usize,
    pub total_lines: usize,
    pub patterns: Vec<PatternSummary>,
    pub constraints: Vec<ConstraintSummary>,
    pub gotchas: Vec<GotchaSummary>,
    pub key_abstractions: Vec<String>,
    pub internal_deps: Vec<String>,
    pub external_deps: Vec<String>,
    pub public_api: Vec<String>,
    pub confidence: f32,
    #[serde(default)]
    pub source_chunk_ids: Vec<String>,
}
