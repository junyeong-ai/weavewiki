//! Pipeline Context Module
//!
//! Provides verified file registry and project context for refinement pipeline.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::types::Result;

use super::analysis::DeepAnalysisResult;
use super::phases::{
    constraint_extraction::ExtractedConstraints, convention_inference::InferredConventions,
    project_detection::ProjectDetection,
};

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

    pub fn set_synthesis(&mut self, synthesis: super::analysis::SynthesizedAnalysis) {
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

    pub fn merge_from(&mut self, other: &ClaudegenContext) {
        self.tier3_constraints
            .extend(other.tier3_constraints.clone());
        self.key_abstractions.extend(other.key_abstractions.clone());

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

const MAX_RECURSION_DEPTH: usize = 50;

#[derive(Debug, Clone, Default)]
pub struct VerifiedFileRegistry {
    verified_files: HashSet<String>,
    file_to_line_count: HashMap<String, usize>,
    directories: HashSet<String>,
}

impl VerifiedFileRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub async fn build(project_root: &Path) -> Result<Self> {
        let mut registry = Self::default();
        registry
            .scan_directory(project_root, project_root, 0)
            .await?;
        Ok(registry)
    }

    async fn scan_directory(&mut self, root: &Path, current: &Path, depth: usize) -> Result<()> {
        if depth > MAX_RECURSION_DEPTH {
            tracing::warn!(path = %current.display(), "Max directory depth exceeded, skipping");
            return Ok(());
        }

        let mut entries = match tokio::fs::read_dir(current).await {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();

            if Self::should_skip(&path) {
                continue;
            }

            if let Ok(relative) = path.strip_prefix(root) {
                let relative_str = relative.to_string_lossy().to_string();

                if path.is_dir() {
                    self.directories.insert(relative_str.clone());
                    Box::pin(self.scan_directory(root, &path, depth + 1)).await?;
                } else if path.is_file() {
                    let line_count = Self::count_lines(&path).await.unwrap_or(0);
                    self.verified_files.insert(relative_str.clone());
                    self.file_to_line_count.insert(relative_str, line_count);
                }
            }
        }

        Ok(())
    }

    fn should_skip(path: &Path) -> bool {
        let skip_dirs = [
            ".git",
            "target",
            "node_modules",
            "dist",
            "build",
            ".venv",
            "__pycache__",
            ".claudegen",
            ".claude",
            "vendor",
        ];

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            return skip_dirs.contains(&name) || name.starts_with('.');
        }
        false
    }

    async fn count_lines(path: &Path) -> Result<usize> {
        let content = tokio::fs::read_to_string(path).await?;
        Ok(content.lines().count())
    }

    pub fn contains(&self, path: &str) -> bool {
        let normalized = path.strip_prefix('@').unwrap_or(path);
        self.verified_files.contains(normalized)
    }

    pub fn directory_exists(&self, path: &str) -> bool {
        let normalized = path.strip_prefix('@').unwrap_or(path);
        self.directories.contains(normalized)
    }

    pub fn line_count(&self, path: &str) -> Option<usize> {
        let normalized = path.strip_prefix('@').unwrap_or(path);
        self.file_to_line_count.get(normalized).copied()
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
        let priority_patterns = [
            "src/main.rs",
            "src/lib.rs",
            "src/pipeline/",
            "src/ai/",
            "src/config/",
            "src/types/",
        ];

        let mut selected_files: Vec<&String> = Vec::new();

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

        for file in &self.verified_files {
            if selected_files.len() >= max_files {
                break;
            }
            if file.ends_with(".rs") && !selected_files.contains(&file) {
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
