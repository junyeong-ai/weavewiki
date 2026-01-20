//! Refinement Module
//!
//! Quality-based multi-stage generation with targeted refinement.
//! Uses bidirectional feedback system with multi-dimensional validation.
//! Integrates learning history for strategy optimization.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::ai::{with_timeout, LlmProvider};
use crate::config::Config;
use crate::types::{Agent, Result, Rule, Skill};

use super::analysis::architectural_analyzer::{
    ArchitecturalAnalyzer, StructuralValidationResult,
};
use super::convergence::{ConvergenceChecker, ConvergencePath, ConvergenceReport, Improvement};
use super::context::VerifiedFileRegistry;
use super::feedback::{AggregatedFeedback, FeedbackAggregator};
use super::learning::{LearningHistory, StrategyOutcome as LearningOutcome};
use super::patterns;
use super::phases::output_router::OutputPlan;
use super::strategy::{IssueKind as StrategyIssueKind, StrategyContext, StrategyOutcome, StrategyRotator};
use super::validation::{
    cross_artifact::{CrossArtifactResult, CrossArtifactValidator},
    cross_validation::{CrossValidationResult, CrossValidator},
    quality_validator::QualityValidator,
    semantic_validator::{
        IssueCategory as SemanticCategory, IssueSeverity as SemanticSeverity,
        SemanticQualityResult, SemanticValidator,
    },
    tier_filter::{self, ItemType, Tier1Violation},
    usability::{UsabilityResult, UsabilityValidator},
};

#[derive(Debug, Clone)]
pub struct RefinementResult {
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub rules: Vec<Rule>,
    pub iterations: usize,
    pub converged: bool,
    pub final_quality: f32,
    pub semantic_quality: Option<SemanticQualityResult>,
    pub structural_quality: Option<StructuralValidationResult>,
    pub aggregated_feedback: Option<AggregatedFeedback>,
    pub learning_summary: Option<super::learning::ProgressSummary>,
    pub convergence_report: Option<super::convergence::ConvergenceReport>,
}

#[derive(Debug, Clone)]
pub struct RefinementIssue {
    pub item_type: ItemType,
    pub item_name: String,
    pub issue: IssueKind,
    pub severity: IssueSeverity,
}

impl RefinementIssue {
    pub fn item_type_str(&self) -> &'static str {
        match self.item_type {
            ItemType::Skill => "skill",
            ItemType::Agent => "agent",
            ItemType::Rule => "rule",
            ItemType::ClaudeMd => "claude_md",
        }
    }
}

#[derive(Debug, Clone)]
pub enum IssueKind {
    TooShort { actual: usize, min: usize },
    MissingReferences { expected: usize, actual: usize },
    MissingSections { expected: usize, actual: usize },
    Tier1Content { violation: String },
    PlanMismatch,
    LowActionability { score: f32, threshold: f32 },
    TooGeneric { description: String },
    WeakEvidence { description: String },
    Redundant { description: String },
    Shallow { description: String },
    MissingModule { module_name: String, file_count: usize, key_files: Vec<String> },
    PartialModuleCoverage { module_name: String, coverage: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
}

/// Snapshot of refinement state for rollback functionality
#[derive(Clone)]
struct RefinementSnapshot {
    skills: Vec<Skill>,
    agents: Vec<Agent>,
    rules: Vec<Rule>,
    quality: f32,
    iteration: usize,
}

const MAX_TRAJECTORY_SIZE: usize = 100;

/// Configuration context for refinement loop (immutable during iteration)
struct RefinementConfig {
    max_iterations: usize,
    min_iterations: usize,
    target_quality: f32,
    stagnation_patience: usize,
    stagnation_threshold: f32,
    require_all_dimensions: bool,
    issues_per_iteration: usize,
    strategy_retry_limit: usize,
    oscillation_strict_passes: usize,
    oscillation_lenient_passes: usize,
    oscillation_stability_variance: f32,
    oscillation_variance_window: usize,
    enable_rollback: bool,
    rollback_threshold: f32,
    max_rollbacks: usize,
    post_convergence_verification: bool,
    post_convergence_passes_required: usize,
    max_convergence_detections: usize,
    min_quality_for_value_exit: f32,
}

impl RefinementConfig {
    fn from_config(config: &Config) -> Self {
        let refinement = config.refinement();
        let target_quality = config.quality().target_score;
        Self {
            max_iterations: refinement.max_iterations,
            min_iterations: refinement.min_iterations,
            target_quality,
            stagnation_patience: refinement.stagnation_patience,
            stagnation_threshold: refinement.stagnation_threshold,
            require_all_dimensions: refinement.require_all_dimensions,
            issues_per_iteration: refinement.issues_per_iteration,
            strategy_retry_limit: refinement.strategy_retry_limit,
            oscillation_strict_passes: refinement.oscillation_strict_passes,
            oscillation_lenient_passes: refinement.oscillation_lenient_passes,
            oscillation_stability_variance: refinement.oscillation_stability_variance,
            oscillation_variance_window: refinement.oscillation_variance_window,
            enable_rollback: refinement.enable_rollback,
            rollback_threshold: refinement.rollback_threshold,
            max_rollbacks: refinement.max_rollbacks,
            post_convergence_verification: refinement.post_convergence_verification,
            post_convergence_passes_required: refinement.post_convergence_passes,
            max_convergence_detections: refinement.max_convergence_detections,
            min_quality_for_value_exit: target_quality * 0.9,
        }
    }
}

/// Mutable state tracked across refinement iterations
struct IterationState {
    prev_quality: Option<f32>,
    stagnation_count: usize,
    last_structural_result: Option<StructuralValidationResult>,
    strategy_failures: HashMap<String, usize>,
    last_semantic_result: Option<SemanticQualityResult>,
    quality_trajectory: VecDeque<f32>,
    critical_improvements: Vec<Improvement>,
    best_state: Option<RefinementSnapshot>,
    rollback_count: usize,
    consecutive_convergence_passes: usize,
    total_convergence_detections: usize,
    tier3_trajectory: VecDeque<usize>,
}

impl IterationState {
    fn new() -> Self {
        Self {
            prev_quality: None,
            stagnation_count: 0,
            last_structural_result: None,
            strategy_failures: HashMap::new(),
            last_semantic_result: None,
            quality_trajectory: VecDeque::with_capacity(MAX_TRAJECTORY_SIZE),
            critical_improvements: Vec::new(),
            best_state: None,
            rollback_count: 0,
            consecutive_convergence_passes: 0,
            total_convergence_detections: 0,
            tier3_trajectory: VecDeque::with_capacity(10),
        }
    }

    fn record_quality(&mut self, quality: f32) {
        if self.quality_trajectory.len() >= MAX_TRAJECTORY_SIZE {
            self.quality_trajectory.pop_front();
        }
        self.quality_trajectory.push_back(quality);
    }

    fn record_tier3(&mut self, count: usize) {
        if self.tier3_trajectory.len() >= 10 {
            self.tier3_trajectory.pop_front();
        }
        self.tier3_trajectory.push_back(count);
    }

