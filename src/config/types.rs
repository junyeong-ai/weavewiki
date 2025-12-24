//! Configuration Types
//!
//! All configuration structures with sensible defaults.
//! Supports global (~/.weavewiki/) and project (.weavewiki/) level configuration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Root configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Configuration version
    pub version: String,

    /// Project-specific settings
    pub project: ProjectConfig,

    /// Code analysis settings
    pub analysis: AnalysisConfig,

    /// Documentation output settings
    pub documentation: DocumentationConfig,

    /// LLM provider settings
    pub llm: LlmConfig,

    /// Session management settings
    pub session: SessionConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            project: ProjectConfig::default(),
            analysis: AnalysisConfig::default(),
            documentation: DocumentationConfig::default(),
            llm: LlmConfig::default(),
            session: SessionConfig::default(),
        }
    }
}

impl Config {
    /// Validate configuration values are within acceptable ranges.
    /// Returns `WeaveError::Config` on validation failure.
    pub fn validate(&self) -> crate::types::Result<()> {
        // LLM temperature validation
        if !(0.0..=2.0).contains(&self.llm.temperature) {
            return Err(crate::types::WeaveError::Config(format!(
                "LLM temperature must be between 0.0 and 2.0, got {}",
                self.llm.temperature
            )));
        }

        // Timeout validation
        if self.llm.timeout_secs == 0 {
            return Err(crate::types::WeaveError::Config(
                "LLM timeout_secs must be greater than 0".to_string(),
            ));
        }

        // Session checkpoint interval
        if self.session.checkpoint_interval == 0 {
            return Err(crate::types::WeaveError::Config(
                "Session checkpoint_interval must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }
}

// =============================================================================
// Project Configuration
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    /// Project name (defaults to directory name)
    pub name: Option<String>,

    /// Project type
    #[serde(rename = "type")]
    pub project_type: ProjectType,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: None,
            project_type: ProjectType::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    Backend,
    Frontend,
    Library,
    Monorepo,
    #[default]
    Auto,
}

// =============================================================================
// Analysis Mode & Project Scale (Multi-Agent Pipeline)
// =============================================================================

/// Analysis mode determining agent counts, turn limits, and quality targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisMode {
    /// Quick analysis for CI/CD and previews
    Fast,
    /// Balanced analysis (default)
    #[default]
    Standard,
    /// Thorough analysis for releases
    Deep,
}

impl std::fmt::Display for AnalysisMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalysisMode::Fast => write!(f, "fast"),
            AnalysisMode::Standard => write!(f, "standard"),
            AnalysisMode::Deep => write!(f, "deep"),
        }
    }
}

impl std::str::FromStr for AnalysisMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "fast" => Ok(AnalysisMode::Fast),
            "standard" => Ok(AnalysisMode::Standard),
            "deep" => Ok(AnalysisMode::Deep),
            _ => Err(format!(
                "Unknown analysis mode: {}. Valid values: fast, standard, deep",
                s
            )),
        }
    }
}

/// Project scale classification based on file count
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectScale {
    /// < 50 files
    Small,
    /// 50-200 files
    #[default]
    Medium,
    /// 200-500 files
    Large,
    /// 500+ files
    Enterprise,
}

impl ProjectScale {
    /// Determine project scale from file count
    pub fn from_file_count(count: usize) -> Self {
        match count {
            0..=49 => ProjectScale::Small,
            50..=199 => ProjectScale::Medium,
            200..=499 => ProjectScale::Large,
            _ => ProjectScale::Enterprise,
        }
    }
}

impl std::fmt::Display for ProjectScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectScale::Small => write!(f, "small"),
            ProjectScale::Medium => write!(f, "medium"),
            ProjectScale::Large => write!(f, "large"),
            ProjectScale::Enterprise => write!(f, "enterprise"),
        }
    }
}

