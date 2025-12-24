//! Project-level type definitions
//!
//! Contains types used for project detection and classification.

use serde::{Deserialize, Serialize};

/// Detected framework information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Framework {
    pub name: String,
    pub version: Option<String>,
    pub confidence: f32,
}

impl Framework {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
            confidence: 1.0,
        }
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }
}

/// Architecture pattern classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ArchitecturePattern {
    Mvc,
    Layered,
    Clean,
    Microservices,
    ComponentBased,
    #[default]
    Unknown,
}
