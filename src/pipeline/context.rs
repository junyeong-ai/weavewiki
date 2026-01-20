//! Pipeline Context Module
//!
//! Provides verified file registry and project context for refinement pipeline.
//! Ensures LLM receives accurate file information to prevent hallucinated references.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::ProjectType;
use crate::types::Result;

use super::phases::{
    convention_inference::InferredConventions, project_detection::ProjectDetection,
};

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub project_root: PathBuf,
    pub detection: ProjectDetection,
    pub conventions: Option<InferredConventions>,
    pub file_registry: VerifiedFileRegistry,
}

impl ProjectContext {
    pub async fn build(project_root: impl AsRef<Path>) -> Result<Self> {
        let project_root = project_root.as_ref().to_path_buf();
        let file_registry = VerifiedFileRegistry::build(&project_root).await?;

        Ok(Self {
            project_root,
            detection: ProjectDetection::default(),
            conventions: None,
            file_registry,
        })
    }

    pub fn with_detection(mut self, detection: ProjectDetection) -> Self {
        self.detection = detection;
        self
    }

    pub fn with_conventions(mut self, conventions: InferredConventions) -> Self {
        self.conventions = Some(conventions);
        self
    }

    pub fn project_type(&self) -> ProjectType {
        self.detection.primary_type
    }

    pub fn is_monorepo(&self) -> bool {
        self.detection.is_monorepo
    }
}

const MAX_RECURSION_DEPTH: usize = 50;

#[derive(Debug, Clone, Default)]
pub struct VerifiedFileRegistry {
    verified_files: HashSet<String>,
    file_to_line_count: HashMap<String, usize>,
    directories: HashSet<String>,
}

impl VerifiedFileRegistry {
    /// Create an empty registry (useful for testing)
    pub fn empty() -> Self {
        Self::default()
    }

    pub async fn build(project_root: &Path) -> Result<Self> {
        let mut registry = Self::default();
        registry.scan_directory(project_root, project_root, 0).await?;
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
        let normalized = Self::normalize_path(path);
        self.verified_files.contains(&normalized)
    }

    pub fn directory_exists(&self, path: &str) -> bool {
        let normalized = Self::normalize_path(path);
        self.directories.contains(&normalized)
    }

    pub fn line_count(&self, path: &str) -> Option<usize> {
        let normalized = Self::normalize_path(path);
        self.file_to_line_count.get(&normalized).copied()
    }

    pub fn validate_line(&self, path: &str, line: usize) -> bool {
        match self.line_count(path) {
            Some(max) => line > 0 && line <= max,
            None => false,
        }
    }

    pub fn validate_reference(&self, reference: &str) -> ReferenceValidation {
        let (path, line) = Self::parse_reference(reference);

        if !self.contains(path) {
            return ReferenceValidation::InvalidPath(path.to_string());
        }

        if let Some(line_num) = line
            && !self.validate_line(path, line_num) {
                let max = self.line_count(path).unwrap_or(0);
                return ReferenceValidation::InvalidLine {
                    path: path.to_string(),
                    line: line_num,
                    max_lines: max,
                };
            }

        ReferenceValidation::Valid
    }

    fn parse_reference(reference: &str) -> (&str, Option<usize>) {
        let reference = reference.strip_prefix('@').unwrap_or(reference);

        if let Some(pos) = reference.rfind(':') {
            let (path, line_part) = reference.split_at(pos);
            let line_str = &line_part[1..];

            if let Ok(line) = line_str.parse::<usize>() {
                return (path, Some(line));
            }
        }

        (reference, None)
    }

    fn normalize_path(path: &str) -> String {
        path.strip_prefix('@').unwrap_or(path).to_string()
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

    /// Get all files within a specific directory path
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

    pub fn to_prompt_context_by_extension(&self, extensions: &[&str], max_files: usize) -> String {
        let mut filtered: Vec<_> = self
            .verified_files
            .iter()
            .filter(|f| extensions.iter().any(|ext| f.ends_with(ext)))
            .collect();
        filtered.sort();

        let shown: Vec<_> = filtered.iter().take(max_files).cloned().collect();
        let remaining = filtered.len().saturating_sub(max_files);

        let mut output = format!(
            "RELEVANT FILES ({} matching {:?}, showing {}):\n",
            filtered.len(),
            extensions,
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

        output.push_str("\nONLY reference files from this list.\n");
        output
    }

    /// Get code samples from key files for evidence references
    pub fn get_code_samples(&self, max_files: usize) -> String {
        // Prioritize source files over others
        let priority_patterns = [
            "src/main.rs",
            "src/lib.rs",
            "src/pipeline/",
            "src/ai/",
            "src/config/",
            "src/types/",
        ];

        let mut selected_files: Vec<&String> = Vec::new();

        // First pass: prioritized files
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

        // Fill remaining slots with other source files
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

            output.push_str(&format!(
                "\n--- {} (lines 1-{}) ---\n",
                file, line_count
            ));
            output.push_str(&format!(
                "Use @{}:N where N is a line number (1 to {})\n",
                file, line_count
            ));

            // Provide some landmark line numbers
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceValidation {
    Valid,
    InvalidPath(String),
    InvalidLine {
        path: String,
        line: usize,
        max_lines: usize,
    },
}

impl ReferenceValidation {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_parsing() {
        let (path, line) = VerifiedFileRegistry::parse_reference("@src/main.rs:42");
        assert_eq!(path, "src/main.rs");
        assert_eq!(line, Some(42));

        let (path, line) = VerifiedFileRegistry::parse_reference("src/lib.rs");
        assert_eq!(path, "src/lib.rs");
        assert_eq!(line, None);

        let (path, line) = VerifiedFileRegistry::parse_reference("@src/file.rs:invalid");
        assert_eq!(path, "src/file.rs:invalid");
        assert_eq!(line, None);
    }

    #[test]
    fn test_path_normalization() {
        assert_eq!(
            VerifiedFileRegistry::normalize_path("@src/main.rs"),
            "src/main.rs"
        );
        assert_eq!(
            VerifiedFileRegistry::normalize_path("src/main.rs"),
            "src/main.rs"
        );
    }

    #[tokio::test]
    async fn test_empty_registry() {
        let registry = VerifiedFileRegistry::default();
        assert!(!registry.contains("any/file.rs"));
        assert_eq!(registry.file_count(), 0);
    }
}