    fn decay_strategy_failures(&mut self) {
        for failures in self.strategy_failures.values_mut() {
            *failures = failures.saturating_sub(1);
        }
        self.strategy_failures.retain(|_, &mut v| v > 0);
    }
}

/// Collection of validators created at refinement start
struct Validators {
    semantic: Option<SemanticValidator>,
    quality: Option<QualityValidator>,
    structural: Option<ArchitecturalAnalyzer>,
    cross_artifact: Option<CrossArtifactValidator>,
    usability: Option<UsabilityValidator>,
}

impl Validators {
    fn new(config: &Config, provider: &Arc<dyn LlmProvider>, project_root: &PathBuf) -> Self {
        let semantic_config = config.semantic_validation();
        let use_ai_validation = semantic_config.use_ai_validation;

        let semantic = if !use_ai_validation {
            Some(SemanticValidator::new(semantic_config.clone(), project_root))
        } else {
            None
        };

        let quality = if use_ai_validation {
            Some(QualityValidator::new(Arc::clone(provider), &semantic_config))
        } else {
            None
        };

        let structural_config = config.structural_validation();
        let structural = if structural_config.enabled {
            Some(ArchitecturalAnalyzer::new(structural_config))
        } else {
            None
        };

        let cross_artifact_config = config.cross_artifact();
        let cross_artifact = if cross_artifact_config.enabled {
            Some(CrossArtifactValidator::new(
                cross_artifact_config.min_coherence_score,
                cross_artifact_config.max_overlap_ratio,
            ))
        } else {
            None
        };

        let usability_config = config.usability();
        let usability = if usability_config.enabled {
            Some(
                UsabilityValidator::new(crate::config::ProjectType::default())
                    .with_config(usability_config),
            )
        } else {
            None
        };

        Self { semantic, quality, structural, cross_artifact, usability }
    }

    async fn run_semantic(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &crate::types::ProjectMemory,
        project_context: &str,
    ) -> Result<SemanticQualityResult> {
        if let Some(ref qv) = self.quality {
            qv.validate(skills, agents, rules, claude_md, project_context).await
        } else {
            self.semantic
                .as_ref()
                .expect("semantic_validator required when ai_validation disabled")
                .validate(skills, agents, rules, claude_md)
                .await
        }
    }

    async fn run_structural(
        &self,
        file_registry: &VerifiedFileRegistry,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &crate::types::ProjectMemory,
    ) -> Result<Option<StructuralValidationResult>> {
        if let Some(ref analyzer) = self.structural {
            Ok(Some(analyzer.validate(file_registry, skills, agents, rules, claude_md).await?))
        } else {
            Ok(None)
        }
    }

    fn run_cross_artifact(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &crate::types::ProjectMemory,
    ) -> Option<CrossArtifactResult> {
        self.cross_artifact
            .as_ref()
            .map(|v| v.validate(skills, agents, rules, claude_md))
    }

    fn run_usability(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &crate::types::ProjectMemory,
    ) -> Option<UsabilityResult> {
        self.usability
            .as_ref()
            .map(|v| v.validate(skills, agents, rules, claude_md))
    }
}

pub struct RefinementEngine {
    project_root: PathBuf,
    provider: Arc<dyn LlmProvider>,
    config: Config,
    file_registry: Option<VerifiedFileRegistry>,
    strategy_rotator: StrategyRotator,
    learning_history: LearningHistory,
    feedback_aggregator: FeedbackAggregator,
}

impl RefinementEngine {
    pub fn new(
        project_root: PathBuf,
        provider: Arc<dyn LlmProvider>,
        config: Config,
        file_registry: VerifiedFileRegistry,
    ) -> Self {
        let strategy_rotator = StrategyRotator::with_strategies(
            Arc::clone(&provider),
            &config.refinement().enabled_strategies,
        );
        let learning_history = LearningHistory::new();
        let target_quality = config.quality().target_score;
        let feedback_aggregator = FeedbackAggregator::new(target_quality);

        Self {
            project_root,
            provider,
            config,
            file_registry: Some(file_registry),
            strategy_rotator,
            learning_history,
            feedback_aggregator,
        }
    }

    pub async fn new_async(
        project_root: PathBuf,
        provider: Arc<dyn LlmProvider>,
        config: Config,
        file_registry: VerifiedFileRegistry,
    ) -> Result<Self> {
        let strategy_rotator = StrategyRotator::with_strategies(
            Arc::clone(&provider),
            &config.refinement().enabled_strategies,
        );
        let target_quality = config.quality().target_score;
        let feedback_aggregator = FeedbackAggregator::new(target_quality);

        let learning_history = if LearningHistory::has_persisted_data(&project_root) {
            match LearningHistory::load(&project_root, crate::config::LearningConfig::default()).await {
                Ok(loaded) => {
                    let pattern_count = loaded.pattern_count();
                    let failing_count = loaded.get_failing_patterns().len();
                    if pattern_count > 0 || failing_count > 0 {
                        tracing::info!(
                            patterns = pattern_count,
                            failing_patterns = failing_count,
                            "Loaded learning patterns from previous session"
                        );
                    }
                    loaded
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load learning patterns, starting fresh");
                    LearningHistory::new()
                }
            }
        } else {
            LearningHistory::new()
        };

        Ok(Self {
            project_root,
            provider,
            config,
            file_registry: Some(file_registry),
            strategy_rotator,
            learning_history,
            feedback_aggregator,
        })
    }

    pub async fn refine(
        &mut self,
        skills: Vec<Skill>,
        agents: Vec<Agent>,
        rules: Vec<Rule>,
        claude_md: &crate::types::ProjectMemory,
        output_plan: &OutputPlan,
    ) -> Result<RefinementResult> {
        let timeout = Duration::from_secs(self.config.refinement().timeout_secs);

        with_timeout(
            timeout,
            self.refine_inner(skills, agents, rules, claude_md, output_plan),
            "refinement_loop",
        )
        .await
    }

