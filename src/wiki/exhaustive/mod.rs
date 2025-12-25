//! Exhaustive Documentation Pipeline
//!
//! AI-driven documentation generation using multi-agent architecture.
//!
//! ## Pipeline Architecture
//!
//! ```text
//! Characterization  → Bottom-Up Analysis → Top-Down Analysis
//!                           ↓                    ↓
//!                     Consolidation ← ← ← ← ← ← ←
//!                           ↓
//!                      Refinement → Wiki Output
//! ```
//!
//! ## Phases
//!
//! 1. **Characterization**: Multi-turn agent discovery of project traits
//! 2. **Bottom-Up**: File-level value extraction with profile context
//! 3. **Top-Down**: Project-level insight synthesis
//! 4. **Consolidation**: Domain grouping and conflict resolution
//! 5. **Refinement**: Quality-driven iterative enhancement
//!
//! ## Guarantees
//!
//! - 100% file coverage (every source file tracked)
//! - 100% fact-based (all claims from actual code)
//! - Universal (language/framework agnostic)
//! - AI-driven flexible structure

// Utilities
mod llms_txt;
mod mermaid;
mod patterns;
pub mod session_context;
mod types;

// Core Infrastructure
pub mod checkpoint;

// Pipeline Modules
pub mod bottom_up;
pub mod characterization;
pub mod consolidation;
pub mod documentation;
pub mod refinement;
pub mod research;
pub mod top_down;

// Public exports
pub use checkpoint::{CheckpointContext, CheckpointManager, PipelinePhase};
pub use llms_txt::LlmsTxtGenerator;
pub use mermaid::{MermaidValidation, MermaidValidator};
pub use patterns::PatternExtractor;
pub use session_context::{SessionContext, TierAntiPatterns};
pub use types::{
    CHECKPOINT_VERSION, CheckpointError, Complexity, DocSession, Importance, PipelineCheckpoint,
    SessionStatus, ValueCategory,
};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{info, instrument};

use crate::ai::budget::{SharedBudget, create_shared_budget};
use crate::ai::metrics::{SharedMetrics, create_shared_metrics};
use crate::ai::provider::SharedProvider;
use crate::analyzer::scanner::FileScanner;
use crate::constants::budget as budget_constants;
use crate::storage::SharedDatabase;
use crate::types::Result;

// =============================================================================
// Multi-Agent Pipeline
// =============================================================================

use crate::config::{AnalysisMode, ProjectScale, get_mode_config};
use bottom_up::BottomUpAnalyzer;
use characterization::CharacterizationAnalyzer;
use consolidation::ConsolidationAnalyzer;
use refinement::RefinementAnalyzer;
use top_down::TopDownAnalyzer;

/// Configuration for the multi-agent pipeline
#[derive(Debug, Clone)]
pub struct MultiAgentConfig {
    /// Analysis mode (Fast, Standard, Deep)
    pub mode: AnalysisMode,
    /// Scale override (None = auto-detect)
    pub scale_override: Option<ProjectScale>,
    /// Quality target override
    pub quality_target_override: Option<f32>,
    /// Max turns override
    pub max_turns_override: Option<u8>,
    /// Show progress
    pub show_progress: bool,
    /// Verbose output
    pub verbose: bool,
    /// Dry run (show config only)
    pub dry_run: bool,
}

impl Default for MultiAgentConfig {
    fn default() -> Self {
        Self {
            mode: AnalysisMode::Standard,
            scale_override: None,
            quality_target_override: None,
            max_turns_override: None,
            show_progress: true,
            verbose: false,
            dry_run: false,
        }
    }
}

/// Result of multi-agent pipeline execution
#[derive(Debug, Clone)]
pub struct MultiAgentResult {
    pub session_id: String,
    pub quality_score: f32,
    pub quality_target: f32,
    pub target_met: bool,
    pub refinement_turns: usize,
    pub pages_generated: usize,
    pub files_analyzed: usize,
    pub output_path: String,
    pub duration_secs: u64,
    /// Total tokens consumed
    pub tokens_consumed: u64,
    /// Token budget utilization (0.0-1.0)
    pub budget_utilization: f64,
    /// Estimated cost in USD (based on model pricing)
    pub estimated_cost_usd: f64,
}

