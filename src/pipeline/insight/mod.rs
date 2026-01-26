//! Insight Types Module
//!
//! Provides core types for insight representation and tier classification.
//! Processing logic is handled by tier_patterns.rs and LlmJudge.

mod types;

pub use types::{
    ArtifactClassification, BusinessRule, Constraint, ConstraintType, DomainKnowledge,
    ExtractedInsight, Insight, InsightCategory, InsightSource, Knowledge, Terminology,
    TierClassification, ValueScore,
};
