use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::node::{EvidenceLocation, InformationTier};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    pub source_id: String,
    pub target_id: String,
    pub metadata: EdgeMetadata,
    pub evidence: EvidenceLocation,
    pub tier: InformationTier,
    pub confidence: f32,
    pub last_verified: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    DependsOn,
    Owns,
    Exposes,
    Calls,
    Implements,
    Extends,
    Persists,
    Validates,
    RoutesTo,
    Renders,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EdgeMetadata {
    pub import_type: Option<ImportType>,
    pub imported_symbols: Option<Vec<String>>,
    pub call_count: Option<u32>,
    pub is_async: Option<bool>,
    pub exposed_as: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportType {
    Static,
    Dynamic,
    TypeOnly,
}
