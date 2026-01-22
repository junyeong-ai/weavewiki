//! Pipeline Context Module
//!
//! Provides verified file registry and project context for refinement pipeline.
//! Ensures LLM receives accurate file information to prevent hallucinated references.
//!
//! ClaudegenContext wraps claude-agent-rs Session with claudegen-specific extensions
//! for tier classification, analysis results, and compaction priority.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ProjectType;
use crate::types::Result;

use super::analysis::DeepAnalysisResult;
use super::phases::{
    constraint_extraction::ExtractedConstraints, convention_inference::InferredConventions,
    project_detection::ProjectDetection,
};

// ════════════════════════════════════════════════════════════════════════════
// ClaudegenContext - Wraps claude-agent Session with claudegen extensions
// ════════════════════════════════════════════════════════════════════════════

/// Content tier classification for quality assessment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentTier {
    /// Generic language/tool knowledge - REJECT
    Tier1Generic,
    /// Project conventions - Keep
    Tier2Convention,
    /// Hidden constraints, gotchas - Essential
    Tier3Constraint,
}

impl ContentTier {
    pub fn is_rejectable(&self) -> bool {
        matches!(self, Self::Tier1Generic)
    }

    pub fn is_essential(&self) -> bool {
        matches!(self, Self::Tier3Constraint)
    }
}

/// Item rejected due to Tier1 (generic) content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedItem {
    pub item_type: String,
    pub name: String,
    pub reason: String,
}

/// Extracted convention from the project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Convention {
    pub name: String,
    pub description: String,
    pub examples: Vec<String>,
}

/// Extracted constraint (Tier3 - essential)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    pub name: String,
    pub description: String,
    pub evidence: Vec<String>,
    pub gotcha: Option<String>,
}

/// Tier classification tracking
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TierTracker {
    pub tier1_rejected: Vec<RejectedItem>,
    pub tier2_conventions: Vec<Convention>,
    pub tier3_constraints: Vec<Constraint>,
}

impl TierTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_rejected(&mut self, item: RejectedItem) {
        self.tier1_rejected.push(item);
    }

    pub fn add_convention(&mut self, conv: Convention) {
        self.tier2_conventions.push(conv);
    }

    pub fn add_constraint(&mut self, constraint: Constraint) {
        self.tier3_constraints.push(constraint);
    }

    pub fn tier3_count(&self) -> usize {
        self.tier3_constraints.len()
    }

    pub fn has_essential_content(&self) -> bool {
        !self.tier3_constraints.is_empty()
    }
}

/// Synthesized analysis results from multiple phases
#[derive(Debug, Clone, Default)]
pub struct SynthesizedAnalysis {
    pub summary: String,
    pub key_insights: Vec<String>,
    pub critical_paths: Vec<String>,
}

/// Aggregated analysis results from pipeline phases
#[derive(Debug, Clone, Default)]
pub struct AnalysisResults {
    pub detection: Option<ProjectDetection>,
    pub conventions: Option<InferredConventions>,
    pub constraints: Option<ExtractedConstraints>,
    pub deep_analysis: Option<DeepAnalysisResult>,
    pub synthesis: Option<SynthesizedAnalysis>,
}

impl AnalysisResults {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_detection(mut self, detection: ProjectDetection) -> Self {
        self.detection = Some(detection);
        self
    }

    pub fn with_conventions(mut self, conventions: InferredConventions) -> Self {
        self.conventions = Some(conventions);
        self
    }

    pub fn with_constraints(mut self, constraints: ExtractedConstraints) -> Self {
        self.constraints = Some(constraints);
        self
    }

    pub fn with_deep_analysis(mut self, analysis: DeepAnalysisResult) -> Self {
        self.deep_analysis = Some(analysis);
        self
    }

    pub fn is_complete(&self) -> bool {
        self.detection.is_some()
            && self.conventions.is_some()
            && self.constraints.is_some()
            && self.deep_analysis.is_some()
    }
}

/// Compaction priority for context management
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompactionPriority {
    /// Can be removed first
    Low,
    /// Keep if space allows
    Medium,
    /// Important to retain
    High,
    /// Never remove (Tier3 content)
    Critical,
}

/// ClaudegenContext - Wrapper around claude-agent Session with claudegen extensions
///
/// Provides:
/// - Tier classification for content quality assessment
/// - Aggregated analysis results from pipeline phases
/// - Compaction priority based on content value
#[derive(Debug, Clone)]
pub struct ClaudegenContext {
    /// Project root path
    project_root: PathBuf,

    /// Tier classification tracking
    tier_classification: TierTracker,

    /// Aggregated analysis results
    analysis_results: AnalysisResults,

    /// Session ID for persistence
    session_id: Option<String>,

    /// Iteration counter for refinement loops
    iteration_count: usize,