impl std::str::FromStr for ProjectScale {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "small" => Ok(ProjectScale::Small),
            "medium" => Ok(ProjectScale::Medium),
            "large" => Ok(ProjectScale::Large),
            "enterprise" => Ok(ProjectScale::Enterprise),
            _ => Err(format!(
                "Unknown project scale: {}. Valid values: small, medium, large, enterprise",
                s
            )),
        }
    }
}

/// Configuration for a specific mode × scale combination
///
/// Controls pipeline behavior across all phases with mode-aware and scale-aware tuning.
///
/// ## Characterization
/// - `char_turn3_enabled`: Enables Turn 3 section discovery for dynamic domain sections
/// - `char_refinement_rounds`: Refinement rounds after initial characterization (0-2)
///   - 0: Standard single-pass characterization
///   - 1: Re-run Turn 2 with synthesis context for deeper understanding
///   - 2: Multiple refinement rounds for enterprise-scale projects
///
/// ## Bottom-Up Analysis
/// - `bottom_up_batch_size`: Files per batch for LLM processing
/// - `bottom_up_max_file_chars`: Truncation limit for file content in prompts
/// - `bottom_up_concurrency`: Maximum concurrent file analyses within a batch
///
/// ## Top-Down Analysis
/// - `top_down_max_agents`: Maximum agents to select (Architecture, Risk, Flow, Domain)
///
/// ## Refinement
/// - `refinement_max_turns`: Maximum quality improvement iterations
/// - `refinement_quality_target`: Target quality score threshold (0.0-1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeConfig {
    // Characterization
    /// Enables Turn 3 section discovery for dynamic domain sections
    pub char_turn3_enabled: bool,
    /// Characterization refinement rounds (0=none, 1=one round, 2=two rounds)
    /// Re-runs Turn 2 agents with synthesized context for deeper understanding
    pub char_refinement_rounds: usize,

    // Bottom-Up Analysis
    /// Files per batch for LLM processing
    pub bottom_up_batch_size: usize,
    /// Maximum characters per file in prompts
    pub bottom_up_max_file_chars: usize,
    /// Maximum concurrent file analyses within a batch (parallelism control)
    pub bottom_up_concurrency: usize,

    // Top-Down Analysis
    /// Maximum agents to run (1-4: Architecture, Risk, Flow, Domain)
    pub top_down_max_agents: usize,

    // Refinement
    /// Maximum refinement iterations
    pub refinement_max_turns: usize,
    /// Target quality score (0.0-1.0)
    pub refinement_quality_target: f32,
}

impl Default for ModeConfig {
    fn default() -> Self {
        // Standard mode, Medium scale defaults
        Self {
            char_turn3_enabled: false,
            char_refinement_rounds: 0,
            bottom_up_batch_size: 10,
            bottom_up_max_file_chars: 10000,
            bottom_up_concurrency: 4,
            top_down_max_agents: 4,
            refinement_max_turns: 3,
            refinement_quality_target: 0.80,
        }
    }
}

