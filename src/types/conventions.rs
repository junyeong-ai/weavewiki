//! Convention types for inferred project conventions
//!
//! These types represent detected conventions and patterns in a codebase.
//! Pure data types only - processing logic lives in `pipeline::phases::convention_inference`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Naming case conventions detected in code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NamingCase {
    #[default]
    SnakeCase,
    CamelCase,
    PascalCase,
    KebabCase,
    ScreamingSnakeCase,
}

impl std::fmt::Display for NamingCase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SnakeCase => write!(f, "snake_case"),
            Self::CamelCase => write!(f, "camelCase"),
            Self::PascalCase => write!(f, "PascalCase"),
            Self::KebabCase => write!(f, "kebab-case"),
            Self::ScreamingSnakeCase => write!(f, "SCREAMING_SNAKE_CASE"),
        }
    }
}

/// Inferred conventions from codebase analysis
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InferredConventions {
    pub architecture: ArchitectureConvention,
    pub naming: NamingConventions,
    pub patterns: Vec<CodePattern>,
    pub file_organization: FileOrganization,
    pub error_handling: ErrorHandlingPattern,
    pub async_pattern: AsyncPattern,
    pub testing: TestingConvention,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArchitectureConvention {
    pub pattern_name: String,
    pub description: String,
    pub layers: Vec<ArchitectureLayer>,
    pub data_flow: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureLayer {
    pub name: String,
    pub path_pattern: String,
    pub responsibility: String,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NamingConventions {
    pub file_naming: FileNaming,
    pub type_naming: TypeNaming,
    pub function_naming: FunctionNaming,
    pub module_naming: ModuleNaming,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileNaming {
    pub case: NamingCase,
    pub suffix_patterns: Vec<SuffixPattern>,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuffixPattern {
    pub suffix: String,
    pub purpose: String,
    pub example: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TypeNaming {
    pub case: NamingCase,
    pub prefix_patterns: Vec<String>,
    pub suffix_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FunctionNaming {
    pub case: NamingCase,
    pub verb_prefixes: Vec<String>,
    pub async_suffix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModuleNaming {
    pub case: NamingCase,
    pub grouping_pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodePattern {
    pub name: String,
    pub description: String,
    pub category: PatternCategory,
    /// Raw frequency value (0.0-1.0) - let LLM interpret significance
    pub frequency: f32,
    pub evidence: Vec<PatternEvidence>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatternCategory {
    ErrorHandling,
    Concurrency,
    StateManagement,
    DataAccess,
    Validation,
    Logging,
    Configuration,
    Testing,
    Other,
}

impl std::fmt::Display for PatternCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ErrorHandling => write!(f, "Error Handling"),
            Self::Concurrency => write!(f, "Concurrency"),
            Self::StateManagement => write!(f, "State Management"),
            Self::DataAccess => write!(f, "Data Access"),
            Self::Validation => write!(f, "Validation"),
            Self::Logging => write!(f, "Logging"),
            Self::Configuration => write!(f, "Configuration"),
            Self::Testing => write!(f, "Testing"),
            Self::Other => write!(f, "Other"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternEvidence {
    pub file: String,
    pub line: u32,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileOrganization {
    pub structure_type: StructureType,
    pub key_directories: Vec<DirectoryRole>,
    pub import_patterns: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StructureType {
    #[default]
    Flat,
    LayeredByType,
    FeatureBased,
    DomainDriven,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryRole {
    pub path: String,
    pub role: String,
    pub file_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorHandlingPattern {
    pub style: ErrorStyle,
    /// Raw count of Result<> pattern occurrences
    pub result_count: usize,
    /// Raw count of exception pattern occurrences
    pub exception_count: usize,
    pub error_types: Vec<String>,
    pub propagation_pattern: String,
    pub recovery_strategy: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorStyle {
    #[default]
    ResultType,
    Exceptions,
    ErrorCodes,
    Mixed,
}

impl std::fmt::Display for ErrorStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResultType => write!(f, "Result Type"),
            Self::Exceptions => write!(f, "Exceptions"),
            Self::ErrorCodes => write!(f, "Error Codes"),
            Self::Mixed => write!(f, "Mixed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AsyncPattern {
    pub style: AsyncStyle,
    /// Raw count of async function occurrences
    pub async_count: usize,
    /// Raw count of sync function occurrences
    pub sync_count: usize,
    pub runtime: Option<String>,
    pub concurrency_patterns: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AsyncStyle {
    #[default]
    Synchronous,
    AsyncAwait,
    Callbacks,
    Reactive,
    Mixed,
}

impl std::fmt::Display for AsyncStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Synchronous => write!(f, "Synchronous"),
            Self::AsyncAwait => write!(f, "Async/Await"),
            Self::Callbacks => write!(f, "Callbacks"),
            Self::Reactive => write!(f, "Reactive"),
            Self::Mixed => write!(f, "Mixed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestingConvention {
    pub framework: Option<String>,
    pub location: TestLocation,
    pub naming_pattern: String,
    pub coverage_tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestLocation {
    #[default]
    SameDirectory,
    TestsDirectory,
    SrcTests,
    Mixed,
}

impl std::fmt::Display for TestLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SameDirectory => write!(f, "Same directory"),
            Self::TestsDirectory => write!(f, "tests/ directory"),
            Self::SrcTests => write!(f, "src/tests/"),
            Self::Mixed => write!(f, "Mixed"),
        }
    }
}
