//! Refinement Module
//!
//! Quality-based multi-stage generation with targeted refinement.
//! Uses bidirectional feedback system with multi-dimensional validation.
//! Integrates learning history for strategy optimization.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::ai::{with_timeout, LlmProvider};
use crate::config::Config;
use crate::types::{Agent, DiagnosticLevel, Result, Rule, Skill};

use super::analysis::architectural_analyzer::{
    ArchitecturalAnalyzer, StructuralValidationResult,
};
use super::quality_assessment::{
    ConvergenceChecker, ConvergencePath, ConvergenceReport, Improvement,
    TerminationDecision, TerminationReason,
};
use super::thinking::{ThinkingState, ExtensionTrigger};
use super::context::VerifiedFileRegistry;
use super::feedback::{AggregatedFeedback, FeedbackAggregator};
use super::learning::{LearningHistory, StrategyOutcome as LearningOutcome};
use super::patterns;
use super::phases::output_router::OutputPlan;
use super::strategy::{IssueKind as StrategyIssueKind, StrategyContext, StrategyOutcome, StrategyRotator};
use super::validation::{
    CrossArtifactResult, CrossValidationResult, IssueCategory as SemanticCategory,
    SemanticQualityResult, TierFilterResult, UsabilityResult,
};
use crate::types::Severity as SemanticSeverity;

/// Artifact item type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemType {
    Skill,
    Agent,
    Rule,
    ClaudeMd,
}

impl std::fmt::Display for ItemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skill => write!(f, "skill"),
            Self::Agent => write!(f, "agent"),
            Self::Rule => write!(f, "rule"),
            Self::ClaudeMd => write!(f, "claude_md"),
        }
    }
}

/// Tier 1 violation placeholder
#[derive(Debug, Clone)]
pub struct Tier1Violation {
    pub item_type: ItemType,
    pub item_name: String,
    pub reason: String,
}

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
    pub convergence_report: Option<super::quality_assessment::ConvergenceReport>,
}

#[derive(Debug, Clone)]
pub struct RefinementIssue {
    pub item_type: ItemType,
    pub item_name: String,
    pub issue: IssueKind,
    pub severity: DiagnosticLevel,
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

/// Snapshot of refinement state for rollback functionality
#[derive(Clone)]
struct RefinementSnapshot {
    skills: Vec<Skill>,
    agents: Vec<Agent>,
    rules: Vec<Rule>,
    quality: f32,
    iteration: usize,
}

/// Configuration context for refinement loop (immutable during iteration)
struct RefinementLoopConfig {
    base_iterations: usize,
    max_extension: usize,
    min_iterations_for_exit: usize,
    quality_improving_delta: f32,
    high_uncertainty_threshold: f32,
    target_quality: f32,
    stagnation_patience: usize,
    stagnation_threshold: f32,
    require_all_dimensions: bool,
    issues_per_iteration: usize,
    strategy_retry_limit: usize,
    enable_rollback: bool,
    rollback_threshold: f32,
    max_rollbacks: usize,
    post_convergence_verification: bool,
    post_convergence_passes_required: usize,
    max_convergence_detections: usize,
    min_quality_for_value_exit: f32,
}

impl RefinementLoopConfig {
    fn from_config(config: &Config) -> Self {
        let refinement = config.refinement();
        let adaptive = &refinement.adaptive_iteration;
        let target_quality = config.quality().target_score;
        Self {
            base_iterations: adaptive.base_iterations,
            max_extension: adaptive.max_extension,
            min_iterations_for_exit: adaptive.min_iterations_for_exit,
            quality_improving_delta: adaptive.quality_improving_delta,
            high_uncertainty_threshold: adaptive.high_uncertainty_threshold,
            target_quality,
            stagnation_patience: refinement.stagnation_patience,
            stagnation_threshold: refinement.stagnation_threshold,
            require_all_dimensions: refinement.require_all_dimensions,
            issues_per_iteration: refinement.issues_per_iteration,
            strategy_retry_limit: refinement.strategy_retry_limit,
            enable_rollback: refinement.enable_rollback,
            rollback_threshold: refinement.rollback_threshold,
            max_rollbacks: refinement.max_rollbacks,
            post_convergence_verification: refinement.post_convergence_verification,
            post_convergence_passes_required: refinement.post_convergence_passes,
            max_convergence_detections: refinement.max_convergence_detections,
            min_quality_for_value_exit: target_quality * 0.9,
        }
    }

