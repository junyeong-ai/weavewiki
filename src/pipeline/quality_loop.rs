//! Quality Loop - Outer Pipeline Verification Loop with Durable Execution
//!
//! Orchestrates the complete generation pipeline with quality gates:
//! 1. Analysis Quality Gate: Ensure analysis confidence meets threshold
//! 2. Generation Quality Gate: Ensure semantic/structural quality meets threshold
//! 3. Evidence Quality Gate: Validate all file references against source
//! 4. Deep Review Gate: Two-pass verification for quality guarantee
//!
//! Durable Execution Features:
//! - Periodic checkpointing for crash recovery
//! - Lock file for concurrent execution prevention
//! - Progress persistence across sessions

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::ai::{with_timeout, LlmProvider};
use crate::config::{AnalysisDepth, Config};
use crate::types::Result;

use super::adaptive::{AdaptivePipeline, AdaptivePipelineOutput};
use super::checkpoint::{
    CheckpointManager, CrashRecovery, ExecutionCheckpoint, PipelinePhase, RecoveryResult,
};
use super::context::VerifiedFileRegistry;
use super::deep_review::{DeepReviewEngine, ReviewArtifacts, TwoPassResult};

#[derive(Debug, Clone)]
pub struct QualityLoopResult {
    pub output: AdaptivePipelineOutput,
    pub outer_iterations: usize,
    pub analysis_rerun_count: usize,
    pub final_confidence: f32,
    pub gaps_discovered: Vec<DiscoveredGap>,
    pub deep_review_passed: bool,
    pub deep_review_attempts: u32,
}

#[derive(Debug, Clone)]
pub struct DiscoveredGap {
    pub area: String,
    pub description: String,
    pub iteration_found: usize,
}

pub struct QualityLoop {
    project_root: PathBuf,
    provider: Arc<dyn LlmProvider>,
    config: Config,
    file_registry: OnceCell<VerifiedFileRegistry>,
}

impl QualityLoop {
    pub fn new(
        project_root: PathBuf,
        provider: Arc<dyn LlmProvider>,
        config: Config,
    ) -> Self {
        Self {
            project_root,
            provider,
            config,
            file_registry: OnceCell::new(),
        }
    }

    async fn get_file_registry(&self) -> Result<VerifiedFileRegistry> {
        self.file_registry
            .get_or_try_init(|| async {
                VerifiedFileRegistry::build(&self.project_root).await
            })
            .await
            .cloned()
    }

