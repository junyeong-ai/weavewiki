//! Pipeline Context Module
//!
//! Provides verified file registry and project context for refinement pipeline.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::AnalysisConfig;
use crate::types::Result;

use super::analysis::{DeepAnalysisResult, SynthesizedInsights};
use super::phases::{
    constraint_extraction::ExtractedConstraints, convention_inference::InferredConventions,
    project_detection::ProjectDetection,
};
use crate::types::domain::DomainAnalysisResult;
use crate::types::module_map::DetectedModule;

/// Extracted constraint (Tier3 - essential)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedConstraint {
    pub name: String,
    pub description: String,
    pub evidence: Vec<String>,
    pub gotcha: Option<String>,
}

/// Aggregated analysis results from pipeline phases
#[derive(Debug, Clone, Default)]
pub struct AnalysisResults {
    pub detection: Option<ProjectDetection>,
    pub conventions: Option<InferredConventions>,
    pub constraints: Option<ExtractedConstraints>,
    pub deep_analysis: Option<DeepAnalysisResult>,
    pub synthesis: Option<AnalysisSynthesis>,
    /// Full coverage analysis from distributed analyzer
    pub aggregated: Option<super::analysis::AggregatedAnalysis>,
    /// Domain-specific knowledge (policies, logic, terminology)
    pub domain_analysis: Option<DomainAnalysisResult>,
    /// Cross-reference synthesis insights
    pub cross_insights: Option<SynthesizedInsights>,
    /// Detected modules for multi-agent orchestration
    pub detected_modules: Option<Vec<DetectedModule>>,
}

/// Synthesis summary for context
#[derive(Debug, Clone, Default)]
pub struct AnalysisSynthesis {
    pub confidence: f32,
    pub modules: Vec<String>,
    pub constraints: Vec<String>,
}

/// Key abstraction found during analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyAbstraction {
    pub name: String,
    pub kind: String,
    pub file_ref: String,
    pub description: String,
    pub usage_notes: Vec<String>,
}

/// Context statistics for reporting
#[derive(Debug, Clone, Default)]
pub struct ContextStats {
    pub tier3_count: usize,
    pub abstraction_count: usize,
    pub convention_count: usize,
    pub has_detection: bool,
    pub has_synthesis: bool,
    pub iteration_count: usize,
}

/// Pipeline execution context
#[derive(Debug, Clone)]
pub struct ClaudegenContext {
    analysis_results: AnalysisResults,
    iteration_count: usize,
    key_abstractions: Vec<KeyAbstraction>,
    tier3_constraints: Vec<TrackedConstraint>,
}

impl ClaudegenContext {
    pub fn new(_project_root: impl AsRef<Path>) -> Self {
        Self {
            analysis_results: AnalysisResults::default(),
            iteration_count: 0,
            key_abstractions: Vec::new(),
            tier3_constraints: Vec::new(),
        }
    }

    pub fn set_detection(&mut self, detection: ProjectDetection) {
        self.analysis_results.detection = Some(detection);
    }

    pub fn set_conventions(&mut self, conventions: InferredConventions) {
        self.analysis_results.conventions = Some(conventions);
    }

    pub fn set_constraints(&mut self, constraints: ExtractedConstraints) {
        // Extract tier3 constraints from gotchas
        self.tier3_constraints = constraints
            .gotchas
            .iter()
            .map(|g| TrackedConstraint {
                name: g.title.clone(),
                description: g.description.clone(),
                evidence: g.related_files.clone(),
                gotcha: Some(g.solution.clone()),
            })
            .collect();
        self.analysis_results.constraints = Some(constraints);
    }

    pub fn set_deep_analysis(&mut self, analysis: DeepAnalysisResult) {
        self.key_abstractions = analysis
            .key_abstractions
            .iter()
            .map(|a| KeyAbstraction {
                name: a.name.clone(),
                kind: format!("{:?}", a.kind),
                file_ref: a.file.clone(),
                description: a.description.clone(),
                usage_notes: a.usage_notes.clone(),
            })
            .collect();
        self.analysis_results.deep_analysis = Some(analysis);
    }

    pub fn set_synthesis(&mut self, synthesis: &super::analysis::SynthesizedAnalysis) {
        self.analysis_results.synthesis = Some(AnalysisSynthesis {
            confidence: synthesis.confidence.overall,
            modules: synthesis.modules.iter().map(|m| m.name.clone()).collect(),
            constraints: synthesis
                .deep
                .constraints
                .iter()
                .map(|c| c.description.clone())
                .collect(),
        });
    }