    async fn refine_inner(
        &mut self,
        mut skills: Vec<Skill>,
        mut agents: Vec<Agent>,
        mut rules: Vec<Rule>,
        claude_md: &crate::types::ProjectMemory,
        output_plan: &OutputPlan,
    ) -> Result<RefinementResult> {
        let cfg = RefinementConfig::from_config(&self.config);
        let validators = Validators::new(&self.config, &self.provider, &self.project_root);
        let file_registry = match &self.file_registry {
            Some(r) => r.clone(),
            None => VerifiedFileRegistry::build(&self.project_root).await?,
        };
        let project_context = format!("Project root: {}", self.project_root.display());
        let claude_md_content = claude_md.to_markdown();
        let mut state = IterationState::new();

        for iteration in 0..cfg.max_iterations {
            state.decay_strategy_failures();

            // Phase 1: Tier filtering
            let tier_result = tier_filter::filter(&skills, &agents, &rules, &claude_md_content);
            let tier1_violations = tier_result.tier1_violations;
            skills = tier_result.filtered_content.skills;
            agents = tier_result.filtered_content.agents;
            rules = tier_result.filtered_content.rules;

            // Phase 2: Run all validations
            let cv_result = self.assess_quality(&skills, &agents, &rules, claude_md, output_plan);
            let semantic_result = validators
                .run_semantic(&skills, &agents, &rules, claude_md, &project_context)
                .await?;
            let structural_result = validators
                .run_structural(&file_registry, &skills, &agents, &rules, claude_md)
                .await?;
            if structural_result.is_some() {
                state.last_structural_result = structural_result.clone();
            }
            let cross_artifact_result = validators.run_cross_artifact(&skills, &agents, &rules, claude_md);
            let usability_result = validators.run_usability(&skills, &agents, &rules, claude_md);

            // Phase 3: Aggregate feedback
            let aggregated_feedback = self.feedback_aggregator.aggregate(
                &semantic_result,
                structural_result.as_ref(),
                cross_artifact_result.as_ref(),
                usability_result.as_ref(),
                Some(&cv_result),
            );

            let surface_quality = cv_result.quality_score;
            let semantic_quality = semantic_result.overall_score;
            let combined_quality = aggregated_feedback.overall_score;

            state.record_quality(combined_quality);

            // Phase 4: Value-based termination check
            let tier3_count = count_tier3_value(&skills, &agents, &rules, claude_md);
            state.record_tier3(tier3_count);

            const VALUE_PLATEAU_WINDOW: usize = 3;
            if state.tier3_trajectory.len() >= VALUE_PLATEAU_WINDOW
                && combined_quality >= cfg.min_quality_for_value_exit
            {
                let recent: Vec<usize> = state.tier3_trajectory.iter().rev().take(VALUE_PLATEAU_WINDOW).copied().collect();
                let max_recent = recent.iter().max().copied().unwrap_or(0);
                let min_recent = recent.iter().min().copied().unwrap_or(0);
                if max_recent == min_recent && tier3_count > 0 {
                    tracing::info!(
                        iteration = iteration + 1,
                        tier3_count,
                        quality = format!("{:.1}%", combined_quality * 100.0),
                        "Value-based early exit: Tier3 content plateaued"
                    );

                    let report = self.build_success_report(
                        ConvergencePath::ValuePlateau,
                        iteration + 1,
                        &state,
                        &semantic_result,
                        structural_result.as_ref(),
                        cross_artifact_result.as_ref(),
                        usability_result.as_ref(),
                    );
                    self.persist_learning().await;

                    return Ok(RefinementResult {
                        skills: skills.clone(),
                        agents: agents.clone(),
                        rules: rules.clone(),
                        iterations: iteration + 1,
                        converged: true,
                        final_quality: combined_quality,
                        semantic_quality: Some(semantic_result),
                        structural_quality: structural_result,
                        aggregated_feedback: Some(aggregated_feedback),
                        learning_summary: Some(self.learning_history.get_progress_summary()),
                        convergence_report: Some(report),
                    });
                }
            }

            // Phase 5: Rollback logic
            if cfg.enable_rollback {
                const SNAPSHOT_MIN_IMPROVEMENT: f32 = 0.02;
                let should_save = match &state.best_state {
                    None => true,
                    Some(s) => combined_quality >= s.quality + SNAPSHOT_MIN_IMPROVEMENT,
                };

                if should_save {
                    state.best_state = Some(RefinementSnapshot {
                        skills: skills.clone(),
                        agents: agents.clone(),
                        rules: rules.clone(),
                        quality: combined_quality,
                        iteration: iteration + 1,
                    });
                }

                if let (Some(best), Some(_)) = (&state.best_state, state.prev_quality) {
                    let degradation = best.quality - combined_quality;
                    if degradation > cfg.rollback_threshold && state.rollback_count < cfg.max_rollbacks {
                        tracing::warn!(
                            iteration = iteration + 1,
                            current = format!("{:.1}%", combined_quality * 100.0),
                            best = format!("{:.1}%", best.quality * 100.0),
                            degradation = format!("{:.1}%", degradation * 100.0),
                            "Quality degraded significantly, rolling back to iteration {}",
                            best.iteration
                        );

                        skills = best.skills.clone();
                        agents = best.agents.clone();
                        rules = best.rules.clone();
                        state.rollback_count += 1;

                        self.strategy_rotator.escalate();
                        state.strategy_failures.clear();
                        continue;
                    }
                }
            }

            // Include structural status in dimension display
            let structural_status = structural_result
                .as_ref()
                .map(|s| if s.passed { "✓" } else { "✗" })
                .unwrap_or("-");

            let cross_artifact_status = cross_artifact_result
                .as_ref()
                .map(|c| if c.passed { "✓" } else { "✗" })
                .unwrap_or("-");

            let usability_status = usability_result
                .as_ref()
                .map(|u| if u.passed { "✓" } else { "✗" })
                .unwrap_or("-");

            let dimension_status = format!(
                "A:{} S:{} E:{} R:{} D:{} M:{} CA:{} U:{}",
                if semantic_result.actionability.passed { "✓" } else { "✗" },
                if semantic_result.specificity.passed { "✓" } else { "✗" },
                if semantic_result.evidence_quality.passed { "✓" } else { "✗" },
                if semantic_result.redundancy.passed { "✓" } else { "✗" },
                if semantic_result.depth.passed { "✓" } else { "✗" },
                structural_status,
                cross_artifact_status,
                usability_status,
            );

            // Log structural coverage info
            if let Some(ref sr) = structural_result {
                tracing::info!(
                    coverage = format!("{:.1}%", sr.coverage_report.coverage * 100.0),
                    core_modules = sr.coverage_report.core_modules,
                    documented = sr.coverage_report.documented_modules,
                    missing = sr.coverage_report.missing_modules.len(),
                    "Structural coverage"
                );
            }

            // Phase 6: Logging
            tracing::info!(
                iteration = iteration + 1,
                surface = format!("{:.1}%", surface_quality * 100.0),
                semantic = format!("{:.1}%", semantic_quality * 100.0),
                combined = format!("{:.1}%", combined_quality * 100.0),
                aggregated = format!("{:.1}%", aggregated_feedback.overall_score * 100.0),
                dimensions = dimension_status,
                target = format!("{:.1}%", cfg.target_quality * 100.0),
                "Quality assessment"
            );

            // Phase 7: Convergence checking
            let dimensions_for_check = build_dimensions_status(
                &semantic_result,
                structural_result.as_ref(),
                cross_artifact_result.as_ref(),
                usability_result.as_ref(),
                &self.config.refinement().dimension_thresholds,
            );

            let issues_estimate = semantic_result.suggestions.len()
                + if semantic_result.passed { 0 } else { 1 };

            let convergence_checker = ConvergenceChecker::new(cfg.target_quality, cfg.require_all_dimensions);
            let convergence_path = convergence_checker.check(
                combined_quality,
                &dimensions_for_check,
                aggregated_feedback.converged,
                issues_estimate,
            );

            let converged = convergence_path.is_some();
            let meets_target = combined_quality >= cfg.target_quality;

            if iteration + 1 >= cfg.min_iterations && converged {
                state.consecutive_convergence_passes += 1;
                state.total_convergence_detections += 1;

                // Oscillation detection with stricter verification
                if state.total_convergence_detections > cfg.max_convergence_detections
                    && state.consecutive_convergence_passes < cfg.post_convergence_passes_required
                {
                    let quality_variance = if state.quality_trajectory.len() >= 3 {
                        let recent: Vec<f32> = state.quality_trajectory
                            .iter()
                            .copied()
                            .skip(state.quality_trajectory.len().saturating_sub(cfg.oscillation_variance_window))
                            .collect();
                        calculate_quality_variance(&recent)
                    } else {
                        1.0
                    };

                    let oscillation_verification_ok = !cfg.post_convergence_verification
                        || state.consecutive_convergence_passes >= cfg.oscillation_strict_passes
                        || (state.consecutive_convergence_passes >= cfg.oscillation_lenient_passes
                            && quality_variance < cfg.oscillation_stability_variance);

                    if oscillation_verification_ok {
                        tracing::warn!(
                            iteration = iteration + 1,
                            total_convergence_detections = state.total_convergence_detections,
                            consecutive_passes = state.consecutive_convergence_passes,
                            quality = format!("{:.1}%", combined_quality * 100.0),
                            "Convergence oscillation detected: accepting with reduced verification"
                        );

                        let (final_skills, final_agents, final_rules, final_quality) =
                            if let Some(ref best) = state.best_state {
                                if best.quality > combined_quality {
                                    tracing::info!(
                                        current = format!("{:.1}%", combined_quality * 100.0),
                                        best = format!("{:.1}%", best.quality * 100.0),
                                        best_iteration = best.iteration,
                                        "Using best state from iteration {}",
                                        best.iteration
                                    );
                                    (best.skills.clone(), best.agents.clone(), best.rules.clone(), best.quality)
                                } else {
                                    (skills, agents, rules, combined_quality)
                                }
                            } else {
                                (skills, agents, rules, combined_quality)
                            };

                        let report = self.build_success_report(
                            ConvergencePath::OscillationSettled,
                            iteration + 1,
                            &state,
                            &semantic_result,
                            structural_result.as_ref(),
                            cross_artifact_result.as_ref(),
                            usability_result.as_ref(),
                        );
                        self.persist_learning().await;

                        return Ok(RefinementResult {
                            skills: final_skills,
                            agents: final_agents,
                            rules: final_rules,
                            iterations: iteration + 1,
                            converged: true,
                            final_quality,
                            semantic_quality: Some(semantic_result),
                            structural_quality: structural_result,
                            aggregated_feedback: Some(aggregated_feedback),
                            learning_summary: Some(self.learning_history.get_progress_summary()),
                            convergence_report: Some(report),
                        });
                    } else {
                        tracing::debug!(
                            iteration = iteration + 1,
                            consecutive_passes = state.consecutive_convergence_passes,
                            "Oscillation detected but waiting for verification pass"
                        );
                    }
                }

                let verification_passed = !cfg.post_convergence_verification
                    || state.consecutive_convergence_passes >= cfg.post_convergence_passes_required;

                if verification_passed {
                    tracing::info!(
                        iteration = iteration + 1,
                        combined = format!("{:.1}%", combined_quality * 100.0),
                        dimensions = dimension_status,
                        consecutive_passes = state.consecutive_convergence_passes,
                        "Convergence achieved: all quality targets met with verification"
                    );

                    let path = determine_convergence_path(
                        meets_target,
                        dimensions_for_check.all_passed(cfg.require_all_dimensions),
                        cfg.require_all_dimensions,
                        aggregated_feedback.converged,
                        0,
                    );
                    let report = self.build_success_report(
                        path,
                        iteration + 1,
                        &state,
                        &semantic_result,
                        structural_result.as_ref(),
                        cross_artifact_result.as_ref(),
                        usability_result.as_ref(),
                    );
                    self.persist_learning().await;

                    return Ok(RefinementResult {
                        skills,
                        agents,
                        rules,
                        iterations: iteration + 1,
                        converged: true,
                        final_quality: combined_quality,
                        semantic_quality: Some(semantic_result),
                        structural_quality: structural_result,
                        aggregated_feedback: Some(aggregated_feedback),
                        learning_summary: Some(self.learning_history.get_progress_summary()),
                        convergence_report: Some(report),
                    });
                } else {
                    tracing::debug!(
                        iteration = iteration + 1,
                        consecutive_passes = state.consecutive_convergence_passes,
                        required = cfg.post_convergence_passes_required,
                        "Convergence detected, continuing verification loop"
                    );
                }
            } else {
                state.consecutive_convergence_passes = 0;
            }

            // Phase 8: Oscillation and stagnation detection state machine
            let refinement_cfg = self.config.refinement();
            if let Some(prev) = state.prev_quality {
                let delta = (combined_quality - prev).abs();
                let improved = combined_quality > prev + refinement_cfg.min_improvement_per_iteration;

                let is_oscillating = refinement_cfg.detect_oscillation
                    && state.quality_trajectory.len() >= refinement_cfg.oscillation_window
                    && {
                        let window: Vec<f32> = state.quality_trajectory
                            .iter()
                            .copied()
                            .skip(state.quality_trajectory.len().saturating_sub(refinement_cfg.oscillation_window))
                            .collect();
                        detect_oscillation(&window, refinement_cfg.oscillation_min_amplitude)
                    };

                let is_stagnating = delta < cfg.stagnation_threshold || !improved;

                match (is_oscillating, is_stagnating) {
                    (true, true) => {
                        tracing::warn!(
                            iteration = iteration + 1,
                            "Oscillation + Stagnation detected: forcing full regeneration"
                        );
                        self.strategy_rotator.force_regeneration();
                        state.strategy_failures.clear();
                        state.stagnation_count = 0;
                    }
                    (true, false) => {
                        tracing::warn!(
                            iteration = iteration + 1,
                            window_size = refinement_cfg.oscillation_window,
                            "Oscillation detected: quality bouncing, escalating strategy"
                        );
                        self.strategy_rotator.escalate();
                        state.strategy_failures.clear();
                        state.stagnation_count = 0;
                    }
                    (false, true) => {
                        state.stagnation_count += 1;
                        tracing::debug!(
                            delta = format!("{:.3}", delta),
                            stagnation_count = state.stagnation_count,
                            patience = cfg.stagnation_patience,
                            "Quality improvement stalled"
                        );

                        if state.stagnation_count >= cfg.stagnation_patience {
                            tracing::info!(
                                iteration = iteration + 1,
                                "Stagnation patience exhausted: escalating to stronger strategies"
                            );
                            self.strategy_rotator.escalate();
                            state.strategy_failures.clear();
                            state.stagnation_count = 0;
                        }
                    }
                    (false, false) => {
                        state.stagnation_count = 0;
                    }
                }
            }
            state.prev_quality = Some(combined_quality);
            state.last_semantic_result = Some(semantic_result.clone());

            let mut issues = self.identify_issues_with_semantic(
                &cv_result,
                &tier1_violations,
                &skills,
                &agents,
                &semantic_result,
            );

            // Add structural issues (missing modules, partial coverage)
            self.add_structural_issues(&mut issues, &structural_result);

            // Re-sort issues after adding structural ones
            issues.sort_by(|a, b| b.severity.cmp(&a.severity));

            // Phase 9: Check for no issues remaining
            if issues.is_empty() && iteration >= cfg.min_iterations {
                let quality_acceptable = meets_target
                    || (!cfg.require_all_dimensions && combined_quality >= cfg.target_quality * 0.9);

                if (converged || !cfg.require_all_dimensions) && quality_acceptable {
                    let no_issues_verification_passed = !cfg.post_convergence_verification
                        || state.consecutive_convergence_passes >= cfg.post_convergence_passes_required;

                    if no_issues_verification_passed {
                        tracing::info!(
                            iteration = iteration + 1,
                            quality = format!("{:.1}%", combined_quality * 100.0),
                            consecutive_passes = state.consecutive_convergence_passes,
                            "No issues found, refinement complete with verification"
                        );

                        let report = self.build_success_report(
                            ConvergencePath::NoIssuesRemaining,
                            iteration + 1,
                            &state,
                            &semantic_result,
                            structural_result.as_ref(),
                            cross_artifact_result.as_ref(),
                            usability_result.as_ref(),
                        );
                        self.persist_learning().await;

                        return Ok(RefinementResult {
                            skills,
                            agents,
                            rules,
                            iterations: iteration + 1,
                            converged: meets_target,
                            final_quality: combined_quality,
                            semantic_quality: Some(semantic_result),
                            structural_quality: structural_result,
                            aggregated_feedback: Some(aggregated_feedback),
                            learning_summary: Some(self.learning_history.get_progress_summary()),
                            convergence_report: Some(report),
                        });
                    } else {
                        tracing::debug!(
                            iteration = iteration + 1,
                            consecutive_passes = state.consecutive_convergence_passes,
                            required = cfg.post_convergence_passes_required,
                            "No issues found but waiting for verification passes"
                        );
                    }
                } else if !quality_acceptable {
                    tracing::debug!(
                        iteration = iteration + 1,
                        quality = format!("{:.1}%", combined_quality * 100.0),
                        target = format!("{:.1}%", cfg.target_quality * 100.0),
                        "No issues found but quality below minimum threshold, continuing"
                    );
                }
            }

            // Phase 10: Apply refinements
            let (new_skills, new_agents, new_rules, iter_improvements) = self
                .apply_refinements_with_strategies(
                    skills,
                    agents,
                    rules,
                    &issues,
                    &semantic_result,
                    &aggregated_feedback,
                    &file_registry,
                    iteration,
                    cfg.issues_per_iteration,
                    cfg.strategy_retry_limit,
                    &mut state.strategy_failures,
                    combined_quality,
                )
                .await?;

            skills = new_skills;
            agents = new_agents;
            rules = new_rules;
            state.critical_improvements.extend(iter_improvements);
        }

        // Final quality assessment after max iterations
        let final_cv = self.assess_quality(&skills, &agents, &rules, claude_md, output_plan);
        let final_semantic = validators
            .run_semantic(&skills, &agents, &rules, claude_md, &project_context)
            .await?;

        let final_aggregated = self.feedback_aggregator.aggregate(
            &final_semantic,
            state.last_structural_result.as_ref(),
            None,
            None,
            Some(&final_cv),
        );
        let final_quality = final_aggregated.overall_score;

        // Recover best_state if it's better than the final state
        let (output_skills, output_agents, output_rules, output_quality) =
            if let Some(ref best) = state.best_state {
                if best.quality > final_quality {
                    tracing::info!(
                        final_quality = format!("{:.1}%", final_quality * 100.0),
                        best_quality = format!("{:.1}%", best.quality * 100.0),
                        best_iteration = best.iteration,
                        "Max iterations reached: using best state from iteration {}",
                        best.iteration
                    );
                    (best.skills.clone(), best.agents.clone(), best.rules.clone(), best.quality)
                } else {
                    (skills, agents, rules, final_quality)
                }
            } else {
                (skills, agents, rules, final_quality)
            };

        tracing::warn!(
            iterations = cfg.max_iterations,
            quality = format!("{:.1}%", output_quality * 100.0),
            semantic = format!("{:.1}%", final_semantic.overall_score * 100.0),
            dimensions = format!(
                "A:{} S:{} E:{} R:{} D:{}",
                if final_semantic.actionability.passed { "✓" } else { "✗" },
                if final_semantic.specificity.passed { "✓" } else { "✗" },
                if final_semantic.evidence_quality.passed { "✓" } else { "✗" },
                if final_semantic.redundancy.passed { "✓" } else { "✗" },
                if final_semantic.depth.passed { "✓" } else { "✗" },
            ),
            "Max iterations reached"
        );

        let dimensions_status = build_dimensions_status(
            &final_semantic,
            state.last_structural_result.as_ref(),
            None,
            None,
            &self.config.refinement().dimension_thresholds,
        );

        let remaining_issues: Vec<super::convergence::RemainingIssue> = final_semantic
            .issues
            .iter()
            .map(|issue| super::convergence::RemainingIssue {
                target: issue.target.clone(),
                category: format!("{:?}", issue.category),
                severity: format!("{:?}", issue.severity),
                description: issue.description.clone(),
                attempts: cfg.max_iterations,
            })
            .collect();

        let report = ConvergenceReport::failure(
            cfg.max_iterations,
            Vec::from(state.quality_trajectory.clone()),
            dimensions_status,
            remaining_issues,
        );

        self.persist_learning().await;

        Ok(RefinementResult {
            skills: output_skills,
            agents: output_agents,
            rules: output_rules,
            iterations: cfg.max_iterations,
            converged: false,
            final_quality: output_quality,
            semantic_quality: state.last_semantic_result.or(Some(final_semantic)),
            structural_quality: state.last_structural_result,
            aggregated_feedback: None,
            learning_summary: Some(self.learning_history.get_progress_summary()),
            convergence_report: Some(report),
        })
    }