/// Multi-Agent Pipeline orchestrator
///
/// Pipeline stages:
/// 1. Characterization: Multi-turn project discovery
/// 2. Bottom-Up: File-level value extraction
/// 3. Top-Down: Project-level insight synthesis
/// 4. Consolidation: Domain grouping and conflict resolution
/// 5. Refinement: Quality-driven iterative enhancement
pub struct MultiAgentPipeline {
    /// Database for checkpoint/resume
    db: SharedDatabase,
    /// Session ID for checkpoint tracking
    session_id: String,
    provider: SharedProvider,
    project_root: PathBuf,
    output_path: PathBuf,
    config: MultiAgentConfig,
    /// Global token budget for the entire pipeline
    budget: SharedBudget,
    /// Pipeline metrics collector
    metrics: SharedMetrics,
}

impl MultiAgentPipeline {
    /// Create a new pipeline (fresh start)
    pub fn new(
        db: SharedDatabase,
        provider: SharedProvider,
        project_root: &Path,
        output_path: &Path,
    ) -> Self {
        let budget = create_shared_budget(budget_constants::DEFAULT_BUDGET);
        let session_id = uuid::Uuid::new_v4().to_string();
        let metrics = create_shared_metrics(&session_id);
        Self {
            db,
            session_id,
            provider,
            project_root: project_root.to_path_buf(),
            output_path: output_path.to_path_buf(),
            config: MultiAgentConfig::default(),
            budget,
            metrics,
        }
    }

    /// Create a pipeline to resume an existing session
    pub fn resume_session(
        db: SharedDatabase,
        session_id: String,
        provider: SharedProvider,
        project_root: &Path,
        output_path: &Path,
    ) -> Self {
        let budget = create_shared_budget(budget_constants::DEFAULT_BUDGET);
        let metrics = create_shared_metrics(&session_id);
        Self {
            db,
            session_id,
            provider,
            project_root: project_root.to_path_buf(),
            output_path: output_path.to_path_buf(),
            config: MultiAgentConfig::default(),
            budget,
            metrics,
        }
    }

    /// Set custom token budget
    pub fn with_budget(mut self, total_tokens: u64) -> Self {
        self.budget = create_shared_budget(total_tokens);
        self
    }

    pub fn with_config(mut self, config: MultiAgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Get session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get metrics collector for recording LLM responses
    pub fn metrics(&self) -> &SharedMetrics {
        &self.metrics
    }

    /// Create session in database for checkpoint tracking
    /// Uses canonical path for consistent session lookup
    fn create_session(&self) -> Result<()> {
        // Use canonical path for consistent session lookup across invocations
        let canonical_path = self
            .project_root
            .canonicalize()
            .unwrap_or_else(|_| self.project_root.clone())
            .to_string_lossy()
            .to_string();

        let now = chrono::Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT OR REPLACE INTO doc_sessions (id, project_path, status, started_at) VALUES (?1, ?2, ?3, ?4)",
            &[&self.session_id, &canonical_path, &"running".to_string(), &now],
        )?;
        tracing::info!(
            "Created session {} for project {}",
            self.session_id,
            canonical_path
        );
        Ok(())
    }

