//! Core types for Multi-Agent Documentation Pipeline
//!
//! This module defines the type system for the 6-phase pipeline:
//!
//! - Session types: DocSession, SessionStatus
//! - Checkpoint types: PipelineCheckpoint
//! - Analysis types: Complexity, Importance, ValueCategory

use serde::{Deserialize, Serialize};

// =============================================================================
// Session Types
// =============================================================================

/// Documentation session tracking pipeline state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSession {
    pub id: String,
    pub project_path: String,

    /// Pipeline state
    pub status: SessionStatus,
    pub current_phase: u8,

    /// Progress
    pub total_files: usize,
    pub files_analyzed: usize,

    /// Quality
    pub quality_score: f32,

    /// Timestamps
    pub started_at: Option<String>,
    pub last_checkpoint_at: Option<String>,
    pub completed_at: Option<String>,

    /// Error handling
    pub last_error: Option<String>,

    /// Analysis mode: fast, standard, deep
    pub analysis_mode: String,

    /// Detected project scale: small, medium, large, enterprise
    pub detected_scale: String,

    /// Project profile from characterization (JSON)
    #[serde(default)]
    pub project_profile: Option<String>,

    /// Quality scores history per refinement turn (JSON array)
    #[serde(default)]
    pub quality_scores_history: Option<String>,

    /// Current refinement turn
    #[serde(default)]
    pub refinement_turn: u8,

    /// Pipeline checkpoint data for resume (JSON blob)
    #[serde(default)]
    pub checkpoint_data: Option<String>,
}

impl DocSession {
    /// Create a new session with defaults
    pub fn new(project_path: String, analysis_mode: &str, detected_scale: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            project_path,
            status: SessionStatus::Pending,
            current_phase: 1,
            total_files: 0,
            files_analyzed: 0,
            quality_score: 0.0,
            started_at: None,
            last_checkpoint_at: None,
            completed_at: None,
            last_error: None,
            analysis_mode: analysis_mode.to_string(),
            detected_scale: detected_scale.to_string(),
            project_profile: None,
            quality_scores_history: None,
            refinement_turn: 0,
            checkpoint_data: None,
        }
    }
}

/// Current checkpoint schema version
///
/// Increment this when making breaking changes to PipelineCheckpoint structure.
pub const CHECKPOINT_VERSION: u8 = 2;

/// Pipeline checkpoint for resume support
///
/// Stores intermediate results from each phase to enable resuming
/// from any point in the pipeline.
///
/// ## Schema Version 2 (Current)
///
/// - `version`: Schema version for compatibility checking
/// - `checksum`: CRC32 for data integrity validation
/// - `files`: Discovered files for analysis
/// - `project_profile_json`: Characterization results (Phase 1)
/// - `file_insights_json`: Bottom-up analysis (Phase 3)
/// - `project_insights_json`: Top-down analysis (Phase 4)
/// - `domain_insights_json`: Consolidation results (Phase 5)
/// - `last_completed_phase`: Resume point (1-6)
/// - `checkpoint_at`: Timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineCheckpoint {
    /// Checkpoint schema version for forward compatibility
    #[serde(default = "default_checkpoint_version")]
    pub version: u8,

    /// CRC32 checksum of serialized data (excluding checksum field)
    #[serde(default)]
    pub checksum: u32,

    /// Files discovered for analysis
    #[serde(default)]
    pub files: Vec<String>,

    /// Project profile from characterization (Phase 1)
    #[serde(default)]
    pub project_profile_json: Option<String>,

    /// File insights from bottom-up analysis (Phase 3)
    #[serde(default)]
    pub file_insights_json: Option<String>,

    /// Project insights from top-down analysis (Phase 4)
    #[serde(default)]
    pub project_insights_json: Option<String>,

    /// Domain insights from consolidation (Phase 5)
    #[serde(default)]
    pub domain_insights_json: Option<String>,

    /// Documentation blueprint from structure discovery (Phase 5.5)
    #[serde(default)]
    pub documentation_blueprint_json: Option<String>,

    /// Last completed phase (1-6)
    pub last_completed_phase: u8,

    /// Timestamp of last checkpoint
    pub checkpoint_at: String,
}