    fn build_success_report(
        &self,
        path: ConvergencePath,
        iteration: usize,
        state: &IterationState,
        semantic_result: &SemanticQualityResult,
        structural_result: Option<&StructuralValidationResult>,
        cross_artifact_result: Option<&CrossArtifactResult>,
        usability_result: Option<&UsabilityResult>,
    ) -> ConvergenceReport {
        let dimensions_status = build_dimensions_status(
            semantic_result,
            structural_result,
            cross_artifact_result,
            usability_result,
            &self.config.refinement().dimension_thresholds,
        );
        ConvergenceReport::success(
            path,
            iteration,
            Vec::from(state.quality_trajectory.clone()),
            dimensions_status,
            state.critical_improvements.clone(),
        )
    }

    async fn persist_learning(&self) {
        if let Err(e) = self.learning_history.persist(&self.project_root).await {
            tracing::error!(error = %e, "Failed to persist learning patterns - data may be lost");
        }
    }

    fn assess_quality(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &crate::types::ProjectMemory,
        output_plan: &OutputPlan,
    ) -> CrossValidationResult {
        CrossValidator::new(
            self.config.cross_validation(),
            self.config.quality(),
            &self.project_root,
        )
        .validate(output_plan, skills, agents, rules, claude_md)
    }

    fn identify_issues_with_semantic(
        &self,
        cv_result: &CrossValidationResult,
        tier1_violations: &[Tier1Violation],
        skills: &[Skill],
        agents: &[Agent],
        semantic_result: &SemanticQualityResult,
    ) -> Vec<RefinementIssue> {
        let mut issues = Vec::new();
        let quality_cfg = self.config.quality();
        let semantic_cfg = self.config.semantic_validation();

        for violation in tier1_violations {
            issues.push(RefinementIssue {
                item_type: violation.item_type,
                item_name: violation.item_name.clone(),
                issue: IssueKind::Tier1Content {
                    violation: violation.violation.clone(),
                },
                severity: IssueSeverity::Warning,
            });
        }

        for skill in skills {
            issues.extend(self.check_skill_quality(skill, &quality_cfg.skill));
        }

        for agent in agents {
            issues.extend(self.check_agent_quality(agent, &quality_cfg.agent));
        }

        for missing in &cv_result.plan_consistency.missing_items {
            let (item_type, name) = parse_missing_item(missing);
            issues.push(RefinementIssue {
                item_type,
                item_name: name,
                issue: IssueKind::PlanMismatch,
                severity: IssueSeverity::Error,
            });
        }

        for semantic_issue in &semantic_result.issues {
            let (item_type, item_name) = parse_semantic_target(&semantic_issue.target);
            let severity = match semantic_issue.severity {
                SemanticSeverity::Critical => IssueSeverity::Error,
                SemanticSeverity::High => IssueSeverity::Error,
                SemanticSeverity::Medium => IssueSeverity::Warning,
                SemanticSeverity::Low => IssueSeverity::Info,
            };

            let issue_kind = match semantic_issue.category {
                SemanticCategory::LowActionability => IssueKind::LowActionability {
                    score: semantic_result.actionability.score,
                    threshold: semantic_cfg.min_actionability,
                },
                SemanticCategory::TooGeneric => IssueKind::TooGeneric {
                    description: semantic_issue.description.clone(),
                },
                SemanticCategory::WeakEvidence => IssueKind::WeakEvidence {
                    description: semantic_issue.description.clone(),
                },
                SemanticCategory::Redundant => IssueKind::Redundant {
                    description: semantic_issue.description.clone(),
                },
                SemanticCategory::Shallow => IssueKind::Shallow {
                    description: semantic_issue.description.clone(),
                },
                SemanticCategory::MissingReference => IssueKind::MissingReferences {
                    expected: semantic_cfg.min_actionable_items,
                    actual: 0,
                },
            };

            issues.push(RefinementIssue {
                item_type,
                item_name,
                issue: issue_kind,
                severity,
            });
        }

        if !semantic_result.actionability.passed {
            issues.push(RefinementIssue {
                item_type: ItemType::ClaudeMd,
                item_name: "CLAUDE.md".to_string(),
                issue: IssueKind::LowActionability {
                    score: semantic_result.actionability.score,
                    threshold: semantic_cfg.min_actionability,
                },
                severity: IssueSeverity::Warning,
            });
        }

        if !semantic_result.specificity.passed && semantic_cfg.reject_generic_content {
            issues.push(RefinementIssue {
                item_type: ItemType::ClaudeMd,
                item_name: "All content".to_string(),
                issue: IssueKind::TooGeneric {
                    description: semantic_result.specificity.details.clone(),
                },
                severity: IssueSeverity::Warning,
            });
        }

        issues.sort_by(|a, b| b.severity.cmp(&a.severity));
        issues
    }

