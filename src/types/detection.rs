//! Project detection types for automatic project type detection
//!
//! These types represent detected project characteristics (type, languages, workspace config).
//! Pure data types only - detection logic lives in `pipeline::phases::project_detection`.

use serde::{Deserialize, Serialize};

use crate::config::ProjectType;

/// Detected project information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDetection {
    pub primary_type: ProjectType,
    pub confidence: f32,
    pub signals: Vec<DetectionSignal>,
    pub languages: Vec<LanguageInfo>,
    pub is_monorepo: bool,
    pub workspace_config: Option<WorkspaceConfig>,
}

impl Default for ProjectDetection {
    fn default() -> Self {
        Self {
            primary_type: ProjectType::Auto,
            confidence: 0.0,
            signals: Vec::new(),
            languages: Vec::new(),
            is_monorepo: false,
            workspace_config: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionSignal {
    pub signal_type: SignalType,
    pub source: String,
    pub suggests: ProjectType,
    pub weight: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SignalType {
    ManifestFile,
    DirectoryStructure,
    EntryPoint,
    Dependency,
    FrameworkMarker,
    ToolConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub language: String,
    pub file_count: usize,
    pub percentage: f32,
    pub primary_manifest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub workspace_type: DetectedWorkspaceKind,
    pub members: Vec<WorkspaceMember>,
    pub shared_packages: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DetectedWorkspaceKind {
    CargoWorkspace,
    PnpmWorkspace,
    NpmWorkspace,
    YarnWorkspace,
    TurboRepo,
    NxWorkspace,
    LernaMonorepo,
    GradleMultiProject,
    MavenMultiModule,
    GoWorkspace,
    Unknown,
}

impl std::fmt::Display for DetectedWorkspaceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CargoWorkspace => write!(f, "Cargo Workspace"),
            Self::PnpmWorkspace => write!(f, "pnpm Workspace"),
            Self::NpmWorkspace => write!(f, "npm Workspace"),
            Self::YarnWorkspace => write!(f, "Yarn Workspace"),
            Self::TurboRepo => write!(f, "Turborepo"),
            Self::NxWorkspace => write!(f, "Nx Workspace"),
            Self::LernaMonorepo => write!(f, "Lerna Monorepo"),
            Self::GradleMultiProject => write!(f, "Gradle Multi-Project"),
            Self::MavenMultiModule => write!(f, "Maven Multi-Module"),
            Self::GoWorkspace => write!(f, "Go Workspace"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMember {
    pub path: String,
    pub name: Option<String>,
    pub project_type: ProjectType,
    pub language: Option<String>,
}
