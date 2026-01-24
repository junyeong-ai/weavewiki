//! Unified Severity Type

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical = 4,
    High = 3,
    #[default]
    Medium = 2,
    Low = 1,
}

impl Severity {
    pub fn weight(&self) -> f32 {
        match self {
            Severity::Critical => 1.0,
            Severity::High => 0.7,
            Severity::Medium => 0.4,
            Severity::Low => 0.1,
        }
    }

    pub fn is_blocking(&self) -> bool {
        matches!(self, Severity::Critical | Severity::High)
    }

    pub fn from_score(score: f32) -> Self {
        match score {
            s if s >= 0.9 => Severity::Critical,
            s if s >= 0.7 => Severity::High,
            s if s >= 0.4 => Severity::Medium,
            _ => Severity::Low,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Critical => write!(f, "critical"),
            Severity::High => write!(f, "high"),
            Severity::Medium => write!(f, "medium"),
            Severity::Low => write!(f, "low"),
        }
    }
}