    fn add_structural_issues(
        &self,
        issues: &mut Vec<RefinementIssue>,
        structural_result: &Option<StructuralValidationResult>,
    ) {
        let Some(structural) = structural_result else {
            return;
        };

        // Add issues for missing modules
        for missing in &structural.coverage_report.missing_modules {
            issues.push(RefinementIssue {
                item_type: ItemType::ClaudeMd,
                item_name: format!("module:{}", missing.module.name),
                issue: IssueKind::MissingModule {
                    module_name: missing.module.name.clone(),
                    file_count: missing.module.file_count,
                    key_files: missing.module.key_files.clone(),
                },
                severity: IssueSeverity::Error,
            });
        }

        // Add issues for partially covered modules
        for partial in &structural.coverage_report.partially_covered {
            if partial.coverage_score < 0.5 {
                issues.push(RefinementIssue {
                    item_type: ItemType::ClaudeMd,
                    item_name: format!("module:{}", partial.module.name),
                    issue: IssueKind::PartialModuleCoverage {
                        module_name: partial.module.name.clone(),
                        coverage: partial.coverage_score,
                    },
                    severity: IssueSeverity::Warning,
                });
            }
        }
    }

    fn check_skill_quality(
        &self,
        skill: &Skill,
        cfg: &crate::config::SkillQualityConfig,
    ) -> Vec<RefinementIssue> {
        let mut issues = Vec::new();

        if skill.body.len() < cfg.min_chars {
            issues.push(RefinementIssue {
                item_type: ItemType::Skill,
                item_name: skill.name.clone(),
                issue: IssueKind::TooShort {
                    actual: skill.body.len(),
                    min: cfg.min_chars,
                },
                severity: IssueSeverity::Warning,
            });
        }

        let ref_count = count_file_references(&skill.body);
        if ref_count < cfg.target_file_refs {
            issues.push(RefinementIssue {
                item_type: ItemType::Skill,
                item_name: skill.name.clone(),
                issue: IssueKind::MissingReferences {
                    expected: cfg.target_file_refs,
                    actual: ref_count,
                },
                severity: IssueSeverity::Info,
            });
        }

        issues
    }

