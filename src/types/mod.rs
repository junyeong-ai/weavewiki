pub mod agent;
pub mod claim;
pub mod domain;
pub mod edge;
pub mod error;
pub mod hook;
pub mod memory;
pub mod node;
pub mod plugin;
pub mod project;
pub mod rule;
pub mod skill;
pub mod utils;
pub mod validation;

pub use agent::{Agent, AgentModel, PermissionMode, VALID_TOOLS, is_valid_tool, MIN_AGENT_FILE_REFS};
pub use claim::{Claim, ClaimEvidence, ClaimType, VerificationIssue, VerificationReport, VerificationStatus};
pub use domain::DomainTerm;
pub use edge::{Edge, EdgeMetadata, EdgeType, ImportType};
pub use error::{
    ClaudegenError, ErrorCategory, ErrorClassifier, LlmError, Result, ResultExt, ValidationError,
    ValidationErrorKind,
};
pub use hook::{Hook, HookCommand, HooksConfig, ToolHooks};
pub use memory::{DevelopmentCommand, ProjectMemory};
pub use node::{
    ApiMetadata, AuthRequirement, ComponentMetadata, EntityMetadata, EvidenceLocation,
    FieldDefinition, FunctionSignature, HttpMethod, InformationTier, Node, NodeMetadata,
    NodeStatus, NodeType, Parameter, PropDefinition, RelationDefinition, SchemaReference,
    StateDefinition, Visibility,
};
pub use plugin::{Plugin, PluginManifest, PluginPermissions, PluginValidationResult, RepositoryInfo};
pub use project::{ArchitecturePattern, Framework};
pub use rule::Rule;
pub use skill::{ContentTier, ContextMode, QualityMetrics, Skill, MIN_FILE_REFS, MIN_ACTIONABLE_COUNT};
pub use utils::{
    ParseWithDefault, enum_to_str, json_bool, json_f64, json_i64, json_string, json_string_array,
    json_string_or, log_filter_error, log_filter_warn,
};
pub use validation::{Severity, ValidationIssue, ValidationResult};

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TokenCount(u64);

impl TokenCount {
    pub const ZERO: Self = Self(0);

    pub const fn new(count: u64) -> Self {
        Self(count)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn exceeds_threshold(self, budget: Self, threshold: f64) -> bool {
        if budget.0 == 0 {
            return false;
        }
        (self.0 as f64 / budget.0 as f64) >= threshold
    }

    pub fn utilization(self, budget: Self) -> f64 {
        if budget.0 == 0 {
            0.0
        } else {
            self.0 as f64 / budget.0 as f64
        }
    }
}

impl fmt::Display for TokenCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for TokenCount {
    fn from(count: u64) -> Self {
        Self(count)
    }
}

impl From<u32> for TokenCount {
    fn from(count: u32) -> Self {
        Self(u64::from(count))
    }
}

impl From<usize> for TokenCount {
    fn from(count: usize) -> Self {
        Self(count as u64) // Safe: usize <= u64 on all supported platforms (32/64-bit)
    }
}

impl std::ops::Add for TokenCount {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::AddAssign for TokenCount {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl std::ops::Sub for TokenCount {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_sub(rhs.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn file(path: &str) -> Self {
        Self(format!("file:{path}"))
    }

    pub fn module(name: &str) -> Self {
        Self(format!("module:{name}"))
    }

    pub fn class(path: &str, name: &str) -> Self {
        Self(format!("class:{path}:{name}"))
    }

    pub fn function(path: &str, name: &str) -> Self {
        Self(format!("function:{path}:{name}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for NodeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Analysis depth mode - 3 clear tiers with guaranteed minimum quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalysisDepth {
    /// Fast mode: Core files only, quick results (~15 LLM calls)
    Fast,
    /// Standard mode: Balanced coverage and cost (~50 LLM calls)
    #[default]
    Standard,
    /// Complete mode: Full coverage with convergence (unlimited LLM calls)
    Complete,
}

impl AnalysisDepth {
    pub fn max_core_files(&self) -> usize {
        match self {
            Self::Fast => 10,
            Self::Standard => 50,
            Self::Complete => usize::MAX,
        }
    }

    pub fn max_important_files(&self) -> usize {
        match self {
            Self::Fast => 20,
            Self::Standard => 100,
            Self::Complete => usize::MAX,
        }
    }

    pub fn max_llm_calls(&self) -> usize {
        match self {
            Self::Fast => 15,
            Self::Standard => 50,
            Self::Complete => usize::MAX,
        }
    }

    pub fn enable_pattern_llm(&self) -> bool {
        match self {
            Self::Fast => false,
            Self::Standard => true,
            Self::Complete => true,
        }
    }

    pub fn characterization_turns(&self) -> usize {
        match self {
            Self::Fast => 1,
            Self::Standard => 2,
            Self::Complete => 5,
        }
    }

    pub fn bottom_up_turns(&self) -> usize {
        match self {
            Self::Fast => 1,
            Self::Standard => 2,
            Self::Complete => 5,
        }
    }

    pub fn pattern_turns(&self) -> usize {
        match self {
            Self::Fast => 1,
            Self::Standard => 2,
            Self::Complete => 5,
        }
    }

    pub fn planning_turns(&self) -> usize {
        match self {
            Self::Fast => 1,
            Self::Standard => 2,
            Self::Complete => 5,
        }
    }

    pub fn generation_turns(&self) -> usize {
        match self {
            Self::Fast => 2,
            Self::Standard => 3,
            Self::Complete => 10,
        }
    }

    pub fn guarantee_turns(&self) -> usize {
        match self {
            Self::Fast => 3,
            Self::Standard => 5,
            Self::Complete => usize::MAX,
        }
    }

    pub fn target_quality(&self) -> f32 {
        match self {
            Self::Fast => 0.7,
            Self::Standard => 0.85,
            Self::Complete => 0.95,
        }
    }

    pub fn minimum_quality(&self) -> f32 {
        0.5
    }

    pub fn enable_convergence(&self) -> bool {
        matches!(self, Self::Complete)
    }

    pub fn enable_cross_validation(&self) -> bool {
        matches!(self, Self::Complete)
    }

    pub fn enable_document_output(&self) -> bool {
        matches!(self, Self::Complete)
    }

    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Standard => "standard",
            Self::Complete => "complete",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Fast => "Core files, quick results (~15 LLM calls)",
            Self::Standard => "Balanced coverage and cost (~50 LLM calls)",
            Self::Complete => "Full coverage with convergence (unlimited)",
        }
    }
}

impl std::str::FromStr for AnalysisDepth {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "fast" => Ok(Self::Fast),
            "standard" => Ok(Self::Standard),
            "complete" => Ok(Self::Complete),
            _ => Err(format!("Unknown depth: '{s}'. Use: fast, standard, complete")),
        }
    }
}
