//! AI-Driven Wiki Generation
//!
//! Multi-agent pipeline that guarantees:
//! - 100% file coverage (every source file analyzed)
//! - 100% fact-based (all docs from actual code)
//! - Universal (any language/framework)
//!
//! ## Pipeline Architecture
//!
//! ```text
//! Characterization → Bottom-Up Analysis → Top-Down Analysis
//!                          ↓                    ↓
//!                    Consolidation ← ← ← ← ← ← ←
//!                          ↓
//!                     Refinement → Wiki Output
//! ```

// Main exhaustive system
pub mod exhaustive;

// Caching system
pub mod cache;

// ============================================================================
// Multi-Agent Pipeline Exports
// ============================================================================

pub use exhaustive::{
    Complexity, DocSession, Importance, MultiAgentConfig, MultiAgentPipeline, MultiAgentResult,
    SessionStatus,
};

// ============================================================================
// Utility Exports
// ============================================================================

pub use cache::{CacheConfig, CacheMetadata, CacheStats, CachedPage, WikiCache, WikiCacheEntry};

// Re-export canonical detect_language from analyzer
pub use crate::analyzer::parser::language::{detect_language, detect_language_or_text};