    fn check_agent_quality(
        &self,
        agent: &Agent,
        cfg: &crate::config::AgentQualityConfig,
    ) -> Vec<RefinementIssue> {
        let mut issues = Vec::new();

        if agent.prompt.len() < cfg.min_chars {
            issues.push(RefinementIssue {
                item_type: ItemType::Agent,
                item_name: agent.name.clone(),
                issue: IssueKind::TooShort {
                    actual: agent.prompt.len(),
                    min: cfg.min_chars,
                },
                severity: IssueSeverity::Warning,
            });
        }

        let section_count = agent.prompt.matches("##").count();
        if section_count < cfg.min_sections {
            issues.push(RefinementIssue {
                item_type: ItemType::Agent,
                item_name: agent.name.clone(),
                issue: IssueKind::MissingSections {
                    expected: cfg.min_sections,
                    actual: section_count,
                },
                severity: IssueSeverity::Warning,
            });
        }

        issues
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_refinements_with_strategies(
        &mut self,
        mut skills: Vec<Skill>,
        mut agents: Vec<Agent>,
        mut rules: Vec<Rule>,
        issues: &[RefinementIssue],
        semantic_result: &SemanticQualityResult,
        aggregated_feedback: &AggregatedFeedback,
        file_registry: &VerifiedFileRegistry,
        iteration: usize,
        issues_per_iteration: usize,
        strategy_retry_limit: usize,
        strategy_failures: &mut std::collections::HashMap<String, usize>,
        combined_quality: f32, // For learning-based strategy recommendation
    ) -> Result<(Vec<Skill>, Vec<Agent>, Vec<Rule>, Vec<super::convergence::Improvement>)> {
        let mut improvements = Vec::new();
        let error_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .collect();

        for issue in error_issues {
            if matches!(issue.issue, IssueKind::PlanMismatch) {
                match issue.item_type {
                    ItemType::Skill => {
                        if let Some(skill) = self.regenerate_skill(&issue.item_name, file_registry).await {
                            skills.push(skill);
                        }
                    }
                    ItemType::Agent => {
                        if let Some(agent) = self.regenerate_agent(&issue.item_name, file_registry).await {
                            agents.push(agent);
                        }
                    }
                    _ => {}
                }
            }
        }

        let strategy_issues: Vec<_> = issues
            .iter()
            .filter(|i| {
                matches!(
                    i.issue,
                    IssueKind::LowActionability { .. }
                        | IssueKind::TooGeneric { .. }
                        | IssueKind::WeakEvidence { .. }
                        | IssueKind::Shallow { .. }
                        | IssueKind::MissingReferences { .. }
                        | IssueKind::TooShort { .. }
                        | IssueKind::MissingSections { .. }
                        | IssueKind::MissingModule { .. }
                        | IssueKind::PartialModuleCoverage { .. }
                )
            })
            .take(issues_per_iteration)
            .collect();

        for issue in &strategy_issues {
            let item_key = format!("{}:{}", issue.item_type_str(), issue.item_name);
            let failures = strategy_failures.get(&item_key).copied().unwrap_or(0);

            if failures >= strategy_retry_limit {
                tracing::debug!(
                    item = item_key,
                    failures,
                    limit = strategy_retry_limit,
                    "Skipping item due to repeated strategy failures"
                );
                continue;
            }

            let strategy_issue_kind = StrategyIssueKind::from_refinement_issue(&issue.issue);

            // Try learning-based strategy selection first
            let strategy = self.learning_history
                .recommend_strategy(&strategy_issue_kind, &issue.item_name, combined_quality)
                .and_then(|name| {
                    tracing::debug!(
                        item = issue.item_name,
                        recommended_strategy = %name,
                        "Using learning-recommended strategy"
                    );
                    self.strategy_rotator.get_strategy_by_name(&name)
                })
                .unwrap_or_else(|| {
                    self.strategy_rotator.select_strategy(&issue.item_name, &strategy_issue_kind)
                });

            // Build suggestions with key_files from MissingModule issues
            let mut suggestions = semantic_result.suggestions.clone();
            if let IssueKind::MissingModule { key_files, .. } = &issue.issue {
                for file in key_files {
                    suggestions.push(format!("Key file: {}", file));
                }
            }

            // Extract validation feedback from aggregated feedback for targeted refinement
            let validation_feedback = Some(super::strategy::ValidationFeedback {
                missing_modules: aggregated_feedback.analysis_feedback.missing_modules.clone(),
                weak_coverage_areas: aggregated_feedback.analysis_feedback.weak_coverage_areas.clone(),
                module_constraints: aggregated_feedback.analysis_feedback.refinement_hints.clone(),
            });

            let context = StrategyContext {
                file_registry,
                issue_description: format_issue_description(&issue.issue),
                suggestions,
                validation_feedback,
            };

            // Track quality before/after for learning history
            let (result, improved, quality_before, quality_after) = match issue.item_type {
                ItemType::Skill => {
                    if let Some(skill) = skills.iter_mut().find(|s| s.name == issue.item_name) {
                        let before = super::strategy::calculate_quick_quality(&skill.body);
                        let result = strategy.refine_skill(skill, &context).await?;
                        let after = super::strategy::calculate_quick_quality(&skill.body);
                        (Some(result), after > before, before, after)
                    } else {
                        (None, false, 0.0, 0.0)
                    }
                }
                ItemType::Agent => {
                    if let Some(agent) = agents.iter_mut().find(|a| a.name == issue.item_name) {
                        let before = super::strategy::calculate_quick_quality(&agent.prompt);
                        let result = strategy.refine_agent(agent, &context).await?;
                        let after = super::strategy::calculate_quick_quality(&agent.prompt);
                        (Some(result), after > before, before, after)
                    } else {
                        (None, false, 0.0, 0.0)
                    }
                }
                ItemType::ClaudeMd => {
                    if let Some(skill) = skills.first_mut() {
                        let before = super::strategy::calculate_quick_quality(&skill.body);
                        let result = strategy.refine_skill(skill, &context).await?;
                        let after = super::strategy::calculate_quick_quality(&skill.body);
                        (Some(result), after > before, before, after)
                    } else if let Some(agent) = agents.first_mut() {
                        let before = super::strategy::calculate_quick_quality(&agent.prompt);
                        let result = strategy.refine_agent(agent, &context).await?;
                        let after = super::strategy::calculate_quick_quality(&agent.prompt);
                        (Some(result), after > before, before, after)
                    } else {
                        (None, false, 0.0, 0.0)
                    }
                }
                ItemType::Rule => {
                    if let Some(rule) = rules.iter_mut().find(|r| r.name == issue.item_name) {
                        let before = super::strategy::calculate_quick_quality(&rule.to_markdown());
                        let result = strategy.refine_rule(rule, &context).await?;
                        let after = super::strategy::calculate_quick_quality(&rule.to_markdown());
                        (Some(result), after > before, before, after)
                    } else {
                        (None, false, 0.0, 0.0)
                    }
                }
            };

            if let Some(result) = result {
                let success = result.success && improved;

                // Record to strategy rotator for local item/issue tracking
                self.strategy_rotator.record_outcome(
                    &issue.item_name,
                    &strategy_issue_kind,
                    StrategyOutcome {
                        strategy_name: strategy.name().to_string(),
                        success,
                        quality_delta: result.quality_delta,
                        iteration,
                    },
                );

                // Record to learning history for cross-session pattern learning
                self.learning_history.record_outcome(LearningOutcome {
                    strategy_name: strategy.name().to_string(),
                    issue_kind: format!("{:?}", strategy_issue_kind),
                    item_name: issue.item_name.clone(),
                    quality_before,
                    quality_after,
                    success,
                    iteration,
                });

                if success {
                    strategy_failures.remove(&item_key);
                    for change in &result.changes_made {
                        tracing::debug!(change, "Strategy applied");
                    }

                    // Track significant improvements
                    if result.quality_delta > 0.05 {
                        improvements.push(super::convergence::Improvement {
                            iteration: iteration + 1,
                            target: issue.item_name.clone(),
                            strategy: strategy.name().to_string(),
                            quality_delta: result.quality_delta,
                            description: result.changes_made.join("; "),
                        });
                    }
                } else {
                    *strategy_failures.entry(item_key.clone()).or_insert(0) += 1;
                }
            }
        }

        Ok((skills, agents, rules, improvements))
    }

    async fn regenerate_skill(&self, name: &str, file_registry: &VerifiedFileRegistry) -> Option<Skill> {
        let file_context = file_registry.to_prompt_context(50);

        let prompt = format!(
            r###"Generate a skill for: {name}

{file_context}

Requirements:
- Minimum 300 characters
- Include specific @file:line references from the available files
- Provide step-by-step instructions
- Include gotchas or common mistakes

Return JSON: {{"name": "...", "description": "...", "body": "..."}}"###,
            name = name,
            file_context = file_context,
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "description": {"type": "string"},
                "body": {"type": "string"}
            },
            "required": ["name", "description", "body"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let skill_name = response
                    .content
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(name);
                let desc = response
                    .content
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let body = response
                    .content
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if body.len() >= 100 {
                    Some(Skill::new(skill_name, desc, body.to_string()).with_user_invocable(true))
                } else {
                    tracing::warn!(skill = name, body_len = body.len(), "Generated skill too short");
                    None
                }
            }
            Err(e) => {
                tracing::error!(skill = name, error = %e, "Failed to regenerate skill");
                None
            }
        }
    }