/// Get configuration for a specific mode and scale combination
///
/// ## Mode Characteristics
/// - **Fast**: Quick analysis for CI/CD, smaller context, minimal refinement
/// - **Standard**: Balanced analysis, moderate context, adequate refinement
/// - **Deep**: Thorough analysis, maximum context, extensive refinement
///
/// ## Scale Characteristics
/// - **Small** (<50 files): Lower concurrency, faster completion
/// - **Medium** (50-200): Balanced settings
/// - **Large** (200-500): Higher concurrency, more refinement
/// - **Enterprise** (500+): Maximum parallelism, highest quality targets
pub fn get_mode_config(mode: AnalysisMode, scale: ProjectScale) -> ModeConfig {
    match (mode, scale) {
        // =====================================================================
        // Fast Mode - Optimized for speed, CI/CD pipelines
        // =====================================================================
        (AnalysisMode::Fast, ProjectScale::Small) => ModeConfig {
            char_turn3_enabled: false,
            char_refinement_rounds: 0,
            bottom_up_batch_size: 15,
            bottom_up_max_file_chars: 8000,
            bottom_up_concurrency: 2,
            top_down_max_agents: 1,
            refinement_max_turns: 1,
            refinement_quality_target: 0.60,
        },
        (AnalysisMode::Fast, ProjectScale::Medium) => ModeConfig {
            char_turn3_enabled: false,
            char_refinement_rounds: 0,
            bottom_up_batch_size: 15,
            bottom_up_max_file_chars: 8000,
            bottom_up_concurrency: 3,
            top_down_max_agents: 2,
            refinement_max_turns: 2,
            refinement_quality_target: 0.60,
        },
        (AnalysisMode::Fast, ProjectScale::Large) => ModeConfig {
            char_turn3_enabled: false,
            char_refinement_rounds: 0,
            bottom_up_batch_size: 20,
            bottom_up_max_file_chars: 8000,
            bottom_up_concurrency: 4,
            top_down_max_agents: 2,
            refinement_max_turns: 2,
            refinement_quality_target: 0.65,
        },
        (AnalysisMode::Fast, ProjectScale::Enterprise) => ModeConfig {
            char_turn3_enabled: false,
            char_refinement_rounds: 0,
            bottom_up_batch_size: 25,
            bottom_up_max_file_chars: 6000,
            bottom_up_concurrency: 6,
            top_down_max_agents: 3,
            refinement_max_turns: 2,
            refinement_quality_target: 0.65,
        },

        // =====================================================================
        // Standard Mode - Balanced quality and performance
        // =====================================================================
        (AnalysisMode::Standard, ProjectScale::Small) => ModeConfig {
            char_turn3_enabled: false,
            char_refinement_rounds: 0,
            bottom_up_batch_size: 10,
            bottom_up_max_file_chars: 10000,
            bottom_up_concurrency: 3,
            top_down_max_agents: 3,
            refinement_max_turns: 3,
            refinement_quality_target: 0.75,
        },
        (AnalysisMode::Standard, ProjectScale::Medium) => ModeConfig::default(),
        (AnalysisMode::Standard, ProjectScale::Large) => ModeConfig {
            char_turn3_enabled: true,
            char_refinement_rounds: 1, // One refinement round
            bottom_up_batch_size: 12,
            bottom_up_max_file_chars: 10000,
            bottom_up_concurrency: 5,
            top_down_max_agents: 4,
            refinement_max_turns: 4,
            refinement_quality_target: 0.85,
        },
        (AnalysisMode::Standard, ProjectScale::Enterprise) => ModeConfig {
            char_turn3_enabled: true,
            char_refinement_rounds: 1, // One refinement round
            bottom_up_batch_size: 15,
            bottom_up_max_file_chars: 10000,
            bottom_up_concurrency: 6,
            top_down_max_agents: 4,
            refinement_max_turns: 5,
            refinement_quality_target: 0.90,
        },

        // =====================================================================
        // Deep Mode - Maximum quality, release documentation
        // =====================================================================
        (AnalysisMode::Deep, ProjectScale::Small) => ModeConfig {
            char_turn3_enabled: true,
            char_refinement_rounds: 1, // One refinement round
            bottom_up_batch_size: 8,
            bottom_up_max_file_chars: 16000, // Increased for Deep Research
            bottom_up_concurrency: 4,
            top_down_max_agents: 4,
            refinement_max_turns: 4,
            refinement_quality_target: 0.85,
        },
        (AnalysisMode::Deep, ProjectScale::Medium) => ModeConfig {
            char_turn3_enabled: true,
            char_refinement_rounds: 1, // One refinement round
            bottom_up_batch_size: 8,
            bottom_up_max_file_chars: 16000, // Increased for Deep Research
            bottom_up_concurrency: 5,
            top_down_max_agents: 4,
            refinement_max_turns: 5,
            refinement_quality_target: 0.90,
        },
        (AnalysisMode::Deep, ProjectScale::Large) => ModeConfig {
            char_turn3_enabled: true,
            char_refinement_rounds: 2, // Two refinement rounds
            bottom_up_batch_size: 10,
            bottom_up_max_file_chars: 14000, // Increased for Deep Research
            bottom_up_concurrency: 6,
            top_down_max_agents: 4,
            refinement_max_turns: 6,
            refinement_quality_target: 0.92,
        },
        (AnalysisMode::Deep, ProjectScale::Enterprise) => ModeConfig {
            char_turn3_enabled: true,
            char_refinement_rounds: 2, // Two refinement rounds
            bottom_up_batch_size: 12,
            bottom_up_max_file_chars: 12000, // Increased for Deep Research
            bottom_up_concurrency: 8,
            top_down_max_agents: 4,
            refinement_max_turns: 8,
            refinement_quality_target: 0.95,
        },
    }
}

