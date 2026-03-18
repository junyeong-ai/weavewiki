//! Completeness Validator
//!
//! Ensures 100% file coverage by identifying and addressing gaps:
//! - Character-truncated files are re-analyzed individually via single-file fallback
//! - Failed files are retried individually
//! - Failed chunks are retried with the standard analyzer
//! - Referenced but unanalyzed modules are reported as gaps
//! - Final coverage report is generated

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ai::LlmProvider;
use crate::types::{Result, ValidationIssue};

use super::aggregator::Coverage;
use super::distributed::{
    AnalysisChunk, ChunkAnalysisResult, DistributedAnalyzer, FailedChunk,
};

/// Approximate tokens per line of code for estimation
const TOKENS_PER_LINE: usize = 15;

/// Fraction of total files that must be truncated before marking analysis as incomplete
const TRUNCATION_INCOMPLETE_THRESHOLD: f32 = 0.10;

/// Result of the completeness validation pass
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletenessReport {
    pub initial_coverage: f32,
    pub final_coverage: f32,
    pub files_reanalyzed: usize,
    pub chunks_retried: usize,
    pub additional_results: Vec<ChunkAnalysisResult>,
    pub remaining_gaps: Vec<AnalysisCoverageGap>,
    /// Validation issues discovered during completeness checking (truncations, failures)
    #[serde(default)]
    pub validation_issues: Vec<ValidationIssue>,
}

/// A gap in analysis coverage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisCoverageGap {
    pub file_path: String,
    pub reason: String,
}

pub struct CompletenessValidator {
    provider: Arc<dyn LlmProvider>,
    project_root: PathBuf,
}

impl CompletenessValidator {
    pub fn new(provider: Arc<dyn LlmProvider>, project_root: &Path) -> Self {
        Self {
            provider,
            project_root: project_root.to_path_buf(),
        }
    }

    /// Validate and improve analysis coverage.
    ///
    /// 1. Retry failed chunks
    /// 2. Single-file fallback for truncated/failed files within successful chunks
    /// 3. Detect referenced-but-unanalyzed modules
    /// 4. Validate and report file completeness issues
    pub async fn validate(
        &self,
        chunk_results: &[ChunkAnalysisResult],
        failed_chunks: &[FailedChunk],
        coverage: &Coverage,
        config: &crate::config::DistributedAnalysisConfig,
    ) -> CompletenessReport {
        let initial_coverage = coverage.coverage_ratio;
        let mut additional_results = Vec::new();
        let mut remaining_gaps = Vec::new();
        let mut files_reanalyzed = 0usize;
        let mut chunks_retried = 0usize;

        // 1. Retry failed chunks
        if !failed_chunks.is_empty() {
            tracing::info!(
                count = failed_chunks.len(),
                "Retrying failed chunks for completeness"
            );
            match self.retry_failed_chunks(failed_chunks, config).await {
                Ok(results) => {
                    chunks_retried += results.len();
                    additional_results.extend(results);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed chunk retry failed");
                    for chunk in failed_chunks {
                        for file in &chunk.files {
                            remaining_gaps.push(AnalysisCoverageGap {
                                file_path: file.clone(),
                                reason: format!(
                                    "Retry failed (original: {}): {e}",
                                    chunk.error
                                ),
                            });
                        }
                    }
                }
            }
        }

        // 2. Single-file fallback for character-truncated and read-failed files
        //    These are files within otherwise-successful chunks that had issues.
        let (char_truncated, read_failed) =
            Self::collect_problematic_files(chunk_results, &additional_results);
        if !char_truncated.is_empty() || !read_failed.is_empty() {
            tracing::info!(
                char_truncated = char_truncated.len(),
                read_failed = read_failed.len(),
                "Running single-file fallback for problematic files"
            );
            let analyzer = DistributedAnalyzer::new(
                Arc::clone(&self.provider),
                config.clone(),
            );
            match analyzer
                .analyze_single_file_fallback(&char_truncated, &read_failed, &self.project_root)
                .await
            {
                Ok(results) => {
                    files_reanalyzed += results.iter().map(|r| r.file_count).sum::<usize>();
                    additional_results.extend(results);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Single-file fallback analysis failed");
                    for path in char_truncated.iter().chain(read_failed.iter()) {
                        remaining_gaps.push(AnalysisCoverageGap {
                            file_path: path.clone(),
                            reason: format!("Single-file fallback failed: {e}"),
                        });
                    }
                }
            }
        }

        // 3. Detect referenced-but-unanalyzed modules
        let analyzed_modules: HashSet<String> = chunk_results
            .iter()
            .map(|c| c.module_path.clone())
            .collect();

        let referenced_modules: HashSet<String> = chunk_results
            .iter()
            .flat_map(|c| c.dependencies.iter().map(|d| d.to_module.clone()))
            .collect();

        let unanalyzed: Vec<String> = referenced_modules
            .difference(&analyzed_modules)
            .filter(|m| !m.is_empty())
            .cloned()
            .collect();

        if !unanalyzed.is_empty() {
            tracing::info!(
                count = unanalyzed.len(),
                "Found referenced but unanalyzed modules"
            );
            for module in &unanalyzed {
                remaining_gaps.push(AnalysisCoverageGap {
                    file_path: module.clone(),
                    reason: "Referenced but not analyzed".to_string(),
                });
            }
        }

        // 4. Validate truncated and failed files from chunk results
        let validation_issues = Self::validate_file_completeness(
            chunk_results,
            coverage.total_files,
        );

        let total_analyzed = coverage.analyzed_files + files_reanalyzed + chunks_retried;
        let final_coverage = if coverage.total_files > 0 {
            total_analyzed as f32 / coverage.total_files as f32
        } else {
            0.0
        };

        CompletenessReport {
            initial_coverage,
            final_coverage: final_coverage.min(1.0),
            files_reanalyzed,
            chunks_retried,
            additional_results,
            remaining_gaps,
            validation_issues,
        }
    }

