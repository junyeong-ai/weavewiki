//! Deep Analyzer - Multi-Agent Codebase Analysis
//!
//! Performs thorough analysis by actually reading code files and extracting
//! project-specific insights that enable high-value generation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::ai::LlmProvider;
use crate::ai::response::generate_schema;
use crate::ai::validation::deserialize_llm_response;
use crate::config::{AnalysisConfig, DeepAnalysisConfig, ProjectType};
use crate::pipeline::phases::ProjectDetection;
use crate::types::{Result, Severity};

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct DeepAnalysisResult {
    pub structure: SemanticStructure,
    pub patterns: Vec<PatternInstance>,
    pub constraints: Vec<DiscoveredConstraint>,
    pub dependencies: Vec<ModuleDependency>,
    pub insights: Vec<FileInsight>,
    pub key_abstractions: Vec<KeyAbstraction>,
    /// Quality metrics for the analysis itself
    pub analysis_quality: AnalysisQuality,
}

/// Metrics measuring the quality and completeness of the analysis
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct AnalysisQuality {
    /// Number of files actually read and analyzed
    pub files_analyzed: usize,
    /// Total lines of code analyzed
    pub lines_analyzed: usize,
    /// Percentage of source files covered (0.0 - 1.0)
    pub coverage_ratio: f32,
    /// Number of distinct evidence references
    pub evidence_count: usize,
    /// Number of validated file references (files that exist)
    pub validated_refs: usize,
    /// Number of invalid/hallucinated references filtered out
    pub filtered_hallucinations: usize,
    /// Overall confidence score (0.0 - 1.0)
    pub confidence_score: f32,
}

/// Raw value breakdown for LLM contextual assessment.
///
/// Provides counts without arbitrary weighting so LLM can apply
/// domain-specific judgment. For example, 2 security constraints
/// in crypto code may be more valuable than 10 patterns in a web app.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct ValueBreakdown {
    pub pattern_count: usize,
    pub constraint_count: usize,
    pub high_value_constraint_count: usize,
    pub gotcha_count: usize,
    pub insight_count: usize,
    pub evidence_count: usize,
    pub validated_refs: usize,
    pub filtered_hallucinations: usize,
    pub confidence_score: f32,
}

impl DeepAnalysisResult {
    /// Quick heuristic score for logging/tracing only.
    ///
    /// **Do not use for quality decisions** - use `value_breakdown()` instead
    /// and let LLM assess value based on domain context.
    ///
    /// Thresholds (0/5 items → 0.3/0.5/0.7):
    /// - 0 items: 0.3 (minimal but not zero - file may have implicit value)
    /// - 1-4 items: 0.5 (some findings, uncertain significance)
    /// - 5+ items: 0.7 (multiple findings, likely valuable)
    ///
    /// These are rough heuristics for tracing output only.
    pub fn value_score(&self) -> f32 {
        let breakdown = self.value_breakdown();
        let total = breakdown.pattern_count
            + breakdown.high_value_constraint_count
            + breakdown.gotcha_count;

        if total == 0 {
            0.3
        } else if total < 5 {
            0.5
        } else {
            0.7
        }
    }

    /// Get raw value breakdown for LLM contextual assessment.
    ///
    /// Provides counts without arbitrary weighting so LLM can apply
    /// domain-specific judgment (e.g., 2 security constraints in crypto
    /// code may be more valuable than 10 patterns in a web app).
    pub fn value_breakdown(&self) -> ValueBreakdown {
        let high_value_constraint_count = self
            .constraints
            .iter()
            .filter(|c| {
                matches!(
                    c.kind,
                    ConstraintKind::HiddenDependency | ConstraintKind::AntiPattern
                )
            })
            .count();

        let gotcha_count: usize = self.insights.iter().map(|i| i.gotchas.len()).sum();

        ValueBreakdown {
            pattern_count: self.patterns.len(),
            constraint_count: self.constraints.len(),
            high_value_constraint_count,
            gotcha_count,
            insight_count: self.insights.len(),
            evidence_count: self.analysis_quality.evidence_count,
            validated_refs: self.analysis_quality.validated_refs,
            filtered_hallucinations: self.analysis_quality.filtered_hallucinations,
            confidence_score: self.analysis_quality.confidence_score,
        }
    }

