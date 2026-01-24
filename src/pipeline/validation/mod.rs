//! Pipeline Validation Module
//!
//! Validation result types for quality assessment.
//! For LLM-as-Judge functionality, use `crate::pipeline::quality` directly.

mod simplified;

pub use simplified::{
    ConsistencyResult, CrossValidationResult, EvidenceTraceabilityResult, PlanConsistencyResult,
    TierFilterResult,
};
