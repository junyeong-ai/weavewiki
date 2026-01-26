//! claudegen - Claude Code Plugin Generator
//!
//! Analyzes codebases and generates Claude Code plugins:
//! - CLAUDE.md (project memory)
//! - {project-name}-plugin/.claude-plugin/plugin.json (plugin manifest)
//! - {project-name}-plugin/skills/{name}/SKILL.md (automated skills)
//! - {project-name}-plugin/agents/{name}.md (specialized agents)

#![recursion_limit = "256"]

pub mod ai;
pub mod analyzer;
pub mod cli;
pub mod config;
pub mod constants;
pub mod pipeline;
pub mod storage;
pub mod types;
pub mod utils;

pub use config::{Config, ConfigLoader, TimeoutConfig};
pub use types::error::{ClaudegenError, ErrorCategory, Result, ResultExt};

pub use ai::{
    GlobalTokenBudget, LlmProvider, LlmResponse, MetricsCollector, ProviderChain,
    ProviderChainBuilder, SharedBudget, SharedMetrics, with_timeout,
};

#[cfg(feature = "claude-agent")]
pub use ai::ClaudeAgentProvider;

pub use analyzer::{
    parser::{Language, ParseResult, Parser, detect_language},
    scanner::FileScanner,
};

pub use pipeline::{AdaptivePipeline, AdaptivePipelineOutput};