    fn max_total(&self) -> usize {
        self.base_iterations + self.max_extension
    }
}

/// Mutable state tracked across refinement iterations
/// Note: quality_trajectory and tier3_trajectory are tracked in ThinkingState
struct IterationState {
    prev_quality: Option<f32>,
    stagnation_count: usize,
    last_structural_result: Option<StructuralValidationResult>,
    strategy_failures: HashMap<String, usize>,
    last_semantic_result: Option<SemanticQualityResult>,
    critical_improvements: Vec<Improvement>,
    best_state: Option<RefinementSnapshot>,
    rollback_count: usize,
    consecutive_convergence_passes: usize,
    total_convergence_detections: usize,
}

impl IterationState {
    fn new() -> Self {
        Self {
            prev_quality: None,
            stagnation_count: 0,
            last_structural_result: None,
            strategy_failures: HashMap::new(),
            last_semantic_result: None,
            critical_improvements: Vec::new(),
            best_state: None,
            rollback_count: 0,
            consecutive_convergence_passes: 0,
            total_convergence_detections: 0,
        }
    }

    fn decay_strategy_failures(&mut self) {
        for failures in self.strategy_failures.values_mut() {
            *failures = failures.saturating_sub(1);
        }
        self.strategy_failures.retain(|_, &mut v| v > 0);
    }
}

/// Simplified validators - actual validation via LLM Judge
struct Validators {
    structural: Option<ArchitecturalAnalyzer>,
}

impl Validators {
    fn new(config: &Config, _provider: &Arc<dyn LlmProvider>, _project_root: &PathBuf) -> Self {
        let structural_config = config.structural_validation();
        let structural = if structural_config.enabled {
            Some(ArchitecturalAnalyzer::new(structural_config))
        } else {
            None
        };

        Self { structural }
    }

    async fn run_semantic(
        &self,
        _skills: &[Skill],
        _agents: &[Agent],
        _rules: &[Rule],
        _claude_md: &crate::types::ProjectMemory,
        _project_context: &str,
    ) -> Result<SemanticQualityResult> {
        Ok(SemanticQualityResult::default())
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
        _skills: &[Skill],
        _agents: &[Agent],
        _rules: &[Rule],
        _claude_md: &crate::types::ProjectMemory,
    ) -> Option<CrossArtifactResult> {
        Some(CrossArtifactResult::default())
    }