fn default_checkpoint_version() -> u8 {
    CHECKPOINT_VERSION
}

impl Default for PipelineCheckpoint {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineCheckpoint {
    pub fn new() -> Self {
        Self {
            version: CHECKPOINT_VERSION,
            checksum: 0,
            files: vec![],
            project_profile_json: None,
            file_insights_json: None,
            project_insights_json: None,
            domain_insights_json: None,
            documentation_blueprint_json: None,
            last_completed_phase: 0,
            checkpoint_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Update checkpoint timestamp
    pub fn touch(&mut self) {
        self.checkpoint_at = chrono::Utc::now().to_rfc3339();
    }

    /// Compute checksum for data integrity validation
    fn compute_checksum(&self) -> u32 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        // Hash all data fields (excluding checksum itself)
        self.version.hash(&mut hasher);
        self.files.hash(&mut hasher);
        self.project_profile_json.hash(&mut hasher);
        self.file_insights_json.hash(&mut hasher);
        self.project_insights_json.hash(&mut hasher);
        self.domain_insights_json.hash(&mut hasher);
        self.documentation_blueprint_json.hash(&mut hasher);
        self.last_completed_phase.hash(&mut hasher);
        self.checkpoint_at.hash(&mut hasher);

        hasher.finish() as u32
    }

    /// Update checksum before serialization
    pub fn finalize(&mut self) {
        self.checksum = self.compute_checksum();
    }

    /// Validate checkpoint integrity and version compatibility
    pub fn validate(&self) -> Result<(), CheckpointError> {
        // Version check
        if self.version > CHECKPOINT_VERSION {
            return Err(CheckpointError::IncompatibleVersion {
                found: self.version,
                expected: CHECKPOINT_VERSION,
            });
        }

        // Checksum validation (skip for v0 checkpoints without checksum)
        if self.checksum != 0 {
            let computed = self.compute_checksum();
            if self.checksum != computed {
                return Err(CheckpointError::ChecksumMismatch {
                    expected: self.checksum,
                    computed,
                });
            }
        }

        // Phase consistency check
        if self.last_completed_phase > 6 {
            return Err(CheckpointError::InvalidPhase(self.last_completed_phase));
        }

        // Data consistency: ensure later phases have earlier phase data
        if self.last_completed_phase >= 3 && self.file_insights_json.is_none() {
            return Err(CheckpointError::MissingData {
                phase: 3,
                field: "file_insights_json",
            });
        }
        if self.last_completed_phase >= 4 && self.project_insights_json.is_none() {
            return Err(CheckpointError::MissingData {
                phase: 4,
                field: "project_insights_json",
            });
        }
        if self.last_completed_phase >= 5 && self.domain_insights_json.is_none() {
            return Err(CheckpointError::MissingData {
                phase: 5,
                field: "domain_insights_json",
            });
        }

        Ok(())
    }

    /// Serialize with checksum for safe storage
    pub fn to_json(&mut self) -> Result<String, serde_json::Error> {
        self.finalize();
        serde_json::to_string(self)
    }

    /// Deserialize and validate from JSON
    pub fn from_json(json: &str) -> Result<Self, CheckpointError> {
        let checkpoint: Self =
            serde_json::from_str(json).map_err(|e| CheckpointError::ParseError(e.to_string()))?;
        checkpoint.validate()?;
        Ok(checkpoint)
    }
}

/// Checkpoint validation errors
#[derive(Debug, Clone)]
pub enum CheckpointError {
    /// Checkpoint version is newer than supported
    IncompatibleVersion { found: u8, expected: u8 },
    /// Checksum validation failed
    ChecksumMismatch { expected: u32, computed: u32 },
    /// Invalid phase number
    InvalidPhase(u8),
    /// Required data missing for phase
    MissingData { phase: u8, field: &'static str },
    /// JSON parse error
    ParseError(String),
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IncompatibleVersion { found, expected } => {
                write!(
                    f,
                    "Checkpoint version {} is newer than supported version {}",
                    found, expected
                )
            }
            Self::ChecksumMismatch { expected, computed } => {
                write!(
                    f,
                    "Checkpoint corrupted: checksum mismatch (expected {}, got {})",
                    expected, computed
                )
            }
            Self::InvalidPhase(phase) => {
                write!(f, "Invalid phase number: {}", phase)
            }
            Self::MissingData { phase, field } => {
                write!(f, "Phase {} requires {} but it's missing", phase, field)
            }
            Self::ParseError(msg) => {
                write!(f, "Failed to parse checkpoint: {}", msg)
            }
        }
    }
}

impl std::error::Error for CheckpointError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SessionStatus {
    #[default]
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Pending => "pending",
            SessionStatus::Running => "running",
            SessionStatus::Paused => "paused",
            SessionStatus::Completed => "completed",
            SessionStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "running" => SessionStatus::Running,
            "paused" => SessionStatus::Paused,
            "completed" => SessionStatus::Completed,
            "failed" => SessionStatus::Failed,
            _ => SessionStatus::Pending,
        }
    }
}

