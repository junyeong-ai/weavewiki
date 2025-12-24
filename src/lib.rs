//! WeaveWiki - AI-Driven Codebase Documentation Generator
//!
//! A comprehensive documentation system that analyzes codebases and generates
//! fact-based, structured documentation using multi-agent AI workflows.
//!
//! ## Core Features
//!
//! - **Multi-Agent Pipeline**: 6-phase analysis with specialized agents
//! - **Language Support**: 30+ languages via tree-sitter parsers
//! - **Checkpoint/Resume**: Persistent state for long-running analysis
//! - **Token Budget Management**: TALE algorithm for efficient LLM usage
//! - **Provider Chain**: Multiple LLM backends with fallback support
//!
//! ## Quick Start
//!
//! ```ignore
//! use weavewiki::{Database, MultiAgentPipeline, MultiAgentConfig};
//! use weavewiki::provider::ClaudeCodeProvider;
//!
//! let db = Database::open("weavewiki.db")?;
//! let provider = ClaudeCodeProvider::new();
//! let pipeline = MultiAgentPipeline::new(
//!     Arc::new(db),
//!     Arc::new(provider),
//!     &project_path,
//!     &output_path,
//! );
//! let result = pipeline.run().await?;
//! ```
//!
//! ## Modules
//!
//! - [`ai`]: LLM provider abstraction, token management, validation
//! - [`analyzer`]: Code parsing with tree-sitter, language detection
//! - [`storage`]: SQLite persistence with connection pooling
//! - [`config`]: Analysis modes and configuration
//! - [`wiki`]: Documentation generation pipelines

#![recursion_limit = "256"]

pub mod ai;
pub mod analyzer;
pub mod cli;
pub mod config;
pub mod constants;
pub mod storage;
pub mod types;
pub mod verifier;
pub mod wiki;

// =============================================================================
// Core Re-exports
// =============================================================================

// Configuration
pub use config::{AnalysisMode, Config, ConfigLoader, ModeConfig, ProjectScale};

// Error Types
pub use types::error::{ErrorCategory, Result, ResultExt, WeaveError};

// Storage
pub use storage::database::PoolConfig;
pub use storage::{Database, SharedDatabase};

// =============================================================================
// Pipeline Re-exports
// =============================================================================

pub use wiki::exhaustive::{
    CheckpointContext, CheckpointManager, MultiAgentConfig, MultiAgentPipeline, MultiAgentResult,
    PipelineCheckpoint, PipelinePhase,
};

// =============================================================================
// AI Re-exports
// =============================================================================

pub use ai::{
    // Providers
    ClaudeCodeProvider,
    // Budget
    GlobalTokenBudget,
    LlmProvider,
    LlmResponse,
    // Metrics
    MetricsCollector,
    ProviderChain,
    ProviderChainBuilder,
    SharedBudget,
    SharedMetrics,
    // Timeout
    TimeoutConfig,
    with_timeout,
};

// =============================================================================
// Analyzer Re-exports
// =============================================================================

pub use analyzer::{
    parser::{Language, ParseResult, Parser, detect_language},
    scanner::FileScanner,
};
