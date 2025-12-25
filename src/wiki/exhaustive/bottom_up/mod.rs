//! Bottom-Up Analysis
//!
//! File-by-file analysis with hierarchical context building and Deep Research.
//!
//! ## Architecture
//!
//! 1. **Leaf-First Processing**: Low importance files analyzed first
//! 2. **Hierarchical Context**: Parent files receive child documentation
//! 3. **Deep Research**: Important/Core files use multi-turn research workflow
//! 4. **Diagram Validation**: Mermaid diagrams validated and auto-fixed
//! 5. **Token Budget**: Content length managed per tier
//!
//! ## Concurrency Design
//!
//! Uses `DashMap`-based `InsightRegistry` for lock-free insight sharing between
//! concurrent file analysis tasks. This eliminates the RwLock bottleneck at tier
//! transitions where previously all files had to wait for a write lock.

mod file_analyzer;
pub mod file_metrics;
pub mod graph_context;
mod parsers;
pub mod prioritizer;
pub mod prompts;
pub mod types;

// Re-exports
pub use file_metrics::FileMetrics;
pub use graph_context::{FileStructuralContext, GraphContextProvider};
pub use prioritizer::{BatchPrioritizer, PrioritizedFile};
pub use types::*;

use crate::ai::provider::SharedProvider;
use crate::config::ModeConfig;
use crate::storage::SharedDatabase;
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::characterization::profile::ProjectProfile;
use crate::wiki::exhaustive::checkpoint::CheckpointContext;
use dashmap::DashMap;
use file_analyzer::FileAnalyzer;
use futures::stream::StreamExt;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tracing::instrument;

// =============================================================================
// Insight Registry (Lock-Free Shared State)
// =============================================================================

/// Thread-safe registry for analyzed file insights
///
/// Uses DashMap for lock-free concurrent access, eliminating the RwLock
/// bottleneck that occurred at tier transitions.
#[derive(Default)]
pub struct InsightRegistry {
    insights: DashMap<String, FileInsight>,
}

impl InsightRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a single insight (lock-free)
    pub fn register(&self, insight: FileInsight) {
        self.insights.insert(insight.file_path.clone(), insight);
    }

    /// Register multiple insights (lock-free batch operation)
    pub fn register_batch(&self, insights: Vec<FileInsight>) {
        for insight in insights {
            self.insights.insert(insight.file_path.clone(), insight);
        }
    }

    /// Get child contexts for a parent file (lock-free read)
    ///
    /// Returns insights from lower tiers that are in the same module hierarchy
    /// or have import relationships with the target file.
    pub fn get_child_contexts(
        &self,
        file_path: &str,
        tier: ProcessingTier,
    ) -> Vec<ChildDocContext> {
        if !tier.uses_child_context() {
            return Vec::new();
        }

        let file_dir = file_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");

        let mut contexts: Vec<ChildDocContext> = self
            .insights
            .iter()
            .filter(|entry| {
                let insight = entry.value();
                // Lower tier files
                (insight.tier as u8) < (tier as u8)
                    // Same module hierarchy or related import
                    && (insight.file_path.starts_with(file_dir)
                        || insight.related_files.iter().any(|r| r.path == file_path))
            })
            .map(|entry| entry.value().to_child_context())
            .collect();

        // Sort by importance and limit by token budget
        contexts.sort_by(|a, b| b.importance.cmp(&a.importance));

        let mut total_tokens = 0;
        let max_tokens = 2000usize;
        contexts.retain(|ctx| {
            let tokens = ctx.estimated_tokens();
            if total_tokens + tokens <= max_tokens {
                total_tokens += tokens;
                true
            } else {
                false
            }
        });

        contexts
    }

    /// Get an insight by file path
    pub fn get(&self, file_path: &str) -> Option<FileInsight> {
        self.insights.get(file_path).map(|r| r.value().clone())
    }

    /// Get the number of registered insights
    pub fn len(&self) -> usize {
        self.insights.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.insights.is_empty()
    }
}

/// Shared handle to InsightRegistry
pub type SharedInsightRegistry = Arc<InsightRegistry>;

/// Orchestrator for bottom-up analysis
pub struct BottomUpAnalyzer {
    project_root: std::path::PathBuf,
    profile: Arc<ProjectProfile>,
    config: ModeConfig,
    provider: SharedProvider,
    checkpoint: Option<CheckpointContext>,
}