    async fn regenerate_agent(&self, name: &str, file_registry: &VerifiedFileRegistry) -> Option<Agent> {
        let file_context = file_registry.to_prompt_context(50);

        let prompt = format!(
            r###"Generate an agent prompt for: {name}

{file_context}

Requirements:
- Clear description of the agent's purpose
- At least 2 markdown sections (##)
- Include @file references from the available files
- Specific responsibilities

Return JSON: {{"name": "...", "description": "...", "prompt": "..."}}"###,
            name = name,
            file_context = file_context,
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "description": {"type": "string"},
                "prompt": {"type": "string"}
            },
            "required": ["name", "description", "prompt"]
        });

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                let agent_name = response
                    .content
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(name);
                let description = response
                    .content
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or(name);
                let prompt_text = response
                    .content
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if prompt_text.len() >= 100 {
                    Some(Agent::new(agent_name, description, prompt_text.to_string()))
                } else {
                    tracing::warn!(agent = name, prompt_len = prompt_text.len(), "Generated agent too short");
                    None
                }
            }
            Err(e) => {
                tracing::error!(agent = name, error = %e, "Failed to regenerate agent");
                None
            }
        }
    }
}

fn format_issue_description(issue: &IssueKind) -> String {
    match issue {
        IssueKind::LowActionability { score, threshold } => format!(
            "LOW ACTIONABILITY ({:.0}% vs {:.0}% target)",
            score * 100.0,
            threshold * 100.0
        ),
        IssueKind::TooGeneric { description } => format!("TOO GENERIC: {}", description),
        IssueKind::WeakEvidence { description } => format!("WEAK EVIDENCE: {}", description),
        IssueKind::Shallow { description } => format!("SHALLOW: {}", description),
        IssueKind::MissingReferences { expected, actual } => {
            format!("MISSING REFERENCES: {} of {} required", actual, expected)
        }
        IssueKind::TooShort { actual, min } => {
            format!("TOO SHORT: {} chars (min: {})", actual, min)
        }
        IssueKind::MissingSections { expected, actual } => {
            format!("MISSING SECTIONS: {} of {} required", actual, expected)
        }
        IssueKind::Redundant { description } => format!("REDUNDANT: {}", description),
        IssueKind::Tier1Content { violation } => format!("TIER1 CONTENT: {}", violation),
        IssueKind::PlanMismatch => "PLAN MISMATCH: Item missing from output".to_string(),
        IssueKind::MissingModule { module_name, file_count, key_files } => {
            format!(
                "MISSING MODULE: '{}' ({} files) - key files: {}",
                module_name,
                file_count,
                key_files.join(", ")
            )
        }
        IssueKind::PartialModuleCoverage { module_name, coverage } => {
            format!(
                "PARTIAL MODULE COVERAGE: '{}' at {:.0}%",
                module_name,
                coverage * 100.0
            )
        }
    }
}

fn count_file_references(content: &str) -> usize {
    patterns::count_file_refs(content)
}

fn parse_missing_item(missing: &str) -> (ItemType, String) {
    if let Some(name) = missing.strip_prefix("Skill: ") {
        (ItemType::Skill, name.to_string())
    } else if let Some(name) = missing.strip_prefix("Agent: ") {
        (ItemType::Agent, name.to_string())
    } else if let Some(name) = missing.strip_prefix("Rule: ") {
        (ItemType::Rule, name.to_string())
    } else {
        (ItemType::Rule, missing.to_string())
    }
}

fn parse_semantic_target(target: &str) -> (ItemType, String) {
    if let Some(name) = target.strip_prefix("Skill:") {
        (ItemType::Skill, name.to_string())
    } else if let Some(name) = target.strip_prefix("Agent:") {
        (ItemType::Agent, name.to_string())
    } else if let Some(name) = target.strip_prefix("Rule:") {
        (ItemType::Rule, name.to_string())
    } else {
        (ItemType::ClaudeMd, target.to_string())
    }
}