    /// Collect deduplicated truncated and failed file paths from chunk results.
    /// Returns (char_truncated, read_failed) as two separate deduplicated vectors.
    fn collect_problematic_files(
        chunk_results: &[ChunkAnalysisResult],
        additional_results: &[ChunkAnalysisResult],
    ) -> (Vec<String>, Vec<String>) {
        let all_results = chunk_results.iter().chain(additional_results.iter());

        let mut truncated_set: HashSet<String> = HashSet::new();
        let mut failed_set: HashSet<String> = HashSet::new();

        for result in all_results {
            for path in &result.truncated_files {
                truncated_set.insert(path.clone());
            }
            for path in &result.failed_files {
                failed_set.insert(path.clone());
            }
        }

        // Remove any files that appear in both sets (avoid double-processing)
        let failed_only: Vec<String> = failed_set
            .difference(&truncated_set)
            .cloned()
            .collect();

        let truncated: Vec<String> = truncated_set.into_iter().collect();

        (truncated, failed_only)
    }

    /// Check chunk results for truncated and failed files, producing validation issues.
    fn validate_file_completeness(
        chunk_results: &[ChunkAnalysisResult],
        total_files: usize,
    ) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Collect all truncated files across chunks (deduplicated)
        let mut all_truncated: Vec<String> = chunk_results
            .iter()
            .flat_map(|r| r.truncated_files.iter().cloned())
            .collect();
        all_truncated.sort();
        all_truncated.dedup();

        // Collect all failed files across chunks (deduplicated)
        let mut all_failed: Vec<String> = chunk_results
            .iter()
            .flat_map(|r| r.failed_files.iter().cloned())
            .collect();
        all_failed.sort();
        all_failed.dedup();

        // Emit a warning for each truncated file
        for path in &all_truncated {
            issues.push(
                ValidationIssue::warning(
                    "TRUNCATED_FILE",
                    format!(
                        "File content was truncated before LLM analysis: {}",
                        path,
                    ),
                )
                .location(path.clone()),
            );
        }

        // Emit a warning for each failed file
        for path in &all_failed {
            issues.push(
                ValidationIssue::warning(
                    "FILE_READ_FAILED",
                    format!(
                        "File could not be read during analysis: {}",
                        path,
                    ),
                )
                .location(path.clone()),
            );
        }

        // If truncation count exceeds threshold, mark analysis as incomplete
        if total_files > 0 {
            let truncation_ratio = all_truncated.len() as f32 / total_files as f32;
            if truncation_ratio > TRUNCATION_INCOMPLETE_THRESHOLD {
                issues.push(ValidationIssue::error(
                    "HIGH_TRUNCATION_RATE",
                    format!(
                        "{}% of files ({}/{}) were truncated during analysis, exceeding {}% threshold; analysis may be incomplete",
                        (truncation_ratio * 100.0) as u32,
                        all_truncated.len(),
                        total_files,
                        (TRUNCATION_INCOMPLETE_THRESHOLD * 100.0) as u32,
                    ),
                ));
            }
        }