    /// Key abstractions extracted from analysis
    key_abstractions: Vec<KeyAbstraction>,
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

impl ClaudegenContext {
    /// Create a new ClaudegenContext for the given project
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            tier_classification: TierTracker::new(),
            analysis_results: AnalysisResults::new(),
            session_id: None,
            iteration_count: 0,
            key_abstractions: Vec::new(),
        }
    }

    /// Create with an existing session ID
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Get the project root path
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Get the session ID if set
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    // ── Analysis Results ──────────────────────────────────────────────────

    /// Set analysis results
    pub fn set_analysis(&mut self, results: AnalysisResults) {
        self.analysis_results = results;
    }

    /// Set project detection
    pub fn set_detection(&mut self, detection: ProjectDetection) {
        self.analysis_results.detection = Some(detection);
    }

    /// Set inferred conventions
    pub fn set_conventions(&mut self, conventions: InferredConventions) {
        self.analysis_results.conventions = Some(conventions);
    }

    /// Set extracted constraints
    pub fn set_constraints(&mut self, constraints: ExtractedConstraints) {
        self.analysis_results.constraints = Some(constraints);
    }

    /// Set deep analysis results
    pub fn set_deep_analysis(&mut self, analysis: DeepAnalysisResult) {
        // Extract key abstractions from deep analysis
        self.key_abstractions = analysis.key_abstractions.iter().map(|a| KeyAbstraction {
            name: a.name.clone(),
            kind: format!("{:?}", a.kind),
            file_ref: a.file.clone(),
            description: a.description.clone(),
            usage_notes: a.usage_notes.clone(),
        }).collect();
        self.analysis_results.deep_analysis = Some(analysis);
    }

    /// Set synthesis results
    pub fn set_synthesis(&mut self, synthesis: super::analysis::SynthesizedAnalysis) {
        self.analysis_results.synthesis = Some(SynthesizedAnalysis {
            summary: format!("Confidence: {:.2}", synthesis.confidence.overall),
            key_insights: synthesis.modules.iter().map(|m| m.name.clone()).collect(),
            critical_paths: synthesis.deep.constraints.iter().map(|c| c.description.clone()).collect(),
        });
    }

    // ── Tier Classification ────────────────────────────────────────────────

    /// Get analysis results
    pub fn analysis(&self) -> &AnalysisResults {
        &self.analysis_results
    }

    /// Get mutable analysis results
    pub fn analysis_mut(&mut self) -> &mut AnalysisResults {
        &mut self.analysis_results
    }

    /// Classify content tier based on patterns
    pub fn classify_content(&self, content: &str) -> ContentTier {
        // Tier3 indicators (essential constraints)
        let tier3_patterns = [
            "MUST", "NEVER", "ALWAYS", "CRITICAL",
            "gotcha", "constraint", "invariant",
            "⚠️", "🚨", "IMPORTANT:",
            "@", ":",  // File references like @src/main.rs:10
        ];

        // Tier1 indicators (generic knowledge)
        let tier1_patterns = [
            "best practice", "generally", "typically",
            "you should", "consider using",
            "good to", "recommended to",
        ];

        let content_lower = content.to_lowercase();

        // Check for Tier3 first (highest priority)
        let has_file_ref = content.contains("@") && content.contains(":");
        let has_tier3_keyword = tier3_patterns.iter().any(|p| content.contains(p));

        if has_file_ref || has_tier3_keyword {
            return ContentTier::Tier3Constraint;
        }

        // Check for Tier1 (generic)
        let has_tier1_pattern = tier1_patterns.iter().any(|p| content_lower.contains(p));
        if has_tier1_pattern && !has_file_ref {
            return ContentTier::Tier1Generic;
        }

        // Default to Tier2 (convention)
        ContentTier::Tier2Convention
    }

    /// Record a tier classification result
    pub fn record_classification(&mut self, item_type: &str, name: &str, tier: ContentTier, content: &str) {
        match tier {
            ContentTier::Tier1Generic => {
                self.tier_classification.add_rejected(RejectedItem {
                    item_type: item_type.to_string(),
                    name: name.to_string(),
                    reason: "Generic content".to_string(),
                });
            }
            ContentTier::Tier2Convention => {
                self.tier_classification.add_convention(Convention {
                    name: name.to_string(),
                    description: content.chars().take(200).collect(),
                    examples: Vec::new(),
                });
            }
            ContentTier::Tier3Constraint => {
                self.tier_classification.add_constraint(Constraint {
                    name: name.to_string(),
                    description: content.chars().take(200).collect(),
                    evidence: Vec::new(),
                    gotcha: None,
                });
            }
        }
    }

    /// Get tier classification summary
    pub fn tier_classification(&self) -> &TierTracker {
        &self.tier_classification
    }

    /// Get tier3 constraint count
    pub fn tier3_count(&self) -> usize {
        self.tier_classification.tier3_count()
    }

    // ── Compaction Priority ────────────────────────────────────────────────

    /// Get compaction priority for content
    pub fn get_compaction_priority(&self, content: &str) -> CompactionPriority {
        let tier = self.classify_content(content);

        match tier {
            ContentTier::Tier3Constraint => CompactionPriority::Critical,
            ContentTier::Tier2Convention => CompactionPriority::High,
            ContentTier::Tier1Generic => CompactionPriority::Low,
        }
    }

    /// Check if content should be preserved during compaction
    pub fn should_preserve(&self, content: &str) -> bool {
        matches!(
            self.get_compaction_priority(content),
            CompactionPriority::Critical | CompactionPriority::High
        )
    }

    // ── Generation Prompt ──────────────────────────────────────────────────

    /// Generate a prompt context from analysis results
    pub fn to_generation_prompt(&self) -> String {
        let mut prompt = String::new();

        if let Some(ref detection) = self.analysis_results.detection {
            prompt.push_str(&format!(
                "## Project Type\n{:?}\n\n",
                detection.primary_type
            ));
        }

        if let Some(ref conventions) = self.analysis_results.conventions {
            prompt.push_str("## Conventions\n");
            prompt.push_str(&format!("- File naming: {:?}\n", conventions.naming.file_naming));
            prompt.push_str(&format!("- Type naming: {:?}\n", conventions.naming.type_naming));
            prompt.push_str(&format!("- Function naming: {:?}\n", conventions.naming.function_naming));
            for pattern in &conventions.patterns {
                prompt.push_str(&format!("- Pattern: {} - {}\n", pattern.name, pattern.description));
            }
            prompt.push('\n');
        }

        if !self.tier_classification.tier3_constraints.is_empty() {
            prompt.push_str("## Critical Constraints (Tier3)\n");
            for constraint in &self.tier_classification.tier3_constraints {
                prompt.push_str(&format!(
                    "- **{}**: {}\n",
                    constraint.name, constraint.description
                ));
            }
            prompt.push('\n');
        }

        prompt
    }

    // ── Merge ──────────────────────────────────────────────────────────────

    /// Merge another context's classifications into this one
    pub fn merge_from(&mut self, other: &ClaudegenContext) {
        self.tier_classification.tier1_rejected.extend(other.tier_classification.tier1_rejected.clone());
        self.tier_classification.tier2_conventions.extend(other.tier_classification.tier2_conventions.clone());
        self.tier_classification.tier3_constraints.extend(other.tier_classification.tier3_constraints.clone());
        self.key_abstractions.extend(other.key_abstractions.clone());

        // Merge analysis results if not set
        if other.analysis_results.detection.is_some() && self.analysis_results.detection.is_none() {
            self.analysis_results.detection = other.analysis_results.detection.clone();
        }
        if other.analysis_results.conventions.is_some() && self.analysis_results.conventions.is_none() {
            self.analysis_results.conventions = other.analysis_results.conventions.clone();
        }
        if other.analysis_results.constraints.is_some() && self.analysis_results.constraints.is_none() {
            self.analysis_results.constraints = other.analysis_results.constraints.clone();
        }
        if other.analysis_results.deep_analysis.is_some() && self.analysis_results.deep_analysis.is_none() {
            self.analysis_results.deep_analysis = other.analysis_results.deep_analysis.clone();
        }
        if other.analysis_results.synthesis.is_some() && self.analysis_results.synthesis.is_none() {
            self.analysis_results.synthesis = other.analysis_results.synthesis.clone();
        }
    }

    // ── Iteration Tracking ────────────────────────────────────────────────

    /// Increment iteration count
    pub fn increment_iteration(&mut self) {
        self.iteration_count += 1;
    }

    /// Get iteration count
    pub fn iteration_count(&self) -> usize {
        self.iteration_count
    }

    // ── Accessors ─────────────────────────────────────────────────────────

    /// Get tier3 constraints (items)
    pub fn tier3_items(&self) -> &[Constraint] {
        &self.tier_classification.tier3_constraints
    }

    /// Get key abstractions
    pub fn key_abstractions(&self) -> &[KeyAbstraction] {
        &self.key_abstractions
    }

    /// Get context statistics
    pub fn stats(&self) -> ContextStats {
        ContextStats {
            tier3_count: self.tier_classification.tier3_constraints.len(),
            abstraction_count: self.key_abstractions.len(),
            convention_count: self.tier_classification.tier2_conventions.len(),
            has_detection: self.analysis_results.detection.is_some(),
            has_synthesis: self.analysis_results.synthesis.is_some(),
            iteration_count: self.iteration_count,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// ProjectContext - Legacy structure retained for compatibility
// ════════════════════════════════════════════════════════════════════════════

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