fn build_dimensions_status(
    semantic: &SemanticQualityResult,
    structural: Option<&StructuralValidationResult>,
    cross_artifact: Option<&super::validation::cross_artifact::CrossArtifactResult>,
    usability: Option<&super::validation::usability::UsabilityResult>,
    thresholds: &crate::config::DimensionThresholds,
) -> super::convergence::DimensionsStatus {
    use super::convergence::{DimensionScore, DimensionsStatus};

    // Use configurable thresholds instead of hardcoded constants
    // Note: actionability/specificity/depth are based on semantic quality config
    // We map them to the dimension thresholds for consistency
    let semantic_threshold = thresholds.semantic;

    DimensionsStatus {
        actionability: DimensionScore::new(
            "actionability",
            semantic.actionability.score,
            semantic_threshold,
        ),
        specificity: DimensionScore::new(
            "specificity",
            semantic.specificity.score,
            semantic_threshold * 0.8, // Slightly lower threshold for specificity
        ),
        evidence_quality: DimensionScore::new(
            "evidence_quality",
            semantic.evidence_quality.score,
            thresholds.evidence,
        ),
        depth: DimensionScore::new("depth", semantic.depth.score, semantic_threshold * 0.8),
        redundancy: DimensionScore::new_inverted(
            "redundancy",
            semantic.redundancy.score,
            0.3, // Max redundancy allowed (inverted scale)
        ),
        structural_coverage: structural.map(|s| {
            DimensionScore::new(
                "structural_coverage",
                s.coverage_report.coverage,
                thresholds.surface,
            )
        }),
        cross_artifact: cross_artifact.map(|c| {
            DimensionScore::new("cross_artifact", c.score, thresholds.cross_artifact)
        }),
        usability: usability.map(|u| {
            DimensionScore::new("usability", u.score, thresholds.usability)
        }),
    }
}

fn determine_convergence_path(
    meets_target: bool,
    all_dimensions_passed: bool,
    require_all_dimensions: bool,
    aggregated_converged: bool,
    issues_remaining: usize,
) -> super::convergence::ConvergencePath {
    use super::convergence::ConvergencePath;

    if issues_remaining == 0 {
        ConvergencePath::NoIssuesRemaining
    } else if require_all_dimensions && meets_target && all_dimensions_passed {
        ConvergencePath::AllDimensionsPassed
    } else if meets_target {
        ConvergencePath::QualityTargetMet
    } else if aggregated_converged {
        ConvergencePath::AggregatedFeedback
    } else {
        ConvergencePath::MaxIterations
    }
}

/// Calculates variance of quality values in a window.
/// Used to determine if quality is stable (low variance) or unstable (high variance).
fn calculate_quality_variance(window: &[f32]) -> f32 {
    if window.is_empty() {
        return 0.0;
    }
    let mean = window.iter().sum::<f32>() / window.len() as f32;
    let variance = window.iter()
        .map(|&x| (x - mean).powi(2))
        .sum::<f32>() / window.len() as f32;
    variance.sqrt() // Return standard deviation as "variance" measure
}

/// Counts Tier3 (high-value) indicators across all artifacts.
/// Tier3 content includes project-specific constraints, hidden gotchas, and domain-specific rules.
fn count_tier3_value(
    skills: &[Skill],
    agents: &[Agent],
    rules: &[Rule],
    claude_md: &crate::types::ProjectMemory,
) -> usize {
    let mut count = 0;

    // Count Tier3 indicators in skills
    for skill in skills {
        count += patterns::count_tier3_indicators(&skill.body);
    }

    // Count Tier3 indicators in agents
    for agent in agents {
        count += patterns::count_tier3_indicators(&agent.prompt);
    }

    // Count Tier3 indicators in rules
    for rule in rules {
        let content = rule.content.join("\n");
        count += patterns::count_tier3_indicators(&content);
    }

    // Count Tier3 indicators in CLAUDE.md content
    let claude_md_content = claude_md.to_markdown();
    count += patterns::count_tier3_indicators(&claude_md_content);

    count
}

/// Detects oscillation pattern in quality trajectory.
/// Returns true if quality is bouncing back and forth without net improvement.
fn detect_oscillation(window: &[f32], min_amplitude: f32) -> bool {
    if window.len() < 3 {
        return false;
    }

    // Count direction changes with significant amplitude
    let mut direction_changes = 0;
    let mut prev_direction: Option<bool> = None;

    for i in 1..window.len() {
        let delta = (window[i] - window[i - 1]).abs();

        // Ignore tiny fluctuations below min_amplitude
        if delta < min_amplitude {
            continue;
        }

        let current_direction = window[i] > window[i - 1];
        if let Some(prev) = prev_direction
            && prev != current_direction {
                direction_changes += 1;
            }
        prev_direction = Some(current_direction);
    }

    // If direction changes >= 50% of possible changes, it's oscillating
    let max_changes = window.len().saturating_sub(2);
    if max_changes == 0 {
        return false;
    }

    direction_changes as f32 / max_changes as f32 >= 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_severity_ordering() {
        assert!(IssueSeverity::Error > IssueSeverity::Warning);
        assert!(IssueSeverity::Warning > IssueSeverity::Info);
    }

    #[test]
    fn test_count_file_references() {
        assert_eq!(count_file_references("See @src/main.rs:10"), 1);
        assert_eq!(count_file_references("@src/lib.rs and @src/main.rs:5"), 2);
        assert_eq!(count_file_references("No references here"), 0);
        assert_eq!(count_file_references("Email: user@example.com"), 0);
    }

    #[test]
    fn test_parse_missing_item() {
        let (t, n) = parse_missing_item("Skill: add-feature");
        assert_eq!(t, ItemType::Skill);
        assert_eq!(n, "add-feature");

        let (t, n) = parse_missing_item("Agent: debugger");
        assert_eq!(t, ItemType::Agent);
        assert_eq!(n, "debugger");

        let (t, n) = parse_missing_item("Rule: naming");
        assert_eq!(t, ItemType::Rule);
        assert_eq!(n, "naming");
    }

    #[test]
    fn test_format_issue_description() {
        let issue = IssueKind::LowActionability {
            score: 0.3,
            threshold: 0.6,
        };
        let desc = format_issue_description(&issue);
        assert!(desc.contains("30%"));
        assert!(desc.contains("60%"));

        let issue = IssueKind::TooShort {
            actual: 100,
            min: 300,
        };
        let desc = format_issue_description(&issue);
        assert!(desc.contains("100"));
        assert!(desc.contains("300"));
    }

    #[test]
    fn test_calculate_quality_variance() {
        // Empty window
        assert_eq!(calculate_quality_variance(&[]), 0.0);

        // Single value - no variance
        assert_eq!(calculate_quality_variance(&[0.5]), 0.0);

        // Stable values - low variance
        let stable = [0.80, 0.81, 0.80, 0.79, 0.80];
        let stable_var = calculate_quality_variance(&stable);
        assert!(stable_var < 0.01, "Stable values should have low variance: {}", stable_var);

        // Oscillating values - higher variance
        let oscillating = [0.70, 0.80, 0.70, 0.80, 0.70];
        let osc_var = calculate_quality_variance(&oscillating);
        assert!(osc_var > 0.04, "Oscillating values should have higher variance: {}", osc_var);
    }

    #[test]
    fn test_detect_oscillation() {
        // Too short window
        assert!(!detect_oscillation(&[0.5, 0.6], 0.01));

        // Monotonic increase - no oscillation
        assert!(!detect_oscillation(&[0.5, 0.6, 0.7, 0.8, 0.9], 0.01));

        // Monotonic decrease - no oscillation
        assert!(!detect_oscillation(&[0.9, 0.8, 0.7, 0.6, 0.5], 0.01));

        // Clear oscillation pattern
        assert!(detect_oscillation(&[0.7, 0.8, 0.7, 0.8, 0.7], 0.01));

        // Small fluctuations below min_amplitude should not count
        assert!(!detect_oscillation(&[0.70, 0.71, 0.70, 0.71, 0.70], 0.05));
    }
}