    /// Mark session as completed
    fn complete_session(&self) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db.execute(
            "UPDATE doc_sessions SET status = ?2, completed_at = ?3, last_checkpoint_at = ?3 WHERE id = ?1",
            &[&self.session_id, &"completed".to_string(), &now],
        )?;
        tracing::info!("Session {} completed", self.session_id);
        Ok(())
    }

    /// Mark session as failed
    fn fail_session(&self, error: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db.execute(
            "UPDATE doc_sessions SET status = ?2, last_error = ?3, last_checkpoint_at = ?4 WHERE id = ?1",
            &[&self.session_id, &"failed".to_string(), &error.to_string(), &now],
        )?;
        tracing::warn!("Session {} failed: {}", self.session_id, error);
        Ok(())
    }

    /// Load pipeline checkpoint from database (Database-First approach)
    ///
    /// Uses database tables as the authoritative source for checkpoint state.
    /// Falls back to JSON blob only if tables don't have sufficient data.
    pub fn load_checkpoint(&self) -> Result<Option<PipelineCheckpoint>> {
        // First, try Database-First approach: load from tables
        match self.db.load_checkpoint_state(&self.session_id) {
            Ok(state) if state.last_completed_phase > 0 || state.total_files > 0 => {
                tracing::info!(
                    "Loaded checkpoint from tables: phase={}, files={}/{}",
                    state.last_completed_phase,
                    state.analyzed_files,
                    state.total_files
                );

                // Build PipelineCheckpoint from table state
                let checkpoint = PipelineCheckpoint {
                    version: CHECKPOINT_VERSION,
                    checksum: 0, // Not using checksum for table-based loading
                    files: state.files,
                    project_profile_json: if state.has_project_profile {
                        self.db
                            .load_session_profile(&self.session_id)?
                            .map(|v| v.to_string())
                    } else {
                        None
                    },
                    file_insights_json: None, // Loaded on-demand from file_analysis table
                    project_insights_json: None, // Loaded on-demand from module_summaries
                    domain_insights_json: None, // Loaded on-demand from domain_summaries
                    documentation_blueprint_json: None, // Loaded on-demand if exists
                    last_completed_phase: state.last_completed_phase,
                    checkpoint_at: self
                        .db
                        .get_last_checkpoint_time(&self.session_id)?
                        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
                };

                return Ok(Some(checkpoint));
            }
            Ok(_) => {
                tracing::debug!("No checkpoint data in tables, trying JSON blob fallback");
            }
            Err(e) => {
                tracing::debug!(
                    "Table-based checkpoint load failed: {}, trying JSON blob",
                    e
                );
            }
        }

        // Fallback: try JSON blob (for backward compatibility)
        let conn = self.db.connection()?;
        let result: std::result::Result<String, _> = conn.query_row(
            "SELECT checkpoint_data FROM doc_sessions WHERE id = ?1",
            [&self.session_id],
            |row| row.get(0),
        );

        match result {
            Ok(json) => match PipelineCheckpoint::from_json(&json) {
                Ok(checkpoint) => {
                    tracing::info!(
                        "Loaded checkpoint from JSON blob (legacy): phase={}, files={}",
                        checkpoint.last_completed_phase,
                        checkpoint.files.len()
                    );
                    Ok(Some(checkpoint))
                }
                Err(e) => {
                    tracing::warn!(
                        "Checkpoint JSON validation failed for session {}: {}",
                        self.session_id,
                        e
                    );
                    // Don't fail - return None to allow fresh start
                    Ok(None)
                }
            },
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(rusqlite::Error::InvalidColumnType(_, _, _)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Auto-detect project scale from file count
    pub fn detect_scale(&self) -> ProjectScale {
        let count = FileScanner::source_files(&self.project_root).count();
        ProjectScale::from_file_count(count)
    }

    /// Run the multi-agent pipeline (fresh start)
    #[instrument(skip(self), fields(project = %self.project_root.display()))]
    pub async fn run(&self) -> Result<MultiAgentResult> {
        self.run_internal(None).await
    }

    /// Resume pipeline from a checkpoint
    #[instrument(skip(self, checkpoint), fields(resume_from = checkpoint.last_completed_phase))]
    pub async fn resume(&self, checkpoint: PipelineCheckpoint) -> Result<MultiAgentResult> {
        self.run_internal(Some(checkpoint)).await
    }

    /// Internal pipeline execution with optional checkpoint for resume
    async fn run_internal(
        &self,
        checkpoint: Option<PipelineCheckpoint>,
    ) -> Result<MultiAgentResult> {
        let start_time = std::time::Instant::now();
        let mut checkpoint = checkpoint.unwrap_or_default();
        let resume_from = checkpoint.last_completed_phase;

        // Determine scale
        let scale = self
            .config
            .scale_override
            .unwrap_or_else(|| self.detect_scale());
        let mode = self.config.mode;
        let mode_config = get_mode_config(mode, scale);

        // Apply overrides
        let quality_target = self
            .config
            .quality_target_override
            .unwrap_or(mode_config.refinement_quality_target);
        let max_turns = self
            .config
            .max_turns_override
            .map(|t| t as usize)
            .unwrap_or(mode_config.refinement_max_turns);

        if resume_from > 0 {
            info!(
                "Multi-Agent Pipeline: Resuming from phase {} (mode={}, scale={})",
                resume_from, mode, scale
            );
        } else {
            info!(
                "Multi-Agent Pipeline: Starting (mode={}, scale={}, target={:.0}%)",
                mode,
                scale,
                quality_target * 100.0
            );
        }

        if self.config.dry_run {
            info!("Dry run mode - showing configuration only");
            return Ok(MultiAgentResult {
                session_id: "dry-run".to_string(),
                quality_score: 0.0,
                quality_target,
                target_met: false,
                refinement_turns: 0,
                pages_generated: 0,
                files_analyzed: 0,
                output_path: self.output_path.to_string_lossy().to_string(),
                duration_secs: 0,
                tokens_consumed: 0,
                budget_utilization: 0.0,
                estimated_cost_usd: 0.0,
            });
        }

        // Create session if fresh start
        if resume_from == 0 {
            self.create_session()?;
        }

        // Use CheckpointManager for consistent checkpoint operations
        let checkpoint_mgr = CheckpointManager::new(self.db.clone(), self.session_id.clone());

        // ===== PHASE 1: Characterization =====
        let profile = if resume_from < 1 {
            info!("Phase 1: Starting project characterization");
            let char_analyzer = CharacterizationAnalyzer::new(
                &self.project_root,
                mode,
                scale,
                mode_config.clone(),
                self.provider.clone(),
            )
            .with_checkpoint(self.db.clone(), self.session_id.clone());
            let profile = char_analyzer.run().await?;

            checkpoint_mgr.complete_phase(PipelinePhase::Characterization, &mut checkpoint)?;

            Arc::new(profile)
        } else {
            info!("Phase 1: Skipped (resuming from checkpoint)");
            let profile = self.load_project_profile()?.ok_or_else(|| {
                crate::types::WeaveError::Session("No profile in checkpoint".to_string())
            })?;
            Arc::new(profile)
        };

        // ===== PHASE 2: File Discovery =====
        let files = if resume_from < 2 {
            info!("Phase 2: Discovering source files");
            let files = FileScanner::source_files(&self.project_root)
                .paths()
                .unwrap_or_default();
            info!("Found {} source files", files.len());

            checkpoint.files = files.clone();
            checkpoint_mgr.complete_phase_with_counts(
                PipelinePhase::FileDiscovery,
                &mut checkpoint,
                Some(files.len()),
                None,
            )?;

            files
        } else {
            info!("Phase 2: Skipped (resuming from checkpoint)");
            checkpoint.files.clone()
        };
        let file_count = files.len();

        // ===== PHASE 3: Bottom-Up Analysis =====
        let file_insights = if resume_from < 3 {
            info!("Phase 3: Bottom-up file analysis");
            let bottom_up = BottomUpAnalyzer::new(
                &self.project_root,
                profile.clone(),
                mode_config.clone(),
                self.provider.clone(),
            )
            .with_checkpoint(self.db.clone(), self.session_id.clone());
            let insights = bottom_up.run(files).await?;

            checkpoint.file_insights_json = Some(serde_json::to_string(&insights)?);
            checkpoint_mgr.complete_phase_with_counts(
                PipelinePhase::BottomUp,
                &mut checkpoint,
                None,
                Some(insights.len()),
            )?;

            insights
        } else {
            info!("Phase 3: Skipped (resuming from checkpoint)");
            checkpoint
                .file_insights_json
                .as_ref()
                .and_then(|json| serde_json::from_str(json).ok())
                .ok_or_else(|| {
                    crate::types::WeaveError::Session("No file insights in checkpoint".to_string())
                })?
        };

        // ===== PHASE 4: Top-Down Analysis =====
        let project_insights = if resume_from < 4 {
            info!("Phase 4: Top-down project analysis");
            let top_down = TopDownAnalyzer::new(
                &self.project_root,
                profile.clone(),
                mode_config.clone(),
                self.provider.clone(),
            )
            .with_checkpoint(self.db.clone(), self.session_id.clone());
            let insights = top_down.run(&file_insights).await?;

            checkpoint.project_insights_json = Some(serde_json::to_string(&insights)?);
            checkpoint_mgr.complete_phase(PipelinePhase::TopDown, &mut checkpoint)?;

            insights
        } else {
            info!("Phase 4: Skipped (resuming from checkpoint)");
            checkpoint
                .project_insights_json
                .as_ref()
                .and_then(|json| serde_json::from_str(json).ok())
                .ok_or_else(|| {
                    crate::types::WeaveError::Session(
                        "No project insights in checkpoint".to_string(),
                    )
                })?
        };

        // ===== PHASE 5: Consolidation =====
        let domain_insights = if resume_from < 5 {
            info!("Phase 5: Consolidation and domain grouping");
            let consolidation = ConsolidationAnalyzer::new(profile.clone(), self.provider.clone())
                .with_checkpoint(self.db.clone(), self.session_id.clone());
            let insights = consolidation.run(file_insights, project_insights).await?;

            checkpoint.domain_insights_json = Some(serde_json::to_string(&insights)?);
            checkpoint_mgr.complete_phase(PipelinePhase::Consolidation, &mut checkpoint)?;

            insights
        } else {
            info!("Phase 5: Skipped (resuming from checkpoint)");
            checkpoint
                .domain_insights_json
                .as_ref()
                .and_then(|json| serde_json::from_str(json).ok())
                .ok_or_else(|| {
                    crate::types::WeaveError::Session(
                        "No domain insights in checkpoint".to_string(),
                    )
                })?
        };

        // ===== PHASE 5.5: Documentation Structure Discovery =====
        let doc_blueprint = if checkpoint.documentation_blueprint_json.is_none() {
            info!("Phase 5.5: Discovering optimal documentation structure");
            let structure_agent =
                documentation::DocumentationStructureAgent::new(self.provider.clone());
            let blueprint = structure_agent.discover(&profile, &domain_insights).await?;

            checkpoint.documentation_blueprint_json = Some(serde_json::to_string(&blueprint)?);
            checkpoint_mgr.save_checkpoint(&checkpoint)?;

            info!(
                "Documentation blueprint created: {} estimated pages, depth {}",
                blueprint.estimated_pages, blueprint.hierarchy_depth
            );

            blueprint
        } else {
            info!("Phase 5.5: Skipped (resuming from checkpoint)");
            checkpoint
                .documentation_blueprint_json
                .as_ref()
                .and_then(|json| serde_json::from_str(json).ok())
                .ok_or_else(|| {
                    crate::types::WeaveError::Session(
                        "No documentation blueprint in checkpoint".to_string(),
                    )
                })?
        };

        // ===== PHASE 6: Refinement & Documentation =====
        info!("Phase 6: Refinement and documentation generation");
        let mut adjusted_config = mode_config.clone();
        adjusted_config.refinement_quality_target = quality_target;
        adjusted_config.refinement_max_turns = max_turns;

        let refinement = RefinementAnalyzer::new(&self.project_root, adjusted_config, mode, scale)
            .with_checkpoint(self.db.clone(), self.session_id.clone());
        let refinement_insight = refinement.run(domain_insights).await?;

        // Load project_insights from checkpoint for hierarchical generator
        let project_insights_for_gen: Vec<top_down::ProjectInsight> = checkpoint
            .project_insights_json
            .as_ref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();

        // Generate hierarchical documentation using blueprint
        info!("Generating hierarchical documentation from blueprint");
        let hierarchical_gen =
            documentation::HierarchicalDocGenerator::new(documentation::GeneratorConfig::default());
        let hierarchical_output = hierarchical_gen.generate(
            &doc_blueprint,
            &profile,
            &refinement_insight.domain_insights,
            &project_insights_for_gen,
        )?;

        // Write all generated files to disk
        let mut total_files_written = 0;
        for (path, content) in &hierarchical_output.files {
            let file_path = self.output_path.join(path);
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&file_path, content)?;
            total_files_written += 1;
        }

        info!(
            "Generated {} hierarchical documentation files ({} pages, {} words)",
            total_files_written,
            hierarchical_output.stats.total_pages,
            hierarchical_output.stats.total_words
        );

        // Also generate legacy flat documentation for backward compatibility
        let doc_gen = refinement::doc_generator::DocGenerator::new(&self.output_path);
        let _generated_files = doc_gen
            .generate(&refinement_insight.domain_insights)
            .await?;

        let final_score = refinement_insight
            .quality_scores
            .last()
            .map(|s| s.overall())
            .unwrap_or(0.0);

        // Generate architecture documentation from top-down insights
        info!("Generating architecture documentation from top-down insights");
        use refinement::architecture_docs::ArchitectureDocGenerator;

        let arch_md =
            ArchitectureDocGenerator::generate_architecture_md(&profile, &project_insights_for_gen);
        let arch_path = self.output_path.join("architecture.md");
        if let Err(e) = std::fs::write(&arch_path, arch_md) {
            tracing::warn!("Failed to write architecture.md: {}", e);
        } else {
            info!("Generated architecture.md at {}", arch_path.display());
        }

        let risks_md =
            ArchitectureDocGenerator::generate_risks_md(&profile, &project_insights_for_gen);
        let risks_path = self.output_path.join("risks.md");
        if let Err(e) = std::fs::write(&risks_path, risks_md) {
            tracing::warn!("Failed to write risks.md: {}", e);
        } else {
            info!("Generated risks.md at {}", risks_path.display());
        }

        let flows_md =
            ArchitectureDocGenerator::generate_flows_md(&profile, &project_insights_for_gen);
        let flows_path = self.output_path.join("flows.md");
        if let Err(e) = std::fs::write(&flows_path, flows_md) {
            tracing::warn!("Failed to write flows.md: {}", e);
        } else {
            info!("Generated flows.md at {}", flows_path.display());
        }

        let terminology_md =
            ArchitectureDocGenerator::generate_terminology_md(&profile, &project_insights_for_gen);
        let terminology_path = self.output_path.join("terminology.md");
        if let Err(e) = std::fs::write(&terminology_path, terminology_md) {
            tracing::warn!("Failed to write terminology.md: {}", e);
        } else {
            info!("Generated terminology.md at {}", terminology_path.display());
        }

        // Load file_insights from checkpoint for additional generators
        let file_insights_for_extra: Vec<bottom_up::FileInsight> = checkpoint
            .file_insights_json
            .as_ref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();

        // Generate llms.txt
        if !file_insights_for_extra.is_empty() {
            let project_name = profile.name.clone();
            let description = profile.purposes.first().cloned();
            let llms_gen = LlmsTxtGenerator::new(&project_name);
            let llms_gen = if let Some(desc) = description {
                llms_gen.with_description(&desc)
            } else {
                llms_gen
            };

            // Create minimal session data for llms.txt
            let mut session = DocSession::new(
                self.project_root.to_string_lossy().to_string(),
                &self.config.mode.to_string(),
                &scale.to_string(),
            );
            session.files_analyzed = file_count;
            session.quality_score = final_score;

            if let Err(e) = llms_gen.write(&session, &file_insights_for_extra, &self.output_path) {
                tracing::warn!("Failed to generate llms.txt: {}", e);
            }

            // Generate patterns.md and constitution.md
            let patterns = PatternExtractor::extract_patterns(&file_insights_for_extra);
            let constitution = PatternExtractor::infer_constitution(&file_insights_for_extra);

            if !patterns.is_empty() {
                let patterns_md = PatternExtractor::generate_patterns_md(&patterns);
                let patterns_path = self.output_path.join("patterns.md");
                if let Err(e) = std::fs::write(&patterns_path, patterns_md) {
                    tracing::warn!("Failed to write patterns.md: {}", e);
                } else {
                    info!("Generated patterns.md at {}", patterns_path.display());
                }
            }

            if !constitution.naming_conventions.is_empty()
                || !constitution.file_organization.is_empty()
                || !constitution.code_style.is_empty()
            {
                let constitution_md = PatternExtractor::generate_constitution_md(&constitution);
                let constitution_path = self.output_path.join("constitution.md");
                if let Err(e) = std::fs::write(&constitution_path, constitution_md) {
                    tracing::warn!("Failed to write constitution.md: {}", e);
                } else {
                    info!(
                        "Generated constitution.md at {}",
                        constitution_path.display()
                    );
                }
            }
        }

        let duration = start_time.elapsed();
        let budget_stats = self.budget.stats();
        let metrics_summary = self.metrics.summary();

        // Mark session as completed
        self.complete_session()?;

        info!(
            "Multi-Agent Pipeline: Complete (score={:.1}%, target={:.1}%, pages={}, tokens={})",
            final_score * 100.0,
            quality_target * 100.0,
            hierarchical_output.stats.total_pages,
            budget_stats.consumed
        );
        info!("Budget: {}", budget_stats.summary());
        if metrics_summary.total_cost_usd > 0.0 {
            info!("Cost: ${:.4}", metrics_summary.total_cost_usd);
        }

        Ok(MultiAgentResult {
            session_id: self.session_id.clone(),
            quality_score: final_score,
            quality_target,
            target_met: refinement_insight.target_met,
            refinement_turns: refinement_insight.turns_used,
            pages_generated: hierarchical_output.stats.total_pages,
            files_analyzed: file_count,
            output_path: self.output_path.to_string_lossy().to_string(),
            duration_secs: duration.as_secs(),
            tokens_consumed: budget_stats.consumed,
            budget_utilization: budget_stats.utilization,
            estimated_cost_usd: metrics_summary.total_cost_usd,
        })
    }

    /// Run the pipeline with proper error handling and session lifecycle
    pub async fn run_with_recovery(&self) -> Result<MultiAgentResult> {
        match self.run().await {
            Ok(result) => Ok(result),
            Err(e) => {
                // Mark session as failed so it can be resumed
                if let Err(session_err) = self.fail_session(&e.to_string()) {
                    tracing::error!(
                        "Failed to mark session as failed: {}. Original error: {}",
                        session_err,
                        e
                    );
                }
                Err(e)
            }
        }
    }

    /// Load project profile from database (saved during characterization)
    fn load_project_profile(&self) -> Result<Option<characterization::ProjectProfile>> {
        let conn = self.db.connection()?;

        let result: std::result::Result<String, _> = conn.query_row(
            "SELECT project_profile FROM doc_sessions WHERE id = ?1",
            [&self.session_id],
            |row| row.get(0),
        );

        match result {
            Ok(json) => {
                let profile: characterization::ProjectProfile = serde_json::from_str(&json)?;
                Ok(Some(profile))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(rusqlite::Error::InvalidColumnType(_, _, _)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
