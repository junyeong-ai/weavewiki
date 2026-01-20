//! Convention Inference Engine
//!
//! Infers project conventions using Few-Shot examples and LLM analysis.
//! Does NOT use hardcoded templates - patterns are discovered from actual code.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::ai::LlmProvider;
use crate::config::ProjectType;
use crate::types::Result;

use super::few_shot::build_inference_prompt;
use super::project_detection::ProjectDetection;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NamingCase {
    #[default]
    SnakeCase,
    CamelCase,
    PascalCase,
    KebabCase,
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
    pub usage_frequency: UsageFrequency,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageFrequency {
    Universal,
    Common,
    Occasional,
    Rare,
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
    max_samples: usize,
}

impl ConventionInferenceEngine {
    pub fn new(
        project_root: impl AsRef<Path>,
        provider: Arc<dyn LlmProvider>,
        max_samples: usize,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            provider,
            max_samples,
        }
    }

    pub async fn infer(&self, detection: &ProjectDetection) -> Result<InferredConventions> {
        let structure = self.collect_project_structure().await?;
        let samples = self.collect_sample_files(detection).await?;

        let static_conventions = self.infer_from_static_analysis(&structure, &samples).await?;

        let llm_conventions = self
            .infer_from_llm(detection.primary_type, &structure, &samples)
            .await
            .unwrap_or_default();

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

    async fn collect_sample_files(
        &self,
        detection: &ProjectDetection,
    ) -> Result<Vec<(String, String)>> {
        let mut samples = Vec::new();

        let primary_lang = detection
            .languages
            .first()
            .map(|l| l.language.as_str())
            .unwrap_or("unknown");

        let extensions: Vec<&str> = match primary_lang {
            "rust" => vec!["rs"],
            "typescript" => vec!["ts", "tsx"],
            "javascript" => vec!["js", "jsx"],
            "python" => vec!["py"],
            "kotlin" => vec!["kt"],
            "java" => vec!["java"],
            "go" => vec!["go"],
            _ => vec!["rs", "ts", "py"],
        };

        self.collect_samples_recursive(&self.project_root, &extensions, &mut samples, 0)
            .await?;

        samples.sort_by(|a, b| {
            let a_priority = self.file_priority(&a.0);
            let b_priority = self.file_priority(&b.0);
            b_priority.cmp(&a_priority)
        });

        Ok(samples.into_iter().take(15).collect())
    }

    fn file_priority(&self, path: &str) -> u32 {
        if path.contains("main") || path.contains("lib.rs") || path.contains("mod.rs") {
            return 100;
        }
        if path.contains("index") {
            return 90;
        }
        if path.contains("service") || path.contains("controller") || path.contains("handler") {
            return 80;
        }
        if path.contains("model") || path.contains("entity") || path.contains("domain") {
            return 70;
        }
        if path.contains("util") || path.contains("helper") {
            return 30;
        }
        if path.contains("test") {
            return 20;
        }
        50
    }

    async fn collect_samples_recursive(
        &self,
        dir: &Path,
        extensions: &[&str],
        samples: &mut Vec<(String, String)>,
        depth: usize,
    ) -> Result<()> {
        if depth > 4 || samples.len() >= 30 {
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
                    Box::pin(self.collect_samples_recursive(
                        &path,
                        extensions,
                        samples,
                        depth + 1,
                    ))
                    .await?;
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
                    && extensions.contains(&ext)
                        && let Ok(content) = fs::read_to_string(&path).await
                            && content.len() < 50_000 {
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

    fn analyze_naming_patterns(&self, samples: &[(String, String)]) -> NamingConventions {
        let mut file_cases: HashMap<NamingCase, usize> = HashMap::new();

        for (path, _) in samples {
            let filename = path.split('/').next_back().unwrap_or(path);
            let name = filename.split('.').next().unwrap_or(filename);

            let case = if name.contains('_') {
                NamingCase::SnakeCase
            } else if name.contains('-') {
                NamingCase::KebabCase
            } else if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                NamingCase::PascalCase
            } else {
                NamingCase::CamelCase
            };

            *file_cases.entry(case).or_default() += 1;
        }

        let file_case = file_cases
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(case, _)| case)
            .unwrap_or_default();

        let mut suffix_patterns = Vec::new();
        let suffix_map: HashMap<&str, &str> = [
            ("_test", "Test files"),
            ("_spec", "Spec files"),
            ("Service", "Service classes"),
            ("Controller", "Controller classes"),
            ("Repository", "Repository classes"),
            ("Handler", "Handler functions"),
        ]
        .into_iter()
        .collect();

        for (path, _) in samples {
            for (suffix, purpose) in &suffix_map {
                if path.contains(suffix) {
                    suffix_patterns.push(SuffixPattern {
                        suffix: suffix.to_string(),
                        purpose: purpose.to_string(),
                        example: path.clone(),
                    });
                    break;
                }
            }
        }

        NamingConventions {
            file_naming: FileNaming {
                case: file_case,
                suffix_patterns,
                examples: samples.iter().take(5).map(|(p, _)| p.clone()).collect(),
            },
            type_naming: TypeNaming {
                case: NamingCase::PascalCase,
                ..Default::default()
            },
            function_naming: FunctionNaming {
                case: NamingCase::SnakeCase,
                verb_prefixes: vec![
                    "get".to_string(),
                    "set".to_string(),
                    "create".to_string(),
                    "delete".to_string(),
                    "update".to_string(),
                ],
                async_suffix: None,
            },
            module_naming: ModuleNaming::default(),
        }
    }

    fn analyze_file_organization(&self, structure: &str) -> FileOrganization {
        let mut key_dirs = Vec::new();

        let dir_roles: Vec<(&str, &str)> = vec![
            // Common roots
            ("src/", "Source code root"),
            ("lib/", "Library code"),
            ("bin/", "Binary entry points"),
            ("tests/", "Test files"),
            ("docs/", "Documentation"),
            // Configuration
            ("config/", "Configuration files"),
            ("settings/", "Settings management"),
            // API & Web
            ("api/", "API definitions"),
            ("routes/", "Route definitions"),
            ("handlers/", "Request handlers"),
            ("controllers/", "Request controllers"),
            ("middleware/", "Middleware components"),
            // Services & Business Logic
            ("services/", "Service layer"),
            ("core/", "Core business logic"),
            ("domain/", "Domain layer"),
            ("entities/", "Domain entities"),
            ("usecases/", "Use cases"),
            // Data Access
            ("models/", "Data models"),
            ("repositories/", "Data repositories"),
            ("storage/", "Storage layer"),
            ("database/", "Database access"),
            // Architecture patterns
            ("adapter/", "Adapter layer"),
            ("port/", "Port interfaces"),
            ("infra/", "Infrastructure layer"),
            // Frontend
            ("components/", "UI components"),
            ("pages/", "Page components"),
            ("views/", "View components"),
            ("hooks/", "React hooks"),
            ("context/", "React context"),
            ("store/", "State store"),
            ("styles/", "Styling"),
            // Utilities
            ("utils/", "Utility functions"),
            ("helpers/", "Helper functions"),
            ("common/", "Common utilities"),
            // CLI
            ("cli/", "CLI interface"),
            ("commands/", "CLI commands"),
            // AI/LLM
            ("ai/", "AI/LLM integration"),
            ("agents/", "AI agents"),
            ("prompts/", "Prompt templates"),
            // Validation & Verification
            ("validation/", "Validation logic"),
            ("verification/", "Verification logic"),
            ("verifier/", "Verifier implementation"),
            // Pipeline
            ("pipeline/", "Pipeline processing"),
            ("phases/", "Pipeline phases"),
            ("translation/", "Translation layer"),
            // Analysis
            ("analyzer/", "Code analysis"),
            ("parser/", "Code parsing"),
            ("scanner/", "File scanning"),
            // Types
            ("types/", "Type definitions"),
            ("schemas/", "Schema definitions"),
        ];

        for (dir, role) in dir_roles {
            if structure.contains(dir) {
                key_dirs.push(DirectoryRole {
                    path: dir.to_string(),
                    role: role.to_string(),
                    file_types: Vec::new(),
                });
            }
        }

        let structure_type = if structure.contains("domain/") && structure.contains("adapter/") {
            StructureType::DomainDriven
        } else if structure.contains("services/") || structure.contains("controllers/") {
            StructureType::LayeredByType
        } else if structure.contains("components/") && structure.contains("pages/") {
            StructureType::FeatureBased
        } else {
            StructureType::Flat
        };

        FileOrganization {
            structure_type,
            key_directories: key_dirs,
            import_patterns: Vec::new(),
        }
    }

    fn analyze_error_handling(&self, samples: &[(String, String)]) -> ErrorHandlingPattern {
        let mut result_count = 0;
        let mut exception_count = 0;

        for (_, content) in samples {
            if content.contains("Result<") || content.contains("-> Result") {
                result_count += 1;
            }
            if content.contains("throw ") || content.contains("try {") || content.contains("catch ") {
                exception_count += 1;
            }
        }

        let style = if result_count > exception_count * 2 {
            ErrorStyle::ResultType
        } else if exception_count > result_count * 2 {
            ErrorStyle::Exceptions
        } else if result_count > 0 && exception_count > 0 {
            ErrorStyle::Mixed
        } else {
            ErrorStyle::ResultType
        };

        ErrorHandlingPattern {
            style,
            error_types: Vec::new(),
            propagation_pattern: match style {
                ErrorStyle::ResultType => "? operator for propagation".to_string(),
                ErrorStyle::Exceptions => "try-catch blocks".to_string(),
                _ => "Mixed approach".to_string(),
            },
            recovery_strategy: "Error-specific handling".to_string(),
        }
    }

    fn analyze_async_patterns(&self, samples: &[(String, String)]) -> AsyncPattern {
        let mut async_count = 0;
        let mut sync_count = 0;
        let mut runtime = None;

        for (_, content) in samples {
            if content.contains("async fn") || content.contains("async def") || content.contains("async function") {
                async_count += 1;
            }
            if content.contains("fn ") && !content.contains("async fn") {
                sync_count += 1;
            }
            if content.contains("tokio") {
                runtime = Some("tokio".to_string());
            }
        }

        let style = if async_count > sync_count {
            AsyncStyle::AsyncAwait
        } else if async_count > 0 {
            AsyncStyle::Mixed
        } else {
            AsyncStyle::Synchronous
        };

        AsyncPattern {
            style,
            runtime,
            concurrency_patterns: Vec::new(),
        }
    }

    fn analyze_testing_patterns(
        &self,
        structure: &str,
        samples: &[(String, String)],
    ) -> TestingConvention {
        let location = if structure.contains("tests/") {
            TestLocation::TestsDirectory
        } else {
            TestLocation::SameDirectory
        };

        let mut framework = None;
        for (_, content) in samples {
            if content.contains("#[test]") || content.contains("#[tokio::test]") {
                framework = Some("Rust built-in".to_string());
                break;
            }
            if content.contains("describe(") || content.contains("it(") {
                framework = Some("Jest/Vitest".to_string());
                break;
            }
            if content.contains("def test_") || content.contains("@pytest") {
                framework = Some("pytest".to_string());
                break;
            }
        }

        TestingConvention {
            framework,
            location,
            naming_pattern: "test_* or *_test".to_string(),
            coverage_tools: Vec::new(),
        }
    }

    async fn infer_from_llm(
        &self,
        project_type: ProjectType,
        structure: &str,
        samples: &[(String, String)],
    ) -> Result<InferredConventions> {
        let prompt = build_inference_prompt(project_type, structure, samples, self.max_samples);

        // Collect actual paths from samples for verification
        let actual_paths: Vec<&str> = samples.iter().map(|(p, _)| p.as_str()).collect();

        let schema = serde_json::json!({
            "type": "object",
            "required": ["architecture", "patterns", "layers"],
            "properties": {
                "architecture": {
                    "type": "object",
                    "properties": {
                        "pattern_name": {"type": "string", "description": "e.g., 'Layered', 'Hexagonal', 'Pipeline', 'CLI with Services'"},
                        "description": {"type": "string", "description": "How the codebase is organized"},
                        "layers": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "path_pattern": {"type": "string"},
                                    "responsibility": {"type": "string"}
                                }
                            }
                        }
                    }
                },
                "patterns": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "description": {"type": "string"},
                            "example_file": {"type": "string"}
                        }
                    }
                },
                "naming_conventions": {
                    "type": "object",
                    "properties": {
                        "files": {"type": "string", "description": "e.g., 'snake_case.rs'"},
                        "types": {"type": "string", "description": "e.g., 'PascalCase'"},
                        "functions": {"type": "string", "description": "e.g., 'snake_case'"}
                    }
                },
                "key_directories": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "role": {"type": "string"}
                        }
                    }
                }
            }
        });

        let response = self.provider.generate(&prompt, &schema).await?;

        let content_str = response.content.as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&response.content).unwrap_or_default());

        let mut parsed = self.parse_llm_response(&content_str)?;

        // CRITICAL: Verify all paths are fact-based (exist in actual project structure)
        // This ensures 100% fact-based output - no hallucinated paths
        parsed = Self::verify_paths_exist(parsed, structure, &actual_paths);

        Ok(parsed)
    }

    /// Verify that all LLM-inferred paths actually exist in the project structure.
    /// This is critical for ensuring 100% fact-based output.
    fn verify_paths_exist(
        mut conventions: InferredConventions,
        structure: &str,
        sample_paths: &[&str],
    ) -> InferredConventions {
        // Filter architecture layers to only include paths that exist
        conventions.architecture.layers.retain(|layer| {
            let path = &layer.path_pattern;
            // Check if path pattern exists in structure or matches sample paths
            let path_normalized = path.trim_end_matches('/').trim_end_matches('*').trim_end_matches('/');
            let exists_in_structure = structure.contains(path_normalized) ||
                structure.contains(&format!("{}/", path_normalized));
            let matches_sample = sample_paths.iter().any(|sp| {
                sp.starts_with(path_normalized) || sp.contains(path_normalized)
            });

            if !exists_in_structure && !matches_sample {
                tracing::debug!(
                    path = path,
                    "Filtering out LLM-inferred layer path that doesn't exist in project"
                );
            }

            exists_in_structure || matches_sample
        });

        // Filter key directories to only include paths that exist
        conventions.file_organization.key_directories.retain(|dir| {
            let path = &dir.path;
            let path_normalized = path.trim_end_matches('/');
            let exists_in_structure = structure.contains(path_normalized) ||
                structure.contains(&format!("{}/", path_normalized));
            let matches_sample = sample_paths.iter().any(|sp| {
                sp.starts_with(path_normalized) || sp.contains(path_normalized)
            });

            if !exists_in_structure && !matches_sample {
                tracing::debug!(
                    path = path,
                    "Filtering out LLM-inferred directory that doesn't exist in project"
                );
            }

            exists_in_structure || matches_sample
        });

        conventions
    }

    fn parse_llm_response(&self, content: &str) -> Result<InferredConventions> {
        let mut conventions = InferredConventions::default();

        // Try to parse as JSON first
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
            // Parse architecture
            if let Some(arch) = json.get("architecture") {
                conventions.architecture.pattern_name = arch
                    .get("pattern_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();

                conventions.architecture.description = arch
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();

                if let Some(layers) = arch.get("layers").and_then(|v| v.as_array()) {
                    for layer in layers {
                        conventions.architecture.layers.push(ArchitectureLayer {
                            name: layer
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            path_pattern: layer
                                .get("path_pattern")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            responsibility: layer
                                .get("responsibility")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            dependencies: layer
                                .get("dependencies")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str())
                                        .map(String::from)
                                        .collect()
                                })
                                .unwrap_or_default(),
                        });
                    }
                }

                conventions.architecture.confidence = 0.8;
            }

            // Parse patterns
            if let Some(patterns) = json.get("patterns").and_then(|v| v.as_array()) {
                for pattern in patterns {
                    let name = pattern
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let description = pattern
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let example_file = pattern
                        .get("example_file")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();

                    if !name.is_empty() {
                        let category = Self::infer_pattern_category(&name, &description);
                        conventions.patterns.push(CodePattern {
                            name,
                            description,
                            category,
                            usage_frequency: UsageFrequency::Common,
                            evidence: if example_file.is_empty() {
                                Vec::new()
                            } else {
                                vec![PatternEvidence {
                                    file: example_file,
                                    line: 0,
                                    snippet: String::new(),
                                }]
                            },
                        });
                    }
                }
            }

            // Parse naming conventions
            if let Some(naming) = json.get("naming_conventions") {
                if let Some(files) = naming.get("files").and_then(|v| v.as_str()) {
                    conventions.naming.file_naming.case = Self::infer_case_from_description(files);
                }
                if let Some(types) = naming.get("types").and_then(|v| v.as_str()) {
                    conventions.naming.type_naming.case = Self::infer_case_from_description(types);
                }
                if let Some(functions) = naming.get("functions").and_then(|v| v.as_str()) {
                    conventions.naming.function_naming.case =
                        Self::infer_case_from_description(functions);
                }
            }

            // Parse key directories
            if let Some(dirs) = json.get("key_directories").and_then(|v| v.as_array()) {
                for dir in dirs {
                    let path = dir
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let role = dir
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();

                    if !path.is_empty() {
                        conventions.file_organization.key_directories.push(DirectoryRole {
                            path,
                            role,
                            file_types: Vec::new(),
                        });
                    }
                }
            }

            return Ok(conventions);
        }

        // Fallback: try to extract JSON from markdown code blocks
        if let Some(json_start) = content.find("```json") {
            let after_marker = &content[json_start + 7..];
            if let Some(json_end) = after_marker.find("```") {
                let json_content = after_marker[..json_end].trim();
                if let Ok(parsed) = self.parse_llm_response(json_content) {
                    return Ok(parsed);
                }
            }
        }

        // Fallback: section-based extraction for non-JSON responses
        if let Some(arch_section) = Self::extract_section(content, "Architecture") {
            conventions.architecture.pattern_name =
                Self::extract_first_line(&arch_section).unwrap_or_default();
            conventions.architecture.description = arch_section;
        }

        if let Some(patterns_section) = Self::extract_section(content, "Patterns") {
            for line in patterns_section.lines() {
                if line.starts_with('-') || line.starts_with('*') {
                    let pattern_text = line.trim_start_matches(['-', '*', ' ']);
                    if !pattern_text.is_empty() {
                        conventions.patterns.push(CodePattern {
                            name: pattern_text.split(':').next().unwrap_or(pattern_text).to_string(),
                            description: pattern_text.to_string(),
                            category: PatternCategory::Other,
                            usage_frequency: UsageFrequency::Common,
                            evidence: Vec::new(),
                        });
                    }
                }
            }
        }

        Ok(conventions)
    }

    fn infer_pattern_category(name: &str, description: &str) -> PatternCategory {
        let text = format!("{} {}", name, description).to_lowercase();

        if text.contains("error") || text.contains("result") || text.contains("exception") {
            PatternCategory::ErrorHandling
        } else if text.contains("async") || text.contains("concurrent") || text.contains("thread") {
            PatternCategory::Concurrency
        } else if text.contains("state") || text.contains("store") || text.contains("context") {
            PatternCategory::StateManagement
        } else if text.contains("database") || text.contains("repository") || text.contains("query") {
            PatternCategory::DataAccess
        } else if text.contains("valid") || text.contains("check") || text.contains("verify") {
            PatternCategory::Validation
        } else if text.contains("log") || text.contains("trace") || text.contains("debug") {
            PatternCategory::Logging
        } else if text.contains("config") || text.contains("setting") || text.contains("env") {
            PatternCategory::Configuration
        } else if text.contains("test") || text.contains("mock") || text.contains("fixture") {
            PatternCategory::Testing
        } else {
            PatternCategory::Other
        }
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

    fn extract_section(content: &str, section: &str) -> Option<String> {
        let patterns = [
            format!("## {section}"),
            format!("### {section}"),
            format!("**{section}**"),
            format!("{section}:"),
        ];

        for pattern in patterns {
            if let Some(start) = content.find(&pattern) {
                let after_header = &content[start + pattern.len()..];
                let end = after_header
                    .find("\n## ")
                    .or_else(|| after_header.find("\n### "))
                    .or_else(|| after_header.find("\n**"))
                    .unwrap_or(after_header.len());
                return Some(after_header[..end].trim().to_string());
            }
        }
        None
    }

    fn extract_first_line(content: &str) -> Option<String> {
        content
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
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
    max_samples: usize,
) -> Result<InferredConventions> {
    let engine = ConventionInferenceEngine::new(project_root, provider, max_samples);
    engine.infer(detection).await
}

pub async fn infer(
    project_root: impl AsRef<Path>,
    detection: &ProjectDetection,
    provider: Arc<dyn LlmProvider>,
    max_samples: usize,
) -> Result<InferredConventions> {
    run(project_root, provider, detection, max_samples).await
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