// =============================================================================
// Analysis Configuration
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    /// Glob patterns to include
    pub include: Vec<String>,

    /// Glob patterns to exclude
    pub exclude: Vec<String>,

    /// Maximum file size in bytes
    pub max_file_size: usize,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            include: vec!["**/*".to_string()],
            exclude: vec![
                "node_modules/**".to_string(),
                "dist/**".to_string(),
                ".git/**".to_string(),
                "target/**".to_string(),
                "vendor/**".to_string(),
                "__pycache__/**".to_string(),
                ".venv/**".to_string(),
                "build/**".to_string(),
            ],
            max_file_size: 1_048_576, // 1MB
        }
    }
}

// =============================================================================
// Documentation Configuration
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DocumentationConfig {
    /// Output directory (relative to .weavewiki/)
    pub output_dir: PathBuf,
}

impl Default for DocumentationConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("wiki"),
        }
    }
}

// =============================================================================
// LLM Configuration
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Provider name
    pub provider: String,

    /// Model name
    pub model: String,

    /// Request timeout in seconds
    pub timeout_secs: u64,

    /// Temperature for LLM generation (0.0 = deterministic, 1.0 = creative)
    /// Default: 0.0 for consistent, fact-based documentation
    pub temperature: f32,

    /// Fallback provider for retry chain
    pub fallback_provider: Option<String>,

    /// Fallback model for retry chain
    pub fallback_model: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "claude-code".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            timeout_secs: 300,
            temperature: 0.0,
            fallback_provider: None,
            fallback_model: None,
        }
    }
}

// =============================================================================
// Session Configuration
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Checkpoint interval (items processed)
    pub checkpoint_interval: u32,

    /// Auto-resume on startup
    pub auto_resume: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            checkpoint_interval: 100,
            auto_resume: true,
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.version, "1.0");
        assert_eq!(config.llm.provider, "claude-code");
    }

    #[test]
    fn test_analysis_mode() {
        assert_eq!(AnalysisMode::Fast.to_string(), "fast");
        assert_eq!(AnalysisMode::Standard.to_string(), "standard");
        assert_eq!(AnalysisMode::Deep.to_string(), "deep");

        assert_eq!("fast".parse::<AnalysisMode>().unwrap(), AnalysisMode::Fast);
        assert_eq!(
            "standard".parse::<AnalysisMode>().unwrap(),
            AnalysisMode::Standard
        );
        assert_eq!("deep".parse::<AnalysisMode>().unwrap(), AnalysisMode::Deep);
    }

    #[test]
    fn test_project_scale() {
        assert_eq!(ProjectScale::from_file_count(10), ProjectScale::Small);
        assert_eq!(ProjectScale::from_file_count(100), ProjectScale::Medium);
        assert_eq!(ProjectScale::from_file_count(300), ProjectScale::Large);
        assert_eq!(
            ProjectScale::from_file_count(1000),
            ProjectScale::Enterprise
        );
    }
}
