//! Top-Down Insight Types

use crate::types::DomainTerm;
use crate::wiki::exhaustive::types::Importance;
use serde::{Deserialize, Serialize};

/// Project-level insight from top-down analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInsight {
    /// Source agent
    pub agent: String, // "architecture", "risk", "flow", "domain"

    // === Architecture Agent Output ===
    pub architecture_pattern: Option<String>,
    pub layers: Vec<Layer>,
    pub boundary_violations: Vec<BoundaryViolation>,
    pub architecture_diagram: Option<String>,

    // === Risk Agent Output ===
    pub risk_map: Vec<RiskArea>,
    pub modification_hotspots: Vec<ModificationHotspot>,
    pub cross_cutting_risks: Vec<CrossCuttingRisk>,

    // === Flow Agent Output ===
    pub business_flows: Vec<BusinessFlow>,
    pub event_flows: Vec<EventFlow>,
    pub data_pipelines: Vec<DataPipeline>,

    // === Domain Agent Output ===
    pub domain_terminology: Vec<DomainTerm>,
    pub domain_patterns: Vec<String>,
    pub domain_recommendations: Vec<String>,
}

impl ProjectInsight {
    pub fn new(agent: &str) -> Self {
        Self {
            agent: agent.to_string(),
            architecture_pattern: None,
            layers: vec![],
            boundary_violations: vec![],
            architecture_diagram: None,
            risk_map: vec![],
            modification_hotspots: vec![],
            cross_cutting_risks: vec![],
            business_flows: vec![],
            event_flows: vec![],
            data_pipelines: vec![],
            domain_terminology: vec![],
            domain_patterns: vec![],
            domain_recommendations: vec![],
        }
    }
}

// DomainTerm is now imported from crate::types::DomainTerm

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub files: Vec<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryViolation {
    pub from_layer: String,
    pub to_layer: String,
    pub file: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskArea {
    pub area: String,
    pub risk_level: Importance,
    pub files: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModificationHotspot {
    pub file: String,
    pub reason: String,
    pub dependents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossCuttingRisk {
    pub name: String,
    pub affected_areas: Vec<String>,
    pub mitigation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessFlow {
    pub name: String,
    pub steps: Vec<String>,
    pub diagram: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFlow {
    pub name: String,
    pub events: Vec<String>,
    pub handlers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPipeline {
    pub name: String,
    pub stages: Vec<String>,
    pub source: Option<String>,
    pub destination: Option<String>,
}
