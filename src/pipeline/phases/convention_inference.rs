//! Convention Inference
//!
//! Type definitions for inferred conventions and analysis hints.
//! Primary entry point: `InferredConventions::from_aggregated()` for
//! conventions derived from 100% file coverage analysis.

use std::path::Path;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::ai::response::generate_schema;
use crate::ai::LlmProvider;
use crate::config::ProjectType;
use crate::pipeline::analysis::{
    AggregatedAnalysis, AsyncStyle as AggAsyncStyle, ErrorStyle as AggErrorStyle, NamingCase,
};
use crate::types::hint::{AnalysisHint, HintCategory, HintCollection};
use crate::types::Result;

use super::few_shot::build_inference_prompt;
use super::project_detection::ProjectDetection;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct ConventionInferenceOutput {
    #[serde(default)]
    architecture: ArchitectureOutput,
    #[serde(default)]
    patterns: Vec<PatternOutput>,
    #[serde(default)]
    naming_conventions: NamingConventionsOutput,
    #[serde(default)]
    key_directories: Vec<KeyDirectoryOutput>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct ArchitectureOutput {
    #[serde(default)]
    pattern_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    layers: Vec<ArchitectureLayerOutput>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct ArchitectureLayerOutput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    path_pattern: String,
    #[serde(default)]
    responsibility: String,
    #[serde(default)]
    dependencies: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct PatternOutput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    example_file: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct NamingConventionsOutput {
    #[serde(default)]
    files: String,
    #[serde(default)]
    types: String,
    #[serde(default)]
    functions: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct KeyDirectoryOutput {
    #[serde(default)]
    path: String,
    #[serde(default)]
    role: String,
}

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

impl InferredConventions {
    /// Create from aggregated analysis (100% coverage data)
    pub fn from_aggregated(aggregated: &AggregatedAnalysis) -> Self {
        let conventions = &aggregated.conventions;

        let naming = NamingConventions {
            file_naming: FileNaming {
                case: conventions.primary_naming.unwrap_or_default(),
                suffix_patterns: Vec::new(),
                examples: Vec::new(),
            },
            // Type naming uses same convention as files - LLM validates actual patterns
            type_naming: TypeNaming {
                case: conventions.primary_naming.unwrap_or_default(),
                ..Default::default()
            },
            // Function naming derived from file analysis - verb prefixes detected by LLM
            function_naming: FunctionNaming {
                case: conventions.primary_naming.unwrap_or_default(),
                verb_prefixes: Vec::new(), // LLM detects actual verb prefixes from code
                async_suffix: None,
            },
            module_naming: ModuleNaming::default(),
        };

        let error_handling = ErrorHandlingPattern {
            style: Self::convert_error_style(conventions.primary_error_handling),
            result_count: 0,
            exception_count: 0,
            error_types: Vec::new(),
            propagation_pattern: match conventions.primary_error_handling {
                Some(AggErrorStyle::ResultType) => "? operator for propagation".to_string(),
                Some(AggErrorStyle::ExceptionBased) => "try-catch blocks".to_string(),
                Some(AggErrorStyle::EarlyReturn) => "Early return on error".to_string(),
                Some(AggErrorStyle::MonadicChain) => "Monadic chaining (and_then, map)".to_string(),
                _ => "Mixed approach".to_string(),
            },
            recovery_strategy: "Error-specific handling".to_string(),
        };

        let async_pattern = AsyncPattern {
            style: Self::convert_async_style(conventions.primary_async),
            async_count: 0,
            sync_count: 0,
            runtime: None,
            concurrency_patterns: Vec::new(),
        };

        let patterns: Vec<CodePattern> = aggregated
            .patterns
            .iter()
            .map(|p| CodePattern {
                name: p.pattern.name.clone(),
                description: p.pattern.description.clone(),
                category: infer_pattern_category(&p.pattern.name, &p.pattern.description),
                frequency: p.frequency,
                evidence: p
                    .pattern
                    .locations
                    .iter()
                    .map(|loc| PatternEvidence {
                        file: loc.file.clone(),
                        line: loc.line,
                        snippet: loc.snippet.clone(),
                    })
                    .collect(),
            })
            .collect();

        // Hub module classification based on dependency in-degree only.
        // Note: "Hub" indicates high coupling (many dependents), not architectural role.
        // LLM should refine roles based on actual module content (utils vs core vs infra).
        let file_organization = FileOrganization {
            structure_type: Self::detect_structure_type(aggregated),
            key_directories: aggregated
                .dependency_graph
                .hub_modules
                .iter()
                .map(|m| DirectoryRole {
                    path: m.clone(),
                    role: "Hub module (high in-degree in dependency graph)".to_string(),
                    file_types: Vec::new(),
                })
                .collect(),
            import_patterns: conventions.common_import_patterns.clone(),
        };

        Self {
            architecture: ArchitectureConvention::default(),
            naming,
            patterns,
            file_organization,
            error_handling,
            async_pattern,
            testing: TestingConvention::default(),
        }
    }

    fn convert_error_style(style: Option<AggErrorStyle>) -> ErrorStyle {
        match style {
            Some(AggErrorStyle::ResultType) => ErrorStyle::ResultType,
            Some(AggErrorStyle::ExceptionBased) => ErrorStyle::Exceptions,
            Some(AggErrorStyle::NullCheck) => ErrorStyle::ErrorCodes,
            Some(AggErrorStyle::EarlyReturn) => ErrorStyle::ResultType,
            Some(AggErrorStyle::MonadicChain) => ErrorStyle::ResultType,
            None => ErrorStyle::default(),
        }
    }

    fn convert_async_style(style: Option<AggAsyncStyle>) -> AsyncStyle {
        match style {
            Some(AggAsyncStyle::AsyncAwait) => AsyncStyle::AsyncAwait,
            Some(AggAsyncStyle::Callbacks) => AsyncStyle::Callbacks,
            Some(AggAsyncStyle::Promises) => AsyncStyle::AsyncAwait,
            Some(AggAsyncStyle::Channels) => AsyncStyle::AsyncAwait,
            Some(AggAsyncStyle::Actors) => AsyncStyle::Reactive,
            None => AsyncStyle::default(),
        }
    }

    fn detect_structure_type(_aggregated: &AggregatedAnalysis) -> StructureType {
        StructureType::default()
    }

    pub fn to_hints(&self) -> HintCollection {
        let mut hints = HintCollection::new();

        // Naming convention hint (high confidence - based on file analysis)
        if self.naming.file_naming.case != NamingCase::default() {
            hints.push(
                AnalysisHint::high_confidence(
                    HintCategory::NamingConvention,
                    format!("File naming uses {:?}", self.naming.file_naming.case),
                )
                .with_evidence(self.naming.file_naming.examples.iter().take(3).cloned()),
            );
        }

        // Error handling hint (medium confidence - based on keyword patterns)
        hints.push(
            AnalysisHint::medium_confidence(
                HintCategory::ErrorHandling,
                format!("Error handling appears to use {:?} style", self.error_handling.style),
            )
            .with_evidence([format!("Propagation: {}", self.error_handling.propagation_pattern)]),
        );

        // Async pattern hint (medium confidence - based on keyword patterns)
        if self.async_pattern.style != AsyncStyle::Synchronous {
            hints.push(
                AnalysisHint::medium_confidence(
                    HintCategory::AsyncPattern,
                    format!("Async pattern detected: {:?}", self.async_pattern.style),
                )
                .with_evidence(
                    self.async_pattern
                        .runtime
                        .iter()
                        .map(|r| format!("Runtime: {}", r)),
                ),
            );
        }

        // Architecture hint (low confidence - needs LLM validation)
        if !self.architecture.pattern_name.is_empty() {
            hints.push(
                AnalysisHint::low_confidence(
                    HintCategory::Architecture,
                    format!("Architecture pattern: {}", self.architecture.pattern_name),
                )
                .with_evidence([self.architecture.description.clone()]),
            );
        }

        // Directory role hints (medium confidence)
        for dir in &self.file_organization.key_directories {
            hints.push(AnalysisHint::medium_confidence(
                HintCategory::DirectoryRole,
                format!("{} → {}", dir.path, dir.role),
            ));
        }

        // Testing framework hint (high confidence if detected)
        if let Some(ref framework) = self.testing.framework {
            hints.push(
                AnalysisHint::high_confidence(
                    HintCategory::TestingFramework,
                    format!("Testing framework: {}", framework),
                )
                .with_evidence([format!("Test location: {:?}", self.testing.location)]),
            );
        }

        hints
    }
}

/// Generate analysis hints from aggregated analysis for LLM context.
///
/// These hints provide programmatic signals for LLM to validate and refine.
/// Definitive/High confidence hints can be trusted; others need verification.
pub fn generate_hints_from_aggregated(aggregated: &AggregatedAnalysis) -> HintCollection {
    let mut hints = HintCollection::new();
    let conventions = &aggregated.conventions;

    // Naming case (high confidence - statistical from file analysis)
    if let Some(case) = conventions.primary_naming {
        hints.push(
            AnalysisHint::high_confidence(
                HintCategory::NamingConvention,
                format!("Primary naming convention: {:?}", case),
            )
            .with_evidence([format!(
                "Based on {} file samples",
                aggregated.coverage.total_files
            )]),
        );
    }

    // Error handling style (medium confidence)
    if let Some(style) = conventions.primary_error_handling {
        hints.push(
            AnalysisHint::medium_confidence(
                HintCategory::ErrorHandling,
                format!("Detected error handling: {:?}", style),
            )
            .with_evidence(["Based on pattern frequency in code"]),
        );
    }

    // Async style (medium confidence)
    if let Some(style) = conventions.primary_async {
        hints.push(
            AnalysisHint::medium_confidence(
                HintCategory::AsyncPattern,
                format!("Detected async pattern: {:?}", style),
            )
            .with_evidence(["Based on async/await keyword frequency"]),
        );
    }

    // Hub modules (definitive - from dependency analysis)
    for hub in &aggregated.dependency_graph.hub_modules {
        hints.push(
            AnalysisHint::definitive(
                HintCategory::ModuleRelationship,
                format!("Hub module: {}", hub),
            )
            .with_evidence(["High in/out degree in dependency graph"]),
        );
    }

    // Import patterns (high confidence)
    // No arbitrary limit - LLM token budget is the natural constraint
    for pattern in &conventions.common_import_patterns {
        hints.push(AnalysisHint::high_confidence(
            HintCategory::NamingConvention,
            format!("Common import pattern: {}", pattern),
        ));
    }

    hints
}

fn infer_pattern_category(_name: &str, _description: &str) -> PatternCategory {
    PatternCategory::Other
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

pub struct ConventionInferenceEngine {
    project_root: std::path::PathBuf,
    provider: Arc<dyn LlmProvider>,
    prompt_file_limit: usize,
}

impl ConventionInferenceEngine {
    pub fn new(
        project_root: impl AsRef<Path>,
        provider: Arc<dyn LlmProvider>,
        prompt_file_limit: usize,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            provider,
            prompt_file_limit,
        }
    }

    pub async fn infer(&self, detection: &ProjectDetection) -> Result<InferredConventions> {
        let structure = self.collect_project_structure().await?;
        let samples = self.collect_source_files(detection).await?;

        let static_conventions = self
            .infer_from_static_analysis(&structure, &samples)
            .await?;

        let llm_conventions = match self
            .infer_from_llm(detection.primary_type, &structure, &samples)
            .await
        {
            Ok(conventions) => conventions,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "LLM convention inference failed, using static analysis only"
                );
                InferredConventions::default()
            }
        };

        let merged = self.merge_conventions(static_conventions, llm_conventions);

        tracing::info!(
            architecture = merged.architecture.pattern_name,
            naming_case = ?merged.naming.file_naming.case,
            patterns = merged.patterns.len(),
            "Convention inference complete"
        );

        Ok(merged)
    }

    async fn collect_project_structure(&self) -> Result<String> {
        let mut structure = Vec::new();
        self.collect_directory_tree(&self.project_root, "", 0, &mut structure)
            .await?;
        Ok(structure.join("\n"))
    }

    async fn collect_directory_tree(
        &self,
        dir: &Path,
        prefix: &str,
        depth: usize,
        output: &mut Vec<String>,
    ) -> Result<()> {
        if depth > 3 {
            return Ok(());
        }

        let skip_dirs = [
            "target",
            "node_modules",
            "dist",
            "build",
            ".git",
            "vendor",
            "__pycache__",
            ".venv",
            ".claudegen",
            ".claude",
            "claudegen-plugin",
        ];

        if let Ok(mut entries) = fs::read_dir(dir).await {
            let mut items: Vec<_> = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                items.push(entry);
            }

            items.sort_by_key(|a| a.file_name());

            for entry in items {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip known directories and generated plugin directories
                if skip_dirs.contains(&name.as_str())
                    || name.starts_with('.')
                    || name.ends_with("-plugin")
                {
                    continue;
                }

                let path = entry.path();
                if path.is_dir() {
                    output.push(format!("{prefix}{name}/"));
                    Box::pin(self.collect_directory_tree(
                        &path,
                        &format!("{prefix}  "),
                        depth + 1,
                        output,
                    ))
                    .await?;
                } else if depth < 2 {
                    output.push(format!("{prefix}{name}"));
                }
            }
        }

        Ok(())
    }

    async fn collect_source_files(
        &self,
        _detection: &ProjectDetection,
    ) -> Result<Vec<(String, String)>> {
        let mut samples = Vec::new();

        let extensions: Vec<&str> = vec![
            "rs", "ts", "tsx", "js", "jsx", "py", "go", "kt", "java", "cs", "rb", "php",
        ];

        self.collect_files_recursive(&self.project_root, &extensions, &mut samples, 0)
            .await?;

        Ok(samples)
    }

    /// Recursively collect source files for convention analysis.
    async fn collect_files_recursive(
        &self,
        dir: &Path,
        extensions: &[&str],
        samples: &mut Vec<(String, String)>,
        depth: usize,
    ) -> Result<()> {
        if depth > 10 {
            return Ok(());
        }

        let skip_dirs = [
            "target",
            "node_modules",
            "dist",
            "build",
            ".git",
            "vendor",
            "test",
            "tests",
            "__pycache__",
        ];

        if let Ok(mut entries) = fs::read_dir(dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();

                if skip_dirs.contains(&name) || name.starts_with('.') {
                    continue;
                }

                if path.is_dir() {
                    Box::pin(self.collect_files_recursive(&path, extensions, samples, depth + 1))
                        .await?;
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
                    && extensions.contains(&ext)
                    && let Ok(content) = fs::read_to_string(&path).await
                    && content.len() < 50_000
                {
                    let relative = path
                        .strip_prefix(&self.project_root)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    samples.push((relative, content));
                }
            }
        }

        Ok(())
    }

    async fn infer_from_static_analysis(
        &self,
        structure: &str,
        samples: &[(String, String)],
    ) -> Result<InferredConventions> {
        Ok(InferredConventions {
            naming: self.analyze_naming_patterns(samples),
            file_organization: self.analyze_file_organization(structure),
            error_handling: self.analyze_error_handling(samples),
            async_pattern: self.analyze_async_patterns(samples),
            testing: self.analyze_testing_patterns(structure, samples),
            ..Default::default()
        })
    }

    fn analyze_naming_patterns(&self, _samples: &[(String, String)]) -> NamingConventions {
        NamingConventions::default()
    }

    fn analyze_file_organization(&self, _structure: &str) -> FileOrganization {
        FileOrganization {
            structure_type: StructureType::default(),
            key_directories: Vec::new(),
            import_patterns: Vec::new(),
        }
    }

    /// Analyze error handling patterns across all supported languages.
    /// Provides raw counts for LLM to interpret - doesn't classify style.
    fn analyze_error_handling(&self, samples: &[(String, String)]) -> ErrorHandlingPattern {
        let mut result_count = 0;
        let mut exception_count = 0;

        for (_, content) in samples {
            // Result/Either types (Rust, Scala, Kotlin, Swift, Haskell)
            if content.contains("Result<")
                || content.contains("-> Result")
                || content.contains("Either<")
                || content.contains("Result.success")
                || content.contains("Result.failure")
            {
                result_count += 1;
            }
            // Go error handling
            if content.contains("if err != nil") || content.contains(", err :=") {
                result_count += 1;
            }
            // Exception-based (Java, Python, JS, C#, Kotlin, PHP)
            if content.contains("throw ")
                || content.contains("try {")
                || content.contains("catch ")
                || content.contains("except ")  // Python
                || content.contains("raise ")   // Python
                || content.contains("try:")     // Python
            {
                exception_count += 1;
            }
        }

        ErrorHandlingPattern {
            style: ErrorStyle::default(), // LLM determines actual style
            result_count,
            exception_count,
            error_types: Vec::new(),
            propagation_pattern: String::new(),
            recovery_strategy: String::new(),
        }
    }

    /// Analyze async patterns across all supported languages.
    /// Provides raw counts for LLM to interpret - doesn't classify style.
    fn analyze_async_patterns(&self, samples: &[(String, String)]) -> AsyncPattern {
        let mut async_count = 0;
        let mut sync_count = 0;

        for (_, content) in samples {
            // Async/await patterns across languages
            if content.contains("async fn")           // Rust
                || content.contains("async def")       // Python
                || content.contains("async function")  // JS/TS
                || content.contains("suspend fun")     // Kotlin
                || content.contains("CompletableFuture") // Java
                || content.contains("async Task")      // C#
                || content.contains("@Async")          // Spring
            {
                async_count += 1;
            }
            // Go concurrency (goroutines, channels)
            if content.contains("go func") || content.contains("make(chan") {
                async_count += 1;
            }
            // Reactive patterns
            if content.contains("Observable<")
                || content.contains("Flowable<")
                || content.contains(".subscribe(")
            {
                async_count += 1;
            }
            // Sync function detection is language-specific and prone to false positives
            // Only count explicit sync markers
            if content.contains("fn ") && !content.contains("async fn") {
                sync_count += 1;
            }
        }

        AsyncPattern {
            style: AsyncStyle::default(), // LLM determines actual style
            async_count,
            sync_count,
            runtime: None,
            concurrency_patterns: Vec::new(),
        }
    }

    /// Analyze testing patterns across all supported languages.
    /// Returns first detected framework - LLM validates and refines.
    fn analyze_testing_patterns(
        &self,
        structure: &str,
        samples: &[(String, String)],
    ) -> TestingConvention {
        // Test location detection (structure-based)
        let location = if structure.contains("tests/")
            || structure.contains("test/")
            || structure.contains("__tests__/")
        {
            TestLocation::TestsDirectory
        } else if structure.contains("src/test/") {
            TestLocation::SrcTests
        } else {
            TestLocation::SameDirectory
        };

        // Framework detection - expanded for more languages
        let mut framework = None;
        for (_, content) in samples {
            // Rust
            if content.contains("#[test]") || content.contains("#[tokio::test]") {
                framework = Some("Rust built-in".to_string());
                break;
            }
            // JS/TS (Jest, Vitest, Mocha)
            if content.contains("describe(") || content.contains("it(") || content.contains("test(") {
                framework = Some("Jest/Vitest/Mocha".to_string());
                break;
            }
            // Python
            if content.contains("def test_") || content.contains("@pytest") || content.contains("unittest.TestCase") {
                framework = Some("pytest/unittest".to_string());
                break;
            }
            // Go
            if content.contains("func Test") && content.contains("*testing.T") {
                framework = Some("Go testing".to_string());
                break;
            }
            // JVM (JUnit, Kotest)
            if content.contains("@Test") || content.contains("@ParameterizedTest") {
                framework = Some("JUnit".to_string());
                break;
            }
            // Ruby (RSpec, Minitest)
            if content.contains("RSpec.describe") || content.contains("def test_") {
                framework = Some("RSpec/Minitest".to_string());
                break;
            }
            // PHP (PHPUnit)
            if content.contains("extends TestCase") || content.contains("@test") {
                framework = Some("PHPUnit".to_string());
                break;
            }
        }

        TestingConvention {
            framework, // LLM validates and provides details
            location,
            naming_pattern: String::new(), // LLM determines from actual test files
            coverage_tools: Vec::new(),
        }
    }

    async fn infer_from_llm(
        &self,
        project_type: ProjectType,
        structure: &str,
        samples: &[(String, String)],
    ) -> Result<InferredConventions> {
        let prompt = build_inference_prompt(project_type, structure, samples, self.prompt_file_limit);

        let schema = generate_schema::<ConventionInferenceOutput>();

        let response = self.provider.generate(&prompt, &schema).await?;

        // Convert response to conventions - trust LLM's semantic understanding
        // Path validation removed: LLM has context about project structure from prompt
        // Overly strict path filtering can drop valid architectural insights for:
        // - Monorepo patterns not in sample files
        // - Glob patterns (e.g., "packages/*/src")
        // - Future/generated paths
        // Downstream consumers should handle confidence appropriately
        let parsed = self.convert_output_to_conventions(&response.content);

        Ok(parsed)
    }

    fn convert_output_to_conventions(&self, content: &serde_json::Value) -> InferredConventions {
        let output: ConventionInferenceOutput =
            serde_json::from_value(content.clone()).unwrap_or_default();

        let mut conventions = InferredConventions::default();

        // Convert architecture
        conventions.architecture.pattern_name = output.architecture.pattern_name;
        conventions.architecture.description = output.architecture.description;
        conventions.architecture.layers = output
            .architecture
            .layers
            .into_iter()
            .map(|l| ArchitectureLayer {
                name: l.name,
                path_pattern: l.path_pattern,
                responsibility: l.responsibility,
                dependencies: l.dependencies,
            })
            .collect();
        conventions.architecture.confidence = 0.8;

        // Convert patterns
        conventions.patterns = output
            .patterns
            .into_iter()
            .filter(|p| !p.name.is_empty())
            .map(|p| {
                let category = infer_pattern_category(&p.name, &p.description);
                CodePattern {
                    name: p.name,
                    description: p.description,
                    category,
                    frequency: 0.5,
                    evidence: if p.example_file.is_empty() {
                        Vec::new()
                    } else {
                        vec![PatternEvidence {
                            file: p.example_file,
                            line: 0,
                            snippet: String::new(),
                        }]
                    },
                }
            })
            .collect();

        // Convert naming conventions
        conventions.naming.file_naming.case =
            Self::infer_case_from_description(&output.naming_conventions.files);
        conventions.naming.type_naming.case =
            Self::infer_case_from_description(&output.naming_conventions.types);
        conventions.naming.function_naming.case =
            Self::infer_case_from_description(&output.naming_conventions.functions);

        // Convert key directories
        conventions.file_organization.key_directories = output
            .key_directories
            .into_iter()
            .filter(|d| !d.path.is_empty())
            .map(|d| DirectoryRole {
                path: d.path,
                role: d.role,
                file_types: Vec::new(),
            })
            .collect();

        conventions
    }

    fn infer_case_from_description(desc: &str) -> NamingCase {
        let lower = desc.to_lowercase();
        if lower.contains("snake") {
            NamingCase::SnakeCase
        } else if lower.contains("pascal") {
            NamingCase::PascalCase
        } else if lower.contains("camel") {
            NamingCase::CamelCase
        } else if lower.contains("kebab") {
            NamingCase::KebabCase
        } else {
            NamingCase::SnakeCase
        }
    }

    fn merge_conventions(
        &self,
        static_conv: InferredConventions,
        llm_conv: InferredConventions,
    ) -> InferredConventions {
        InferredConventions {
            architecture: if llm_conv.architecture.pattern_name.is_empty() {
                static_conv.architecture
            } else {
                llm_conv.architecture
            },
            naming: static_conv.naming,
            patterns: {
                let mut patterns = static_conv.patterns;
                patterns.extend(llm_conv.patterns);
                patterns
            },
            file_organization: static_conv.file_organization,
            error_handling: static_conv.error_handling,
            async_pattern: static_conv.async_pattern,
            testing: static_conv.testing,
        }
    }
}

pub async fn run(
    project_root: impl AsRef<Path>,
    provider: Arc<dyn LlmProvider>,
    detection: &ProjectDetection,
    prompt_file_limit: usize,
) -> Result<InferredConventions> {
    let engine = ConventionInferenceEngine::new(project_root, provider, prompt_file_limit);
    engine.infer(detection).await
}

pub async fn infer(
    project_root: impl AsRef<Path>,
    detection: &ProjectDetection,
    provider: Arc<dyn LlmProvider>,
    prompt_file_limit: usize,
) -> Result<InferredConventions> {
    run(project_root, provider, detection, prompt_file_limit).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_naming_case() {
        assert_eq!(NamingCase::SnakeCase, NamingCase::SnakeCase);
        assert_ne!(NamingCase::SnakeCase, NamingCase::CamelCase);
    }

    #[test]
    fn test_structure_type() {
        assert!(matches!(StructureType::default(), StructureType::Flat));
    }

    #[test]
    fn test_error_style() {
        assert!(matches!(ErrorStyle::default(), ErrorStyle::ResultType));
    }
}