    /// Check if analysis has minimum evidence for confidence.
    ///
    /// Threshold rationale:
    /// - `evidence_count >= 3`: Single evidence point could be anomaly, two could
    ///   be coincidence, three suggests a pattern. Aligns with "three strikes" rule
    ///   common in pattern detection.
    /// - `validated_refs > filtered_hallucinations`: More real than fabricated
    ///   references indicates the analysis is grounded in actual code.
    pub fn has_sufficient_evidence(&self) -> bool {
        const MIN_EVIDENCE_FOR_PATTERN: usize = 3;
        self.analysis_quality.evidence_count >= MIN_EVIDENCE_FOR_PATTERN
            && self.analysis_quality.validated_refs > self.analysis_quality.filtered_hallucinations
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct SemanticStructure {
    pub entry_points: Vec<EntryPoint>,
    pub core_modules: Vec<CoreModule>,
    pub layer_boundaries: Vec<LayerBoundary>,
    pub config_locations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EntryPoint {
    pub path: String,
    pub kind: EntryPointKind,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryPointKind {
    Main,
    LibRoot,
    ApiHandler,
    CliCommand,
    Test,
    // Extended types for diverse frameworks
    WebServer,
    Worker,
    Lambda,
    Plugin,
    Middleware,
    /// Catch-all for custom entry point types
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CoreModule {
    pub path: String,
    pub name: String,
    pub responsibility: String,
    pub public_items: Vec<String>,
    pub internal_deps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LayerBoundary {
    pub from_layer: String,
    pub to_layer: String,
    pub allowed: bool,
    pub evidence: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct PatternInstance {
    pub name: String,
    pub category: PatternCategory,
    pub description: String,
    pub locations: Vec<PatternLocation>,
    pub usage_guidance: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PatternCategory {
    Architecture,
    ErrorHandling,
    Concurrency,
    DataFlow,
    Testing,
    Configuration,
    Logging,
    // Extended categories
    Performance,
    Security,
    Caching,
    Validation,
    /// Catch-all for custom pattern categories
    #[default]
    #[serde(other)]
    Other,
}

impl std::fmt::Display for PatternCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PatternLocation {
    pub file: String,
    pub line: u32,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveredConstraint {
    pub kind: ConstraintKind,
    pub title: String,
    pub description: String,
    pub rationale: String,
    pub evidence: Vec<ConstraintEvidence>,
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    AntiPattern,
    HiddenDependency,
    Invariant,
    WorkflowRequirement,
    NamingConvention,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConstraintEvidence {
    pub file: String,
    pub line: Option<u32>,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModuleDependency {
    pub from_module: String,
    pub to_module: String,
    pub dependency_type: DependencyType,
    pub is_public: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependencyType {
    Import,
    Inheritance,
    Composition,
    RuntimeCall,
    Configuration,
    // Extended types
    EventDriven,
    DependencyInjection,
    Plugin,
    Async,
    /// Catch-all for custom dependency types
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileInsight {
    pub file: String,
    pub purpose: String,
    pub key_exports: Vec<String>,
    pub notable_patterns: Vec<String>,
    pub gotchas: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KeyAbstraction {
    pub name: String,
    pub kind: AbstractionKind,
    pub file: String,
    pub line: u32,
    pub description: String,
    pub usage_notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AbstractionKind {
    Trait,
    Struct,
    Enum,
    Function,
    Macro,
    Interface,
    Class,
    Type,
    // Extended types for diverse languages
    Protocol, // Swift, Python
    Module,
    Namespace,
    Component, // React, Vue
    Service,
    Hook, // React
    /// Catch-all for custom abstraction types
    #[serde(other)]
    Other,
}

// File-level deep analysis types (consolidated from deep/ subdirectory)

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileDeepAnalysis {
    pub file_path: String,
    pub gotchas: Vec<Gotcha>,
    pub constraints: Vec<FileConstraint>,
    pub patterns: Vec<CodePattern>,
    pub relationships: Vec<Relationship>,
    pub summary: String,
    pub analyzed_at: DateTime<Utc>,
}

impl FileDeepAnalysis {
    pub fn high_severity_gotchas(&self) -> Vec<&Gotcha> {
        self.gotchas
            .iter()
            .filter(|g| matches!(g.severity, Severity::High | Severity::Critical))
            .collect()
    }

    pub fn has_critical_constraints(&self) -> bool {
        !self.constraints.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Gotcha {
    pub description: String,
    #[serde(default)]
    pub lines: Vec<usize>,
    pub severity: Severity,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileConstraint {
    pub description: String,
    #[serde(default)]
    pub lines: Vec<usize>,
    #[serde(default)]
    pub enforcement: Option<ConstraintEnforcement>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintEnforcement {
    CompileTime,
    Runtime,
    Convention,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodePattern {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub example_lines: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Relationship {
    pub target: String,
    pub relationship_type: String,
    #[serde(default)]
    pub description: Option<String>,
}

pub struct DeepAnalyzer {
    project_root: PathBuf,
    provider: Arc<dyn LlmProvider>,
    analysis_config: AnalysisConfig,
    deep_config: DeepAnalysisConfig,
}

impl DeepAnalyzer {
    pub fn new(
        project_root: impl AsRef<Path>,
        provider: Arc<dyn LlmProvider>,
        analysis_config: AnalysisConfig,
        deep_config: DeepAnalysisConfig,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            provider,
            analysis_config,
            deep_config,
        }
    }

    pub async fn analyze(&self, detection: &ProjectDetection) -> Result<DeepAnalysisResult> {
        if !self.deep_config.enabled {
            return Ok(DeepAnalysisResult::default());
        }

        tracing::info!("Starting deep analysis with code reading");

        let file_map = self.collect_key_files(detection).await?;
        tracing::debug!(files = file_map.len(), "Collected key files for analysis");

        let structure = self.analyze_structure(detection, &file_map).await?;
        let patterns = self.extract_patterns(detection, &file_map).await?;
        let constraints = self.discover_constraints(detection, &file_map).await?;
        let dependencies = self.map_dependencies(&file_map).await?;
        let insights = self.generate_insights(detection, &file_map).await?;
        let key_abstractions = self.identify_abstractions(detection, &file_map).await?;

        // Calculate analysis quality metrics
        let analysis_quality = self.calculate_analysis_quality(
            &file_map,
            &patterns,
            &constraints,
            &insights,
            detection,
        );

        let result = DeepAnalysisResult {
            structure,
            patterns,
            constraints,
            dependencies,
            insights,
            key_abstractions,
            analysis_quality,
        };

        tracing::info!(
            patterns = result.patterns.len(),
            constraints = result.constraints.len(),
            insights = result.insights.len(),
            abstractions = result.key_abstractions.len(),
            value_score = result.value_score(),
            confidence = result.analysis_quality.confidence_score,
            "Deep analysis complete"
        );

        Ok(result)
    }

    /// Calculate quality metrics for the analysis
    fn calculate_analysis_quality(
        &self,
        file_map: &HashMap<String, FileContent>,
        patterns: &[PatternInstance],
        constraints: &[DiscoveredConstraint],
        _insights: &[FileInsight],
        detection: &ProjectDetection,
    ) -> AnalysisQuality {
        // Files and lines analyzed
        let files_analyzed = file_map.len();
        let lines_analyzed: usize = file_map.values().map(|f| f.lines).sum();

        // Estimate total source files for coverage calculation
        let estimated_total = detection
            .languages
            .iter()
            .map(|l| l.file_count)
            .sum::<usize>()
            .max(1);
        let coverage_ratio = (files_analyzed as f32 / estimated_total as f32).min(1.0);

        // Count and validate distinct evidence references
        let mut validated_refs = 0;
        let mut filtered_hallucinations = 0;

        // Validate pattern location references
        for pattern in patterns {
            for location in &pattern.locations {
                if file_map.contains_key(&location.file) || self.file_exists_sync(&location.file) {
                    validated_refs += 1;
                } else {
                    filtered_hallucinations += 1;
                }
            }
        }

        // Validate constraint evidence references
        for constraint in constraints {
            for ev in &constraint.evidence {
                if file_map.contains_key(&ev.file) || self.file_exists_sync(&ev.file) {
                    validated_refs += 1;
                } else {
                    filtered_hallucinations += 1;
                }
            }
        }

        let evidence_count = validated_refs + filtered_hallucinations;

        let confidence = if evidence_count == 0 {
            coverage_ratio * 0.5
        } else {
            let validity_ratio = validated_refs as f32 / evidence_count as f32;
            (coverage_ratio + validity_ratio) / 2.0
        };

        AnalysisQuality {
            files_analyzed,
            lines_analyzed,
            coverage_ratio,
            evidence_count,
            validated_refs,
            filtered_hallucinations,
            confidence_score: confidence.min(1.0),
        }
    }

    async fn collect_key_files(
        &self,
        detection: &ProjectDetection,
    ) -> Result<HashMap<String, FileContent>> {
        let mut files = HashMap::new();
        let max_files = self.analysis_config.max_file_samples;
        let max_chars = self.deep_config.max_code_context_chars;

        let priority_patterns = self.get_priority_patterns(detection.primary_type);

        for pattern in &priority_patterns {
            if files.len() >= max_files {
                break;
            }
            self.collect_matching_files(&self.project_root, pattern, &mut files, max_chars)
                .await?;
        }

        if files.len() < max_files {
            self.collect_source_files(
                &self.project_root,
                detection,
                &mut files,
                max_files,
                max_chars,
            )
            .await?;
        }

        Ok(files)
    }

    fn get_priority_patterns(&self, project_type: ProjectType) -> Vec<&'static str> {
        let mut patterns = vec![
            "src/main.rs",
            "src/lib.rs",
            "src/mod.rs",
            "main.rs",
            "lib.rs",
            "index.ts",
            "index.js",
            "main.py",
            "__init__.py",
            "main.go",
        ];

        match project_type {
            ProjectType::Cli => {
                patterns.extend(["src/cli/mod.rs", "src/commands/mod.rs", "cli/mod.rs"]);
            }
            ProjectType::Backend => {
                patterns.extend([
                    "src/api/mod.rs",
                    "src/routes/mod.rs",
                    "src/handlers/mod.rs",
                    "src/domain/mod.rs",
                ]);
            }
            ProjectType::Frontend => {
                patterns.extend(["src/App.tsx", "src/App.vue", "src/pages/index.tsx"]);
            }
            ProjectType::Library => {
                patterns.extend(["src/lib.rs", "src/index.ts", "src/__init__.py"]);
            }
            _ => {}
        }

        patterns
    }

    async fn collect_matching_files(
        &self,
        dir: &Path,
        pattern: &str,
        files: &mut HashMap<String, FileContent>,
        max_chars: usize,
    ) -> Result<()> {
        let path = dir.join(pattern);
        if path.exists()
            && path.is_file()
            && let Ok(content) = fs::read_to_string(&path).await
        {
            let lines = content.lines().count();
            let truncated = if content.len() > max_chars {
                // Find valid UTF-8 char boundary
                let mut end = max_chars;
                while end > 0 && !content.is_char_boundary(end) {
                    end -= 1;
                }
                content[..end].to_string()
            } else {
                content
            };
            let relative = path
                .strip_prefix(&self.project_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| pattern.to_string());
            files.insert(
                relative,
                FileContent {
                    content: truncated,
                    lines,
                },
            );
        }
        Ok(())
    }

    async fn collect_source_files(
        &self,
        dir: &Path,
        detection: &ProjectDetection,
        files: &mut HashMap<String, FileContent>,
        max_files: usize,
        max_chars: usize,
    ) -> Result<()> {
        let extensions = self.get_source_extensions(detection);
        self.collect_files_recursive(dir, &extensions, files, max_files, max_chars, 0)
            .await
    }

    /// Get source file extensions based on detected languages.
    /// Returns extensions for ALL detected languages, not just the primary one.
    /// Fallback includes comprehensive list of common source file extensions.
    fn get_source_extensions(&self, detection: &ProjectDetection) -> Vec<&'static str> {
        let mut extensions = Vec::new();

        // Add extensions for all detected languages
        for lang_info in &detection.languages {
            let lang_exts = match lang_info.language.to_lowercase().as_str() {
                "rust" => vec!["rs"],
                "typescript" => vec!["ts", "tsx", "mts", "cts"],
                "javascript" => vec!["js", "jsx", "mjs", "cjs"],
                "python" => vec!["py", "pyi"],
                "go" => vec!["go"],
                "kotlin" => vec!["kt", "kts"],
                "java" => vec!["java"],
                "c#" | "csharp" => vec!["cs"],
                "c++" | "cpp" => vec!["cpp", "cc", "cxx", "hpp", "h"],
                "c" => vec!["c", "h"],
                "swift" => vec!["swift"],
                "ruby" => vec!["rb"],
                "php" => vec!["php"],
                "scala" => vec!["scala", "sc"],
                "elixir" => vec!["ex", "exs"],
                "clojure" => vec!["clj", "cljs", "cljc"],
                "haskell" => vec!["hs"],
                "lua" => vec!["lua"],
                "dart" => vec!["dart"],
                "vue" => vec!["vue"],
                "svelte" => vec!["svelte"],
                _ => vec![],
            };
            for ext in lang_exts {
                if !extensions.contains(&ext) {
                    extensions.push(ext);
                }
            }
        }

        // If no languages detected, use comprehensive fallback
        if extensions.is_empty() {
            extensions = vec![
                "rs", "ts", "tsx", "js", "jsx", "py", "go", "kt", "java", "cs", "cpp", "c",
                "swift", "rb", "php", "scala",
            ];
        }

        extensions
    }

    /// Collect source files using .gitignore-aware walking.
    /// Uses `ignore` crate's WalkBuilder for proper .gitignore handling.
    async fn collect_files_recursive(
        &self,
        dir: &Path,
        extensions: &[&str],
        files: &mut HashMap<String, FileContent>,
        max_files: usize,
        max_chars: usize,
        _depth: usize, // Kept for API compatibility, WalkBuilder handles depth
    ) -> Result<()> {
        use ignore::WalkBuilder;

        // Use ignore crate for .gitignore-aware walking
        // This automatically respects:
        // - .gitignore patterns
        // - .git/info/exclude
        // - Global gitignore (~/.gitignore)
        let walker = WalkBuilder::new(dir)
            .hidden(true) // Skip hidden by default
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .follow_links(false)
            .max_depth(Some(15)) // Allow deep monorepo structures
            .build();

        for entry in walker.filter_map(|e| e.ok()) {
            if files.len() >= max_files {
                break;
            }

            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // Check extension
            match path.extension().and_then(|e| e.to_str()) {
                Some(ext) if extensions.contains(&ext) => {}
                _ => continue,
            };

            // Read file content
            let content = match fs::read_to_string(path).await {
                Ok(c) if c.len() <= self.analysis_config.max_file_size => c,
                _ => continue,
            };

            let truncated = if content.len() > max_chars {
                // Find valid UTF-8 char boundary
                let mut end = max_chars;
                while end > 0 && !content.is_char_boundary(end) {
                    end -= 1;
                }
                content[..end].to_string()
            } else {
                content.clone()
            };

            let relative = path
                .strip_prefix(&self.project_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            files.insert(
                relative,
                FileContent {
                    content: truncated,
                    lines: content.lines().count(),
                },
            );
        }

        Ok(())
    }

    async fn analyze_structure(
        &self,
        detection: &ProjectDetection,
        files: &HashMap<String, FileContent>,
    ) -> Result<SemanticStructure> {
        let file_list: Vec<&str> = files.keys().map(|s| s.as_str()).collect();
        let sample_contents = self.build_sample_context(files, 5);

        let prompt = format!(
            r#"Analyze the structure of this {project_type} project.

Files: {files}

Sample code:
{samples}

Return JSON:
{{
  "entry_points": [
    {{"path": "src/main.rs", "kind": "main", "description": "CLI entry point"}}
  ],
  "core_modules": [
    {{"path": "src/pipeline/", "name": "pipeline", "responsibility": "...", "public_items": ["AdaptivePipeline"], "internal_deps": ["ai", "config"]}}
  ],
  "layer_boundaries": [
    {{"from_layer": "cli", "to_layer": "pipeline", "allowed": true, "evidence": "cli/commands imports pipeline"}}
  ],
  "config_locations": ["src/config/types.rs", ".claudegen/config.toml"]
}}

Be specific to THIS project's actual structure."#,
            project_type = detection.primary_type,
            files = file_list.join(", "),
            samples = sample_contents
        );

        let schema = generate_schema::<StructureOutput>();

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let output: StructureOutput =
                    deserialize_llm_response(&response.content, "structure")?;
                Ok(self.convert_structure_output(output))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Structure analysis failed, using fallback");
                Ok(SemanticStructure::default())
            }
        }
    }

    fn convert_structure_output(&self, output: StructureOutput) -> SemanticStructure {
        let entry_points = output
            .entry_points
            .into_iter()
            .map(|e| {
                let kind = match e.kind.as_str() {
                    "lib_root" => EntryPointKind::LibRoot,
                    "api_handler" => EntryPointKind::ApiHandler,
                    "cli_command" => EntryPointKind::CliCommand,
                    "test" => EntryPointKind::Test,
                    _ => EntryPointKind::Main,
                };
                EntryPoint {
                    path: e.path,
                    kind,
                    description: e.description,
                }
            })
            .collect();

        let core_modules = output
            .core_modules
            .into_iter()
            .map(|m| CoreModule {
                path: m.path,
                name: m.name,
                responsibility: m.responsibility,
                public_items: m.public_items,
                internal_deps: m.internal_deps,
            })
            .collect();

        let layer_boundaries = output
            .layer_boundaries
            .into_iter()
            .map(|b| LayerBoundary {
                from_layer: b.from_layer,
                to_layer: b.to_layer,
                allowed: b.allowed,
                evidence: b.evidence,
            })
            .collect();

        SemanticStructure {
            entry_points,
            core_modules,
            layer_boundaries,
            config_locations: output.config_locations,
        }
    }

    async fn extract_patterns(
        &self,
        detection: &ProjectDetection,
        files: &HashMap<String, FileContent>,
    ) -> Result<Vec<PatternInstance>> {
        let sample_contents = self.build_sample_context(files, 8);

        let prompt = format!(
            r#"Extract code patterns from this {project_type} project.

{samples}

Find ACTUAL patterns in the code above. Return JSON:
{{
  "patterns": [
    {{
      "name": "Result-Based Error Propagation",
      "category": "error_handling",
      "description": "All fallible operations return Result<T, Error> and use ? for propagation",
      "locations": [{{"file": "src/pipeline/adaptive.rs", "line": 75, "snippet": "pub async fn run(&self) -> Result<...>"}}],
      "usage_guidance": "Always use Result<T, ClaudegenError> for fallible operations, propagate with ?"
    }}
  ]
}}

Categories: architecture, error_handling, concurrency, data_flow, testing, configuration, logging
ONLY include patterns you can see evidence for in the code."#,
            project_type = detection.primary_type,
            samples = sample_contents
        );

        let schema = generate_schema::<PatternsOutput>();

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let output: PatternsOutput =
                    deserialize_llm_response(&response.content, "patterns")?;
                Ok(self.convert_patterns_output(output, files))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Pattern extraction failed");
                Ok(Vec::new())
            }
        }
    }

    fn convert_patterns_output(
        &self,
        output: PatternsOutput,
        files: &HashMap<String, FileContent>,
    ) -> Vec<PatternInstance> {
        output
            .patterns
            .into_iter()
            .filter_map(|p| {
                if p.name.is_empty() {
                    return None;
                }

                let category = match p.category.as_str() {
                    "architecture" => PatternCategory::Architecture,
                    "error_handling" => PatternCategory::ErrorHandling,
                    "concurrency" => PatternCategory::Concurrency,
                    "data_flow" => PatternCategory::DataFlow,
                    "testing" => PatternCategory::Testing,
                    "configuration" => PatternCategory::Configuration,
                    "logging" => PatternCategory::Logging,
                    _ => PatternCategory::Architecture,
                };

                let locations: Vec<PatternLocation> = p
                    .locations
                    .into_iter()
                    .filter_map(|loc| {
                        if !files.contains_key(&loc.file) && !self.file_exists_sync(&loc.file) {
                            return None;
                        }
                        Some(PatternLocation {
                            file: loc.file,
                            line: loc.line,
                            snippet: loc.snippet,
                        })
                    })
                    .collect();

                if locations.is_empty() {
                    return None;
                }

                Some(PatternInstance {
                    name: p.name,
                    category,
                    description: p.description,
                    locations,
                    usage_guidance: p.usage_guidance,
                })
            })
            .collect()
    }

    fn file_exists_sync(&self, relative_path: &str) -> bool {
        self.project_root.join(relative_path).exists()
    }

    async fn discover_constraints(
        &self,
        detection: &ProjectDetection,
        files: &HashMap<String, FileContent>,
    ) -> Result<Vec<DiscoveredConstraint>> {
        let sample_contents = self.build_sample_context(files, 10);

        let prompt = format!(
            r#"Discover hidden constraints in this {project_type} codebase.

{samples}

Find constraints that would surprise a new developer. Return JSON:
{{
  "constraints": [
    {{
      "kind": "anti_pattern",
      "title": "Direct stdout in library code",
      "description": "Using println! in non-CLI modules breaks library usage",
      "rationale": "Library code must be usable as a dependency without side effects",
      "evidence": [{{"file": "src/pipeline/mod.rs", "line": 45, "context": "Uses tracing instead of println"}}],
      "severity": "high"
    }},
    {{
      "kind": "hidden_dependency",
      "title": "Pipeline phase ordering",
      "description": "Phase 4 (Constraint Extraction) depends on Phase 3 (Convention Inference) results",
      "rationale": "Conventions inform what constraints to look for",
      "evidence": [{{"file": "src/pipeline/adaptive.rs", "line": 110, "context": "extract_constraints takes conventions parameter"}}],
      "severity": "critical"
    }}
  ]
}}

Kinds: anti_pattern, hidden_dependency, invariant, workflow_requirement, naming_convention
Severity: critical, high, medium, low
ONLY include constraints with evidence from the actual code."#,
            project_type = detection.primary_type,
            samples = sample_contents
        );

        let schema = generate_schema::<ConstraintsOutput>();

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let output: ConstraintsOutput =
                    deserialize_llm_response(&response.content, "constraints")?;
                Ok(self.convert_constraints_output(output, files))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Constraint discovery failed");
                Ok(Vec::new())
            }
        }
    }

    fn convert_constraints_output(
        &self,
        output: ConstraintsOutput,
        files: &HashMap<String, FileContent>,
    ) -> Vec<DiscoveredConstraint> {
        output
            .constraints
            .into_iter()
            .filter(|c| !c.title.is_empty())
            .map(|c| {
                let kind = match c.kind.as_str() {
                    "anti_pattern" => ConstraintKind::AntiPattern,
                    "hidden_dependency" => ConstraintKind::HiddenDependency,
                    "invariant" => ConstraintKind::Invariant,
                    "workflow_requirement" => ConstraintKind::WorkflowRequirement,
                    "naming_convention" => ConstraintKind::NamingConvention,
                    _ => ConstraintKind::AntiPattern,
                };

                let severity = match c.severity.as_str() {
                    "critical" => Severity::Critical,
                    "high" => Severity::High,
                    "low" => Severity::Low,
                    _ => Severity::Medium,
                };

                let evidence = c
                    .evidence
                    .into_iter()
                    .filter_map(|e| {
                        if !files.contains_key(&e.file) && !self.file_exists_sync(&e.file) {
                            return None;
                        }
                        Some(ConstraintEvidence {
                            file: e.file,
                            line: if e.line > 0 { Some(e.line) } else { None },
                            context: e.context,
                        })
                    })
                    .collect();

                DiscoveredConstraint {
                    kind,
                    title: c.title,
                    description: c.description,
                    rationale: c.rationale,
                    evidence,
                    severity,
                }
            })
            .collect()
    }

    async fn map_dependencies(
        &self,
        files: &HashMap<String, FileContent>,
    ) -> Result<Vec<ModuleDependency>> {
        let mut deps = Vec::new();

        for (path, content) in files {
            let module_name = self.extract_module_name(path);

            for line in content.content.lines() {
                if let Some(import) = self.parse_import_line(line) {
                    deps.push(ModuleDependency {
                        from_module: module_name.clone(),
                        to_module: import,
                        dependency_type: DependencyType::Import,
                        is_public: line.contains("pub use"),
                    });
                }
            }
        }

        Ok(deps)
    }

    fn extract_module_name(&self, path: &str) -> String {
        path.trim_end_matches(".rs")
            .trim_end_matches(".ts")
            .trim_end_matches(".py")
            .trim_end_matches("/mod")
            .trim_end_matches("/index")
            .split('/')
            .next_back()
            .unwrap_or(path)
            .to_string()
    }

    fn parse_import_line(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();

        if trimmed.starts_with("use crate::") {
            return trimmed
                .strip_prefix("use crate::")
                .and_then(|s| s.split("::").next())
                .map(|s| s.trim_end_matches(';').to_string());
        }

        if trimmed.starts_with("use super::") {
            return trimmed
                .strip_prefix("use super::")
                .and_then(|s| s.split("::").next())
                .map(|s| s.trim_end_matches(';').to_string());
        }

        if trimmed.starts_with("from ") && trimmed.contains(" import ") {
            return trimmed
                .strip_prefix("from ")
                .and_then(|s| s.split(" import").next())
                .map(|s| s.trim().trim_matches('.').to_string());
        }

        if trimmed.starts_with("import ") && !trimmed.contains(" from ") {
            return trimmed
                .strip_prefix("import ")
                .map(|s| s.split('/').next().unwrap_or(s).trim().to_string());
        }

        None
    }

    async fn generate_insights(
        &self,
        detection: &ProjectDetection,
        files: &HashMap<String, FileContent>,
    ) -> Result<Vec<FileInsight>> {
        let mut insights = Vec::new();

        let key_files: Vec<_> = files
            .iter()
            .filter(|(path, _)| {
                path.contains("mod.rs")
                    || path.contains("lib.rs")
                    || path.contains("main.rs")
                    || path.contains("index.")
                    || path.contains("types.")
            })
            .collect();

        for (path, content) in key_files {
            let insight = self
                .analyze_single_file(detection, path, &content.content)
                .await?;
            if !insight.purpose.is_empty() {
                insights.push(insight);
            }
        }

        Ok(insights)
    }

    async fn analyze_single_file(
        &self,
        detection: &ProjectDetection,
        path: &str,
        content: &str,
    ) -> Result<FileInsight> {
        // Use configurable max chars (default 50,000, was hardcoded 3000)
        let max_chars = self.deep_config.max_code_context_chars;
        let truncated = if content.len() > max_chars {
            // Find valid UTF-8 char boundary
            let mut end = max_chars;
            while end > 0 && !content.is_char_boundary(end) {
                end -= 1;
            }
            &content[..end]
        } else {
            content
        };

        let prompt = format!(
            r#"Analyze this file from a {project_type} project.

File: {path}
```
{content}
```

Return JSON:
{{
  "purpose": "One-line description of what this file does",
  "key_exports": ["exported_function", "ExportedType"],
  "notable_patterns": ["Pattern used in this file"],
  "gotchas": ["Things that could trip up developers"]
}}

Be specific and concise. Only include notable findings."#,
            project_type = detection.primary_type,
            path = path,
            content = truncated
        );

        let schema = generate_schema::<FileInsightOutput>();

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let output: FileInsightOutput =
                    deserialize_llm_response(&response.content, "file_insight")?;
                Ok(FileInsight {
                    file: path.to_string(),
                    purpose: output.purpose,
                    key_exports: output.key_exports,
                    notable_patterns: output.notable_patterns,
                    gotchas: output.gotchas,
                })
            }
            Err(_) => Ok(FileInsight {
                file: path.to_string(),
                purpose: String::new(),
                key_exports: Vec::new(),
                notable_patterns: Vec::new(),
                gotchas: Vec::new(),
            }),
        }
    }

    async fn identify_abstractions(
        &self,
        detection: &ProjectDetection,
        files: &HashMap<String, FileContent>,
    ) -> Result<Vec<KeyAbstraction>> {
        let sample_contents = self.build_sample_context(files, 6);

        let prompt = format!(
            r#"Identify key abstractions in this {project_type} codebase.

{samples}

Find the most important types/traits/functions that define the project's API. Return JSON:
{{
  "abstractions": [
    {{
      "name": "LlmProvider",
      "kind": "trait",
      "file": "src/ai/provider/mod.rs",
      "line": 25,
      "description": "Core abstraction for all LLM interactions",
      "usage_notes": ["Implement for new providers", "Use Arc<dyn LlmProvider> for sharing"]
    }}
  ]
}}

Kinds: trait, struct, enum, function, macro, interface, class, type
Focus on abstractions that define the project's architecture."#,
            project_type = detection.primary_type,
            samples = sample_contents
        );

        let schema = generate_schema::<AbstractionsOutput>();

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let output: AbstractionsOutput =
                    deserialize_llm_response(&response.content, "abstractions")?;
                Ok(self.convert_abstractions_output(output, files))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Abstraction identification failed");
                Ok(Vec::new())
            }
        }
    }

    fn convert_abstractions_output(
        &self,
        output: AbstractionsOutput,
        files: &HashMap<String, FileContent>,
    ) -> Vec<KeyAbstraction> {
        output
            .abstractions
            .into_iter()
            .filter_map(|a| {
                if a.name.is_empty()
                    || (!files.contains_key(&a.file) && !self.file_exists_sync(&a.file))
                {
                    return None;
                }

                let kind = match a.kind.as_str() {
                    "trait" => AbstractionKind::Trait,
                    "enum" => AbstractionKind::Enum,
                    "function" => AbstractionKind::Function,
                    "macro" => AbstractionKind::Macro,
                    "interface" => AbstractionKind::Interface,
                    "class" => AbstractionKind::Class,
                    "type" => AbstractionKind::Type,
                    _ => AbstractionKind::Struct,
                };

                Some(KeyAbstraction {
                    name: a.name,
                    kind,
                    file: a.file,
                    line: a.line,
                    description: a.description,
                    usage_notes: a.usage_notes,
                })
            })
            .collect()
    }

    fn build_sample_context(
        &self,
        files: &HashMap<String, FileContent>,
        max_files: usize,
    ) -> String {
        let mut samples = String::new();
        let mut char_budget = self.deep_config.max_code_context_chars;

        for (path, content) in files.iter().take(max_files) {
            if char_budget == 0 {
                break;
            }

            let file_header = format!("\n--- {} ---\n", path);
            let available = char_budget.saturating_sub(file_header.len());
            let truncated = if content.content.len() > available {
                // Find valid UTF-8 char boundary
                let mut end = available;
                while end > 0 && !content.content.is_char_boundary(end) {
                    end -= 1;
                }
                &content.content[..end]
            } else {
                &content.content
            };

            samples.push_str(&file_header);
            samples.push_str(truncated);
            char_budget = char_budget.saturating_sub(file_header.len() + truncated.len());
        }

        samples
    }
}

struct FileContent {
    content: String,
    lines: usize,
}

// =============================================================================
// LLM OUTPUT TYPES (for schema generation)
// =============================================================================

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct StructureOutput {
    #[serde(default)]
    entry_points: Vec<EntryPointOutput>,
    #[serde(default)]
    core_modules: Vec<CoreModuleOutput>,
    #[serde(default)]
    layer_boundaries: Vec<LayerBoundaryOutput>,
    #[serde(default)]
    config_locations: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct EntryPointOutput {
    #[serde(default)]
    path: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct CoreModuleOutput {
    #[serde(default)]
    path: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    responsibility: String,
    #[serde(default)]
    public_items: Vec<String>,
    #[serde(default)]
    internal_deps: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct LayerBoundaryOutput {
    #[serde(default)]
    from_layer: String,
    #[serde(default)]
    to_layer: String,
    #[serde(default)]
    allowed: bool,
    #[serde(default)]
    evidence: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct PatternsOutput {
    #[serde(default)]
    patterns: Vec<PatternOutput>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct PatternOutput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    locations: Vec<LocationOutput>,
    #[serde(default)]
    usage_guidance: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct LocationOutput {
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    snippet: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct ConstraintsOutput {
    #[serde(default)]
    constraints: Vec<ConstraintOutput>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct ConstraintOutput {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    evidence: Vec<EvidenceOutput>,
    #[serde(default)]
    severity: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct EvidenceOutput {
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    context: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct FileInsightOutput {
    #[serde(default)]
    purpose: String,
    #[serde(default)]
    key_exports: Vec<String>,
    #[serde(default)]
    notable_patterns: Vec<String>,
    #[serde(default)]
    gotchas: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct AbstractionsOutput {
    #[serde(default)]
    abstractions: Vec<AbstractionOutput>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct AbstractionOutput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    description: String,
    #[serde(default)]
    usage_notes: Vec<String>,
}

pub async fn analyze(
    project_root: impl AsRef<Path>,
    provider: Arc<dyn LlmProvider>,
    analysis_config: &AnalysisConfig,
    deep_config: &DeepAnalysisConfig,
    detection: &ProjectDetection,
) -> Result<DeepAnalysisResult> {
    let analyzer = DeepAnalyzer::new(
        project_root,
        provider,
        analysis_config.clone(),
        deep_config.clone(),
    );
    analyzer.analyze(detection).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_name_extraction() {
        let analyzer = DeepAnalyzer {
            project_root: PathBuf::from("/test"),
            provider: Arc::new(MockProvider),
            analysis_config: AnalysisConfig::default(),
            deep_config: DeepAnalysisConfig::default(),
        };

        assert_eq!(
            analyzer.extract_module_name("src/pipeline/mod.rs"),
            "pipeline"
        );
        assert_eq!(analyzer.extract_module_name("src/types.rs"), "types");
        assert_eq!(
            analyzer.extract_module_name("src/ai/provider/index.ts"),
            "provider"
        );
    }

    #[test]
    fn test_import_parsing() {
        let analyzer = DeepAnalyzer {
            project_root: PathBuf::from("/test"),
            provider: Arc::new(MockProvider),
            analysis_config: AnalysisConfig::default(),
            deep_config: DeepAnalysisConfig::default(),
        };

        assert_eq!(
            analyzer.parse_import_line("use crate::config::Config;"),
            Some("config".to_string())
        );
        assert_eq!(
            analyzer.parse_import_line("use super::types::Result;"),
            Some("types".to_string())
        );
        assert_eq!(
            analyzer.parse_import_line("from . import utils"),
            Some("".to_string())
        );
        assert_eq!(analyzer.parse_import_line("let x = 5;"), None);
    }

    struct MockProvider;

    #[async_trait::async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(
            &self,
            _prompt: &str,
            _schema: &serde_json::Value,
        ) -> crate::types::Result<crate::ai::LlmResponse> {
            Ok(crate::ai::LlmResponse::content_only(serde_json::json!({})))
        }

        fn name(&self) -> &str {
            "mock"
        }

        fn model(&self) -> &str {
            "mock-model"
        }

        async fn health_check(&self) -> crate::types::Result<bool> {
            Ok(true)
        }
    }
}
