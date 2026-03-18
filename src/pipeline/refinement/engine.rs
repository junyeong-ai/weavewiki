use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ai::LlmProvider;
use crate::config::Config;
use crate::types::{
    Agent, ArtifactCategory, ArtifactQuality, ClaudegenError, ClaudeMdContent, Result, Rule, Skill,
};

use crate::pipeline::analysis::{ArchitecturalAnalyzer, StructuralValidationResult};
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::context::FileRegistryExt;
use crate::pipeline::events::{
    EventPayload, EventStore, EventType, IncrementalCompactor, IterationProgress,
    QualityLevelSnapshot, RefinementCheckpoint, ResumeState, StrategyOutcome,
};
use crate::pipeline::feedback::FeedbackAggregator;
use crate::pipeline::quality::{JudgmentResult, LlmJudge};
use crate::types::artifacts::GeneratedArtifacts;
use crate::pipeline::quality_assessment::{
    AssessmentPath, DimensionScore, DimensionsStatus, QualityAssessment,
};
use crate::pipeline::strategy::{
    IssueKind as StrategyIssueKind, StrategyAttempt, StrategyContext, StrategyRotator,
};
use crate::pipeline::validation::CrossValidationResult;
use crate::utils::patterns;

use super::failure_tracker::FailureTracker;
use super::strategy_selector::FeedbackAwareSelector;
use super::types::{DetectedArtifactIssue, DetectedIssue, ItemType, RefinementResult};

use crate::constants::refinement::{
    CONVERGENCE_ACCEPTABLE_RATIO, DIMENSION_THRESHOLD_MULTIPLIER, FLOOR_CONVERGENCE_PASS_MULTIPLIER,
    LOW_VALIDITY_SCORE_THRESHOLD, MAX_LEVEL_HISTORY, MAX_QUALITY_HISTORY,
    MIN_ACTIONABILITY_THRESHOLD, MIN_ARTIFACT_CONTENT_LENGTH, OSCILLATION_DETECTION_THRESHOLD,
    REDUNDANCY_THRESHOLD,
};