    pub async fn run(&self) -> Result<QualityLoopResult> {
        // Check for crash recovery if durable execution is enabled
        if self.config.performance.resume_on_crash {
            if let Some(result) = self.try_recover().await? {
                return Ok(result);
            }
        }

        // Initialize checkpoint manager for durable execution
        let mut checkpoint_manager = if self.config.performance.checkpoint_interval_minutes > 0 {
            let manager = CheckpointManager::new(&self.project_root, &self.config.execution());
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
        let manager = CheckpointManager::new(&self.project_root, &self.config.execution());
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
            RecoveryResult::Recovered(checkpoint) => {
                self.resume_from_checkpoint(checkpoint).await
            }
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
        if let Some(ref claude_md) = checkpoint.generated_artifacts.claude_md {
            if checkpoint.current_phase == PipelinePhase::DeepReview
                || checkpoint.current_phase == PipelinePhase::Finalization
            {
                tracing::info!(
                    phase = ?checkpoint.current_phase,
                    "Checkpoint has generated content, resuming deep review"
                );

                // Reconstruct artifacts for deep review
                let artifacts = ReviewArtifacts {
                    claude_md: Some(claude_md.clone()),
                    skills: checkpoint.generated_artifacts.skills.clone().into_iter().collect(),
                    agents: checkpoint.generated_artifacts.agents.clone().into_iter().collect(),
                    rules: checkpoint.generated_artifacts.rules.clone().into_iter().collect(),
                };

                // Run deep review on the recovered artifacts
                let file_registry = self.get_file_registry().await?;
                let engine = DeepReviewEngine::new(
                    Arc::clone(&self.provider),
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
        checkpoint.budget_remaining = self.config.budget.total_tokens;

        for outer_iter in 0..loop_config.max_iterations {
            checkpoint.current_phase = PipelinePhase::Analysis;
            checkpoint.phase_progress = outer_iter as f32 / loop_config.max_iterations as f32;

            // Save checkpoint periodically
            if let Some(manager) = checkpoint_manager.as_mut() {
                let _ = manager.maybe_checkpoint(&checkpoint).await;
            }

            tracing::info!(
                iteration = outer_iter + 1,
                max = loop_config.max_iterations,
                "Quality loop iteration starting"
            );

            let pipeline = AdaptivePipeline::new(
                self.project_root.clone(),
                Arc::clone(&self.provider),
                current_config.clone(),
            );

            let timeout = Duration::from_secs(self.config.network().timeout_ms / 1000);
            let result = with_timeout(timeout, pipeline.run(), "quality_loop").await?;

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

                current_config = self.escalate_analysis_depth(current_config);
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

                current_config = self.escalate_analysis_depth(current_config);
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
                    current_config = self.escalate_analysis_depth(current_config);
                    analysis_rerun_count += 1;
                    continue;
                }
            }

            if result.quality_score >= self.config.quality().min_score {
                // Run deep review if enabled
                let (deep_review_passed, deep_review_attempts) = if self
                    .config
                    .deep_review()
                    .required_passes
                    > 0
                {
                    checkpoint.current_phase = PipelinePhase::DeepReview;
                    self.run_deep_review(&result).await?
                } else {
                    (true, 0)
                };

                if deep_review_passed {
                    tracing::info!(
                        iteration = outer_iter + 1,
                        quality = format!("{:.1}%", result.quality_score * 100.0),
                        deep_review_attempts,
                        "Quality loop converged successfully"
                    );

                    checkpoint.current_phase = PipelinePhase::Finalization;
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
                    });
                } else {
                    tracing::warn!(
                        iteration = outer_iter + 1,
                        deep_review_attempts,
                        "Deep review failed, continuing refinement"
                    );
                }
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
            "Quality loop reached max iterations"
        );

        let pipeline = AdaptivePipeline::new(
            self.project_root.clone(),
            Arc::clone(&self.provider),
            current_config,
        );

        let timeout = Duration::from_secs(self.config.network().timeout_ms / 1000);
        let final_result = with_timeout(timeout, pipeline.run(), "quality_loop_final").await?;

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

        Ok(QualityLoopResult {
            output: final_result,
            outer_iterations: loop_config.max_iterations,
            analysis_rerun_count,
            final_confidence,
            gaps_discovered,
            deep_review_passed,
            deep_review_attempts,
        })
    }

    async fn run_single_pass(
        &self,
        checkpoint_manager: &mut Option<CheckpointManager>,
    ) -> Result<QualityLoopResult> {
        let mut checkpoint = ExecutionCheckpoint::new();
        checkpoint.current_phase = PipelinePhase::Analysis;

        if let Some(manager) = checkpoint_manager.as_mut() {
            let _ = manager.save_checkpoint(&checkpoint).await;
        }

        let pipeline = AdaptivePipeline::new(
            self.project_root.clone(),
            Arc::clone(&self.provider),
            self.config.clone(),
        );

        let timeout = Duration::from_secs(self.config.network().timeout_ms / 1000);
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

        Ok(QualityLoopResult {
            output: result,
            outer_iterations: 1,
            analysis_rerun_count: 0,
            final_confidence: confidence,
            gaps_discovered: Vec::new(),
            deep_review_passed,
            deep_review_attempts,
        })
    }

    async fn run_deep_review(
        &self,
        result: &AdaptivePipelineOutput,
    ) -> Result<(bool, u32)> {
        let file_registry = self.get_file_registry().await?;
        let engine = DeepReviewEngine::new(
            Arc::clone(&self.provider),
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

    fn escalate_analysis_depth(&self, mut config: Config) -> Config {
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
                return config;
            }
        };

        config.analysis.depth = new_depth;
        config.analysis.max_file_samples =
            (config.analysis.max_file_samples as f32 * factor) as usize;
        config.analysis.deep_analysis.max_iterations += 1;

        tracing::debug!(
            depth = ?config.analysis.depth,
            max_samples = config.analysis.max_file_samples,
            "Analysis escalated"
        );

        config
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

        for skill in &result.plugin.skills {
            let refs = extract_file_refs(&skill.body);
            total_refs += refs.len();
            for r in refs {
                if !file_registry.contains(&r) {
                    invalid_refs += 1;
                    gaps.push(format!("skill:{}", skill.name));
                }
            }
        }

        for agent in &result.plugin.agents {
            let refs = extract_file_refs(&agent.prompt);
            total_refs += refs.len();
            for r in refs {
                if !file_registry.contains(&r) {
                    invalid_refs += 1;
                    gaps.push(format!("agent:{}", agent.name));
                }
            }
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

    pub async fn write_output(&self, result: &QualityLoopResult) -> Result<()> {
        let pipeline = AdaptivePipeline::new(
            self.project_root.clone(),
            Arc::clone(&self.provider),
            self.config.clone(),
        );
        pipeline.write_output(&result.output).await
    }
}

#[derive(Debug, Default)]
struct EvidenceCheckResult {
    total_refs: usize,
    invalid_refs: usize,
    invalid_ratio: f32,
    gaps: Vec<String>,
}

fn extract_file_refs(content: &str) -> Vec<String> {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"@([a-zA-Z0-9_/.-]+(?::\d+)?)").expect("Invalid file ref regex")
    });

    RE.captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| {
            m.as_str()
                .split(':')
                .next()
                .unwrap_or(m.as_str())
                .to_string()
        })
        .collect()
}
