//! Pipeline Validation Module
//!
//! ## Architecture
//!
//! This module provides **deterministic validation** for pipeline outputs:
//! - `TierFilterResult`: Tier ratio validation (Tier1 ≤ 10%, Tier3 ≥ 30%)
//! - `ConsistencyResult`: Duplicate names, cross-references
//! - `CrossValidationResult`: Evidence traceability, plan consistency
//! - `EvidenceTraceabilityResult`: File reference validation against registry
//!
//! ## Role Separation
//!
//! | Module | Type | Purpose |
//! |--------|------|---------|
//! | `validation` | Deterministic | Fast pre-checks, tier ratios, file existence |
//! | `quality::LlmJudge` | Semantic | LLM-based content quality assessment |
//!
//! Use this module for programmatic checks. For semantic quality assessment,
//! use `crate::pipeline::quality::LlmJudge` directly.

mod simplified;

pub use simplified::{
    ConsistencyResult, CrossValidationResult, EvidenceTraceabilityResult, PlanConsistencyResult,
    TierFilterResult,
};