fn should_checkpoint(iteration: usize, interval: usize) -> bool {
    interval > 0 && iteration > 0 && (iteration + 1).is_multiple_of(interval)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RefinementSnapshot {
    skills: Vec<Skill>,
    agents: Vec<Agent>,
    rules: Vec<Rule>,
    quality: f32,
    iteration: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QualityLevel {
    BelowFloor,
    AtFloor,
    AtTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConvergenceResult {
    Converged(ConvergencePath),
    NotConverged,
    LevelOscillation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConvergencePath {
    TargetMet,
    FloorMetExtended,
}

/// Quality metrics derived from judgment results.
///
/// Uses fact-based validation for acceptance decisions.
#[derive(Debug, Clone)]
struct QualityMetrics {
    low_validity_count: usize,
    low_validity_ratio: f32,
    avg_score: f32,
    total_artifacts: usize,
    acceptable_count: usize,
}

impl QualityMetrics {
    /// Create metrics from judgment results and artifact content.
    ///
    /// Uses ArtifactQuality.from_judgment for unified quality assessment.
    fn from_results_with_content(
        results: &[JudgmentResult],
        contents: &[&str],
        valid_paths: &impl Fn(&str) -> bool,
    ) -> Self {
        let total = results.len().max(1);
        let low_validity_count = results.iter().filter(|r| r.overall_score < LOW_VALIDITY_SCORE_THRESHOLD).count();
        let avg_score = results.iter().map(|r| r.overall_score).sum::<f32>() / total as f32;

        let acceptable_count = results
            .iter()
            .zip(contents.iter())
            .map(|(_result, content)| {
                ArtifactQuality::from_judgment(content, valid_paths)
            })
            .filter(|q| q.is_acceptable())
            .count();

        Self {
            low_validity_count,
            low_validity_ratio: low_validity_count as f32 / total as f32,
            avg_score,
            total_artifacts: results.len(),
            acceptable_count,
        }
    }

    /// Check if quality is acceptable using ratio-based threshold.
    ///
    /// Requires CONVERGENCE_ACCEPTABLE_RATIO (85%) of artifacts to pass.
    fn is_acceptable(&self) -> bool {
        self.acceptable_ratio() >= CONVERGENCE_ACCEPTABLE_RATIO
    }

    /// Get the ratio of acceptable artifacts (0.0 - 1.0)
    fn acceptable_ratio(&self) -> f32 {
        if self.total_artifacts == 0 {
            return 0.0;
        }
        self.acceptable_count as f32 / self.total_artifacts as f32
    }
}

struct RefinementState {
    prev_quality: Option<f32>,
    quality_history: Vec<f32>,
    stagnation_count: usize,
    best: Option<RefinementSnapshot>,
    best_quality: f32,
    consecutive_clean_passes: usize,
    last_quality_level: Option<QualityLevel>,
    level_history: Vec<QualityLevel>,
    /// Artifacts modified in current iteration (reset each iteration)
    modified_this_iteration: HashSet<String>,
    /// All artifacts modified across all refinement iterations (cumulative)
    all_modified: HashSet<String>,
    /// Cached judgments for unmodified artifacts (keyed by "type:name")
    cached_judgments: HashMap<String, JudgmentResult>,
    /// Artifacts that have individually converged (passed quality threshold)
    converged_artifacts: HashSet<String>,
    /// Preserved compaction summary from prior compaction cycles
    compaction_summary: Option<crate::pipeline::events::CompactionSummary>,
    /// Path to the most recent periodic artifact checkpoint
    latest_checkpoint_path: Option<String>,
}

impl RefinementState {
    fn new() -> Self {
        Self {
            prev_quality: None,
            quality_history: Vec::with_capacity(MAX_QUALITY_HISTORY),
            stagnation_count: 0,
            best: None,
            best_quality: 0.0,
            consecutive_clean_passes: 0,
            last_quality_level: None,
            level_history: Vec::with_capacity(MAX_LEVEL_HISTORY),
            modified_this_iteration: HashSet::new(),
            all_modified: HashSet::new(),
            cached_judgments: HashMap::new(),
            converged_artifacts: HashSet::new(),
            compaction_summary: None,
            latest_checkpoint_path: None,
        }
    }

    fn mark_converged(&mut self, artifact_key: &str) {
        self.converged_artifacts.insert(artifact_key.to_string());
    }

    fn is_converged(&self, artifact_key: &str) -> bool {
        self.converged_artifacts.contains(artifact_key)
    }

    fn mark_modified(&mut self, artifact_key: &str) {
        self.modified_this_iteration
            .insert(artifact_key.to_string());
        self.all_modified.insert(artifact_key.to_string());
    }

    fn clear_modifications(&mut self) {
        self.modified_this_iteration.clear();
    }

    /// Check if artifact needs evaluation (modified or not yet cached).
    fn needs_evaluation(&self, artifact_key: &str) -> bool {
        self.modified_this_iteration.contains(artifact_key)
            || !self.cached_judgments.contains_key(artifact_key)
    }

    fn cache_judgment(&mut self, artifact_key: &str, result: JudgmentResult) {
        self.cached_judgments
            .insert(artifact_key.to_string(), result);
    }

    fn cached_judgment(&self, artifact_key: &str) -> Option<&JudgmentResult> {
        self.cached_judgments.get(artifact_key)
    }

    /// Retain only cache entries whose keys are in the valid set.
    fn retain_valid_cache_entries(&mut self, valid_keys: &HashSet<String>) {
        self.cached_judgments
            .retain(|key, _| valid_keys.contains(key));
    }

    /// Record quality with bounded history
    fn record_quality(&mut self, quality: f32) {
        self.quality_history.push(quality);
        if self.quality_history.len() > MAX_QUALITY_HISTORY {
            self.quality_history.remove(0);
        }
    }

    /// Record quality level with bounded history
    fn record_level(&mut self, level: QualityLevel) {
        self.level_history.push(level);
        if self.level_history.len() > MAX_LEVEL_HISTORY {
            self.level_history.remove(0);
        }
    }

    /// Convert internal state to a checkpoint for persistence
    fn to_checkpoint(
        &self,
        iteration: usize,
        strategy_outcomes: HashMap<String, StrategyOutcome>,
    ) -> RefinementCheckpoint {
        let mut checkpoint = RefinementCheckpoint::new(
            iteration,
            self.quality_history.clone(),
            self.level_history
                .iter()
                .map(|l| match l {
                    QualityLevel::BelowFloor => QualityLevelSnapshot::BelowFloor,
                    QualityLevel::AtFloor => QualityLevelSnapshot::AtFloor,
                    QualityLevel::AtTarget => QualityLevelSnapshot::AtTarget,
                })
                .collect(),
            self.stagnation_count,
            self.consecutive_clean_passes,
            strategy_outcomes,
            self.best_quality,
            self.best.as_ref().map(|_| "best_state".to_string()),
        );
        checkpoint.compaction_summary = self.compaction_summary.clone();
        checkpoint.latest_checkpoint_path = self.latest_checkpoint_path.clone();
        checkpoint
    }

    /// Restore state from resume state
    fn from_resume_state(resume: &crate::pipeline::events::RefinementResumeState) -> Self {
        let mut state = Self::new();

        if !resume.quality_history.is_empty() {
            state.quality_history = resume.quality_history.clone();
            state.prev_quality = resume.quality_history.last().copied();
        }

        state.level_history = resume
            .level_history
            .iter()
            .map(|l| match l {
                QualityLevelSnapshot::BelowFloor => QualityLevel::BelowFloor,
                QualityLevelSnapshot::AtFloor => QualityLevel::AtFloor,
                QualityLevelSnapshot::AtTarget => QualityLevel::AtTarget,
            })
            .collect();

        state.last_quality_level = state.level_history.last().copied();
        state.stagnation_count = resume.stagnation_count;
        state.consecutive_clean_passes = resume.consecutive_clean_passes;
        state.best_quality = resume.best_quality;
        state.compaction_summary = resume.compaction_summary.clone();
        state.latest_checkpoint_path = resume.latest_checkpoint_path.clone();

        state
    }
}

struct RefinementConfig {
    max_iterations: usize,
    target_quality: f32,
    quality_floor: f32,
    issues_per_iteration: usize,
    stagnation_patience: usize,
    stagnation_threshold: f32,
    min_improvement: f32,
    consecutive_passes_required: usize,
    detect_oscillation: bool,
    oscillation_window: usize,
    oscillation_min_amplitude: f32,
    quality_acceptance_delta: f32,
    /// Save a full artifact checkpoint every N iterations (0 = disabled).
    checkpoint_every_iterations: usize,
}

impl From<&Config> for RefinementConfig {
    fn from(config: &Config) -> Self {
        let refinement = &config.refinement;
        let adaptive = &refinement.adaptive_iteration;

        Self {
            max_iterations: adaptive.base_iterations + adaptive.max_extension,
            target_quality: config.convergence.target_quality,
            quality_floor: config.convergence.quality_floor,
            issues_per_iteration: refinement.issues_per_iteration,
            stagnation_patience: refinement.stagnation_patience,
            stagnation_threshold: refinement.stagnation_threshold,
            min_improvement: refinement.min_improvement_per_iteration,
            consecutive_passes_required: refinement.post_convergence_passes,
            detect_oscillation: refinement.detect_oscillation,
            oscillation_window: refinement.oscillation_window,
            oscillation_min_amplitude: refinement.oscillation_min_amplitude,
            quality_acceptance_delta: refinement.quality_acceptance_delta,
            checkpoint_every_iterations: refinement.checkpoint_every_iterations,
        }
    }
}

pub struct RefinementEngine {
    project_root: PathBuf,
    config: Config,
    event_store: Arc<EventStore>,
    judge: LlmJudge,
    strategy_rotator: StrategyRotator,
    failure_tracker: FailureTracker,
    feedback_selector: FeedbackAwareSelector,
    feedback_aggregator: FeedbackAggregator,
    file_registry: Option<VerifiedFileRegistry>,
    llm_modules: Vec<crate::pipeline::analysis::deep_analyzer::CoreModule>,
}

impl RefinementEngine {
    pub fn new(
        project_root: PathBuf,
        provider: Arc<dyn LlmProvider>,
        config: Config,
        event_store: Arc<EventStore>,
    ) -> Self {
        let strategy_rotator = StrategyRotator::with_strategies(
            Arc::clone(&provider),
            &config.refinement.enabled_strategies,
        );
        let target_quality = config.convergence.target_quality;
        let strategy_retry_limit = config.refinement.strategy_retry_limit;
        let fw = &config.feedback_weights;
        let feedback_aggregator = FeedbackAggregator::new(target_quality)
            .weights(fw.quality, fw.structural, fw.evidence);
        let judge = LlmJudge::new(provider);

        Self {
            project_root,
            config,
            event_store,
            judge,
            strategy_rotator,
            failure_tracker: FailureTracker::new(strategy_retry_limit),
            feedback_selector: FeedbackAwareSelector::new(),
            feedback_aggregator,
            file_registry: None,
            llm_modules: Vec::new(),
        }
    }

    /// Async constructor that creates an EventStore internally from the project root.
    /// Accepts a file registry directly instead of an EventStore.
    pub async fn new_async(
        project_root: PathBuf,
        provider: Arc<dyn LlmProvider>,
        config: Config,
        file_registry: VerifiedFileRegistry,
    ) -> Result<Self> {
        let event_store = Arc::new(EventStore::create(&project_root).await?);
        let mut engine = Self::new(project_root, provider, config, event_store);
        engine = engine.file_registry(file_registry);
        Ok(engine)
    }

    pub fn file_registry(mut self, registry: VerifiedFileRegistry) -> Self {
        self.judge = self.judge.file_registry(registry.clone());
        self.file_registry = Some(registry);
        self
    }

    pub fn llm_modules(mut self, modules: Vec<crate::pipeline::analysis::deep_analyzer::CoreModule>) -> Self {
        self.llm_modules = modules;
        self
    }

    pub fn project_context(mut self, ctx: crate::pipeline::quality::ProjectContext) -> Self {
        self.judge = self.judge.project_context(ctx);
        self
    }

    pub async fn refine(
        &mut self,
        mut skills: Vec<Skill>,
        mut agents: Vec<Agent>,
        mut rules: Vec<Rule>,
        claude_md: &ClaudeMdContent,
        resume_state: &ResumeState,
    ) -> Result<RefinementResult> {
        let cfg = RefinementConfig::from(&self.config);

        let start_iteration = resume_state
            .refinement
            .last_completed_iteration
            .map(|i| i + 1)
            .unwrap_or(0);

        // Restore state from resume or create fresh
        let mut state = if start_iteration > 0 {
            // Compact state before loading to prevent unbounded memory growth
            let mut resume_refinement = resume_state.refinement.clone();
            let compactor = IncrementalCompactor::new();
            let compaction_result = compactor.compact(&mut resume_refinement);
            if compaction_result.was_compacted() {
                tracing::info!(
                    iterations_removed = compaction_result.iterations_removed,
                    quality_entries_compacted = compaction_result.quality_entries_compacted,
                    "State compacted during resume"
                );
                // Preserve compaction summary so it survives across resume cycles
                if let Some(summary) = compaction_result.quality_summary {
                    resume_refinement.compaction_summary = Some(summary);
                }
            }

            let mut restored = RefinementState::from_resume_state(&resume_refinement);
            // Also restore best snapshot if available
            if let Some(ref path) = resume_refinement.best_state_path
                && let Ok(snapshot) = self.load_snapshot(path).await
            {
                restored.best = Some(snapshot.clone());
                restored.best_quality = snapshot.quality;
            }

            // Restore artifacts from the most recent periodic checkpoint.
            // This prevents losing all intermediate refinement work on crash:
            // without this, resume would start from the original (pre-refinement)
            // artifacts even though dozens of iterations may have completed.
            if let Some(ref path) = resume_refinement.latest_checkpoint_path
                && let Ok(snapshot) = self.load_snapshot(path).await
            {
                tracing::info!(
                    checkpoint_iteration = snapshot.iteration,
                    checkpoint_quality = format!("{:.1}%", snapshot.quality * 100.0),
                    "Restoring artifacts from periodic checkpoint"
                );
                skills = snapshot.skills;
                agents = snapshot.agents;
                rules = snapshot.rules;
            }

            tracing::info!(
                resuming_from = start_iteration,
                quality_history_len = restored.quality_history.len(),
                stagnation_count = restored.stagnation_count,
                has_checkpoint = restored.latest_checkpoint_path.is_some(),
                "Resuming refinement with restored state"
            );
            restored
        } else {
            RefinementState::new()
        };

        let file_registry = match &self.file_registry {
            Some(r) => r.clone(),
            None => VerifiedFileRegistry::build(&self.project_root).await?,
        };

        let mut iteration = start_iteration;

        loop {
            if iteration >= cfg.max_iterations {
                break;
            }

            self.emit(
                EventType::IterationStarted,
                EventPayload::IterationStarted { iteration },
            )
            .await?;

            tracing::info!(
                iteration = iteration + 1,
                max = cfg.max_iterations,
                "Starting refinement iteration"
            );

            let iter_progress = resume_state.refinement.iteration_progress.get(&iteration);

            // Use selective assessment after first iteration (uses cached judgments for unmodified artifacts)
            let (judgment, combined_quality, structural_result, per_artifact) = if iteration == 0 {
                let result = self
                    .assess_quality(&skills, &agents, &rules, claude_md)
                    .await?;
                // Seed the cache with initial judgments
                Self::seed_judgment_cache(&mut state, &skills, &agents, &rules, &result.3)?;
                result
            } else {
                self.assess_quality_selective(&skills, &agents, &rules, claude_md, &mut state)
                    .await?
            };

            // Calculate metrics using unified ArtifactQuality system
            let contents = Self::extract_artifact_contents(&skills, &agents, &rules);
            let content_refs: Vec<&str> = contents.iter().map(|s| s.as_str()).collect();
            let metrics = QualityMetrics::from_results_with_content(
                &per_artifact,
                &content_refs,
                &|path: &str| file_registry.file_exists(path),
            );

            self.emit(
                EventType::QualityAssessed,
                EventPayload::QualityAssessed {
                    iteration,
                    surface: judgment.overall_score,
                    judgment: judgment.overall_score,
                    combined: combined_quality,
                    issues_count: judgment.issues.len(),
                },
            )
            .await?;

            // Emit per-artifact judgment events (ordered: skills, agents, rules)
            {
                let mut idx = 0;
                for skill in &skills {
                    if let Some(result) = per_artifact.get(idx)
                        && let Err(e) = self.emit(
                            EventType::ArtifactJudged,
                            EventPayload::ArtifactJudged {
                                artifact_type: "skill".to_string(),
                                artifact_name: skill.name.clone(),
                                score: result.overall_score,
                                pass: result.overall_score >= cfg.quality_floor,
                            },
                        ).await
                    {
                        tracing::warn!(error = %e, "Failed to emit event");
                    }
                    idx += 1;
                }
                for agent in &agents {
                    if let Some(result) = per_artifact.get(idx)
                        && let Err(e) = self.emit(
                            EventType::ArtifactJudged,
                            EventPayload::ArtifactJudged {
                                artifact_type: "agent".to_string(),
                                artifact_name: agent.name.clone(),
                                score: result.overall_score,
                                pass: result.overall_score >= cfg.quality_floor,
                            },
                        ).await
                    {
                        tracing::warn!(error = %e, "Failed to emit event");
                    }
                    idx += 1;
                }
                for rule in &rules {
                    if let Some(result) = per_artifact.get(idx)
                        && let Err(e) = self.emit(
                            EventType::ArtifactJudged,
                            EventPayload::ArtifactJudged {
                                artifact_type: "rule".to_string(),
                                artifact_name: rule.name.clone(),
                                score: result.overall_score,
                                pass: result.overall_score >= cfg.quality_floor,
                            },
                        ).await
                    {
                        tracing::warn!(error = %e, "Failed to emit event");
                    }
                    idx += 1;
                }
            }

            tracing::info!(
                iteration = iteration + 1,
                quality = format!("{:.1}%", combined_quality * 100.0),
                avg_score = format!("{:.2}", metrics.avg_score),
                low_validity = format!(
                    "{}/{} ({:.0}%)",
                    metrics.low_validity_count,
                    metrics.total_artifacts,
                    metrics.low_validity_ratio * 100.0
                ),
                acceptable = format!(
                    "{}/{} ({:.0}%)",
                    metrics.acceptable_count,
                    metrics.total_artifacts,
                    metrics.acceptable_ratio() * 100.0
                ),
                issues = judgment.issues.len(),
                "Quality assessed"
            );

            // Update per-artifact convergence tracking
            Self::update_converged_artifacts(
                &mut state,
                &skills,
                &agents,
                &rules,
                &per_artifact,
                &|path: &str| file_registry.file_exists(path),
            );

            if state.best.is_none() || combined_quality > state.best_quality {
                let snapshot = RefinementSnapshot {
                    skills: skills.clone(),
                    agents: agents.clone(),
                    rules: rules.clone(),
                    quality: combined_quality,
                    iteration,
                };

                let snapshot_path = self.save_snapshot(&snapshot, iteration).await?;
                state.best = Some(snapshot);
                state.best_quality = combined_quality;

                self.emit(
                    EventType::BestStateUpdated,
                    EventPayload::BestStateUpdated {
                        iteration,
                        quality: combined_quality,
                        snapshot_path,
                    },
                )
                .await?;

                tracing::debug!(
                    iteration = iteration + 1,
                    quality = format!("{:.1}%", combined_quality * 100.0),
                    "Best state updated"
                );
            }

            match self.check_convergence(&metrics, combined_quality, &cfg, &mut state) {
                ConvergenceResult::Converged(path) => {
                    tracing::info!(
                        path = ?path,
                        iteration,
                        quality = format!("{:.1}%", combined_quality * 100.0),
                        "Refinement converged"
                    );
                    self.emit(
                        EventType::IterationCompleted,
                        EventPayload::IterationCompleted {
                            iteration,
                            quality: combined_quality,
                            converged: true,
                        },
                    )
                    .await?;
                    return Ok(self.build_success_result(
                        skills,
                        agents,
                        rules,
                        &state,
                        &judgment,
                        structural_result,
                        iteration,
                    ));
                }
                ConvergenceResult::LevelOscillation => {
                    tracing::info!(
                        iteration,
                        quality = format!("{:.1}%", combined_quality * 100.0),
                        "Accepting state after level oscillation"
                    );
                    self.emit(
                        EventType::IterationCompleted,
                        EventPayload::IterationCompleted {
                            iteration,
                            quality: combined_quality,
                            converged: true,
                        },
                    )
                    .await?;
                    return Ok(self.build_success_result(
                        skills,
                        agents,
                        rules,
                        &state,
                        &judgment,
                        structural_result,
                        iteration,
                    ));
                }
                ConvergenceResult::NotConverged => {}
            }

            // Prune artifacts with hallucinations BEFORE refinement
            // These cannot be meaningfully refined - they have invalid references
            let prune_stats = Self::prune_hallucinated_artifacts(
                &mut skills,
                &mut agents,
                &mut rules,
                &per_artifact,
            )?;

            // Re-assess if anything was pruned, and invalidate cache for pruned artifacts
            // NOTE: We capture per_artifact for consistency even though identify_all_issues uses judgment
            let (judgment, combined_quality, _per_artifact) = if prune_stats.total() > 0 {
                // Invalidate cache entries for pruned artifacts
                Self::invalidate_pruned_from_cache(&mut state, &skills, &agents, &rules);
                let (j, q, _, new_per_artifact) = self
                    .assess_quality(&skills, &agents, &rules, claude_md)
                    .await?;
                (j, q, new_per_artifact)
            } else {
                (judgment, combined_quality, per_artifact)
            };

            let issues = self.identify_all_issues(&judgment, &skills, &agents, &rules);

            let completed_items = iter_progress
                .map(|p| p.completed_items.clone())
                .unwrap_or_default();

            self.apply_refinements(
                &mut skills,
                &mut agents,
                &mut rules,
                &issues,
                &judgment,
                &file_registry,
                iteration,
                &completed_items,
                &cfg,
                &mut state,
            )
            .await?;

            // Content hash oscillation detection: catch flip-flopping refinements
            let content_hash = {
                let contents = Self::extract_artifact_contents(&skills, &agents, &rules);
                crate::utils::hash::content_hash(&contents.join("\n"))
            };
            if self.strategy_rotator.record_content_hash(content_hash) {
                if self.strategy_rotator.is_at_max_escalation() {
                    tracing::warn!(iteration, "Content oscillation at max escalation — forcing regeneration");
                    self.strategy_rotator.force_regeneration();
                } else {
                    tracing::warn!(iteration, "Content oscillation: escalating strategy");
                    self.strategy_rotator.escalate();
                }
            }

            self.handle_quality_patterns(&mut state, combined_quality, &cfg);

            self.emit(
                EventType::IterationCompleted,
                EventPayload::IterationCompleted {
                    iteration,
                    quality: combined_quality,
                    converged: false,
                },
            )
            .await?;

            // Save periodic artifact checkpoint every N iterations.
            // This ensures crash recovery loses at most N-1 iterations of work
            // instead of all previous iterations.
            if should_checkpoint(iteration, cfg.checkpoint_every_iterations) {
                let checkpoint_snapshot = RefinementSnapshot {
                    skills: skills.clone(),
                    agents: agents.clone(),
                    rules: rules.clone(),
                    quality: combined_quality,
                    iteration,
                };
                let checkpoint_path = self
                    .save_snapshot(&checkpoint_snapshot, iteration)
                    .await?;
                state.latest_checkpoint_path = Some(checkpoint_path.clone());
                tracing::info!(
                    iteration = iteration + 1,
                    quality = format!("{:.1}%", combined_quality * 100.0),
                    path = %checkpoint_path,
                    "Periodic artifact checkpoint saved"
                );
            }

            // Emit checkpoint for resume state preservation
            let checkpoint = state.to_checkpoint(iteration, HashMap::new());
            self.emit(
                EventType::RefinementCheckpoint,
                EventPayload::RefinementCheckpoint { checkpoint },
            )
            .await?;

            iteration += 1;
        }

        Ok(self.build_max_iterations_result(&state, iteration))
    }

    async fn emit(&self, event_type: EventType, payload: EventPayload) -> Result<()> {
        self.event_store.append(event_type, payload).await
    }

    async fn assess_quality(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ClaudeMdContent,
    ) -> Result<(
        JudgmentResult,
        f32,
        Option<StructuralValidationResult>,
        Vec<JudgmentResult>,
    )> {
        let artifacts = GeneratedArtifacts {
            skills: skills.to_vec(),
            agents: agents.to_vec(),
            rules: rules.to_vec(),
        };

        let judge_artifacts = crate::pipeline::quality::Artifacts::from(&artifacts);
        let per_artifact_results = self
            .judge
            .evaluate_all(&judge_artifacts)
            .await?;
        self.finalize_assessment(per_artifact_results, skills, agents, rules, claude_md)
            .await
    }

    /// Selective quality assessment that only re-evaluates modified artifacts.
    ///
    /// Uses cached judgments for unmodified artifacts, reducing LLM calls by ~85%
    /// when only a few artifacts are modified per iteration.
    async fn assess_quality_selective(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ClaudeMdContent,
        state: &mut RefinementState,
    ) -> Result<(
        JudgmentResult,
        f32,
        Option<StructuralValidationResult>,
        Vec<JudgmentResult>,
    )> {
        let mut per_artifact_results =
            Vec::with_capacity(skills.len() + agents.len() + rules.len());
        let mut skills_to_evaluate = Vec::new();
        let mut agents_to_evaluate = Vec::new();
        let mut rules_to_evaluate = Vec::new();

        // Collect artifacts needing evaluation
        for skill in skills {
            let key = Self::artifact_key(&ItemType::Skill, &skill.name);
            if state.needs_evaluation(&key) {
                skills_to_evaluate.push(skill.clone());
            }
        }
        for agent in agents {
            let key = Self::artifact_key(&ItemType::Agent, &agent.name);
            if state.needs_evaluation(&key) {
                agents_to_evaluate.push(agent.clone());
            }
        }
        for rule in rules {
            let key = Self::artifact_key(&ItemType::Rule, &rule.name);
            if state.needs_evaluation(&key) {
                rules_to_evaluate.push(rule.clone());
            }
        }

        let needs_eval =
            skills_to_evaluate.len() + agents_to_evaluate.len() + rules_to_evaluate.len();
        let total = skills.len() + agents.len() + rules.len();
        let cached = total - needs_eval;

        if cached > 0 {
            tracing::info!(
                needs_eval,
                cached,
                total,
                "Selective assessment: reusing cached judgments"
            );
        }

        // Evaluate only modified/new artifacts
        if !skills_to_evaluate.is_empty()
            || !agents_to_evaluate.is_empty()
            || !rules_to_evaluate.is_empty()
        {
            let artifacts = GeneratedArtifacts {
                skills: skills_to_evaluate.clone(),
                agents: agents_to_evaluate.clone(),
                rules: rules_to_evaluate.clone(),
            };
            let judge_artifacts = crate::pipeline::quality::Artifacts::from(&artifacts);
            let new_results = self
                .judge
                .evaluate_all(&judge_artifacts)
                .await?;

            // Update cache with new results
            // Verify result count matches expected (skills + agents + rules)
            let expected_count =
                skills_to_evaluate.len() + agents_to_evaluate.len() + rules_to_evaluate.len();
            if new_results.len() != expected_count {
                tracing::error!(
                    expected = expected_count,
                    actual = new_results.len(),
                    "evaluate_all returned incorrect number of results"
                );
                return Err(ClaudegenError::pipeline(
                    4, // Refinement phase
                    "evaluate_all",
                    format!(
                        "Expected {} results from evaluate_all, got {}",
                        expected_count,
                        new_results.len()
                    ),
                ));
            }

            for ((item_type, name), result) in Self::artifact_names(
                &skills_to_evaluate,
                &agents_to_evaluate,
                &rules_to_evaluate,
            )
            .zip(new_results)
            {
                state.cache_judgment(&Self::artifact_key(&item_type, name), result);
            }
        }

        // Build results in order from cache — all artifacts must have cached results
        // (either from initial assessment or from evaluation above)
        for (item_type, name) in Self::artifact_names(skills, agents, rules) {
            let key = Self::artifact_key(&item_type, name);
            let result = state.cached_judgment(&key).cloned().ok_or_else(|| {
                ClaudegenError::pipeline(
                    4,
                    "cache_miss",
                    format!("Missing cached judgment for {} '{}'", item_type, name),
                )
            })?;
            per_artifact_results.push(result);
        }

        // Clear modification tracking for next iteration
        state.clear_modifications();

        self.finalize_assessment(per_artifact_results, skills, agents, rules, claude_md)
            .await
    }

    /// Shared tail for quality assessment: aggregate judgments, run structural
    /// validation and cross-validation, compute combined quality score.
    async fn finalize_assessment(
        &self,
        per_artifact_results: Vec<JudgmentResult>,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ClaudeMdContent,
    ) -> Result<(
        JudgmentResult,
        f32,
        Option<StructuralValidationResult>,
        Vec<JudgmentResult>,
    )> {
        let judgment = LlmJudge::aggregate_results(&per_artifact_results);

        // Convert ClaudeMdContent to ProjectMemory for validation APIs
        let project_memory = crate::types::ProjectMemory {
            overview: claude_md.overview.clone(),
            architecture: claude_md.architecture.clone(),
            commands: Vec::new(),
            standards: claude_md.standards.clone(),
            imports: claude_md.imports.clone(),
            domain_knowledge: claude_md.domain_knowledge.clone(),
            gotchas: claude_md.gotchas.clone(),
            navigation: claude_md.navigation.clone(),
        };

        let structural_result = if !self.llm_modules.is_empty() {
            let analyzer =
                ArchitecturalAnalyzer::new(&self.config.structural_validation, &self.llm_modules);
            match analyzer.validate(skills, agents, rules, &project_memory).await {
                Ok(result) => {
                    tracing::debug!(
                        coverage = format!("{:.1}%", result.coverage_report.coverage * 100.0),
                        issues = result.issues.len(),
                        "Structural validation complete"
                    );
                    Some(result)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Structural validation failed");
                    None
                }
            }
        } else {
            None
        };

        let file_registry = self
            .file_registry
            .clone()
            .unwrap_or_else(VerifiedFileRegistry::empty);
        let cv_result =
            CrossValidationResult::check(skills, agents, rules, &project_memory, &file_registry);

        let aggregated = self.feedback_aggregator.aggregate(
            &judgment,
            structural_result.as_ref(),
            Some(&cv_result),
        );
        let combined_quality = aggregated.overall_score;

        Ok((
            judgment,
            combined_quality,
            structural_result,
            per_artifact_results,
        ))
    }

    fn check_convergence(
        &self,
        metrics: &QualityMetrics,
        combined_quality: f32,
        cfg: &RefinementConfig,
        state: &mut RefinementState,
    ) -> ConvergenceResult {
        let meets_target = combined_quality >= cfg.target_quality;
        let meets_floor = combined_quality >= cfg.quality_floor;
        let quality_acceptable = metrics.is_acceptable();

        tracing::debug!(
            avg_score = format!("{:.2}", metrics.avg_score),
            low_validity_ratio = format!("{:.1}%", metrics.low_validity_ratio * 100.0),
            acceptable = format!("{}/{}", metrics.acceptable_count, metrics.total_artifacts),
            quality_acceptable,
            "Convergence metrics"
        );

        let current_level = match (
            meets_target && quality_acceptable,
            meets_floor && quality_acceptable,
        ) {
            (true, _) => QualityLevel::AtTarget,
            (false, true) => QualityLevel::AtFloor,
            _ => QualityLevel::BelowFloor,
        };

        state.record_level(current_level);

        if self.detect_level_oscillation(&state.level_history, cfg.oscillation_window) {
            if meets_floor {
                tracing::info!(
                    quality = format!("{:.1}%", combined_quality * 100.0),
                    level = ?current_level,
                    "Detected level oscillation at acceptable quality - accepting current state"
                );
                return ConvergenceResult::LevelOscillation;
            }
            tracing::debug!(
                quality = format!("{:.1}%", combined_quality * 100.0),
                floor = format!("{:.1}%", cfg.quality_floor * 100.0),
                "Oscillation detected but quality below floor - continuing refinement"
            );
        }

        if state.last_quality_level == Some(current_level) {
            state.consecutive_clean_passes += 1;
        } else {
            state.consecutive_clean_passes = 1;
            state.last_quality_level = Some(current_level);
        }

        match current_level {
            QualityLevel::AtTarget
                if state.consecutive_clean_passes >= cfg.consecutive_passes_required =>
            {
                tracing::info!(
                    quality = format!("{:.1}%", combined_quality * 100.0),
                    avg_score = format!("{:.2}", metrics.avg_score),
                    low_validity_ratio = format!("{:.1}%", metrics.low_validity_ratio * 100.0),
                    passes = state.consecutive_clean_passes,
                    "Converged at target quality"
                );
                ConvergenceResult::Converged(ConvergencePath::TargetMet)
            }
            QualityLevel::AtFloor
                if state.consecutive_clean_passes >= (cfg.consecutive_passes_required as f32 * FLOOR_CONVERGENCE_PASS_MULTIPLIER) as usize =>
            {
                tracing::info!(
                    quality = format!("{:.1}%", combined_quality * 100.0),
                    avg_score = format!("{:.2}", metrics.avg_score),
                    low_validity_ratio = format!("{:.1}%", metrics.low_validity_ratio * 100.0),
                    passes = state.consecutive_clean_passes,
                    "Converged at quality floor"
                );
                ConvergenceResult::Converged(ConvergencePath::FloorMetExtended)
            }
            _ => ConvergenceResult::NotConverged,
        }
    }

    fn detect_level_oscillation(&self, history: &[QualityLevel], window: usize) -> bool {
        if history.len() < window || window < 2 {
            return false;
        }

        let recent: Vec<_> = history.iter().rev().take(window).collect();
        let mut changes = 0;

        for i in 1..recent.len() {
            if recent[i] != recent[i - 1] {
                changes += 1;
            }
        }

        let oscillation_ratio = changes as f32 / (window - 1) as f32;
        oscillation_ratio >= OSCILLATION_DETECTION_THRESHOLD
    }

    fn identify_all_issues(
        &self,
        judgment: &JudgmentResult,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
    ) -> Vec<DetectedArtifactIssue> {
        use crate::pipeline::quality::{IssueCode, KnownIssueCode};
        use crate::pipeline::quality::IssueSeverity;
        use crate::types::DiagnosticLevel;

        let mut issues = Vec::new();

        for quality_issue in &judgment.issues {
            let Some((item_type, item_name)) =
                self.find_issue_target(quality_issue, skills, agents, rules)
            else {
                tracing::debug!(code = %quality_issue.code, "No valid target for issue, skipping");
                continue;
            };

            let severity = match quality_issue.severity {
                IssueSeverity::Critical => DiagnosticLevel::Error,
                IssueSeverity::Major => DiagnosticLevel::Warning,
                IssueSeverity::Minor => DiagnosticLevel::Info,
            };

            let issue = match IssueCode::from(quality_issue.code.as_str()) {
                IssueCode::Known(KnownIssueCode::TooShort) => {
                    let actual = Self::artifact_content_len(&item_type, &item_name, skills, agents, rules);
                    DetectedIssue::TooShort { actual, min: MIN_ARTIFACT_CONTENT_LENGTH }
                }
                IssueCode::Known(KnownIssueCode::MissingReferences | KnownIssueCode::InvalidFileReference) => {
                    let actual = Self::artifact_ref_count(&item_type, &item_name, skills, agents, rules);
                    let expected = self.min_file_refs_for(&item_type);
                    DetectedIssue::MissingReferences { expected, actual }
                }
                IssueCode::Known(KnownIssueCode::MissingSections) => {
                    let actual = Self::artifact_section_count(&item_type, &item_name, skills, agents, rules);
                    let expected = self.min_sections_for(&item_type);
                    DetectedIssue::MissingSections { expected, actual }
                }
                IssueCode::Known(KnownIssueCode::LowActionability) => {
                    // JudgmentResult doesn't have per-dimension scores; use overall_score as proxy
                    let score = judgment.overall_score;
                    DetectedIssue::LowActionability { score, threshold: MIN_ACTIONABILITY_THRESHOLD }
                }
                IssueCode::Known(KnownIssueCode::TooGeneric | KnownIssueCode::VagueGuidance) => {
                    DetectedIssue::TooGeneric { description: quality_issue.message.clone() }
                }
                IssueCode::Known(KnownIssueCode::WeakEvidence | KnownIssueCode::MissingExamples) => {
                    DetectedIssue::WeakEvidence { description: quality_issue.message.clone() }
                }
                IssueCode::Known(KnownIssueCode::LowVerificationRatio) => {
                    let (ratio, threshold) = Self::artifact_content(&item_type, &item_name, skills, agents, rules)
                        .map(|content| {
                            let profile = crate::pipeline::quality::EvidenceLabelScanner::scan(&content);
                            (profile.verification_ratio(), self.config.quality.min_verification_ratio)
                        })
                        .unwrap_or((0.0, 0.0));
                    DetectedIssue::LowVerificationRatio { ratio, threshold }
                }
                IssueCode::Known(KnownIssueCode::Shallow) => {
                    DetectedIssue::Shallow { description: quality_issue.message.clone() }
                }
                IssueCode::Known(KnownIssueCode::Redundant) => {
                    DetectedIssue::Redundant { description: quality_issue.message.clone() }
                }
                IssueCode::Known(KnownIssueCode::MissingModule) => {
                    DetectedIssue::MissingModule {
                        module_name: quality_issue.message.clone(),
                        file_count: 0,
                        key_files: Vec::new(),
                    }
                }
                IssueCode::Known(KnownIssueCode::PartialModuleCoverage) => {
                    DetectedIssue::PartialModuleCoverage {
                        module_name: quality_issue.message.clone(),
                        reference_count: 0,
                    }
                }
                IssueCode::Unknown(_) => {
                    DetectedIssue::Other {
                        kind: quality_issue.code.clone(),
                        description: quality_issue.message.clone(),
                    }
                }
            };

            issues.push(DetectedArtifactIssue {
                item_type,
                item_name,
                issue,
                severity,
            });
        }

        issues.sort_by(|a, b| b.severity.cmp(&a.severity));
        issues
    }

    fn artifact_content_len(
        item_type: &ItemType,
        item_name: &str,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
    ) -> usize {
        match item_type {
            ItemType::Skill => skills.iter().find(|s| s.name == item_name).map(|s| s.body.len()).unwrap_or(0),
            ItemType::Agent => agents.iter().find(|a| a.name == item_name).map(|a| a.prompt.len()).unwrap_or(0),
            ItemType::Rule => rules.iter().find(|r| r.name == item_name)
                .map(|r| r.content.iter().map(|s| s.len()).sum())
                .unwrap_or(0),
        }
    }

    fn min_file_refs_for(&self, item_type: &ItemType) -> usize {
        let qc = &self.config.quality;
        match item_type {
            ItemType::Skill => qc.skill.min_file_refs,
            ItemType::Agent => qc.agent.min_file_refs,
            ItemType::Rule => qc.min_file_refs,
        }
    }

    fn min_sections_for(&self, item_type: &ItemType) -> usize {
        match item_type {
            ItemType::Agent => self.config.quality.agent.min_sections,
            _ => 3,
        }
    }

    fn artifact_ref_count(
        item_type: &ItemType,
        item_name: &str,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
    ) -> usize {
        match item_type {
            ItemType::Skill => skills.iter().find(|s| s.name == item_name)
                .map(|s| patterns::count_file_line_refs(&s.body)).unwrap_or(0),
            ItemType::Agent => agents.iter().find(|a| a.name == item_name)
                .map(|a| patterns::count_file_line_refs(&a.prompt)).unwrap_or(0),
            ItemType::Rule => rules.iter().find(|r| r.name == item_name)
                .map(|r| r.content.iter().map(|s| patterns::count_file_line_refs(s)).sum())
                .unwrap_or(0),
        }
    }

    fn artifact_section_count(
        item_type: &ItemType,
        item_name: &str,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
    ) -> usize {
        match item_type {
            ItemType::Skill => skills.iter().find(|s| s.name == item_name)
                .map(|s| s.body.lines().filter(|l| l.starts_with('#')).count()).unwrap_or(0),
            ItemType::Agent => agents.iter().find(|a| a.name == item_name)
                .map(|a| a.prompt.lines().filter(|l| l.starts_with('#')).count()).unwrap_or(0),
            ItemType::Rule => rules.iter().find(|r| r.name == item_name)
                .map(|r| r.content.iter().flat_map(|s| s.lines()).filter(|l| l.starts_with('#')).count())
                .unwrap_or(0),
        }
    }

    fn artifact_content<'b>(
        item_type: &ItemType,
        item_name: &str,
        skills: &'b [Skill],
        agents: &'b [Agent],
        rules: &'b [Rule],
    ) -> Option<String> {
        match item_type {
            ItemType::Skill => skills.iter().find(|s| s.name == item_name).map(|s| s.body.clone()),
            ItemType::Agent => agents.iter().find(|a| a.name == item_name).map(|a| a.prompt.clone()),
            ItemType::Rule => rules.iter().find(|r| r.name == item_name)
                .map(|r| r.content.join("\n")),
        }
    }

    fn find_issue_target(
        &self,
        issue: &crate::pipeline::quality::QualityIssue,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
    ) -> Option<(ItemType, String)> {
        for (item_type, name) in Self::artifact_names(skills, agents, rules) {
            if issue.message.contains(name)
                || issue.evidence.iter().any(|e| e.contains(name))
            {
                return Some((item_type, name.to_string()));
            }
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_refinements(
        &mut self,
        skills: &mut [Skill],
        agents: &mut [Agent],
        rules: &mut [Rule],
        issues: &[DetectedArtifactIssue],
        judgment: &JudgmentResult,
        file_registry: &VerifiedFileRegistry,
        iteration: usize,
        completed_items: &HashSet<String>,
        cfg: &RefinementConfig,
        state: &mut RefinementState,
    ) -> Result<()> {
        for (idx, issue) in issues.iter().take(cfg.issues_per_iteration).enumerate() {
            let item_key =
                IterationProgress::item_key(&issue.item_type.to_string(), &issue.item_name);
            if completed_items.contains(&item_key) {
                tracing::debug!(item = %item_key, "Skipping completed item");
                continue;
            }

            // Skip artifacts that have already converged (passed quality threshold)
            let artifact_key = Self::artifact_key(&issue.item_type, &issue.item_name);
            if state.is_converged(&artifact_key) {
                tracing::debug!(artifact = %artifact_key, "Skipping converged artifact");
                continue;
            }

            tracing::info!(
                iteration = iteration + 1,
                issue = idx + 1,
                item = %issue.item_name,
                "Refining issue"
            );

            let strategy_issue_kind = StrategyIssueKind::from(&issue.issue);

            // Collect suggestions for feedback-aware selection
            let suggestions: Vec<String> = judgment
                .suggestions
                .iter()
                .map(|s| s.action.clone())
                .collect();
            let first_suggestion: Option<String> = suggestions.first().cloned();

            // Use FeedbackAwareSelector to get recommended strategy based on issue + suggestions
            let recommended = self.feedback_selector.select_strategy(
                &issue.item_name,
                &issue.issue,
                first_suggestion.as_deref(),
                &self.failure_tracker,
            );

            // Get strategy from rotator, preferring the feedback-recommended strategy
            let strategy = if let Some(s) = self
                .strategy_rotator
                .get_strategy_by_name(recommended.as_str())
            {
                s
            } else {
                self.strategy_rotator
                    .select_strategy(&issue.item_name, &strategy_issue_kind)
            };
            let strategy_name = strategy.name().to_string();

            // Skip if this strategy has failed repeatedly for this artifact
            if self
                .failure_tracker
                .should_skip(&issue.item_name, &strategy_name)
            {
                tracing::debug!(
                    item = %issue.item_name,
                    strategy = %strategy_name,
                    "Skipping strategy due to repeated failures"
                );
                continue;
            }

            let context = StrategyContext::new(file_registry)
                .issues(vec![issue.issue.to_strategy_issue()])
                .suggestions(suggestions)
                .acceptance_delta(self.config.refinement.quality_acceptance_delta);

            // Use validated quality (same metric as strategies) for consistency
            let (success, quality_before, quality_after) = match issue.item_type {
                ItemType::Skill => {
                    if let Some(skill) = skills.iter_mut().find(|s| s.name == issue.item_name) {
                        let before = crate::pipeline::strategy::calculate_validated_quality(
                            &skill.body,
                            context.file_registry,
                        );
                        let result = strategy.refine_skill(skill, &context).await?;
                        let after = crate::pipeline::strategy::calculate_validated_quality(
                            &skill.body,
                            context.file_registry,
                        );
                        (result.success, before, after)
                    } else {
                        (false, 0.0, 0.0)
                    }
                }
                ItemType::Agent => {
                    if let Some(agent) = agents.iter_mut().find(|a| a.name == issue.item_name) {
                        let before = crate::pipeline::strategy::calculate_validated_quality(
                            &agent.prompt,
                            context.file_registry,
                        );
                        let result = strategy.refine_agent(agent, &context).await?;
                        let after = crate::pipeline::strategy::calculate_validated_quality(
                            &agent.prompt,
                            context.file_registry,
                        );
                        (result.success, before, after)
                    } else {
                        (false, 0.0, 0.0)
                    }
                }
                ItemType::Rule => {
                    if let Some(rule) = rules.iter_mut().find(|r| r.name == issue.item_name) {
                        let before = crate::pipeline::strategy::calculate_validated_quality(
                            &rule.to_markdown(),
                            context.file_registry,
                        );
                        let result = strategy.refine_rule(rule, &context).await?;
                        let after = crate::pipeline::strategy::calculate_validated_quality(
                            &rule.to_markdown(),
                            context.file_registry,
                        );
                        (result.success, before, after)
                    } else {
                        (false, 0.0, 0.0)
                    }
                }
            };

            let quality_delta = quality_after - quality_before;

            self.emit(
                EventType::IssueRefined,
                EventPayload::IssueRefined {
                    iteration,
                    issue_index: idx,
                    item_type: issue.item_type.to_string(),
                    item_name: issue.item_name.clone(),
                    strategy: strategy_name.clone(),
                    success,
                    quality_delta,
                },
            )
            .await?;

            // Record outcome for strategy rotation optimization
            self.strategy_rotator.record_outcome(
                &issue.item_name,
                &strategy_issue_kind,
                StrategyAttempt {
                    strategy_name: strategy_name.clone(),
                    success,
                    quality_delta,
                    iteration,
                },
            );

            if success {
                self.failure_tracker
                    .record_success(&issue.item_name, &strategy_name);
                self.feedback_selector.record_success(&issue.item_name);
                // Mark artifact as modified for selective re-assessment in next iteration
                state.mark_modified(&Self::artifact_key(&issue.item_type, &issue.item_name));

                // Early termination: if significant improvement achieved, skip remaining issues
                if quality_delta >= cfg.quality_acceptance_delta {
                    tracing::debug!(
                        strategy = %strategy_name,
                        delta = format!("{:.3}", quality_delta),
                        threshold = format!("{:.3}", cfg.quality_acceptance_delta),
                        remaining = issues.len().saturating_sub(idx + 1),
                        "Early termination - significant improvement achieved, skipping remaining issues"
                    );
                    break;
                }
            } else {
                self.failure_tracker
                    .record_failure(&issue.item_name, &strategy_name);
                self.feedback_selector.record_failure(
                    &issue.item_name,
                    &strategy_name,
                    first_suggestion.clone(),
                );
            }
        }

        Ok(())
    }

    fn handle_quality_patterns(
        &mut self,
        state: &mut RefinementState,
        quality: f32,
        cfg: &RefinementConfig,
    ) {
        state.record_quality(quality);

        if let Some(prev) = state.prev_quality {
            let delta = (quality - prev).abs();
            let improved = quality > prev + cfg.min_improvement;

            let is_oscillating = self.detect_oscillation(&state.quality_history, cfg);
            let is_stagnating = delta < cfg.stagnation_threshold || !improved;

            match (is_oscillating, is_stagnating) {
                (true, true) => {
                    tracing::warn!("Oscillation + Stagnation: forcing regeneration");
                    self.strategy_rotator.force_regeneration();
                    state.stagnation_count = 0;
                }
                (true, false) => {
                    tracing::warn!("Oscillation detected: escalating");
                    self.strategy_rotator.escalate();
                    state.stagnation_count = 0;
                }
                (false, true) => {
                    state.stagnation_count += 1;
                    if state.stagnation_count >= cfg.stagnation_patience {
                        tracing::info!("Stagnation: escalating");
                        self.strategy_rotator.escalate();
                        state.stagnation_count = 0;
                    }
                }
                (false, false) => {
                    state.stagnation_count = 0;
                }
            }
        }

        state.prev_quality = Some(quality);
    }

    fn detect_oscillation(&self, history: &[f32], cfg: &RefinementConfig) -> bool {
        if !cfg.detect_oscillation {
            return false;
        }
        if history.len() < cfg.oscillation_window {
            return false;
        }

        let window: Vec<f32> = history
            .iter()
            .copied()
            .skip(history.len().saturating_sub(cfg.oscillation_window))
            .collect();

        if window.len() < 3 {
            return false;
        }

        let mut direction_changes = 0usize;
        let mut meaningful_pairs = 0usize;
        let mut prev_direction: Option<bool> = None;

        for i in 1..window.len() {
            let delta = (window[i] - window[i - 1]).abs();
            if delta < cfg.oscillation_min_amplitude {
                continue;
            }

            meaningful_pairs += 1;
            let current_direction = window[i] > window[i - 1];
            if let Some(prev) = prev_direction
                && prev != current_direction
            {
                direction_changes += 1;
            }
            prev_direction = Some(current_direction);
        }

        if meaningful_pairs == 0 {
            return false;
        }

        direction_changes as f32 / meaningful_pairs as f32 >= OSCILLATION_DETECTION_THRESHOLD
    }

    async fn save_snapshot(
        &self,
        snapshot: &RefinementSnapshot,
        iteration: usize,
    ) -> Result<String> {
        let json = serde_json::to_string_pretty(snapshot)?;
        self.event_store.save_iteration(iteration, &json).await
    }

    async fn load_snapshot(&self, path: &str) -> Result<RefinementSnapshot> {
        let json = EventStore::load_snapshot(path).await?;
        Ok(serde_json::from_str(&json)?)
    }

    /// Create artifact cache key from type and name.
    #[inline]
    fn artifact_key(item_type: &ItemType, name: &str) -> String {
        format!("{}:{}", item_type, name)
    }

    /// Update converged_artifacts based on per-artifact quality assessment.
    ///
    /// Artifacts that pass quality threshold are marked as converged and
    /// will be skipped in future refinement iterations.
    fn update_converged_artifacts(
        state: &mut RefinementState,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        per_artifact: &[JudgmentResult],
        valid_paths: &impl Fn(&str) -> bool,
    ) {
        let contents = Self::extract_artifact_contents(skills, agents, rules);
        let artifact_keys: Vec<_> = Self::artifact_names(skills, agents, rules)
            .map(|(t, n)| Self::artifact_key(&t, n))
            .collect();

        for ((key, _result), content) in artifact_keys
            .into_iter()
            .zip(per_artifact.iter())
            .zip(contents.iter())
        {
            let quality = ArtifactQuality::from_judgment(content, valid_paths);
            if quality.is_acceptable()
                && !state.is_converged(&key)
            {
                tracing::debug!(artifact = %key, "Artifact converged");
                state.mark_converged(&key);
            }
        }
    }

    /// Iterate over all artifacts in evaluation order, yielding `(ItemType, name)`.
    ///
    /// Order: skills, agents, rules (matches JudgmentResult ordering).
    fn artifact_names<'a>(
        skills: &'a [Skill],
        agents: &'a [Agent],
        rules: &'a [Rule],
    ) -> impl Iterator<Item = (ItemType, &'a str)> {
        skills
            .iter()
            .map(|s| (ItemType::Skill, s.name.as_str()))
            .chain(agents.iter().map(|a| (ItemType::Agent, a.name.as_str())))
            .chain(rules.iter().map(|r| (ItemType::Rule, r.name.as_str())))
    }

    /// Extract content strings from all artifacts in evaluation order.
    ///
    /// Order: skills, agents, rules (matches JudgmentResult ordering).
    fn extract_artifact_contents(
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
    ) -> Vec<String> {
        skills
            .iter()
            .map(|s| s.body.clone())
            .chain(agents.iter().map(|a| a.prompt.clone()))
            .chain(rules.iter().map(|r| r.to_markdown()))
            .collect()
    }

    /// Remove cache entries for artifacts that no longer exist.
    fn invalidate_pruned_from_cache(
        state: &mut RefinementState,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
    ) {
        let valid_keys: HashSet<String> = Self::artifact_names(skills, agents, rules)
            .map(|(t, n)| Self::artifact_key(&t, n))
            .collect();
        state.retain_valid_cache_entries(&valid_keys);
    }

    /// Seed judgment cache with initial assessment results.
    fn seed_judgment_cache(
        state: &mut RefinementState,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        per_artifact: &[JudgmentResult],
    ) -> Result<()> {
        let expected = skills.len() + agents.len() + rules.len();
        if expected != per_artifact.len() {
            return Err(ClaudegenError::pipeline(
                4,
                "cache_seeding",
                format!(
                    "Judgment count mismatch: expected {} ({}s+{}a+{}r), got {}",
                    expected,
                    skills.len(),
                    agents.len(),
                    rules.len(),
                    per_artifact.len()
                ),
            ));
        }
        for ((item_type, name), result) in
            Self::artifact_names(skills, agents, rules).zip(per_artifact)
        {
            state.cache_judgment(&Self::artifact_key(&item_type, name), result.clone());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_success_result(
        &self,
        skills: Vec<Skill>,
        agents: Vec<Agent>,
        rules: Vec<Rule>,
        state: &RefinementState,
        judgment: &JudgmentResult,
        structural_result: Option<StructuralValidationResult>,
        iteration: usize,
    ) -> RefinementResult {
        let (final_skills, final_agents, final_rules, final_quality) = match &state.best {
            Some(best) => (
                best.skills.clone(),
                best.agents.clone(),
                best.rules.clone(),
                best.quality,
            ),
            None => (skills, agents, rules, state.best_quality),
        };

        let dimensions = self.build_dimensions_status(judgment);
        let report = QualityAssessment::success(
            AssessmentPath::QualityTargetMet,
            iteration + 1,
            state.quality_history.clone(),
            dimensions,
            Vec::new(),
        );

        RefinementResult {
            skills: final_skills,
            agents: final_agents,
            rules: final_rules,
            iterations: iteration + 1,
            converged: true,
            final_quality,
            judgment: Some(judgment.clone()),
            structural_quality: structural_result,
            aggregated_feedback: None,
            convergence_report: Some(report),
            dirty_artifacts: state.all_modified.clone(),
        }
    }

    fn build_max_iterations_result(
        &self,
        state: &RefinementState,
        iteration: usize,
    ) -> RefinementResult {
        let (skills, agents, rules, quality) = if let Some(ref best) = state.best {
            (
                best.skills.clone(),
                best.agents.clone(),
                best.rules.clone(),
                best.quality,
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new(), 0.0)
        };

        tracing::warn!(
            iterations = iteration,
            quality = format!("{:.1}%", quality * 100.0),
            "Max iterations reached, returning best state"
        );

        RefinementResult {
            skills,
            agents,
            rules,
            iterations: iteration,
            converged: false,
            final_quality: quality,
            judgment: None,
            structural_quality: None,
            aggregated_feedback: None,
            convergence_report: None,
            dirty_artifacts: state.all_modified.clone(),
        }
    }

    fn build_dimensions_status(&self, judgment: &JudgmentResult) -> DimensionsStatus {
        let thresholds = &self.config.refinement.dimension_thresholds;
        let fallback = judgment.overall_score;

        // Use per-dimension scores from ValueAssessment when available,
        // falling back to overall_score when the LLM didn't produce them.
        let (actionability_score, specificity_score, evidence_score, depth_score) =
            match &judgment.value_assessment {
                Some(va) => (
                    if va.actionability > 0.0 { va.actionability } else { fallback },
                    if va.domain_specificity > 0.0 { va.domain_specificity } else { fallback },
                    fallback, // evidence score derived from overall when no separate metric
                    if va.information_density > 0.0 { va.information_density } else { fallback },
                ),
                None => (fallback, fallback, fallback, fallback),
            };

        DimensionsStatus {
            actionability: DimensionScore::new(
                "actionability",
                actionability_score,
                thresholds.semantic,
            ),
            specificity: DimensionScore::new(
                "specificity",
                specificity_score,
                thresholds.semantic * DIMENSION_THRESHOLD_MULTIPLIER,
            ),
            evidence_quality: DimensionScore::new(
                "evidence_quality",
                evidence_score,
                thresholds.evidence,
            ),
            depth: DimensionScore::new("depth", depth_score, thresholds.semantic * DIMENSION_THRESHOLD_MULTIPLIER),
            redundancy: DimensionScore::new_inverted(
                "redundancy",
                1.0 - fallback,
                REDUNDANCY_THRESHOLD,
            ),
            structural_coverage: None,
            cross_artifact: None,
            usability: None,
        }
    }

    /// Prune artifacts with hallucinations that cannot be meaningfully refined.
    ///
    /// Uses LLM judgment results for fact-based classification.
    ///
    /// Design principle: Artifacts with hallucinated references are fundamentally
    /// broken and should be deleted rather than refined.
    ///
    /// # Arguments
    /// * `per_artifact` - Judgment results in order: skills, agents, rules
    fn prune_hallucinated_artifacts(
        skills: &mut Vec<Skill>,
        agents: &mut Vec<Agent>,
        rules: &mut Vec<Rule>,
        per_artifact: &[JudgmentResult],
    ) -> Result<PruneStats> {
        let mut stats = PruneStats::default();

        let expected_len = skills.len() + agents.len() + rules.len();
        if per_artifact.len() != expected_len {
            return Err(ClaudegenError::pipeline(
                4,
                "artifact_pruning",
                format!(
                    "Judgment result count mismatch: expected {} ({}s+{}a+{}r), got {}",
                    expected_len,
                    skills.len(),
                    agents.len(),
                    rules.len(),
                    per_artifact.len()
                ),
            ));
        }

        // Results are ordered: skills (0..n), agents (n..n+m), rules (n+m..)
        let skill_results = &per_artifact[..skills.len()];
        let agent_start = skills.len();
        let agent_results = &per_artifact[agent_start..agent_start + agents.len()];
        let rule_start = agent_start + agents.len();
        let rule_results = &per_artifact[rule_start..];

        // Collect indices to prune (in reverse order for safe removal)
        let mut skills_to_prune: Vec<usize> = skill_results
            .iter()
            .enumerate()
            .filter_map(|(i, result)| {
                let skill = &skills[i];
                if skill.artifact_category() == ArtifactCategory::ProjectSpecific && result.tier == crate::types::ContentTier::Tier0Hallucinated {
                    tracing::info!(skill = %skill.name, "Pruning skill with hallucinations");
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        let mut agents_to_prune: Vec<usize> = agent_results
            .iter()
            .enumerate()
            .filter_map(|(i, result)| {
                let agent = &agents[i];
                if agent.artifact_category() == ArtifactCategory::ProjectSpecific && result.tier == crate::types::ContentTier::Tier0Hallucinated {
                    tracing::info!(agent = %agent.name, "Pruning agent with hallucinations");
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        let mut rules_to_prune: Vec<usize> = rule_results
            .iter()
            .enumerate()
            .filter_map(|(i, result)| {
                let rule = &rules[i];
                if rule.artifact_category() == ArtifactCategory::ProjectSpecific && result.tier == crate::types::ContentTier::Tier0Hallucinated {
                    tracing::info!(rule = %rule.name, "Pruning rule with hallucinations");
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        // Remove in reverse order to preserve indices
        skills_to_prune.sort_by(|a, b| b.cmp(a));
        agents_to_prune.sort_by(|a, b| b.cmp(a));
        rules_to_prune.sort_by(|a, b| b.cmp(a));

        stats.skills_pruned = skills_to_prune.len();
        stats.agents_pruned = agents_to_prune.len();
        stats.rules_pruned = rules_to_prune.len();

        for i in skills_to_prune {
            skills.remove(i);
        }
        for i in agents_to_prune {
            agents.remove(i);
        }
        for i in rules_to_prune {
            rules.remove(i);
        }

        if stats.total() > 0 {
            tracing::info!(
                skills = stats.skills_pruned,
                agents = stats.agents_pruned,
                rules = stats.rules_pruned,
                "Pruned artifacts with hallucinations"
            );
        }

        Ok(stats)
    }
}

#[derive(Debug, Default)]
struct PruneStats {
    skills_pruned: usize,
    agents_pruned: usize,
    rules_pruned: usize,
}

impl PruneStats {
    fn total(&self) -> usize {
        self.skills_pruned + self.agents_pruned + self.rules_pruned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_refinement_config_from_config() {
        let config = Config::default();
        let refinement_cfg = RefinementConfig::from(&config);

        assert!(refinement_cfg.max_iterations > 0);
        assert!(refinement_cfg.target_quality > 0.0);
        assert!(refinement_cfg.issues_per_iteration > 0);
        assert_eq!(
            refinement_cfg.checkpoint_every_iterations,
            crate::constants::refinement::DEFAULT_CHECKPOINT_EVERY_ITERATIONS,
        );
    }

    #[test]
    fn test_refinement_state_initialization() {
        let state = RefinementState::new();

        assert!(state.prev_quality.is_none());
        assert!(state.quality_history.is_empty());
        assert_eq!(state.stagnation_count, 0);
        assert!(state.best.is_none());
        assert_eq!(state.best_quality, 0.0);
        assert!(state.latest_checkpoint_path.is_none());
    }

    #[test]
    fn test_checkpoint_included_in_to_checkpoint() {
        let mut state = RefinementState::new();
        state.latest_checkpoint_path = Some("/tmp/iter_4.json".to_string());
        state.record_quality(0.75);

        let checkpoint = state.to_checkpoint(4, HashMap::new());
        assert_eq!(
            checkpoint.latest_checkpoint_path,
            Some("/tmp/iter_4.json".to_string())
        );
        assert_eq!(checkpoint.iteration, 4);
    }

    #[test]
    fn test_from_resume_state_restores_checkpoint_path() {
        use crate::pipeline::events::RefinementResumeState;

        let resume = RefinementResumeState {
            last_completed_iteration: Some(9),
            quality_history: vec![0.5, 0.6, 0.7],
            stagnation_count: 2,
            consecutive_clean_passes: 1,
            best_quality: 0.7,
            latest_checkpoint_path: Some("/tmp/iter_9.json".to_string()),
            ..Default::default()
        };

        let state = RefinementState::from_resume_state(&resume);
        assert_eq!(
            state.latest_checkpoint_path,
            Some("/tmp/iter_9.json".to_string())
        );
        assert_eq!(state.stagnation_count, 2);
        assert_eq!(state.consecutive_clean_passes, 1);
        assert_eq!(state.best_quality, 0.7);
    }

    #[test]
    fn test_checkpoint_interval_boundary() {
        assert!(!should_checkpoint(0, 5));
        assert!(!should_checkpoint(1, 5));
        assert!(!should_checkpoint(2, 5));
        assert!(!should_checkpoint(3, 5));
        assert!(should_checkpoint(4, 5));
        assert!(!should_checkpoint(5, 5));
        assert!(!should_checkpoint(6, 5));
        assert!(!should_checkpoint(7, 5));
        assert!(!should_checkpoint(8, 5));
        assert!(should_checkpoint(9, 5));
        assert!(should_checkpoint(14, 5));
    }

    #[test]
    fn test_checkpoint_interval_disabled() {
        assert!(!should_checkpoint(0, 0));
        assert!(!should_checkpoint(4, 0));
        assert!(!should_checkpoint(9, 0));
        assert!(!should_checkpoint(99, 0));
    }
}
