//! Domain types for artifact generation
//!
//! Pure domain types without pipeline dependencies.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct GenerationSynthesis {
    pub modules: Vec<ModuleAnalysis>,
    pub patterns: Vec<Pattern>,
    pub constraints: Vec<SynthesisConstraint>,
    pub architectural_decisions: Vec<ArchitecturalDecision>,
    pub cross_cutting_concerns: Vec<CrossCuttingConcern>,
    pub dependencies: Vec<ModuleDependency>,
    pub confidence: ConfidenceMetrics,
}

#[derive(Debug, Clone)]
pub struct ModuleAnalysis {
    pub name: String,
    pub path: String,
    pub files: Vec<String>,
    pub files_analyzed: usize,
    pub responsibility: String,
    pub constraints: Vec<String>,
    pub dependencies: Vec<String>,
    pub confidence: f32,
}

impl Default for ModuleAnalysis {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: String::new(),
            files: Vec::new(),
            files_analyzed: 0,
            responsibility: String::new(),
            constraints: Vec::new(),
            dependencies: Vec::new(),
            confidence: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub name: String,
    pub description: String,
    pub locations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SynthesisConstraint {
    pub name: String,
    pub description: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ArchitecturalDecision {
    pub title: String,
    pub description: String,
    pub rationale: String,
    pub affected_modules: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CrossCuttingConcern {
    pub name: String,
    pub description: String,
    pub affected_modules: Vec<String>,
    pub implementation_notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModuleDependency {
    pub source: String,
    pub target: String,
    pub dependency_type: DependencyType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyType {
    Import,
    Api,
    Event,
    Data,
}

#[derive(Debug, Clone)]
pub struct ArtifactRef {
    pub artifact_type: ArtifactType,
    pub name: String,
    pub relationship: RelationshipType,
    pub summary: String,
}

impl ArtifactRef {
    pub fn new(artifact_type: ArtifactType, name: impl Into<String>) -> Self {
        Self {
            artifact_type,
            name: name.into(),
            relationship: RelationshipType::SameContext,
            summary: String::new(),
        }
    }

    pub fn with_relationship(mut self, relationship: RelationshipType) -> Self {
        self.relationship = relationship;
        self
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    #[default]
    Skill,
    Agent,
    Rule,
    ClaudeMd,
}

impl ArtifactType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Agent => "agent",
            Self::Rule => "rule",
            Self::ClaudeMd => "claude_md",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipType {
    References,
    ReferencedBy,
    SameContext,
    RelatedConstraint,
}

#[derive(Debug, Clone, Default)]
pub struct ConfidenceMetrics {
    pub overall: f32,
    pub coverage: f32,
    pub evidence_strength: f32,
}

/// Quality thresholds for artifact generation
/// Distinct from config::QualityConfig which is the global quality configuration
#[derive(Debug, Clone)]
pub struct GenerationQualityThresholds {
    pub minimum_quality: f32,
    pub target_quality: f32,
    pub max_iterations: usize,
    pub acceptance_delta: f32,
}

impl Default for GenerationQualityThresholds {
    fn default() -> Self {
        Self {
            minimum_quality: 0.70,
            target_quality: 0.85,
            max_iterations: 10,
            acceptance_delta: 0.02,
        }
    }
}

// NOTE: Convention types are defined in pipeline::phases::convention_inference
// NOTE: ProjectDetection is defined in pipeline::phases::project_detection

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectType {
    #[default]
    Unknown,
    RustCli,
    RustLib,
    RustWeb,
    TypeScriptNode,
    TypeScriptReact,
    TypeScriptNext,
    PythonCli,
    PythonWeb,
    PythonLib,
    GoService,
    JavaSpring,
    Monorepo,
}

impl ProjectType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::RustCli => "rust-cli",
            Self::RustLib => "rust-lib",
            Self::RustWeb => "rust-web",
            Self::TypeScriptNode => "typescript-node",
            Self::TypeScriptReact => "typescript-react",
            Self::TypeScriptNext => "typescript-next",
            Self::PythonCli => "python-cli",
            Self::PythonWeb => "python-web",
            Self::PythonLib => "python-lib",
            Self::GoService => "go-service",
            Self::JavaSpring => "java-spring",
            Self::Monorepo => "monorepo",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LanguageInfo {
    pub language: Language,
    pub percentage: f32,
    pub file_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    Kotlin,
    CSharp,
    Cpp,
    C,
    Ruby,
    Swift,
    Other,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::CSharp => "csharp",
            Self::Cpp => "cpp",
            Self::C => "c",
            Self::Ruby => "ruby",
            Self::Swift => "swift",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generation_quality_thresholds_defaults() {
        let config = GenerationQualityThresholds::default();
        assert_eq!(config.minimum_quality, 0.70);
        assert_eq!(config.target_quality, 0.85);
        assert_eq!(config.max_iterations, 10);
    }

    #[test]
    fn test_confidence_metrics_default() {
        let metrics = ConfidenceMetrics::default();
        assert_eq!(metrics.overall, 0.0);
        assert_eq!(metrics.coverage, 0.0);
    }

    #[test]
    fn test_project_type_str() {
        assert_eq!(ProjectType::RustCli.as_str(), "rust-cli");
        assert_eq!(ProjectType::TypeScriptReact.as_str(), "typescript-react");
    }
}