    fn run_usability(
        &self,
        _skills: &[Skill],
        _agents: &[Agent],
        _rules: &[Rule],
        _claude_md: &crate::types::ProjectMemory,
    ) -> Option<UsabilityResult> {
        Some(UsabilityResult::default())
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
        let cfg = RefinementLoopConfig::from_config(&self.config);
        let validators = Validators::new(&self.config, &self.provider, &self.project_root);
        let file_registry = match &self.file_registry {
            Some(r) => r.clone(),
            None => VerifiedFileRegistry::build(&self.project_root).await?,
        };
        let project_context = format!("Project root: {}", self.project_root.display());
        let claude_md_content = claude_md.to_markdown();
        let mut state = IterationState::new();
        let mut thinking = ThinkingState::new(cfg.base_iterations, cfg.max_extension);

        while thinking.should_continue() {
            let iteration = thinking.iteration;
            state.decay_strategy_failures();

            // Phase 1: Tier filtering (simplified - actual filtering done by LLM Judge)
            let _tier_result = TierFilterResult::evaluate(&skills, &agents, &rules, &claude_md_content);
            let tier1_violations: Vec<Tier1Violation> = Vec::new(); // Violations tracked via LLM Judge
            // Keep all artifacts - filtering decision moved to quality validation

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

            let surface_quality = if cv_result.passed { 0.8 } else { 0.5 };
            let semantic_quality = semantic_result.overall_score;
            let combined_quality = aggregated_feedback.overall_score;

            thinking.record_quality(combined_quality);

            // Phase 4: Value-based termination check
            let tier3_count = count_tier3_value(&skills, &agents, &rules, claude_md);
            thinking.record_tier3(tier3_count);

            const VALUE_PLATEAU_WINDOW: usize = 3;
            if thinking.tier3_trajectory.len() >= VALUE_PLATEAU_WINDOW
                && combined_quality >= cfg.min_quality_for_value_exit
            {
                let recent: Vec<usize> = thinking.tier3_trajectory.iter().rev().take(VALUE_PLATEAU_WINDOW).copied().collect();
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
                        thinking.quality_trajectory_vec(),
                        state.critical_improvements.clone(),
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

                        // Track revision for Sequential Thinking history
                        thinking.start_revision(
                            best.iteration,
                            &format!("Rollback: quality degraded {:.1}%", degradation * 100.0),
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

            // Phase 7: Termination decision with uncertainty
            let dimensions_for_check = build_dimensions_status(
                &semantic_result,
                structural_result.as_ref(),
                cross_artifact_result.as_ref(),
                usability_result.as_ref(),
                &self.config.refinement().dimension_thresholds,
            );

            let convergence_checker = ConvergenceChecker::new(cfg.target_quality, cfg.require_all_dimensions);
            let termination_decision = convergence_checker.check_with_thinking(
                combined_quality,
                &dimensions_for_check,
                thinking.uncertainty,
                thinking.iteration,
                thinking.estimated_total,
                thinking.is_quality_improving(cfg.quality_improving_delta),
            );

            let converged = termination_decision.is_terminate();
            let meets_target = combined_quality >= cfg.target_quality;

            if iteration + 1 >= cfg.min_iterations_for_exit && converged {
                state.consecutive_convergence_passes += 1;
                state.total_convergence_detections += 1;

                let verification_passed = !cfg.post_convergence_verification
                    || state.consecutive_convergence_passes >= cfg.post_convergence_passes_required;

                // Oscillation override: too many convergence detections without progress
                let oscillation_override = state.total_convergence_detections > cfg.max_convergence_detections
                    && thinking.uncertainty < cfg.high_uncertainty_threshold;

                if verification_passed || oscillation_override {
                    let (convergence_path, decision_rationale) = match &termination_decision {
                        TerminationDecision::Terminate(TerminationReason::EarlyExit { quality, uncertainty }) => (
                            ConvergencePath::EarlyExit,
                            format!("Early exit: quality={:.1}%, uncertainty={:.2}", quality * 100.0, uncertainty),
                        ),
                        TerminationDecision::Terminate(TerminationReason::Converged(path)) => (
                            *path,
                            format!("Converged via {}", path.as_str()),
                        ),
                        TerminationDecision::Terminate(TerminationReason::Satisfied) => (
                            ConvergencePath::QualityTargetMet,
                            "Satisfied all requirements".to_string(),
                        ),
                        _ => (
                            if oscillation_override { ConvergencePath::OscillationSettled } else { ConvergencePath::QualityTargetMet },
                            "Verification passed".to_string(),
                        ),
                    };

                    tracing::info!(
                        iteration = iteration + 1,
                        quality = format!("{:.1}%", combined_quality * 100.0),
                        uncertainty = format!("{:.2}", thinking.uncertainty),
                        path = convergence_path.as_str(),
                        rationale = %decision_rationale,
                        "Convergence achieved"
                    );

                    // Use best state if current quality degraded
                    let (final_skills, final_agents, final_rules, final_quality) =
                        if let Some(ref best) = state.best_state {
                            if best.quality > combined_quality {
                                (best.skills.clone(), best.agents.clone(), best.rules.clone(), best.quality)
                            } else {
                                (skills, agents, rules, combined_quality)
                            }
                        } else {
                            (skills, agents, rules, combined_quality)
                        };

                    thinking.mark_satisfied();

                    let report = self.build_success_report(
                        convergence_path,
                        iteration + 1,
                        thinking.quality_trajectory_vec(),
                        state.critical_improvements.clone(),
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
                    && thinking.quality_trajectory.len() >= refinement_cfg.oscillation_window
                    && {
                        let window: Vec<f32> = thinking.quality_trajectory
                            .iter()
                            .copied()
                            .skip(thinking.quality_trajectory.len().saturating_sub(refinement_cfg.oscillation_window))
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
            if issues.is_empty() && iteration >= cfg.min_iterations_for_exit {
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
                            thinking.quality_trajectory_vec(),
                            state.critical_improvements.clone(),
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
            let quality_before = thinking.current_quality();
            let (new_skills, new_agents, new_rules, iter_improvements, strategies_applied) = self
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

            // Extract changes made from improvements (before moving iter_improvements)
            let changes_made: Vec<String> = iter_improvements.iter()
                .map(|imp| imp.description.clone())
                .collect();

            state.critical_improvements.extend(iter_improvements);

            // Check if iteration extension is warranted
            if thinking.iteration >= thinking.estimated_total {
                let extended_quality = thinking.maybe_extend(ExtensionTrigger::QualityImproving {
                    min_delta: cfg.quality_improving_delta,
                });
                let extended_uncertainty = thinking.maybe_extend(ExtensionTrigger::HighUncertainty {
                    threshold: cfg.high_uncertainty_threshold,
                });
                let extended_value = thinking.maybe_extend(ExtensionTrigger::ValueDiscovery);

                if extended_quality || extended_uncertainty || extended_value {
                    tracing::info!(
                        iteration = iteration + 1,
                        new_estimate = thinking.estimated_total,
                        uncertainty = format!("{:.2}", thinking.uncertainty),
                        "Iteration budget extended"
                    );
                }
            }

            // Record ThinkingRecord for full history
            let should_continue = !converged && iteration + 1 < cfg.max_total();

            // Build rationale based on current state
            let decision_rationale = if converged {
                format!("Converged: quality={:.1}%, uncertainty={:.2}", combined_quality * 100.0, thinking.uncertainty)
            } else if thinking.uncertainty > cfg.high_uncertainty_threshold {
                format!("Continue: high uncertainty ({:.2} > {:.2})", thinking.uncertainty, cfg.high_uncertainty_threshold)
            } else if thinking.is_quality_improving(cfg.quality_improving_delta) {
                "Continue: quality improving".to_string()
            } else {
                format!("Continue: iteration {}/{}", iteration + 1, cfg.max_total())
            };

            // Extract issues addressed in this iteration
            let issues_addressed: Vec<String> = issues.iter()
                .take(cfg.issues_per_iteration)
                .map(|i| format!("{}:{}", i.item_type, i.item_name))
                .collect();

            let mut thinking_record = super::thinking::ThinkingRecord::new(iteration, quality_before)
                .with_quality_after(combined_quality)
                .with_uncertainty(thinking.uncertainty)
                .with_strategies(strategies_applied)
                .with_rationale(&decision_rationale)
                .with_issues(issues_addressed)
                .with_changes(changes_made)
                .needs_continuation(should_continue);

            // Include revision info if this iteration follows a rollback
            if let Some(ref revision) = thinking.revision {
                thinking_record = thinking_record.with_revision(revision.revises_iteration, &revision.reason);
                thinking.end_revision();
            }

            thinking.record(thinking_record);
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
            iterations = cfg.max_total(),
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

        let remaining_issues: Vec<super::quality_assessment::RemainingIssue> = final_semantic
            .issues
            .iter()
            .map(|issue| super::quality_assessment::RemainingIssue {
                target: issue.target.clone(),
                category: format!("{:?}", issue.category),
                severity: format!("{:?}", issue.severity),
                description: issue.description.clone(),
                attempts: cfg.max_total(),
            })
            .collect();

        let report = ConvergenceReport::failure(
            cfg.max_total(),
            thinking.quality_trajectory_vec(),
            dimensions_status,
            remaining_issues,
        );

        self.persist_learning().await;

        Ok(RefinementResult {
            skills: output_skills,
            agents: output_agents,
            rules: output_rules,
            iterations: cfg.max_total(),
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
        quality_trajectory: Vec<f32>,
        critical_improvements: Vec<Improvement>,
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
            quality_trajectory,
            dimensions_status,
            critical_improvements,
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
        _output_plan: &OutputPlan,
    ) -> CrossValidationResult {
        // Simplified: actual validation via LLM Judge
        CrossValidationResult::validate(skills, agents, rules, claude_md)
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
                    violation: violation.reason.clone(),
                },
                severity: DiagnosticLevel::Warning,
            });
        }

        for skill in skills {
            issues.extend(self.check_skill_quality(skill, &quality_cfg.skill));
        }

        for agent in agents {
            issues.extend(self.check_agent_quality(agent, &quality_cfg.agent));
        }

        for missing in &cv_result.plan_consistency.missing_coverage {
            let (item_type, name) = parse_missing_item(missing);
            issues.push(RefinementIssue {
                item_type,
                item_name: name,
                issue: IssueKind::PlanMismatch,
                severity: DiagnosticLevel::Error,
            });
        }

        for semantic_issue in &semantic_result.issues {
            let (item_type, item_name) = parse_semantic_target(&semantic_issue.target);
            let severity = match semantic_issue.severity {
                SemanticSeverity::Critical => DiagnosticLevel::Error,
                SemanticSeverity::High => DiagnosticLevel::Error,
                SemanticSeverity::Medium => DiagnosticLevel::Warning,
                SemanticSeverity::Low => DiagnosticLevel::Info,
            };

            let issue_kind = match semantic_issue.category {
                SemanticCategory::Actionability => IssueKind::LowActionability {
                    score: semantic_result.actionability.score,
                    threshold: semantic_cfg.min_actionability,
                },
                SemanticCategory::Specificity => IssueKind::TooGeneric {
                    description: semantic_issue.description.clone(),
                },
                SemanticCategory::Evidence => IssueKind::WeakEvidence {
                    description: semantic_issue.description.clone(),
                },
                SemanticCategory::Redundancy => IssueKind::Redundant {
                    description: semantic_issue.description.clone(),
                },
                SemanticCategory::Depth => IssueKind::Shallow {
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
                severity: DiagnosticLevel::Warning,
            });
        }

        if !semantic_result.specificity.passed && semantic_cfg.reject_generic_content {
            issues.push(RefinementIssue {
                item_type: ItemType::ClaudeMd,
                item_name: "All content".to_string(),
                issue: IssueKind::TooGeneric {
                    description: format!(
                        "Specificity score below threshold: {:.0}%",
                        semantic_result.specificity.score * 100.0
                    ),
                },
                severity: DiagnosticLevel::Warning,
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
                severity: DiagnosticLevel::Error,
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
                    severity: DiagnosticLevel::Warning,
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
                severity: DiagnosticLevel::Warning,
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
                severity: DiagnosticLevel::Info,
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
                severity: DiagnosticLevel::Warning,
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
                severity: DiagnosticLevel::Warning,
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
    ) -> Result<(Vec<Skill>, Vec<Agent>, Vec<Rule>, Vec<super::quality_assessment::Improvement>, Vec<String>)> {
        let mut improvements = Vec::new();
        let mut strategies_used = Vec::new();
        let error_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == DiagnosticLevel::Error)
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
            let item_key = format!("{}:{}", issue.item_type, issue.item_name);
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

            let strategy_issue_kind = StrategyIssueKind::from(&issue.issue);

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
                quality_acceptance_delta: self.config.refinement().quality_acceptance_delta,
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
                strategies_used.push(strategy.name().to_string());

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
                        improvements.push(super::quality_assessment::Improvement {
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

        Ok((skills, agents, rules, improvements, strategies_used))
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
    cross_artifact: Option<&CrossArtifactResult>,
    usability: Option<&UsabilityResult>,
    thresholds: &crate::config::DimensionThresholds,
) -> super::quality_assessment::DimensionsStatus {
    use super::quality_assessment::{DimensionScore, DimensionsStatus};

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
            semantic_threshold * 0.8,
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
            0.3,
        ),
        structural_coverage: structural.map(|s| {
            DimensionScore::new(
                "structural_coverage",
                s.coverage_report.coverage,
                thresholds.surface,
            )
        }),
        cross_artifact: cross_artifact.map(|c| {
            DimensionScore::new(
                "cross_artifact",
                (c.overlap_score + c.consistency_score) / 2.0,
                thresholds.cross_artifact,
            )
        }),
        usability: usability.map(|u| {
            DimensionScore::new("usability", u.score, thresholds.usability)
        }),
    }
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
        assert!(DiagnosticLevel::Error > DiagnosticLevel::Warning);
        assert!(DiagnosticLevel::Warning > DiagnosticLevel::Info);
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
