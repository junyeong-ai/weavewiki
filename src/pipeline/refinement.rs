//! Refinement Module
//!
//! Quality-based multi-stage generation with targeted refinement.
//! Uses bidirectional feedback system with multi-dimensional validation.
//! Integrates learning history for strategy optimization.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::ai::{with_timeout, LlmProvider};
use crate::config::Config;
use crate::types::{Agent, Result, Rule, Skill};

use super::analysis::architectural_analyzer::{
    ArchitecturalAnalyzer, StructuralValidationResult,
};
use super::convergence::ConvergenceChecker;
use super::context::VerifiedFileRegistry;
use super::feedback::{AggregatedFeedback, FeedbackAggregator};
use super::learning::{LearningHistory, StrategyOutcome as LearningOutcome};
use super::patterns;
use super::phases::output_router::OutputPlan;
use super::strategy::{IssueKind as StrategyIssueKind, StrategyContext, StrategyOutcome, StrategyRotator};
use super::validation::{
    cross_artifact::CrossArtifactValidator,
    cross_validation::{CrossValidationResult, CrossValidator},
    quality_validator::QualityValidator,
    semantic_validator::{
        IssueCategory as SemanticCategory, IssueSeverity as SemanticSeverity,
        SemanticQualityResult, SemanticValidator,
    },
    tier_filter::{self, ItemType, Tier1Violation},
    usability::UsabilityValidator,
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
        let refinement_config = self.config.refinement().clone();
        let max_iterations = refinement_config.max_iterations;
        let min_iterations = refinement_config.min_iterations;
        let target_quality = self.config.quality().target_score;
        let stagnation_patience = refinement_config.stagnation_patience;
        let stagnation_threshold = refinement_config.stagnation_threshold;
        let require_all_dimensions = refinement_config.require_all_dimensions;
        let issues_per_iteration = refinement_config.issues_per_iteration;
        let strategy_retry_limit = refinement_config.strategy_retry_limit;
        let oscillation_strict_passes = refinement_config.oscillation_strict_passes;
        let oscillation_lenient_passes = refinement_config.oscillation_lenient_passes;
        let oscillation_stability_variance = refinement_config.oscillation_stability_variance;
        let oscillation_variance_window = refinement_config.oscillation_variance_window;

        let claude_md_content = claude_md.to_markdown();
        let semantic_config = self.config.semantic_validation();
        let use_ai_validation = semantic_config.use_ai_validation;

        // Create only the validator that will be used
        let semantic_validator = if !use_ai_validation {
            Some(SemanticValidator::new(semantic_config.clone(), &self.project_root))
        } else {
            None
        };

        let quality_validator = if use_ai_validation {
            Some(QualityValidator::new(
                Arc::clone(&self.provider),
                &semantic_config,
            ))
        } else {
            None
        };

        let file_registry = match &self.file_registry {
            Some(r) => r.clone(),
            None => VerifiedFileRegistry::build(&self.project_root).await?,
        };

        // Create structural analyzer for module coverage validation
        let structural_config = self.config.structural_validation();
        let structural_analyzer = if structural_config.enabled {
            Some(ArchitecturalAnalyzer::new(structural_config))
        } else {
            None
        };

        // Create cross-artifact validator for coherence checking
        let cross_artifact_config = self.config.cross_artifact();
        let cross_artifact_validator = if cross_artifact_config.enabled {
            Some(CrossArtifactValidator::new(
                cross_artifact_config.min_coherence_score,
                cross_artifact_config.max_overlap_ratio,
            ))
        } else {
            None
        };

        // Create usability validator for AI coding effectiveness
        let usability_config = self.config.usability();
        let usability_validator = if usability_config.enabled {
            Some(
                UsabilityValidator::new(crate::config::ProjectType::default())
                    .with_config(usability_config),
            )
        } else {
            None
        };

        let project_context = format!(
            "Project root: {}",
            self.project_root.display()
        );

        let mut prev_quality: Option<f32> = None;
        let mut stagnation_count = 0;
        let mut last_structural_result: Option<StructuralValidationResult> = None;
        let mut strategy_failures: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut last_semantic_result: Option<SemanticQualityResult> = None;

        // Track quality trajectory for convergence reporting
        // Use VecDeque with bounded size to prevent unbounded memory growth
        const MAX_TRAJECTORY_SIZE: usize = 100;
        let mut quality_trajectory: VecDeque<f32> = VecDeque::with_capacity(MAX_TRAJECTORY_SIZE);
        let mut critical_improvements: Vec<super::convergence::Improvement> = Vec::new();

        // Rollback state tracking
        let enable_rollback = refinement_config.enable_rollback;
        let rollback_threshold = refinement_config.rollback_threshold;
        let max_rollbacks = refinement_config.max_rollbacks;
        let mut best_state: Option<RefinementSnapshot> = None;
        let mut rollback_count = 0;

        // Post-convergence verification tracking
        let post_convergence_verification = refinement_config.post_convergence_verification;
        let post_convergence_passes_required = refinement_config.post_convergence_passes;
        let mut consecutive_convergence_passes = 0;
        let mut total_convergence_detections = 0; // Track total to detect oscillation around threshold
        let max_convergence_detections = refinement_config.max_convergence_detections;

        for iteration in 0..max_iterations {
            // Decay failure counts to allow strategy retry after cooldown
            for failures in strategy_failures.values_mut() {
                *failures = failures.saturating_sub(1);
            }
            // Remove entries with zero failures to keep map clean
            strategy_failures.retain(|_, &mut v| v > 0);

            let tier_result = tier_filter::filter(&skills, &agents, &rules, &claude_md_content);
            let tier1_violations = tier_result.tier1_violations;
            skills = tier_result.filtered_content.skills;
            agents = tier_result.filtered_content.agents;
            rules = tier_result.filtered_content.rules;

            let cv_result = self.assess_quality(&skills, &agents, &rules, claude_md, output_plan);

            // Use AI validation if enabled, otherwise use static pattern matching
            let semantic_result = if let Some(ref qv) = quality_validator {
                qv.validate(&skills, &agents, &rules, claude_md, &project_context)
                    .await?
            } else {
                // semantic_validator is guaranteed to be Some when quality_validator is None
                semantic_validator
                    .as_ref()
                    .expect("semantic_validator required when ai_validation disabled")
                    .validate(&skills, &agents, &rules, claude_md)
                    .await?
            };

            // Run structural validation for module coverage
            let structural_result = if let Some(ref analyzer) = structural_analyzer {
                let result = analyzer
                    .validate(&file_registry, &skills, &agents, &rules, claude_md)
                    .await?;
                last_structural_result = Some(result.clone());
                Some(result)
            } else {
                None
            };

            // Run cross-artifact validation for coherence
            let cross_artifact_result = cross_artifact_validator.as_ref().map(|validator| validator.validate(&skills, &agents, &rules, claude_md));

            // Run usability validation for AI coding effectiveness
            let usability_result = usability_validator.as_ref().map(|validator| validator.validate(&skills, &agents, &rules, claude_md));

            // Aggregate feedback from all validation sources
            let aggregated_feedback = self.feedback_aggregator.aggregate(
                &semantic_result,
                structural_result.as_ref(),
                cross_artifact_result.as_ref(),
                usability_result.as_ref(),
                Some(&cv_result),
            );

            // Use unified quality calculation from FeedbackAggregator
            // This combines semantic, structural, cross-artifact, and usability dimensions
            let surface_quality = cv_result.quality_score;
            let semantic_quality = semantic_result.overall_score;
            let combined_quality = aggregated_feedback.overall_score;

            // Track quality trajectory for convergence analysis
            // Maintain bounded size to prevent memory growth
            if quality_trajectory.len() >= MAX_TRAJECTORY_SIZE {
                quality_trajectory.pop_front();
            }
            quality_trajectory.push_back(combined_quality);

            // Rollback logic: save best state and rollback if quality degrades significantly
            if enable_rollback {
                // Save best state when quality improves significantly
                // Minimum 2% improvement required to avoid excessive cloning on small fluctuations
                const SNAPSHOT_MIN_IMPROVEMENT: f32 = 0.02;
                let should_save = match &best_state {
                    None => true, // Always save first snapshot
                    Some(state) => combined_quality >= state.quality + SNAPSHOT_MIN_IMPROVEMENT,
                };

                if should_save {
                    best_state = Some(RefinementSnapshot {
                        skills: skills.clone(),
                        agents: agents.clone(),
                        rules: rules.clone(),
                        quality: combined_quality,
                        iteration: iteration + 1,
                    });
                }

                // Check for significant quality degradation (only after first iteration)
                if let (Some(state), Some(_)) = (&best_state, prev_quality) {
                    let degradation = state.quality - combined_quality;
                    if degradation > rollback_threshold && rollback_count < max_rollbacks {
                        tracing::warn!(
                            iteration = iteration + 1,
                            current = format!("{:.1}%", combined_quality * 100.0),
                            best = format!("{:.1}%", state.quality * 100.0),
                            degradation = format!("{:.1}%", degradation * 100.0),
                            "Quality degraded significantly, rolling back to iteration {}",
                            state.iteration
                        );

                        skills = state.skills.clone();
                        agents = state.agents.clone();
                        rules = state.rules.clone();
                        rollback_count += 1;

                        // Force strategy escalation after rollback
                        self.strategy_rotator.escalate();
                        strategy_failures.clear();
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

            // Log aggregated feedback score
            tracing::info!(
                iteration = iteration + 1,
                surface = format!("{:.1}%", surface_quality * 100.0),
                semantic = format!("{:.1}%", semantic_quality * 100.0),
                combined = format!("{:.1}%", combined_quality * 100.0),
                aggregated = format!("{:.1}%", aggregated_feedback.overall_score * 100.0),
                dimensions = dimension_status,
                target = format!("{:.1}%", target_quality * 100.0),
                "Quality assessment"
            );

            // Convergence: Use unified ConvergenceChecker for consistent quality evaluation
            // Build dimensions status for checker (reused later in reports)
            let dimensions_for_check = build_dimensions_status(
                &semantic_result,
                structural_result.as_ref(),
                cross_artifact_result.as_ref(),
                usability_result.as_ref(),
                &self.config.refinement().dimension_thresholds,
            );

            // Count issues that need addressing (estimate from semantic result)
            let issues_estimate = semantic_result.suggestions.len()
                + if semantic_result.passed { 0 } else { 1 };

            // Use centralized ConvergenceChecker for unified metrics
            let convergence_checker = ConvergenceChecker::new(target_quality, require_all_dimensions);
            let convergence_path = convergence_checker.check(
                combined_quality,
                &dimensions_for_check,
                aggregated_feedback.converged,
                issues_estimate,
            );

            let converged = convergence_path.is_some();
            let meets_target = combined_quality >= target_quality;

            // Note: iteration is 0-indexed, so add 1 for human-readable count
            if iteration + 1 >= min_iterations && converged {
                consecutive_convergence_passes += 1;
                total_convergence_detections += 1;

                // Detect oscillation around convergence threshold
                // CRITICAL FIX: Require stricter verification during oscillation
                // Previous logic was too lenient (>= 1 pass), now requires:
                // 1. At least 3 consecutive passes OR
                // 2. Quality variance in window below 2%
                if total_convergence_detections > max_convergence_detections
                    && consecutive_convergence_passes < post_convergence_passes_required
                {
                    // Calculate quality variance in recent window for stability check
                    let quality_variance = if quality_trajectory.len() >= 3 {
                        let recent: Vec<f32> = quality_trajectory
                            .iter()
                            .copied()
                            .skip(quality_trajectory.len().saturating_sub(oscillation_variance_window))
                            .collect();
                        calculate_quality_variance(&recent)
                    } else {
                        1.0 // High variance if not enough data
                    };

                    // Stricter verification: need N+ passes OR (lenient passes + low variance)
                    let oscillation_verification_ok = !post_convergence_verification
                        || consecutive_convergence_passes >= oscillation_strict_passes
                        || (consecutive_convergence_passes >= oscillation_lenient_passes
                            && quality_variance < oscillation_stability_variance);

                    if oscillation_verification_ok {
                        tracing::warn!(
                            iteration = iteration + 1,
                            total_convergence_detections,
                            consecutive_passes = consecutive_convergence_passes,
                            quality = format!("{:.1}%", combined_quality * 100.0),
                            "Convergence oscillation detected: quality unstable around threshold, accepting with reduced verification"
                        );

                        // Use best_state if it's better than current state
                        let (final_skills, final_agents, final_rules, final_quality) =
                            if let Some(ref state) = best_state {
                                if state.quality > combined_quality {
                                    tracing::info!(
                                        current = format!("{:.1}%", combined_quality * 100.0),
                                        best = format!("{:.1}%", state.quality * 100.0),
                                        best_iteration = state.iteration,
                                        "Using best state from iteration {} instead of oscillating current state",
                                        state.iteration
                                    );
                                    (state.skills.clone(), state.agents.clone(), state.rules.clone(), state.quality)
                                } else {
                                    (skills, agents, rules, combined_quality)
                                }
                            } else {
                                (skills, agents, rules, combined_quality)
                            };

                        let dimensions_status = build_dimensions_status(
                            &semantic_result,
                            structural_result.as_ref(),
                            cross_artifact_result.as_ref(),
                            usability_result.as_ref(),
                            &self.config.refinement().dimension_thresholds,
                        );
                        let report = super::convergence::ConvergenceReport::success(
                            super::convergence::ConvergencePath::OscillationSettled,
                            iteration + 1,
                            Vec::from(quality_trajectory.clone()),
                            dimensions_status,
                            critical_improvements.clone(),
                        );

                        // Persist learning patterns for future sessions
                        if let Err(e) = self.learning_history.persist(&self.project_root).await {
                            tracing::error!(error = %e, "Failed to persist learning patterns - data may be lost");
                        }

                        return Ok(RefinementResult {
                            skills: final_skills,
                            agents: final_agents,
                            rules: final_rules,
                            iterations: iteration + 1,
                            converged: true, // Accepting as converged despite oscillation
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
                            consecutive_passes = consecutive_convergence_passes,
                            "Oscillation detected but waiting for at least one verification pass"
                        );
                    }
                }

                // Post-convergence verification: require multiple consecutive passes
                let verification_passed = if post_convergence_verification {
                    consecutive_convergence_passes >= post_convergence_passes_required
                } else {
                    true
                };

                if verification_passed {
                    tracing::info!(
                        iteration = iteration + 1,
                        combined = format!("{:.1}%", combined_quality * 100.0),
                        dimensions = dimension_status,
                        consecutive_passes = consecutive_convergence_passes,
                        "Convergence achieved: all quality targets met with verification"
                    );

                    let dimensions_status = build_dimensions_status(
                        &semantic_result,
                        structural_result.as_ref(),
                        cross_artifact_result.as_ref(),
                        usability_result.as_ref(),
                        &self.config.refinement().dimension_thresholds,
                    );
                    let path = determine_convergence_path(
                        meets_target,
                        dimensions_for_check.all_passed(require_all_dimensions),
                        require_all_dimensions,
                        aggregated_feedback.converged,
                        0,
                    );
                    let report = super::convergence::ConvergenceReport::success(
                        path,
                        iteration + 1,
                        Vec::from(quality_trajectory.clone()),
                        dimensions_status,
                        critical_improvements.clone(),
                    );

                    // Persist learning patterns for future sessions
                    if let Err(e) = self.learning_history.persist(&self.project_root).await {
                        tracing::error!(error = %e, "Failed to persist learning patterns - data may be lost");
                    }

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
                        consecutive_passes = consecutive_convergence_passes,
                        required = post_convergence_passes_required,
                        "Convergence detected, continuing verification loop"
                    );
                }
            } else {
                // Reset consecutive passes if convergence is lost
                consecutive_convergence_passes = 0;
            }

            // CRITICAL FIX: Separate oscillation and stagnation detection into distinct states
            // Previous implementation had collision where both could trigger escalate() independently
            let refinement_cfg = self.config.refinement();
            if let Some(prev) = prev_quality {
                let delta = (combined_quality - prev).abs();
                let improved = combined_quality > prev + refinement_cfg.min_improvement_per_iteration;

                // Detect oscillation state
                let is_oscillating = refinement_cfg.detect_oscillation
                    && quality_trajectory.len() >= refinement_cfg.oscillation_window
                    && {
                        let window: Vec<f32> = quality_trajectory
                            .iter()
                            .copied()
                            .skip(quality_trajectory.len().saturating_sub(refinement_cfg.oscillation_window))
                            .collect();
                        detect_oscillation(&window, refinement_cfg.oscillation_min_amplitude)
                    };

                // Detect stagnation state
                let is_stagnating = delta < stagnation_threshold || !improved;

                // State machine: handle combinations properly without collision
                match (is_oscillating, is_stagnating) {
                    (true, true) => {
                        // Both oscillating AND stagnating: most severe - force regeneration
                        tracing::warn!(
                            iteration = iteration + 1,
                            "Oscillation + Stagnation detected: forcing full regeneration"
                        );
                        self.strategy_rotator.force_regeneration();
                        strategy_failures.clear();
                        stagnation_count = 0;
                    }
                    (true, false) => {
                        // Oscillating only: quality is bouncing, need strategy change
                        tracing::warn!(
                            iteration = iteration + 1,
                            window_size = refinement_cfg.oscillation_window,
                            "Oscillation detected: quality bouncing, escalating strategy"
                        );
                        self.strategy_rotator.escalate();
                        strategy_failures.clear();
                        stagnation_count = 0;
                    }
                    (false, true) => {
                        // Stagnating only: increment patience counter
                        stagnation_count += 1;
                        tracing::debug!(
                            delta = format!("{:.3}", delta),
                            stagnation_count,
                            patience = stagnation_patience,
                            "Quality improvement stalled"
                        );

                        // Only act when patience is exhausted - always escalate
                        if stagnation_count >= stagnation_patience {
                            tracing::info!(
                                iteration = iteration + 1,
                                "Stagnation patience exhausted: escalating to stronger strategies"
                            );
                            self.strategy_rotator.escalate();
                            strategy_failures.clear();
                            stagnation_count = 0;
                        }
                    }
                    (false, false) => {
                        // Normal progress: reset stagnation counter
                        stagnation_count = 0;
                    }
                }
            }
            prev_quality = Some(combined_quality);
            last_semantic_result = Some(semantic_result.clone());

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

            if issues.is_empty() && iteration >= min_iterations {
                // Enforce minimum quality threshold even when no issues found
                let quality_acceptable = meets_target ||
                    (!require_all_dimensions && combined_quality >= target_quality * 0.9);

                if (converged || !require_all_dimensions) && quality_acceptable {
                    // NoIssuesRemaining also needs to respect post-convergence verification
                    // to ensure the "no issues" state is stable
                    let no_issues_verification_passed = if post_convergence_verification {
                        // For NoIssuesRemaining, we use consecutive_convergence_passes
                        // which gets incremented when converged is true
                        consecutive_convergence_passes >= post_convergence_passes_required
                    } else {
                        true
                    };

                    if no_issues_verification_passed {
                        tracing::info!(
                            iteration = iteration + 1,
                            quality = format!("{:.1}%", combined_quality * 100.0),
                            consecutive_passes = consecutive_convergence_passes,
                            "No issues found, refinement complete with verification"
                        );

                        let dimensions_status = build_dimensions_status(
                            &semantic_result,
                            structural_result.as_ref(),
                            cross_artifact_result.as_ref(),
                            usability_result.as_ref(),
                            &self.config.refinement().dimension_thresholds,
                        );
                        let report = super::convergence::ConvergenceReport::success(
                            super::convergence::ConvergencePath::NoIssuesRemaining,
                            iteration + 1,
                            Vec::from(quality_trajectory.clone()),
                            dimensions_status,
                            critical_improvements.clone(),
                        );

                        // Persist learning patterns for future sessions
                        if let Err(e) = self.learning_history.persist(&self.project_root).await {
                            tracing::error!(error = %e, "Failed to persist learning patterns - data may be lost");
                        }

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
                            consecutive_passes = consecutive_convergence_passes,
                            required = post_convergence_passes_required,
                            "No issues found but waiting for verification passes"
                        );
                    }
                } else if !quality_acceptable {
                    // Quality below threshold despite no issues - need to continue refinement
                    tracing::debug!(
                        iteration = iteration + 1,
                        quality = format!("{:.1}%", combined_quality * 100.0),
                        target = format!("{:.1}%", target_quality * 100.0),
                        "No issues found but quality below minimum threshold, continuing refinement"
                    );
                }
            }

            let (new_skills, new_agents, new_rules, iter_improvements) = self
                .apply_refinements_with_strategies(
                    skills,
                    agents,
                    rules,
                    &issues,
                    &semantic_result,
                    &file_registry,
                    iteration,
                    issues_per_iteration,
                    strategy_retry_limit,
                    &mut strategy_failures,
                    combined_quality,
                )
                .await?;

            skills = new_skills;
            agents = new_agents;
            rules = new_rules;
            critical_improvements.extend(iter_improvements);
        }

        let final_cv = self.assess_quality(&skills, &agents, &rules, claude_md, output_plan);
        let final_semantic = if let Some(ref qv) = quality_validator {
            qv.validate(&skills, &agents, &rules, claude_md, &project_context)
                .await?
        } else {
            semantic_validator
                .as_ref()
                .expect("semantic_validator required when ai_validation disabled")
                .validate(&skills, &agents, &rules, claude_md)
                .await?
        };

        // Use unified FeedbackAggregator for final quality calculation
        let final_aggregated = self.feedback_aggregator.aggregate(
            &final_semantic,
            last_structural_result.as_ref(),
            None,
            None,
            Some(&final_cv),
        );
        let final_quality = final_aggregated.overall_score;

        // CRITICAL: Recover best_state if it's better than the final state
        // This ensures we don't return a degraded state when max iterations is reached
        let (output_skills, output_agents, output_rules, output_quality) =
            if let Some(ref state) = best_state {
                if state.quality > final_quality {
                    tracing::info!(
                        final_quality = format!("{:.1}%", final_quality * 100.0),
                        best_quality = format!("{:.1}%", state.quality * 100.0),
                        best_iteration = state.iteration,
                        "Max iterations reached: using best state from iteration {} instead of final degraded state",
                        state.iteration
                    );
                    (state.skills.clone(), state.agents.clone(), state.rules.clone(), state.quality)
                } else {
                    (skills, agents, rules, final_quality)
                }
            } else {
                (skills, agents, rules, final_quality)
            };

        tracing::warn!(
            iterations = max_iterations,
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

        // Build convergence failure report
        let dimensions_status = build_dimensions_status(
            &final_semantic,
            last_structural_result.as_ref(),
            None, // No cross-artifact result available at this point
            None, // No usability result available at this point
            &self.config.refinement().dimension_thresholds,
        );

        // Populate remaining issues from semantic validation results
        let remaining_issues: Vec<super::convergence::RemainingIssue> = final_semantic
            .issues
            .iter()
            .map(|issue| super::convergence::RemainingIssue {
                target: issue.target.clone(),
                category: format!("{:?}", issue.category),
                severity: format!("{:?}", issue.severity),
                description: issue.description.clone(),
                attempts: max_iterations, // All issues have been attempted for max iterations
            })
            .collect();

        let report = super::convergence::ConvergenceReport::failure(
            max_iterations,
            Vec::from(quality_trajectory),
            dimensions_status,
            remaining_issues,
        );

        // Persist learning patterns for future sessions
        if let Err(e) = self.learning_history.persist(&self.project_root).await {
            tracing::error!(error = %e, "Failed to persist learning patterns - data may be lost");
        }

        Ok(RefinementResult {
            skills: output_skills,
            agents: output_agents,
            rules: output_rules,
            iterations: max_iterations,
            converged: false,
            final_quality: output_quality,
            semantic_quality: last_semantic_result.or(Some(final_semantic)),
            structural_quality: last_structural_result,
            aggregated_feedback: None,
            learning_summary: Some(self.learning_history.get_progress_summary()),
            convergence_report: Some(report),
        })
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

            let context = StrategyContext {
                file_registry,
                issue_description: format_issue_description(&issue.issue),
                suggestions,
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