    pub fn set_aggregated(&mut self, aggregated: super::analysis::AggregatedAnalysis) {
        self.analysis_results.aggregated = Some(aggregated);
    }

    pub fn aggregated(&self) -> Option<&super::analysis::AggregatedAnalysis> {
        self.analysis_results.aggregated.as_ref()
    }

    pub fn set_domain_analysis(&mut self, domain: DomainAnalysisResult) {
        self.analysis_results.domain_analysis = Some(domain);
    }

    pub fn domain_analysis(&self) -> Option<&DomainAnalysisResult> {
        self.analysis_results.domain_analysis.as_ref()
    }

    pub fn set_cross_insights(&mut self, insights: SynthesizedInsights) {
        self.analysis_results.cross_insights = Some(insights);
    }

    pub fn cross_insights(&self) -> Option<&SynthesizedInsights> {
        self.analysis_results.cross_insights.as_ref()
    }

    pub fn set_detected_modules(&mut self, modules: Vec<DetectedModule>) {
        self.analysis_results.detected_modules = Some(modules);
    }

    pub fn detected_modules(&self) -> Option<&[DetectedModule]> {
        self.analysis_results.detected_modules.as_deref()
    }

    pub fn merge_from(&mut self, other: &ClaudegenContext) {
        self.tier3_constraints
            .extend(other.tier3_constraints.clone());
        self.tier3_constraints.sort_by(|a, b| a.name.cmp(&b.name));
        self.tier3_constraints.dedup_by(|a, b| a.name == b.name);

        self.key_abstractions.extend(other.key_abstractions.clone());
        self.key_abstractions.sort_by(|a, b| a.name.cmp(&b.name));
        self.key_abstractions.dedup_by(|a, b| a.name == b.name);

        if other.analysis_results.detection.is_some() && self.analysis_results.detection.is_none() {
            self.analysis_results.detection = other.analysis_results.detection.clone();
        }
        if other.analysis_results.conventions.is_some()
            && self.analysis_results.conventions.is_none()
        {
            self.analysis_results.conventions = other.analysis_results.conventions.clone();
        }
        if other.analysis_results.constraints.is_some()
            && self.analysis_results.constraints.is_none()
        {
            self.analysis_results.constraints = other.analysis_results.constraints.clone();
        }
        if other.analysis_results.deep_analysis.is_some()
            && self.analysis_results.deep_analysis.is_none()
        {
            self.analysis_results.deep_analysis = other.analysis_results.deep_analysis.clone();
        }
        if other.analysis_results.synthesis.is_some() && self.analysis_results.synthesis.is_none() {
            self.analysis_results.synthesis = other.analysis_results.synthesis.clone();
        }
        if other.analysis_results.aggregated.is_some() && self.analysis_results.aggregated.is_none()
        {
            self.analysis_results.aggregated = other.analysis_results.aggregated.clone();
        }
        if other.analysis_results.domain_analysis.is_some()
            && self.analysis_results.domain_analysis.is_none()
        {
            self.analysis_results.domain_analysis = other.analysis_results.domain_analysis.clone();
        }
        if other.analysis_results.cross_insights.is_some()
            && self.analysis_results.cross_insights.is_none()
        {
            self.analysis_results.cross_insights = other.analysis_results.cross_insights.clone();
        }
        if other.analysis_results.detected_modules.is_some()
            && self.analysis_results.detected_modules.is_none()
        {
            self.analysis_results.detected_modules =
                other.analysis_results.detected_modules.clone();
        }
    }

    pub fn increment_iteration(&mut self) {
        self.iteration_count += 1;
    }

    pub fn tier3_items(&self) -> &[TrackedConstraint] {
        &self.tier3_constraints
    }

    pub fn key_abstractions(&self) -> &[KeyAbstraction] {
        &self.key_abstractions
    }

