//! Quality Loop - Outer Pipeline Verification Loop with Durable Execution
//!
//! Orchestrates the complete generation pipeline with quality gates:
//! 1. Analysis Quality Gate: Ensure analysis confidence meets threshold
//! 2. Synthesis Quality Gate: Ensure synthesis confidence meets threshold
//! 3. Evidence Quality Gate: Validate file references against VerifiedFileRegistry
//! 4. 3-Layer Validation: Tier Filter → Consistency → Cross-Artifact
//! 5. Deep Review: Two-pass LLM review (optional)
//!
//! Durable Execution Features:
//! - Periodic checkpointing for crash recovery
//! - Lock file for concurrent execution prevention
//! - Progress persistence across sessions

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::ai::{LlmProvider, ProviderSet, phase_id, with_timeout};
use crate::config::{AnalysisDepth, Config};
use crate::types::Result;

use super::adaptive::{AdaptivePipeline, AdaptivePipelineOutput};
use super::checkpoint::{
    CheckpointManager, CrashRecovery, ExecutionCheckpoint, GeneratedArtifacts, PipelinePhase,
    QualitySnapshot, RecoveryResult,
};
use super::context::ClaudegenContext;
use super::context::VerifiedFileRegistry;
use super::deep_review::{DeepReviewEngine, ReviewArtifacts, TwoPassResult};
// Simplified: ValidationPipeline replaced with LLM Judge in quality module

/// Simplified clean pass status
#[derive(Debug, Clone)]
pub enum CleanPassStatus {
    InProgress { streak: usize, required: usize },
    Converged { passes: usize },
    Failed { reason: FailureReason },
}

/// Simplified failure reason
#[derive(Debug, Clone)]
pub enum FailureReason {
    MaxAttemptsReached,
    QualityBelow { score: f32, threshold: f32 },
}

/// Simplified validation results
#[derive(Debug, Clone, Default)]
pub struct ValidationResults {
    pub total_issues: usize,
    pub error_count: usize,
    pub warning_count: usize,
}

impl ValidationResults {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error_issues(&self) -> usize {
        self.error_count
    }
}

#[derive(Debug, Clone)]
pub struct QualityLoopResult {
    pub output: AdaptivePipelineOutput,
    pub outer_iterations: usize,
    pub analysis_rerun_count: usize,
    pub final_confidence: f32,
    pub gaps_discovered: Vec<DiscoveredGap>,
    pub deep_review_passed: bool,
    pub deep_review_attempts: u32,
    pub validation_results: Option<ValidationResults>,
    pub clean_pass_status: CleanPassStatus,
}

#[derive(Debug, Clone)]
pub struct DiscoveredGap {
    pub area: String,
    pub description: String,
    pub iteration_found: usize,
}

pub struct QualityLoop {
    project_root: PathBuf,
    output_dir: Option<PathBuf>,
    providers: ProviderSet,
    config: Config,
    file_registry: OnceCell<VerifiedFileRegistry>,
    budget: Option<crate::ai::budget::SharedBudget>,
    metrics: Option<crate::ai::metrics::SharedMetrics>,
}

impl QualityLoop {
    /// Create a new QualityLoop with tiered providers for phase-based model routing
    pub fn new(project_root: PathBuf, providers: ProviderSet, config: Config) -> Self {
        Self {
            project_root,
            output_dir: None,
            providers,
            config,
            file_registry: OnceCell::new(),
            budget: None,
            metrics: None,
        }
    }

