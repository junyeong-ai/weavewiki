//! claudegen - Claude Code Plugin Generator
//!
//! Analyzes codebases and generates Claude Code plugins following official structure:
//! - CLAUDE.md (project memory)
//! - .claudegen/.claude-plugin/plugin.json (plugin manifest)
//! - .claudegen/skills/{name}/SKILL.md (automated skills)
//! - .claudegen/agents/{name}.md (specialized agents)

#![recursion_limit = "256"]

pub mod ai;
pub mod analyzer;
pub mod cli;
pub mod config;
pub mod constants;
pub mod pipeline;
pub mod storage;
pub mod types;
pub mod verifier;

pub use config::{Config, ConfigLoader};
pub use storage::database::PoolConfig;
pub use storage::{Database, SharedDatabase};
pub use types::error::{ClaudegenError, ErrorCategory, Result, ResultExt};

pub use ai::{
    GlobalTokenBudget, LlmProvider, LlmResponse, MetricsCollector, ProviderChain,
    ProviderChainBuilder, SharedBudget, SharedMetrics, TimeoutConfig, with_timeout,
};

#[cfg(feature = "claude-agent")]
pub use ai::ClaudeAgentProvider;

pub use analyzer::{
    parser::{Language, ParseResult, Parser, detect_language},
    scanner::FileScanner,
};

pub use pipeline::{AdaptivePipeline, AdaptivePipelineOutput};