    pub fn stats(&self) -> ContextStats {
        ContextStats {
            tier3_count: self.tier3_constraints.len(),
            abstraction_count: self.key_abstractions.len(),
            convention_count: self
                .analysis_results
                .conventions
                .as_ref()
                .map(|c| c.patterns.len())
                .unwrap_or(0),
            has_detection: self.analysis_results.detection.is_some(),
            has_synthesis: self.analysis_results.synthesis.is_some(),
            iteration_count: self.iteration_count,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// VerifiedFileRegistry - File validation for LLM references
// ════════════════════════════════════════════════════════════════════════════

use ignore::WalkBuilder;

/// Enhanced file metadata for 100% coverage analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub line_count: usize,
    pub extension: Option<String>,
    pub parent_module: String,
    pub estimated_complexity: u8,
    pub estimated_tokens: usize,
}

impl FileMetadata {
    fn new(path: String, line_count: usize) -> Self {
        let extension = std::path::Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_string());

        let parent_module = Self::extract_parent_module(&path);
        let estimated_complexity = Self::estimate_complexity(line_count);
        let estimated_tokens = Self::estimate_tokens(line_count);

        Self {
            path,
            line_count,
            extension,
            parent_module,
            estimated_complexity,
            estimated_tokens,
        }
    }

    fn extract_parent_module(path: &str) -> String {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            if parts[0] == "src" && parts.len() >= 3 {
                parts[1].to_string()
            } else {
                parts[0].to_string()
            }
        } else {
            String::from("root")
        }
    }

    fn estimate_complexity(line_count: usize) -> u8 {
        match line_count {
            0..=50 => 10,
            51..=150 => 25,
            151..=300 => 40,
            301..=500 => 55,
            501..=800 => 70,
            801..=1200 => 85,
            _ => 100,
        }
    }

    fn estimate_tokens(line_count: usize) -> usize {
        (line_count as f64 * 15.0) as usize
    }