impl BottomUpAnalyzer {
    pub fn new(
        project_root: impl AsRef<Path>,
        profile: Arc<ProjectProfile>,
        config: ModeConfig,
        provider: SharedProvider,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            profile,
            config,
            provider,
            checkpoint: None,
        }
    }

    /// Enable checkpoint/resume with database storage
    pub fn with_checkpoint(mut self, db: SharedDatabase, session_id: String) -> Self {
        self.checkpoint = Some(CheckpointContext::new(db, session_id));
        self
    }

    /// Run bottom-up analysis with hierarchical context
    #[instrument(skip(self, files), fields(file_count = files.len()))]
    pub async fn run(&self, files: Vec<String>) -> Result<Vec<FileInsight>, WeaveError> {
        tracing::info!(
            "Bottom-Up: Starting analysis ({} files, concurrency={})",
            files.len(),
            self.config.bottom_up_concurrency
        );

        // Load already-analyzed files (for resume)
        let completed_files = self.load_completed_files()?;
        if !completed_files.is_empty() {
            tracing::info!(
                "Bottom-Up: Resuming with {} already analyzed files",
                completed_files.len()
            );
        }

        // Filter and validate files
        let (remaining, unreadable_count) = self.filter_files(files, &completed_files).await?;
        if unreadable_count > 0 {
            tracing::info!(
                "Bottom-Up: Skipped {} unreadable files, {} to analyze",
                unreadable_count,
                remaining.len()
            );
        }

        // Prioritize files with metadata
        let prioritizer = BatchPrioritizer::new(&self.profile);
        let prioritized = prioritizer.prioritize_with_metadata(remaining);

        tracing::info!(
            "Bottom-Up: Processing {} files in leaf-first order",
            prioritized.len()
        );

        // Create shared insight registry (lock-free DashMap)
        let registry: SharedInsightRegistry = Arc::new(InsightRegistry::new());

        // Create shared analyzer (immutable, no lock needed)
        let analyzer = Arc::new(FileAnalyzer::new(
            self.project_root.clone(),
            self.profile.clone(),
            self.config.clone(),
            self.provider.clone(),
            self.checkpoint.clone(),
        ));

        // Group files by tier for parallel processing within each tier
        let mut tier_groups: std::collections::HashMap<ProcessingTier, Vec<PrioritizedFile>> =
            std::collections::HashMap::new();
        for pf in prioritized {
            tier_groups.entry(pf.tier).or_default().push(pf);
        }

        let mut all_insights: Vec<FileInsight> = Vec::new();
        let max_concurrency = self.config.bottom_up_concurrency;

        // Process tiers in order: Leaf → Standard → Important → Core
        // (Higher tiers need context from lower tiers)
        for tier in [
            ProcessingTier::Leaf,
            ProcessingTier::Standard,
            ProcessingTier::Important,
            ProcessingTier::Core,
        ] {
            let Some(files) = tier_groups.remove(&tier) else {
                continue;
            };

            tracing::info!(
                "Bottom-Up: Processing tier {:?} ({} files, concurrency={})",
                tier,
                files.len(),
                max_concurrency
            );

            // Process files within this tier in parallel (up to max_concurrency)
            // Insights are registered to the shared registry immediately after analysis
            let tier_insights = self
                .analyze_tier_parallel(&analyzer, &registry, files, max_concurrency)
                .await?;

            all_insights.extend(tier_insights);
        }

        tracing::info!(
            "Bottom-Up: Complete ({} files analyzed)",
            all_insights.len()
        );

        Ok(all_insights)
    }

    /// Analyze files within a single tier in parallel
    ///
    /// Uses lock-free InsightRegistry for concurrent access to analyzed insights.
    async fn analyze_tier_parallel(
        &self,
        analyzer: &Arc<FileAnalyzer>,
        registry: &SharedInsightRegistry,
        files: Vec<PrioritizedFile>,
        max_concurrency: usize,
    ) -> Result<Vec<FileInsight>, WeaveError> {
        let mut results = Vec::with_capacity(files.len());

        // Process files with concurrency limit using buffer_unordered
        let mut stream = futures::stream::iter(files)
            .map(|pf| {
                let analyzer = Arc::clone(analyzer);
                let registry = Arc::clone(registry);
                async move {
                    let path = pf.path;
                    let tier = pf.tier;

                    // Get child contexts from registry (lock-free)
                    let child_contexts = registry.get_child_contexts(&path, tier);

                    let request = AnalysisRequest::new(path.clone(), tier)
                        .with_child_contexts(child_contexts);

                    // Analyze file (no lock needed - analyzer is immutable)
                    let insight_result = analyzer.analyze(request).await;

                    match insight_result {
                        Ok(insight) => {
                            // Store checkpoint
                            if let Err(e) = analyzer.store_insight(&insight) {
                                tracing::warn!("Failed to store insight for {}: {}", path, e);
                            }

                            // Register insight for child context (lock-free)
                            registry.register(insight.clone());

                            Ok(insight)
                        }
                        Err(e) => {
                            if let Err(store_err) = analyzer.mark_unanalyzed(&path, &e.to_string())
                            {
                                tracing::warn!(
                                    "Failed to mark {} as unanalyzed: {}",
                                    path,
                                    store_err
                                );
                            }
                            Err((path, e))
                        }
                    }
                }
            })
            .buffer_unordered(max_concurrency);

        while let Some(result) = stream.next().await {
            match result {
                Ok(insight) => results.push(insight),
                Err((path, e)) => {
                    tracing::warn!("Failed to analyze {}: {}", path, e);
                }
            }
        }

        Ok(results)
    }

    /// Filter files and validate readability
    async fn filter_files(
        &self,
        files: Vec<String>,
        completed: &HashSet<String>,
    ) -> Result<(Vec<String>, usize), WeaveError> {
        let mut remaining = Vec::new();
        let mut unreadable_count = 0;

        for file in files {
            if completed.contains(&file) {
                continue;
            }

            match self.validate_file_readable(&file).await {
                Ok(true) => remaining.push(file),
                Ok(false) => {
                    tracing::debug!("File not readable (binary or empty): {}", file);
                    self.mark_file_unanalyzed(&file, "Binary or empty file")?;
                    unreadable_count += 1;
                }
                Err(e) => {
                    tracing::debug!("Cannot read file {}: {}", file, e);
                    self.mark_file_unanalyzed(&file, &e.to_string())?;
                    unreadable_count += 1;
                }
            }
        }

        Ok((remaining, unreadable_count))
    }

    /// Check if a file is readable for analysis
    async fn validate_file_readable(&self, file_path: &str) -> Result<bool, WeaveError> {
        let full_path = self.project_root.join(file_path);

        let metadata = match tokio::fs::metadata(&full_path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("File not found: {}", file_path),
                )
                .into());
            }
            Err(e) => return Err(e.into()),
        };

        if !metadata.is_file() {
            return Ok(false);
        }

        if metadata.len() == 0 {
            return Ok(false);
        }

        // Check for binary content using spawn_blocking (small read)
        let path = full_path.clone();
        let is_binary = tokio::task::spawn_blocking(move || {
            let mut file = std::fs::File::open(&path)?;
            let mut buffer = [0u8; 512];
            let bytes_read = std::io::Read::read(&mut file, &mut buffer)?;
            Ok::<bool, std::io::Error>(buffer[..bytes_read].contains(&0))
        })
        .await
        .map_err(|e| WeaveError::Io(std::io::Error::other(e.to_string())))??;

        Ok(!is_binary)
    }

    /// Mark a file as unanalyzed
    fn mark_file_unanalyzed(&self, file_path: &str, reason: &str) -> Result<(), WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(());
        };

        let now = chrono::Utc::now().to_rfc3339();
        ctx.db.execute(
            "INSERT OR REPLACE INTO file_tracking
             (file_path, session_id, content_hash, line_count, status, error_message, discovered_at)
             VALUES (?1, ?2, '', 0, 'unanalyzed', ?3, ?4)",
            &[
                &file_path.to_string(),
                &ctx.session_id,
                &reason.to_string(),
                &now,
            ],
        )?;

        Ok(())
    }

    /// Load set of already-analyzed file paths (for resume)
    fn load_completed_files(&self) -> Result<HashSet<String>, WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(HashSet::new());
        };

        let conn = ctx.db.connection()?;
        let mut stmt = conn.prepare(
            "SELECT file_path FROM file_tracking WHERE session_id = ?1 AND status = 'analyzed'",
        )?;

        let files: HashSet<String> = stmt
            .query_map([&ctx.session_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processing_tier_order() {
        assert!((ProcessingTier::Leaf as u8) < (ProcessingTier::Standard as u8));
        assert!((ProcessingTier::Standard as u8) < (ProcessingTier::Important as u8));
        assert!((ProcessingTier::Important as u8) < (ProcessingTier::Core as u8));
    }

    #[test]
    fn test_tier_deep_research() {
        // Leaf and Standard don't use Deep Research
        assert!(!ProcessingTier::Leaf.uses_deep_research());
        assert!(!ProcessingTier::Standard.uses_deep_research());

        // Important and Core use Deep Research
        assert!(ProcessingTier::Important.uses_deep_research());
        assert!(ProcessingTier::Core.uses_deep_research());

        // Research iterations: Important=3, Core=4
        assert_eq!(ProcessingTier::Important.research_iterations(), 3);
        assert_eq!(ProcessingTier::Core.research_iterations(), 4);
    }

    #[test]
    fn test_tier_uses_child_context() {
        assert!(!ProcessingTier::Leaf.uses_child_context());
        assert!(!ProcessingTier::Standard.uses_child_context());
        assert!(ProcessingTier::Important.uses_child_context());
        assert!(ProcessingTier::Core.uses_child_context());
    }
}