        if !all_truncated.is_empty() || !all_failed.is_empty() {
            tracing::info!(
                truncated = all_truncated.len(),
                failed = all_failed.len(),
                total_files = total_files,
                "File completeness validation complete"
            );
        }

        issues
    }

    async fn retry_failed_chunks(
        &self,
        failed: &[FailedChunk],
        config: &crate::config::DistributedAnalysisConfig,
    ) -> Result<Vec<ChunkAnalysisResult>> {
        let chunks: Vec<AnalysisChunk> = failed
            .iter()
            .enumerate()
            .map(|(idx, fc)| {
                let total_lines = fc.files.len() * 100; // Estimate
                AnalysisChunk {
                    chunk_id: format!("completeness-retry-{}", idx),
                    module_path: fc.module_path.clone(),
                    files: fc.files.clone(),
                    total_lines,
                    estimated_tokens: total_lines * TOKENS_PER_LINE,
                    cross_references: Vec::new(),
                }
            })
            .collect();

        let analyzer = DistributedAnalyzer::new(
            Arc::clone(&self.provider),
            config.clone(),
        );
        let result = analyzer
            .analyze_all_chunks(chunks, &self.project_root)
            .await?;

        Ok(result.results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completeness_report_default() {
        let report = CompletenessReport::default();
        assert_eq!(report.initial_coverage, 0.0);
        assert_eq!(report.final_coverage, 0.0);
        assert_eq!(report.files_reanalyzed, 0);
        assert!(report.remaining_gaps.is_empty());
        assert!(report.validation_issues.is_empty());
    }

    #[test]
    fn test_coverage_gap_creation() {
        let gap = AnalysisCoverageGap {
            file_path: "src/main.rs".into(),
            reason: "Truncated".into(),
        };
        assert_eq!(gap.file_path, "src/main.rs");
    }

    #[test]
    fn test_validate_file_completeness_no_issues() {
        let results = vec![ChunkAnalysisResult {
            chunk_id: "chunk-1".into(),
            file_count: 3,
            ..Default::default()
        }];
        let issues = CompletenessValidator::validate_file_completeness(&results, 100);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_validate_file_completeness_truncated_files() {
        let results = vec![
            ChunkAnalysisResult {
                chunk_id: "chunk-1".into(),
                truncated_files: vec!["src/big.rs".into(), "src/huge.rs".into()],
                ..Default::default()
            },
            ChunkAnalysisResult {
                chunk_id: "chunk-2".into(),
                truncated_files: vec!["src/big.rs".into()], // duplicate
                ..Default::default()
            },
        ];
        let issues = CompletenessValidator::validate_file_completeness(&results, 100);

        // 2 unique truncated files => 2 warnings (deduplicated)
        let warnings: Vec<_> = issues.iter().filter(|i| i.severity.is_warning()).collect();
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].code == "TRUNCATED_FILE");
        assert!(warnings[0].location.as_deref() == Some("src/big.rs"));
        assert!(warnings[1].location.as_deref() == Some("src/huge.rs"));

        // 2/100 = 2% < 10% threshold, so no error
        let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_validate_file_completeness_failed_files() {
        let results = vec![ChunkAnalysisResult {
            chunk_id: "chunk-1".into(),
            failed_files: vec!["src/missing.rs".into()],
            ..Default::default()
        }];
        let issues = CompletenessValidator::validate_file_completeness(&results, 50);

        let warnings: Vec<_> = issues.iter().filter(|i| i.severity.is_warning()).collect();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "FILE_READ_FAILED");
        assert!(warnings[0].location.as_deref() == Some("src/missing.rs"));
    }

    #[test]
    fn test_validate_file_completeness_high_truncation_rate() {
        // 6 truncated out of 10 total => 60% > 10% threshold
        let results = vec![ChunkAnalysisResult {
            chunk_id: "chunk-1".into(),
            truncated_files: vec![
                "a.rs".into(),
                "b.rs".into(),
                "c.rs".into(),
                "d.rs".into(),
                "e.rs".into(),
                "f.rs".into(),
            ],
            ..Default::default()
        }];
        let issues = CompletenessValidator::validate_file_completeness(&results, 10);

        // 6 warnings + 1 error
        let warnings: Vec<_> = issues.iter().filter(|i| i.severity.is_warning()).collect();
        assert_eq!(warnings.len(), 6);

        let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "HIGH_TRUNCATION_RATE");
    }

    #[test]
    fn test_validate_file_completeness_zero_total_files() {
        let results = vec![ChunkAnalysisResult {
            chunk_id: "chunk-1".into(),
            truncated_files: vec!["a.rs".into()],
            ..Default::default()
        }];
        // 0 total files should not trigger threshold error (avoid division by zero)
        let issues = CompletenessValidator::validate_file_completeness(&results, 0);
        let errors: Vec<_> = issues.iter().filter(|i| i.is_error()).collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_validate_file_completeness_mixed_truncated_and_failed() {
        let results = vec![ChunkAnalysisResult {
            chunk_id: "chunk-1".into(),
            truncated_files: vec!["src/big.rs".into()],
            failed_files: vec!["src/broken.rs".into()],
            ..Default::default()
        }];
        let issues = CompletenessValidator::validate_file_completeness(&results, 100);
        assert_eq!(issues.len(), 2);

        let truncated: Vec<_> = issues
            .iter()
            .filter(|i| i.code == "TRUNCATED_FILE")
            .collect();
        assert_eq!(truncated.len(), 1);

        let failed: Vec<_> = issues
            .iter()
            .filter(|i| i.code == "FILE_READ_FAILED")
            .collect();
        assert_eq!(failed.len(), 1);
    }

    #[test]
    fn test_collect_problematic_files_deduplication() {
        let chunk_results = vec![
            ChunkAnalysisResult {
                chunk_id: "chunk-1".into(),
                truncated_files: vec!["src/big.rs".into()],
                failed_files: vec!["src/broken.rs".into()],
                ..Default::default()
            },
            ChunkAnalysisResult {
                chunk_id: "chunk-2".into(),
                truncated_files: vec!["src/big.rs".into()], // duplicate
                failed_files: vec!["src/other.rs".into()],
                ..Default::default()
            },
        ];
        let additional = vec![];
        let (truncated, failed) =
            CompletenessValidator::collect_problematic_files(&chunk_results, &additional);

        // src/big.rs should appear once in truncated
        assert_eq!(truncated.len(), 1);
        assert!(truncated.contains(&"src/big.rs".to_string()));

        // failed should have src/broken.rs and src/other.rs
        assert_eq!(failed.len(), 2);
    }

    #[test]
    fn test_collect_problematic_files_no_overlap() {
        // If a file appears in both truncated and failed, it should only be in truncated
        let chunk_results = vec![
            ChunkAnalysisResult {
                chunk_id: "chunk-1".into(),
                truncated_files: vec!["src/both.rs".into()],
                failed_files: vec!["src/both.rs".into()],
                ..Default::default()
            },
        ];
        let additional = vec![];
        let (truncated, failed) =
            CompletenessValidator::collect_problematic_files(&chunk_results, &additional);

        assert_eq!(truncated.len(), 1);
        assert!(truncated.contains(&"src/both.rs".to_string()));

        // "src/both.rs" is in truncated, so it should NOT be in failed
        assert!(failed.is_empty());
    }

    #[test]
    fn test_collect_problematic_files_empty() {
        let chunk_results = vec![ChunkAnalysisResult::default()];
        let additional = vec![];
        let (truncated, failed) =
            CompletenessValidator::collect_problematic_files(&chunk_results, &additional);
        assert!(truncated.is_empty());
        assert!(failed.is_empty());
    }

    #[test]
    fn test_collect_problematic_files_includes_additional_results() {
        let chunk_results = vec![ChunkAnalysisResult {
            chunk_id: "chunk-1".into(),
            truncated_files: vec!["src/a.rs".into()],
            ..Default::default()
        }];
        let additional = vec![ChunkAnalysisResult {
            chunk_id: "retry-1".into(),
            failed_files: vec!["src/b.rs".into()],
            ..Default::default()
        }];
        let (truncated, failed) =
            CompletenessValidator::collect_problematic_files(&chunk_results, &additional);

        assert_eq!(truncated.len(), 1);
        assert!(truncated.contains(&"src/a.rs".to_string()));
        assert_eq!(failed.len(), 1);
        assert!(failed.contains(&"src/b.rs".to_string()));
    }
}