    pub fn is_source_file(&self) -> bool {
        matches!(
            self.extension.as_deref(),
            Some(
                "rs" | "ts"
                    | "tsx"
                    | "js"
                    | "jsx"
                    | "py"
                    | "go"
                    | "kt"
                    | "java"
                    | "cs"
                    | "cpp"
                    | "c"
                    | "swift"
                    | "rb"
                    | "php"
                    | "scala"
                    | "ex"
                    | "sh"
                    | "bash"
            )
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct VerifiedFileRegistry {
    verified_files: HashSet<String>,
    file_to_line_count: HashMap<String, usize>,
    directories: HashSet<String>,
    file_metadata: HashMap<String, FileMetadata>,
}

impl VerifiedFileRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build registry with default analysis config
    pub async fn build(project_root: &Path) -> Result<Self> {
        Self::build_with_config(project_root, &AnalysisConfig::default()).await
    }

    /// Build registry respecting analysis include/exclude patterns and gitignore
    ///
    /// Uses ignore crate's WalkBuilder for consistent gitignore handling with FileScanner.
    pub async fn build_with_config(project_root: &Path, config: &AnalysisConfig) -> Result<Self> {
        let mut registry = Self::default();
        let excludes: Vec<glob::Pattern> = config
            .exclude
            .iter()
            .filter_map(|p| glob::Pattern::new(p).ok())
            .collect();
        let includes: Vec<glob::Pattern> = config
            .include
            .iter()
            .filter_map(|p| glob::Pattern::new(p).ok())
            .collect();

        // Use ignore crate for gitignore-aware walking (consistent with FileScanner)
        let walker = WalkBuilder::new(project_root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(false)
            .build();

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();

            if let Ok(relative) = path.strip_prefix(project_root) {
                let relative_str = relative.to_string_lossy().to_string();

                // Skip empty path (root)
                if relative_str.is_empty() {
                    continue;
                }

                // Check exclude patterns
                if excludes.iter().any(|p| p.matches(&relative_str)) {
                    continue;
                }

                // Skip hidden unless explicitly included
                if Self::is_hidden(path) && !includes.iter().any(|p| p.matches(&relative_str)) {
                    continue;
                }

                if path.is_dir() {
                    registry.directories.insert(relative_str);
                } else if path.is_file() {
                    let matches_include =
                        includes.is_empty() || includes.iter().any(|p| p.matches(&relative_str));
                    if matches_include {
                        let line_count = Self::count_lines(path).await.unwrap_or(0);
                        registry.verified_files.insert(relative_str.clone());
                        registry
                            .file_to_line_count
                            .insert(relative_str.clone(), line_count);
                        registry.file_metadata.insert(
                            relative_str.clone(),
                            FileMetadata::new(relative_str, line_count),
                        );
                    }
                }
            }
        }

        Ok(registry)
    }

    fn is_hidden(path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|name| name.starts_with('.'))
            .unwrap_or(false)
    }

    async fn count_lines(path: &Path) -> Result<usize> {
        let content = tokio::fs::read_to_string(path).await?;
        Ok(content.lines().count())
    }

    /// Normalize path by stripping optional @ prefix (used in documentation references)
    fn normalize_path(path: &str) -> &str {
        path.strip_prefix('@').unwrap_or(path)
    }

    pub fn contains(&self, path: &str) -> bool {
        self.verified_files.contains(Self::normalize_path(path))
    }

    pub fn directory_exists(&self, path: &str) -> bool {
        self.directories.contains(Self::normalize_path(path))
    }

    pub fn line_count(&self, path: &str) -> Option<usize> {
        self.file_to_line_count
            .get(Self::normalize_path(path))
            .copied()
    }

    pub fn file_count(&self) -> usize {
        self.verified_files.len()
    }

    pub fn all_files(&self) -> impl Iterator<Item = &String> {
        self.verified_files.iter()
    }

    pub fn files_matching(&self, pattern: &str) -> Vec<&String> {
        let pattern_lower = pattern.to_lowercase();
        self.verified_files
            .iter()
            .filter(|f| f.to_lowercase().contains(&pattern_lower))
            .collect()
    }

    pub fn files_in_directory(&self, dir_path: &str) -> Vec<String> {
        let normalized = dir_path.trim_end_matches('/');
        self.verified_files
            .iter()
            .filter(|f| f.starts_with(normalized) && f.len() > normalized.len())
            .cloned()
            .collect()
    }

    pub fn get_metadata(&self, path: &str) -> Option<&FileMetadata> {
        self.file_metadata.get(Self::normalize_path(path))
    }

    pub fn all_metadata(&self) -> impl Iterator<Item = &FileMetadata> {
        self.file_metadata.values()
    }

    pub fn files_by_module(&self) -> HashMap<String, Vec<&FileMetadata>> {
        let mut by_module: HashMap<String, Vec<&FileMetadata>> = HashMap::new();
        for meta in self.file_metadata.values() {
            by_module
                .entry(meta.parent_module.clone())
                .or_default()
                .push(meta);
        }
        by_module
    }

    pub fn total_lines(&self) -> usize {
        self.file_to_line_count.values().sum()
    }

    pub fn total_estimated_tokens(&self) -> usize {
        self.file_metadata
            .values()
            .map(|m| m.estimated_tokens)
            .sum()
    }

    pub fn source_files(&self) -> impl Iterator<Item = &FileMetadata> {
        self.file_metadata.values().filter(|m| m.is_source_file())
    }

    pub fn modules(&self) -> Vec<String> {
        let mut modules: Vec<_> = self
            .file_metadata
            .values()
            .map(|m| m.parent_module.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        modules.sort();
        modules
    }

    pub fn to_prompt_context(&self, max_files: usize) -> String {
        let mut files: Vec<_> = self.verified_files.iter().collect();
        files.sort();

        let shown: Vec<_> = files.iter().take(max_files).cloned().collect();
        let remaining = files.len().saturating_sub(max_files);

        let mut output = format!(
            "AVAILABLE FILES ({} total, showing {}):\n",
            files.len(),
            shown.len()
        );

        for file in shown {
            if let Some(lines) = self.file_to_line_count.get(file) {
                output.push_str(&format!("  {} ({} lines)\n", file, lines));
            } else {
                output.push_str(&format!("  {}\n", file));
            }
        }

        if remaining > 0 {
            output.push_str(&format!("  ... and {} more files\n", remaining));
        }

        output.push_str("\nONLY reference files from this list. Use @path:line format.\n");
        output
    }

    pub fn get_code_samples(&self, max_files: usize) -> String {
        // Language-agnostic priority patterns for common entry points and core directories
        let priority_patterns = [
            // Entry points (various languages)
            "main.",
            "index.",
            "app.",
            "server.",
            "lib.",
            // Common source directories
            "/src/",
            "/lib/",
            "/pkg/",
            "/internal/",
            "/cmd/",
            // Configuration and types
            "/config/",
            "/types/",
            "/models/",
            "/schemas/",
            // Core logic
            "/core/",
            "/domain/",
            "/services/",
            "/handlers/",
        ];

        let mut selected_files: Vec<&String> = Vec::new();

        // First pass: match priority patterns
        for pattern in &priority_patterns {
            for file in &self.verified_files {
                if file.contains(pattern)
                    && !selected_files.contains(&file)
                    && selected_files.len() < max_files
                {
                    selected_files.push(file);
                }
            }
        }

        // Second pass: add any source files (language-agnostic extension check)
        let source_extensions = [
            ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".kt", ".java", ".cs", ".cpp", ".c",
            ".swift", ".rb", ".php", ".scala", ".ex",
        ];
        for file in &self.verified_files {
            if selected_files.len() >= max_files {
                break;
            }
            let is_source = source_extensions.iter().any(|ext| file.ends_with(ext));
            if is_source && !selected_files.contains(&file) {
                selected_files.push(file);
            }
        }

        selected_files.sort();

        let mut output = String::new();

        for file in selected_files {
            let line_count = self.file_to_line_count.get(file).copied().unwrap_or(0);

            output.push_str(&format!("\n--- {} (lines 1-{}) ---\n", file, line_count));
            output.push_str(&format!(
                "Use @{}:N where N is a line number (1 to {})\n",
                file, line_count
            ));

            if line_count > 50 {
                output.push_str(&format!(
                    "Example references: @{}:1, @{}:25, @{}:50\n",
                    file, file, file
                ));
            }
        }

        output
    }

    /// Register a file for testing purposes (test-only helper).
    ///
    /// Adds a file to the registry with a default line count of 100.
    #[cfg(test)]
    pub fn register_test_file(&mut self, path: &str) {
        self.register_test_file_with_lines(path, 100);
    }

    /// Register a file for testing with a specific line count (test-only helper).
    #[cfg(test)]
    pub fn register_test_file_with_lines(&mut self, path: &str, lines: usize) {
        self.verified_files.insert(path.to_string());
        self.file_to_line_count.insert(path.to_string(), lines);
        self.file_metadata
            .insert(path.to_string(), FileMetadata::new(path.to_string(), lines));

        // Register parent directories
        let path_buf = std::path::Path::new(path);
        let mut current = path_buf.parent();
        while let Some(parent) = current {
            let dir_str = parent.to_string_lossy().to_string();
            if dir_str.is_empty() {
                break;
            }
            self.directories.insert(dir_str);
            current = parent.parent();
        }
    }

    /// Check if a README file exists at the project root.
    pub fn has_readme(&self) -> bool {
        self.verified_files.iter().any(|f| {
            let lower = f.to_lowercase();
            lower == "readme.md" || lower == "readme" || lower == "readme.txt" || lower == "readme.rst"
        })
    }

    /// Check if a docs/ directory exists.
    pub fn has_docs_directory(&self) -> bool {
        self.directories.iter().any(|d| {
            let lower = d.to_lowercase();
            lower == "docs" || lower == "doc" || lower == "documentation"
        })
    }

    /// Return files that appear to be test files.
    pub fn test_files(&self) -> Vec<&String> {
        self.verified_files
            .iter()
            .filter(|f| {
                let lower = f.to_lowercase();
                lower.contains("test") || lower.contains("spec") || lower.contains("_test.")
                    || lower.starts_with("tests/") || lower.starts_with("test/")
            })
            .collect()
    }
}

/// Extension trait for file existence and line count queries on the registry.
pub trait FileRegistryExt {
    fn file_exists(&self, path: &str) -> bool;
    fn get_line_count(&self, path: &str) -> Result<usize>;
}

impl FileRegistryExt for VerifiedFileRegistry {
    fn file_exists(&self, path: &str) -> bool {
        let clean_path = path.trim_start_matches('@').trim_start_matches("./");
        self.contains(clean_path)
            || self.contains(&format!("./{}", clean_path))
            || self.contains(&format!("src/{}", clean_path))
    }

    fn get_line_count(&self, path: &str) -> Result<usize> {
        let clean_path = path.trim_start_matches('@').trim_start_matches("./");
        self.line_count(clean_path).ok_or_else(|| {
            crate::types::ClaudegenError::Config(format!("File not found: {}", path))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_normalization() {
        let registry = VerifiedFileRegistry::default();
        assert!(!registry.contains("@src/main.rs"));
        assert!(!registry.contains("src/main.rs"));
    }

    #[tokio::test]
    async fn test_empty_registry() {
        let registry = VerifiedFileRegistry::default();
        assert!(!registry.contains("any/file.rs"));
        assert_eq!(registry.file_count(), 0);
    }
}