    /// Enable budget tracking for checkpoint persistence
    pub fn with_budget(mut self, budget: crate::ai::budget::SharedBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Enable metrics tracking for checkpoint persistence
    pub fn with_metrics(mut self, metrics: crate::ai::metrics::SharedMetrics) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Create a QualityLoop with a single provider for all phases (backward compatibility)
    ///
    /// **Warning**: This path does NOT include ProviderChain resilience (circuit breaker, retry).
    /// For production use, prefer `QualityLoop::new()` with a ProviderSet from `create_provider_set()`.
    pub fn with_single_provider(
        project_root: PathBuf,
        provider: Arc<dyn LlmProvider>,
        config: Config,
    ) -> Self {
        Self::new(project_root, ProviderSet::single(provider), config)
    }

    pub fn with_output_dir(mut self, output_dir: PathBuf) -> Self {
        self.output_dir = Some(output_dir);
        self
    }

    pub fn with_resume(mut self, resume: bool) -> Self {
        self.config.performance.resume_on_crash = resume;
        self
    }

    /// Update checkpoint with current budget and metrics state
    fn update_checkpoint_stats(&self, checkpoint: &mut ExecutionCheckpoint) {
        // Update budget stats
        if let Some(ref budget) = self.budget {
            let stats = budget.stats();
            checkpoint.tokens_used = stats.consumed;
            checkpoint.budget_remaining = stats.remaining;
        } else {
            checkpoint.budget_remaining = self.config.budget.total_tokens;
        }

        // Update metrics stats
        if let Some(ref metrics) = self.metrics {
            let summary = metrics.summary();
            checkpoint.api_calls = summary.api_calls;
            checkpoint.input_tokens = summary.input_tokens;
            checkpoint.output_tokens = summary.output_tokens;
            checkpoint.avg_latency_ms = summary.avg_latency_ms;
            checkpoint.total_cost_usd = summary.total_cost_usd;
            checkpoint.total_duration_ms = summary.total_duration_ms;
        }
    }

    async fn get_file_registry(&self) -> Result<VerifiedFileRegistry> {
        let root = &self.project_root;
        let analysis_config = &self.config.analysis;
        self.file_registry
            .get_or_try_init(|| async move {
                VerifiedFileRegistry::build_with_config(root, analysis_config).await
            })
            .await
            .cloned()
    }

    pub async fn run(&self) -> Result<QualityLoopResult> {
        // Check for crash recovery if durable execution is enabled
        if self.config.performance.resume_on_crash
            && let Some(result) = self.try_recover().await?
        {
            return Ok(result);
        }

        // Initialize checkpoint manager for durable execution
        let mut checkpoint_manager = if self.config.performance.checkpoint_interval_minutes > 0 {
            let manager = CheckpointManager::new(&self.project_root, self.config.timeout());
            manager.initialize().await?;
            manager.acquire_lock().await?;
            Some(manager)
        } else {
            None
        };

        let result = self.run_with_checkpoints(&mut checkpoint_manager).await;

        // Release lock on completion
        if let Some(ref manager) = checkpoint_manager {
            let _ = manager.release_lock().await;
        }

        result
    }

    async fn try_recover(&self) -> Result<Option<QualityLoopResult>> {
        let manager = CheckpointManager::new(&self.project_root, self.config.timeout());
        let recovery = CrashRecovery::new(manager);

        match recovery.attempt_recovery().await? {
            RecoveryResult::NoRecoveryNeeded => Ok(None),
            RecoveryResult::ProcessRunning(lock) => {
                tracing::warn!(
                    pid = lock.pid,
                    started = %lock.started_at,
                    "Another process is running"
                );
                Err(crate::types::ClaudegenError::Config(
                    "Another claudegen process is already running".into(),
                ))
            }
            RecoveryResult::StartFresh => {
                tracing::info!("No checkpoint found, starting fresh");
                Ok(None)
            }
            RecoveryResult::Recovered(checkpoint) => self.resume_from_checkpoint(*checkpoint).await,
        }
    }

    async fn resume_from_checkpoint(
        &self,
        checkpoint: ExecutionCheckpoint,
    ) -> Result<Option<QualityLoopResult>> {
        tracing::info!(
            phase = ?checkpoint.current_phase,
            progress = format!("{:.1}%", checkpoint.progress_percentage()),
            tokens_used = checkpoint.tokens_used,
            refinement_iter = checkpoint.refinement_iteration,
            "Resuming from checkpoint"
        );

        // If we have generated artifacts from a previous run, try to use them
        if let Some(ref claude_md) = checkpoint.generated_artifacts.claude_md
            && (checkpoint.current_phase == PipelinePhase::DeepReview
                || checkpoint.current_phase == PipelinePhase::Finalization)
        {
            tracing::info!(
                phase = ?checkpoint.current_phase,
                "Checkpoint has generated content, resuming deep review"
            );

            // Reconstruct artifacts for deep review
            let artifacts = ReviewArtifacts {
                claude_md: Some(claude_md.clone()),
                skills: checkpoint
                    .generated_artifacts
                    .skills
                    .clone()
                    .into_iter()
                    .collect(),
                agents: checkpoint
                    .generated_artifacts
                    .agents
                    .clone()
                    .into_iter()
                    .collect(),
                rules: checkpoint
                    .generated_artifacts
                    .rules
                    .clone()
                    .into_iter()
                    .collect(),
            };

            // Run deep review on the recovered artifacts (performance tier)
            let file_registry = self.get_file_registry().await?;
            let engine = DeepReviewEngine::new(
                Arc::clone(self.providers.provider_for_phase(phase_id::DEEP_REVIEW)),
                self.config.deep_review(),
                file_registry,
            );

            match engine.execute_two_pass_review(&artifacts).await? {
                TwoPassResult::Passed {
                    total_attempts,
                    final_quality,
                } => {
                    tracing::info!(
                        attempts = total_attempts,
                        quality = format!("{:.1}%", final_quality * 100.0),
                        "Resumed deep review PASSED"
                    );
                    // Cannot fully reconstruct AdaptivePipelineOutput from checkpoint
                    // Fall through to re-run pipeline with checkpointed context
                }
                TwoPassResult::Failed { .. } => {
                    tracing::warn!("Resumed deep review FAILED, re-running pipeline");
                }
            }
        }

        // For phases before deep review or if artifacts are incomplete,
        // we need to re-run the pipeline. Log what we recovered.
        let completed_phases: Vec<_> = checkpoint
            .completed_phases
            .iter()
            .map(|p| p.phase.as_str())
            .collect();

        tracing::info!(
            completed_phases = ?completed_phases,
            quality_samples = checkpoint.quality_history.len(),
            "Restarting pipeline from recovered context"
        );

        // Return None to signal that normal pipeline run should proceed
        // The checkpoint context is logged but not directly usable due to
        // AdaptivePipeline's current architecture
        Ok(None)
    }

    async fn run_with_checkpoints(
        &self,
        checkpoint_manager: &mut Option<CheckpointManager>,
    ) -> Result<QualityLoopResult> {
        let loop_config = self.config.quality_loop();

        if !loop_config.enabled {
            return self.run_single_pass(checkpoint_manager).await;
        }

        let mut current_config = self.config.clone();
        let mut gaps_discovered = Vec::new();
        let mut analysis_rerun_count = 0;
        let mut checkpoint = ExecutionCheckpoint::new();
        self.update_checkpoint_stats(&mut checkpoint);

        // Track best result across iterations
        let mut best_result: Option<AdaptivePipelineOutput> = None;
        let mut best_quality: f32 = 0.0;

        // Accumulate context across iterations for zero information loss
        let mut ctx = ClaudegenContext::new(&self.project_root);

        for outer_iter in 0..loop_config.max_iterations {
            checkpoint.current_phase = PipelinePhase::Analysis;
            checkpoint.phase_progress = outer_iter as f32 / loop_config.max_iterations as f32;
            self.update_checkpoint_stats(&mut checkpoint);

            // Save checkpoint periodically
            if let Some(manager) = checkpoint_manager.as_mut() {
                let _ = manager.maybe_checkpoint(&checkpoint).await;
            }

            tracing::info!(
                iteration = outer_iter + 1,
                max = loop_config.max_iterations,
                config_depth = ?current_config.analysis.depth,
                "Quality loop iteration starting"
            );

            // Create pipeline with current config (may have escalated depth)
            let pipeline = AdaptivePipeline::new(
                self.project_root.clone(),
                self.providers.clone(),
                current_config.clone(),
            );

            let timeout = Duration::from_secs(self.config.timeout().quality_loop_timeout_secs);
            let result = with_timeout(timeout, pipeline.run(), "quality_loop").await?;

            // Update checkpoint with generated artifacts and quality history
            checkpoint.generated_artifacts = GeneratedArtifacts {
                claude_md: Some(result.claude_md.to_markdown()),
                skills: result
                    .plugin
                    .skills
                    .iter()
                    .filter_map(|s| {
                        serde_json::to_string(s)
                            .ok()
                            .map(|json| (s.name.clone(), json))
                    })
                    .collect(),
                agents: result
                    .plugin
                    .agents
                    .iter()
                    .filter_map(|a| {
                        serde_json::to_string(a)
                            .ok()
                            .map(|json| (a.name.clone(), json))
                    })
                    .collect(),
                rules: result
                    .rules
                    .iter()
                    .filter_map(|r| {
                        serde_json::to_string(r)
                            .ok()
                            .map(|json| (r.name.clone(), json))
                    })
                    .collect(),
            };
            checkpoint.quality_history.push(QualitySnapshot {
                timestamp: chrono::Utc::now(),
                iteration: outer_iter,
                semantic_score: result.quality_score,
                evidence_score: result
                    .cross_validation_result
                    .evidence_traceability
                    .coverage_score,
                overall_score: result.quality_score,
            });
            checkpoint.refinement_iteration = result.refinement_iterations;
            checkpoint.current_phase = PipelinePhase::Refinement;
            self.update_checkpoint_stats(&mut checkpoint);

            // Save checkpoint after each pipeline run
            if let Some(manager) = checkpoint_manager.as_mut() {
                let _ = manager.save_checkpoint(&checkpoint).await;
            }

            // Merge context from this iteration (accumulate across iterations)
            ctx.merge_from(&result.context);
            ctx.increment_iteration();

            // Track best result and write output after each improvement
            if result.quality_score > best_quality {
                best_quality = result.quality_score;
                // Clone the result and replace its context with the accumulated context
                let mut best = result.clone();
                best.context = ctx.clone();
                best_result = Some(best);

                // Write output files after each quality improvement
                tracing::info!(
                    iteration = outer_iter + 1,
                    quality = format!("{:.1}%", result.quality_score * 100.0),
                    accumulated_tier3 = ctx.tier3_items().len(),
                    accumulated_abstractions = ctx.key_abstractions().len(),
                    "New best quality - writing output files with accumulated context"
                );
                if let Err(e) = self.write_result_output(&result).await {
                    tracing::warn!(error = %e, "Failed to write output files");
                }
            }

            let analysis_confidence = result
                .synthesis
                .as_ref()
                .map(|s| s.confidence.overall)
                .unwrap_or(0.0);

            let synthesis_confidence = result
                .synthesis
                .as_ref()
                .map(|s| {
                    let ref_ratio = s.reference_validation.validation_ratio;
                    let coverage = s.confidence.coverage;
                    (ref_ratio + coverage) / 2.0
                })
                .unwrap_or(0.0);

            if analysis_confidence < loop_config.target_score {
                tracing::warn!(
                    iteration = outer_iter + 1,
                    confidence = format!("{:.1}%", analysis_confidence * 100.0),
                    threshold = format!("{:.1}%", loop_config.target_score * 100.0),
                    "Analysis confidence below threshold, escalating depth"
                );

                gaps_discovered.push(DiscoveredGap {
                    area: "analysis".into(),
                    description: format!(
                        "Low analysis confidence: {:.1}%",
                        analysis_confidence * 100.0
                    ),
                    iteration_found: outer_iter,
                });

                if let Some(escalated) = self.try_escalate_analysis_depth(current_config.clone()) {
                    current_config = escalated;
                    analysis_rerun_count += 1;
                    continue;
                } else if let Some(best) = best_result {
                    // At max depth, return best result we have
                    tracing::info!(
                        quality = format!("{:.1}%", best_quality * 100.0),
                        "At maximum depth, returning best result"
                    );
                    return Ok(QualityLoopResult {
                        output: best,
                        outer_iterations: outer_iter + 1,
                        analysis_rerun_count,
                        final_confidence: analysis_confidence,
                        gaps_discovered,
                        deep_review_passed: false,
                        deep_review_attempts: 0,
                        validation_results: None,
                        clean_pass_status: CleanPassStatus::InProgress {
                            streak: 0,
                            required: 1,
                        },
                    });
                }
                analysis_rerun_count += 1;
                continue;
            }

            if synthesis_confidence < loop_config.target_score {
                tracing::warn!(
                    iteration = outer_iter + 1,
                    confidence = format!("{:.1}%", synthesis_confidence * 100.0),
                    threshold = format!("{:.1}%", loop_config.target_score * 100.0),
                    "Synthesis confidence below threshold, escalating depth"
                );

                gaps_discovered.push(DiscoveredGap {
                    area: "synthesis".into(),
                    description: format!(
                        "Low synthesis confidence: {:.1}%",
                        synthesis_confidence * 100.0
                    ),
                    iteration_found: outer_iter,
                });

                if let Some(escalated) = self.try_escalate_analysis_depth(current_config.clone()) {
                    current_config = escalated;
                    analysis_rerun_count += 1;
                    continue;
                } else if let Some(best) = best_result {
                    // At max depth, return best result we have
                    tracing::info!(
                        quality = format!("{:.1}%", best_quality * 100.0),
                        "At maximum depth (synthesis), returning best result"
                    );
                    return Ok(QualityLoopResult {
                        output: best,
                        outer_iterations: outer_iter + 1,
                        analysis_rerun_count,
                        final_confidence: synthesis_confidence,
                        gaps_discovered,
                        deep_review_passed: false,
                        deep_review_attempts: 0,
                        validation_results: None,
                        clean_pass_status: CleanPassStatus::InProgress {
                            streak: 0,
                            required: 1,
                        },
                    });
                }
                analysis_rerun_count += 1;
                continue;
            }

            let evidence_check = self.validate_evidence(&result).await;
            if evidence_check.invalid_ratio > 0.3 {
                tracing::warn!(
                    iteration = outer_iter + 1,
                    invalid = evidence_check.invalid_refs,
                    total = evidence_check.total_refs,
                    ratio = format!("{:.1}%", evidence_check.invalid_ratio * 100.0),
                    threshold = format!("{:.1}%", 0.3 * 100.0),
                    "Evidence validation failed: too many invalid file references"
                );

                for gap in &evidence_check.gaps {
                    gaps_discovered.push(DiscoveredGap {
                        area: gap.clone(),
                        description: "Invalid file reference".into(),
                        iteration_found: outer_iter,
                    });
                }

                if evidence_check.gaps.len() >= 5 {
                    if let Some(escalated) =
                        self.try_escalate_analysis_depth(current_config.clone())
                    {
                        current_config = escalated;
                        analysis_rerun_count += 1;
                        continue;
                    } else if let Some(best) = best_result.clone() {
                        // At max depth, return best result we have
                        tracing::info!(
                            quality = format!("{:.1}%", best_quality * 100.0),
                            "At maximum depth (evidence), returning best result"
                        );
                        return Ok(QualityLoopResult {
                            output: best,
                            outer_iterations: outer_iter + 1,
                            analysis_rerun_count,
                            final_confidence: analysis_confidence,
                            gaps_discovered,
                            deep_review_passed: false,
                            deep_review_attempts: 0,
                            validation_results: None,
                            clean_pass_status: CleanPassStatus::InProgress {
                                streak: 0,
                                required: 1,
                            },
                        });
                    }
                    analysis_rerun_count += 1;
                    continue;
                }
            }

            if result.quality_score >= self.config.quality().min_score {
                // Run deep review if enabled
                let (deep_review_passed, deep_review_attempts) =
                    if self.config.deep_review().required_passes > 0 {
                        checkpoint.current_phase = PipelinePhase::DeepReview;
                        self.run_deep_review(&result).await?
                    } else {
                        (true, 0)
                    };

                // Run 3-layer validation: Tier Filter → Consistency → Cross-Artifact
                let validation_result = self.run_validation_pipeline(&result).await?;

                let clean_pass_status = validation_result.1;
                let validation_passed =
                    matches!(clean_pass_status, CleanPassStatus::Converged { .. });

                if deep_review_passed && validation_passed {
                    tracing::info!(
                        iteration = outer_iter + 1,
                        quality = format!("{:.1}%", result.quality_score * 100.0),
                        deep_review_attempts,
                        clean_passes = ?clean_pass_status,
                        "Quality loop converged with clean pass guarantee"
                    );

                    checkpoint.current_phase = PipelinePhase::Finalization;
                    self.update_checkpoint_stats(&mut checkpoint);
                    if let Some(manager) = checkpoint_manager.as_mut() {
                        let _ = manager.save_checkpoint(&checkpoint).await;
                    }

                    return Ok(QualityLoopResult {
                        output: result,
                        outer_iterations: outer_iter + 1,
                        analysis_rerun_count,
                        final_confidence: analysis_confidence,
                        gaps_discovered,
                        deep_review_passed,
                        deep_review_attempts,
                        validation_results: Some(validation_result.0),
                        clean_pass_status,
                    });
                }
                tracing::warn!(
                    iteration = outer_iter + 1,
                    deep_review_attempts,
                    deep_review_passed,
                    validation_passed,
                    "Review/validation incomplete, continuing refinement"
                );
            }

            tracing::debug!(
                iteration = outer_iter + 1,
                quality = format!("{:.1}%", result.quality_score * 100.0),
                target = format!("{:.1}%", self.config.quality().min_score * 100.0),
                "Quality below target, continuing loop"
            );
        }

        tracing::warn!(
            iterations = loop_config.max_iterations,
            best_quality = format!("{:.1}%", best_quality * 100.0),
            "Quality loop reached max iterations, using best result"
        );

        // If we have a best result, use it instead of running another pipeline
        if let Some(best) = best_result {
            let final_confidence = best
                .synthesis
                .as_ref()
                .map(|s| s.confidence.overall)
                .unwrap_or(0.0);

            return Ok(QualityLoopResult {
                output: best,
                outer_iterations: loop_config.max_iterations,
                analysis_rerun_count,
                final_confidence,
                gaps_discovered,
                deep_review_passed: false,
                deep_review_attempts: 0,
                validation_results: None,
                clean_pass_status: CleanPassStatus::InProgress {
                    streak: 0,
                    required: 1,
                },
            });
        }

        // Fallback: run one more pipeline attempt
        let fallback_pipeline = AdaptivePipeline::new(
            self.project_root.clone(),
            self.providers.clone(),
            current_config,
        );
        let timeout = Duration::from_secs(self.config.timeout().quality_loop_timeout_secs);
        let final_result =
            with_timeout(timeout, fallback_pipeline.run(), "quality_loop_final").await?;

        let final_confidence = final_result
            .synthesis
            .as_ref()
            .map(|s| s.confidence.overall)
            .unwrap_or(0.0);

        // Run final deep review
        let (deep_review_passed, deep_review_attempts) =
            if self.config.deep_review().required_passes > 0 {
                self.run_deep_review(&final_result).await?
            } else {
                (true, 0)
            };

        // Run final validation pipeline
        let (validation_results, clean_pass_status) =
            self.run_validation_pipeline(&final_result).await?;

        Ok(QualityLoopResult {
            output: final_result,
            outer_iterations: loop_config.max_iterations,
            analysis_rerun_count,
            final_confidence,
            gaps_discovered,
            deep_review_passed,
            deep_review_attempts,
            validation_results: Some(validation_results),
            clean_pass_status,
        })
    }

    async fn run_single_pass(
        &self,
        checkpoint_manager: &mut Option<CheckpointManager>,
    ) -> Result<QualityLoopResult> {
        let mut checkpoint = ExecutionCheckpoint::new();
        checkpoint.current_phase = PipelinePhase::Analysis;
        self.update_checkpoint_stats(&mut checkpoint);

        if let Some(manager) = checkpoint_manager.as_mut() {
            let _ = manager.save_checkpoint(&checkpoint).await;
        }

        let pipeline = AdaptivePipeline::new(
            self.project_root.clone(),
            self.providers.clone(),
            self.config.clone(),
        );

        let timeout = Duration::from_secs(self.config.timeout().quality_loop_timeout_secs);
        let result = with_timeout(timeout, pipeline.run(), "single_pass").await?;

        let confidence = result
            .synthesis
            .as_ref()
            .map(|s| s.confidence.overall)
            .unwrap_or(0.0);

        // Run deep review if enabled
        let (deep_review_passed, deep_review_attempts) =
            if self.config.deep_review().required_passes > 0 {
                checkpoint.current_phase = PipelinePhase::DeepReview;
                self.run_deep_review(&result).await?
            } else {
                (true, 0)
            };

        // Run validation pipeline
        let (validation_results, clean_pass_status) = self.run_validation_pipeline(&result).await?;

        Ok(QualityLoopResult {
            output: result,
            outer_iterations: 1,
            analysis_rerun_count: 0,
            final_confidence: confidence,
            gaps_discovered: Vec::new(),
            deep_review_passed,
            deep_review_attempts,
            validation_results: Some(validation_results),
            clean_pass_status,
        })
    }

    async fn run_deep_review(&self, result: &AdaptivePipelineOutput) -> Result<(bool, u32)> {
        let file_registry = self.get_file_registry().await?;
        // Use performance tier for deep review (high-intelligence task)
        let engine = DeepReviewEngine::new(
            Arc::clone(self.providers.provider_for_phase(phase_id::DEEP_REVIEW)),
            self.config.deep_review(),
            file_registry,
        );

        let artifacts = self.build_review_artifacts(result);

        tracing::info!(
            required_passes = self.config.deep_review().required_passes,
            "Starting two-pass deep review"
        );

        match engine.execute_two_pass_review(&artifacts).await? {
            TwoPassResult::Passed {
                total_attempts,
                final_quality,
            } => {
                tracing::info!(
                    attempts = total_attempts,
                    quality = format!("{:.1}%", final_quality * 100.0),
                    "Deep review PASSED"
                );
                Ok((true, total_attempts))
            }
            TwoPassResult::Failed {
                total_attempts,
                remaining_issues,
            } => {
                tracing::warn!(
                    attempts = total_attempts,
                    issues = remaining_issues.len(),
                    "Deep review FAILED"
                );
                for issue in remaining_issues.iter().take(5) {
                    tracing::debug!(
                        artifact = %issue.artifact,
                        message = %issue.message,
                        "Remaining issue"
                    );
                }
                Ok((false, total_attempts))
            }
        }
    }

    fn build_review_artifacts(&self, result: &AdaptivePipelineOutput) -> ReviewArtifacts {
        ReviewArtifacts {
            claude_md: Some(result.claude_md.to_markdown()),
            skills: result
                .plugin
                .skills
                .iter()
                .map(|s| (s.name.clone(), s.to_markdown()))
                .collect(),
            agents: result
                .plugin
                .agents
                .iter()
                .map(|a| (a.name.clone(), a.to_markdown()))
                .collect(),
            rules: result
                .rules
                .iter()
                .map(|r| (r.name.clone(), r.to_markdown()))
                .collect(),
        }
    }

    async fn run_validation_pipeline(
        &self,
        result: &AdaptivePipelineOutput,
    ) -> Result<(ValidationResults, CleanPassStatus)> {
        if !self.config.validation.enabled {
            tracing::debug!("Validation pipeline disabled, skipping");
            return Ok((
                ValidationResults::new(),
                CleanPassStatus::Converged { passes: 0 },
            ));
        }

        let quality_score = result.quality_score;
        let required_quality = self.config.quality().minimum_quality;

        // Collect issues from all validation layers
        let mut error_count = 0;
        let mut warning_count = 0;

        // Layer 1: Tier Filter (Tier1 content = error)
        if !result.tier_filter_result.passed {
            error_count += result.tier_filter_result.tier1_count;
            tracing::debug!(
                tier1 = result.tier_filter_result.tier1_count,
                tier3_ratio = format!("{:.1}%", result.tier_filter_result.tier3_ratio * 100.0),
                "Tier filter: FAILED"
            );
        }

        // Layer 2: Consistency (duplicates, broken refs = error)
        if !result.consistency_result.passed {
            error_count += result.consistency_result.issues.len();
            for issue in &result.consistency_result.issues {
                tracing::debug!(issue, "Consistency issue");
            }
        }

        // Layer 3: Cross-artifact validation
        let cross = &result.cross_validation_result;
        if !cross.passed {
            // Evidence traceability (invalid refs = error)
            if cross.evidence_traceability.invalid_references > 0 {
                error_count += cross.evidence_traceability.invalid_references;
                tracing::debug!(
                    invalid = cross.evidence_traceability.invalid_references,
                    valid = cross.evidence_traceability.valid_references,
                    "Evidence traceability: FAILED"
                );
            }

            // Plan consistency (missing coverage = warning)
            if !cross.plan_consistency.passed {
                warning_count += cross.plan_consistency.missing_coverage.len();
            }
        }

        let total_issues = error_count + warning_count;

        tracing::info!(
            quality_score = format!("{:.1}%", quality_score * 100.0),
            required = format!("{:.1}%", required_quality * 100.0),
            errors = error_count,
            warnings = warning_count,
            tier_filter = result.tier_filter_result.passed,
            consistency = result.consistency_result.passed,
            cross_validation = cross.passed,
            "Validation pipeline complete"
        );

        let validation_results = ValidationResults {
            total_issues,
            error_count,
            warning_count,
        };

        // Convergence requires: quality met + no errors + all validations pass
        let all_validations_pass =
            result.tier_filter_result.passed && result.consistency_result.passed && cross.passed;

        let status =
            if quality_score >= required_quality && error_count == 0 && all_validations_pass {
                CleanPassStatus::Converged { passes: 1 }
            } else if quality_score < required_quality {
                CleanPassStatus::Failed {
                    reason: FailureReason::QualityBelow {
                        score: quality_score,
                        threshold: required_quality,
                    },
                }
            } else {
                CleanPassStatus::InProgress {
                    streak: 0,
                    required: 1,
                }
            };

        Ok((validation_results, status))
    }

    /// Try to escalate analysis depth. Returns None if already at max depth.
    fn try_escalate_analysis_depth(&self, mut config: Config) -> Option<Config> {
        let current = &config.analysis.depth;
        let factor = 1.5; // Default escalation factor

        let new_depth = match current {
            AnalysisDepth::Fast => {
                tracing::debug!("Escalating depth: Fast → Standard");
                AnalysisDepth::Standard
            }
            AnalysisDepth::Standard => {
                tracing::debug!("Escalating depth: Standard → Complete");
                AnalysisDepth::Complete
            }
            AnalysisDepth::Complete => {
                tracing::warn!("Already at maximum depth, cannot escalate further");
                return None;
            }
        };

        config.analysis.depth = new_depth;
        config.analysis.max_file_samples =
            (config.analysis.max_file_samples as f32 * factor) as usize;
        config.deep_analysis.max_iterations += 1;

        tracing::debug!(
            depth = ?config.analysis.depth,
            max_samples = config.analysis.max_file_samples,
            "Analysis escalated"
        );

        Some(config)
    }

    async fn validate_evidence(&self, result: &AdaptivePipelineOutput) -> EvidenceCheckResult {
        let file_registry = match self.get_file_registry().await {
            Ok(registry) => registry,
            Err(e) => {
                tracing::warn!(error = %e, "Registry build failed, skipping evidence validation");
                return EvidenceCheckResult::default();
            }
        };

        let mut total_refs = 0;
        let mut invalid_refs = 0;
        let mut gaps = Vec::new();

        // Validate skills
        for skill in &result.plugin.skills {
            Self::validate_content_refs(
                &skill.body,
                &format!("skill:{}", skill.name),
                &file_registry,
                &mut total_refs,
                &mut invalid_refs,
                &mut gaps,
            );
        }

        // Validate agents
        for agent in &result.plugin.agents {
            Self::validate_content_refs(
                &agent.prompt,
                &format!("agent:{}", agent.name),
                &file_registry,
                &mut total_refs,
                &mut invalid_refs,
                &mut gaps,
            );
        }

        // Validate rules
        for rule in &result.rules {
            let content = rule.content.join("\n");
            Self::validate_content_refs(
                &content,
                &format!("rule:{}", rule.name),
                &file_registry,
                &mut total_refs,
                &mut invalid_refs,
                &mut gaps,
            );
        }

        // Validate CLAUDE.md sections
        if let Some(ref arch) = result.claude_md.architecture {
            Self::validate_content_refs(
                arch,
                "claude_md:architecture",
                &file_registry,
                &mut total_refs,
                &mut invalid_refs,
                &mut gaps,
            );
        }
        for (i, standard) in result.claude_md.standards.iter().enumerate() {
            Self::validate_content_refs(
                standard,
                &format!("claude_md:standard[{}]", i),
                &file_registry,
                &mut total_refs,
                &mut invalid_refs,
                &mut gaps,
            );
        }

        let invalid_ratio = if total_refs > 0 {
            invalid_refs as f32 / total_refs as f32
        } else {
            0.0
        };

        gaps.dedup();

        EvidenceCheckResult {
            total_refs,
            invalid_refs,
            invalid_ratio,
            gaps,
        }
    }

    fn validate_content_refs(
        content: &str,
        source: &str,
        registry: &VerifiedFileRegistry,
        total: &mut usize,
        invalid: &mut usize,
        gaps: &mut Vec<String>,
    ) {
        let refs = super::patterns::extract_paths(content);
        *total += refs.len();
        for r in refs {
            if !registry.contains(&r) {
                *invalid += 1;
                gaps.push(source.to_string());
            }
        }
    }

    pub async fn write_output(&self, result: &QualityLoopResult) -> Result<()> {
        self.write_result_output(&result.output).await
    }

    async fn write_result_output(&self, output: &AdaptivePipelineOutput) -> Result<()> {
        // CLAUDE.md and .claude/rules always go to project root
        // Plugin artifacts go to output_dir (if set) or project root
        let pipeline = AdaptivePipeline::new(
            self.project_root.clone(),
            self.providers.clone(),
            self.config.clone(),
        );

        // Write CLAUDE.md and rules to project root
        pipeline.write_output(output).await?;

        // If output_dir is different from project_root, write plugin there
        if let Some(ref output_dir) = self.output_dir
            && output_dir != &self.project_root
        {
            let plugin_pipeline = AdaptivePipeline::new(
                output_dir.clone(),
                self.providers.clone(),
                self.config.clone(),
            );
            plugin_pipeline.write_plugin_only(output).await?;
        }

        // Save output directory to state for status command to find
        let base_dir = self
            .output_dir
            .clone()
            .unwrap_or_else(|| self.project_root.clone());
        if let Err(e) = self.save_output_state(&base_dir) {
            tracing::warn!(error = %e, "Failed to save output state");
        }

        Ok(())
    }

    fn save_output_state(&self, output_dir: &std::path::Path) -> Result<()> {
        use crate::cli::util::ProjectState;

        let mut state = ProjectState::load().unwrap_or_default();
        state.set_output_dir(output_dir.to_path_buf());
        state.save()
    }
}

#[derive(Debug, Default)]
struct EvidenceCheckResult {
    total_refs: usize,
    invalid_refs: usize,
    invalid_ratio: f32,
    gaps: Vec<String>,
}