// =============================================================================
// Core Enums
// =============================================================================

/// Complexity level for documentation depth determination
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl Complexity {
    /// Parse complexity from string (case-insensitive)
    pub fn parse(s: &str) -> Self {
        match s.trim_matches('"').to_lowercase().as_str() {
            "critical" => Complexity::Critical,
            "high" => Complexity::High,
            "medium" => Complexity::Medium,
            _ => Complexity::Low,
        }
    }
}

/// Importance level for prioritization and architectural significance
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default, Hash,
)]
#[serde(rename_all = "lowercase")]
pub enum Importance {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl Importance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Importance::Critical => "critical",
            Importance::High => "high",
            Importance::Medium => "medium",
            Importance::Low => "low",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "critical" => Importance::Critical,
            "high" => Importance::High,
            "low" => Importance::Low,
            _ => Importance::Medium,
        }
    }
}

impl std::fmt::Display for Importance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// Multi-Agent Pipeline Types
// =============================================================================

/// Categories of valuable documentation content for quality scoring
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ValueCategory {
    /// Implicit assumptions not enforced by code
    HiddenAssumptions,
    /// Areas dangerous to modify
    ModificationRisks,
    /// State machines and transitions
    StateMachines,
    /// Magic numbers and business constants
    CriticalConstants,
    /// Multi-step processes and flows
    Workflows,
    /// API contracts and interfaces
    ApiContracts,
    /// Integration points with external systems
    IntegrationPoints,
    /// Domain-specific business rules
    DomainRules,
}

impl ValueCategory {
    /// All value categories for iteration
    pub fn all() -> &'static [ValueCategory] {
        &[
            ValueCategory::HiddenAssumptions,
            ValueCategory::ModificationRisks,
            ValueCategory::StateMachines,
            ValueCategory::CriticalConstants,
            ValueCategory::Workflows,
            ValueCategory::ApiContracts,
            ValueCategory::IntegrationPoints,
            ValueCategory::DomainRules,
        ]
    }

    /// Display name for the category
    pub fn display_name(&self) -> &'static str {
        match self {
            ValueCategory::HiddenAssumptions => "Hidden Assumptions",
            ValueCategory::ModificationRisks => "Modification Risks",
            ValueCategory::StateMachines => "State Machines",
            ValueCategory::CriticalConstants => "Critical Constants",
            ValueCategory::Workflows => "Workflows",
            ValueCategory::ApiContracts => "API Contracts",
            ValueCategory::IntegrationPoints => "Integration Points",
            ValueCategory::DomainRules => "Domain Rules",
        }
    }
}
