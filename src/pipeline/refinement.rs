//! Refinement Module
//!
//! Quality-based multi-stage generation with targeted refinement.
//! Uses bidirectional feedback system with multi-dimensional validation.
//! Integrates learning history for strategy optimization.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::ai::{LlmProvider, with_timeout};
use crate::config::Config;
use crate::types::{Agent, DiagnosticLevel, Result, Rule, Skill};

use super::analysis::architectural_analyzer::{ArchitecturalAnalyzer, StructuralValidationResult};
use super::analysis::deep_analyzer::CoreModule;
use super::context::VerifiedFileRegistry;
use super::feedback::{AggregatedFeedback, FeedbackAggregator};
use super::iteration_state::{BudgetExtensionTrigger, IterationRecord, IterationState};
use super::learning::{LearningHistory, StrategyOutcome as LearningOutcome};
use super::patterns;
use super::phases::output_router::OutputPlan;
use super::quality::{Artifacts, JudgmentResult, LlmJudge, QualityIssue};
use super::quality_assessment::{
    AssessmentPath, Improvement, QualityAssessment, QualityAssessor, TerminationDecision,
    TerminationReason,
};
use super::strategy::{
    IssueKind as StrategyIssueKind, StrategyAttempt, StrategyContext, StrategyIssue,
    StrategyRotator,
};
use super::validation::{CrossValidationResult, TierFilterResult};

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

#[derive(Debug, Clone)]
pub struct RefinementResult {
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub rules: Vec<Rule>,
    pub iterations: usize,
    pub converged: bool,
    pub final_quality: f32,
    pub judgment: Option<JudgmentResult>,
    pub structural_quality: Option<StructuralValidationResult>,
    pub aggregated_feedback: Option<AggregatedFeedback>,
    pub learning_summary: Option<super::learning::ProgressSummary>,
    pub convergence_report: Option<super::quality_assessment::QualityAssessment>,
}

#[derive(Debug, Clone)]
pub struct DetectedArtifactIssue {
    pub item_type: ItemType,
    pub item_name: String,
    pub issue: DetectedIssue,
    pub severity: DiagnosticLevel,
}

#[derive(Debug, Clone)]
pub enum DetectedIssue {
    TooShort {
        actual: usize,
        min: usize,
    },
    MissingReferences {
        expected: usize,
        actual: usize,
    },
    MissingSections {
        expected: usize,
        actual: usize,
    },
    Tier1Content {
        violation: String,
    },
    PlanMismatch,
    LowActionability {
        score: f32,
        threshold: f32,
    },
    TooGeneric {
        description: String,
    },
    WeakEvidence {
        description: String,
    },
    Redundant {
        description: String,
    },
    Shallow {
        description: String,
    },
    MissingModule {
        module_name: String,
        file_count: usize,
        key_files: Vec<String>,
    },
    PartialModuleCoverage {
        module_name: String,
        coverage: f32,
    },
    /// Custom issue detected by LLM that doesn't fit predefined categories
    Other {
        kind: String,
        description: String,
    },
}

impl DetectedIssue {
    /// Convert to StrategyIssue for refinement context
    pub fn to_strategy_issue(&self) -> StrategyIssue {
        let kind = StrategyIssueKind::from(self);
        let (severity, message) = match self {
            Self::LowActionability { score, threshold } => (
                DiagnosticLevel::Warning,
                format!(
                    "Low actionability ({:.0}% vs {:.0}% target)",
                    score * 100.0,
                    threshold * 100.0
                ),
            ),
            Self::TooGeneric { description } => (
                DiagnosticLevel::Warning,
                format!("Too generic: {description}"),
            ),
            Self::WeakEvidence { description } => (
                DiagnosticLevel::Warning,
                format!("Weak evidence: {description}"),
            ),
            Self::Shallow { description } => (
                DiagnosticLevel::Warning,
                format!("Shallow coverage: {description}"),
            ),
            Self::MissingReferences { expected, actual } => (
                DiagnosticLevel::Error,
                format!("Missing references: {actual} of {expected} required"),
            ),
            Self::TooShort { actual, min } => (
                DiagnosticLevel::Error,
                format!("Too short: {actual} chars (min: {min})"),
            ),
            Self::MissingSections { expected, actual } => (
                DiagnosticLevel::Error,
                format!("Missing sections: {actual} of {expected} required"),
            ),
            Self::Redundant { description } => {
                (DiagnosticLevel::Info, format!("Redundant: {description}"))
            }
            Self::Tier1Content { violation } => (
                DiagnosticLevel::Error,
                format!("Tier 1 content: {violation}"),
            ),
            Self::PlanMismatch => (
                DiagnosticLevel::Warning,
                "Item missing from output plan".to_string(),
            ),
            Self::MissingModule {
                module_name,
                file_count,
                key_files,
            } => (
                DiagnosticLevel::Error,
                format!(
                    "Missing module: '{module_name}' ({file_count} files) - key: {}",
                    key_files.join(", ")
                ),
            ),
            Self::PartialModuleCoverage {
                module_name,
                coverage,
            } => (
                DiagnosticLevel::Warning,
                format!(
                    "Partial coverage: '{module_name}' at {:.0}%",
                    coverage * 100.0
                ),
            ),
            Self::Other { kind, description } => {
                (DiagnosticLevel::Warning, format!("{kind}: {description}"))
            }
        };
        StrategyIssue::new(kind, severity, message)
    }
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

/// Result of self-critique inner loop
#[derive(Debug, Clone)]
pub enum CritiqueResult {
    /// No issues found, artifacts are clean
    Clean { quality: f32 },
    /// Issues found and fixed within iteration limit
    Fixed {
        iterations: usize,
        final_quality: f32,
    },
    /// Max iterations reached, some issues may remain
    MaxIterationsReached {
        iterations: usize,
        remaining_issues: usize,
        final_quality: f32,
    },
    /// Skipped due to quality already above threshold
    Skipped { reason: String },
}

/// Validation results context for report building
struct ValidationContext<'a> {
    judgment: &'a JudgmentResult,
    structural: Option<&'a StructuralValidationResult>,
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
        }
    }

    fn max_total(&self) -> usize {
        self.base_iterations + self.max_extension
    }
}

/// Mutable state tracked across refinement iterations
/// Note: quality_trajectory is tracked in IterationState
struct RefinementState {
    prev_quality: Option<f32>,
    stagnation_count: usize,
    last_structural_result: Option<StructuralValidationResult>,
    strategy_failures: HashMap<String, usize>,
    last_judgment: Option<JudgmentResult>,
    critical_improvements: Vec<Improvement>,
    best_state: Option<RefinementSnapshot>,
    rollback_count: usize,
    consecutive_convergence_passes: usize,
    total_convergence_detections: usize,
}

impl RefinementState {
    fn new() -> Self {
        Self {
            prev_quality: None,
            stagnation_count: 0,
            last_structural_result: None,
            strategy_failures: HashMap::new(),
            last_judgment: None,
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

/// Structural validator - semantic validation handled by LlmJudge
struct Validators {
    structural: Option<ArchitecturalAnalyzer>,
}

impl Validators {
    fn new(
        config: &Config,
        _provider: &Arc<dyn LlmProvider>,
        _project_root: &PathBuf,
        core_modules: &[CoreModule],
    ) -> Self {
        let structural_config = config.structural_validation();
        let structural = if structural_config.enabled && !core_modules.is_empty() {
            Some(ArchitecturalAnalyzer::new(structural_config, core_modules))
        } else {
            None
        };

        Self { structural }
    }

    async fn run_structural(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &crate::types::ProjectMemory,
    ) -> Result<Option<StructuralValidationResult>> {
        if let Some(ref analyzer) = self.structural {
            Ok(Some(
                analyzer.validate(skills, agents, rules, claude_md).await?,
            ))
        } else {
            Ok(None)
        }
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
    judge: LlmJudge,
    llm_modules: Vec<CoreModule>,
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
        let judge = LlmJudge::new(Arc::clone(&provider));

        Self {
            project_root,
            provider,
            config,
            file_registry: Some(file_registry),
            strategy_rotator,
            learning_history,
            feedback_aggregator,
            judge,
            llm_modules: Vec::new(),
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
        let judge = LlmJudge::new(Arc::clone(&provider));

        let learning_history = if LearningHistory::has_persisted_data(&project_root) {
            match LearningHistory::load(&project_root, crate::config::LearningConfig::default())
                .await
            {
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
            judge,
            llm_modules: Vec::new(),
        })
    }

    /// Set modules identified by LLM for structural validation
    pub fn with_llm_modules(mut self, modules: Vec<CoreModule>) -> Self {
        self.llm_modules = modules;
        self
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

    /// Self-critique inner loop: iteratively improve artifacts until clean or max iterations
    pub async fn self_critique_loop(
        &self,
        skills: &mut Vec<Skill>,
        agents: &mut Vec<Agent>,
        rules: &mut Vec<Rule>,
    ) -> Result<CritiqueResult> {
        let critique_cfg = &self.config.refinement().self_critique;

        if !critique_cfg.enabled {
            return Ok(CritiqueResult::Skipped {
                reason: "Self-critique disabled".to_string(),
            });
        }

        let artifacts = Artifacts {
            skills: skills.clone(),
            agents: agents.clone(),
            rules: rules.clone(),
        };

        // Initial quality check
        let initial_judgment = self.judge.evaluate_artifacts(&artifacts).await?;
        let initial_quality = initial_judgment.overall_score;

        if initial_quality >= critique_cfg.quality_skip_threshold {
            return Ok(CritiqueResult::Skipped {
                reason: format!(
                    "Quality {:.1}% already above threshold {:.1}%",
                    initial_quality * 100.0,
                    critique_cfg.quality_skip_threshold * 100.0
                ),
            });
        }

        if initial_judgment.issues.is_empty() {
            return Ok(CritiqueResult::Clean {
                quality: initial_quality,
            });
        }

        let mut current_quality = initial_quality;
        let mut iteration = 0;

        while iteration < critique_cfg.max_iterations {
            iteration += 1;

            let artifacts = Artifacts {
                skills: skills.clone(),
                agents: agents.clone(),
                rules: rules.clone(),
            };

            let judgment = self.judge.evaluate_artifacts(&artifacts).await?;

            if judgment.issues.is_empty() {
                tracing::debug!(
                    iteration,
                    quality = format!("{:.1}%", judgment.overall_score * 100.0),
                    "Self-critique: no issues found"
                );
                return Ok(CritiqueResult::Fixed {
                    iterations: iteration,
                    final_quality: judgment.overall_score,
                });
            }

            // Apply fixes based on suggestions
            let fixes_applied = self
                .apply_critique_fixes(skills, agents, rules, &judgment)
                .await?;

            if fixes_applied == 0 {
                tracing::debug!(
                    iteration,
                    remaining_issues = judgment.issues.len(),
                    "Self-critique: no fixes could be applied"
                );
                break;
            }

            // Re-evaluate after fixes to measure actual improvement
            let post_fix_artifacts = Artifacts {
                skills: skills.clone(),
                agents: agents.clone(),
                rules: rules.clone(),
            };
            let post_fix_judgment = self.judge.evaluate_artifacts(&post_fix_artifacts).await?;
            let new_quality = post_fix_judgment.overall_score;
            let improvement = new_quality - current_quality;

            tracing::debug!(
                iteration,
                quality = format!("{:.1}%", new_quality * 100.0),
                improvement = format!("{:+.1}%", improvement * 100.0),
                fixes_applied,
                remaining_issues = post_fix_judgment.issues.len(),
                "Self-critique iteration"
            );

            if improvement < critique_cfg.min_improvement && iteration > 1 {
                tracing::debug!(
                    iteration,
                    improvement = format!("{:.3}", improvement),
                    threshold = format!("{:.3}", critique_cfg.min_improvement),
                    "Self-critique: insufficient improvement, stopping"
                );
                break;
            }

            current_quality = new_quality;
        }

        // Final evaluation
        let final_artifacts = Artifacts {
            skills: skills.clone(),
            agents: agents.clone(),
            rules: rules.clone(),
        };
        let final_judgment = self.judge.evaluate_artifacts(&final_artifacts).await?;

        Ok(CritiqueResult::MaxIterationsReached {
            iterations: iteration,
            remaining_issues: final_judgment.issues.len(),
            final_quality: final_judgment.overall_score,
        })
    }

    /// Apply fixes based on judgment suggestions
    async fn apply_critique_fixes(
        &self,
        skills: &mut Vec<Skill>,
        agents: &mut Vec<Agent>,
        rules: &mut Vec<Rule>,
        judgment: &JudgmentResult,
    ) -> Result<usize> {
        let mut fixes_applied = 0;

        for issue in &judgment.issues {
            // Find matching artifact and apply fix based on issue code
            match issue.code.as_str() {
                "weak_evidence" | "missing_references" => {
                    // These require evidence strategy - mark for later
                    fixes_applied += self.mark_for_evidence_fix(skills, agents, rules, issue);
                }
                "too_generic" | "tier1_content" => {
                    // Remove or flag generic content
                    fixes_applied += self.remove_generic_content(skills, agents, rules, issue);
                }
                "low_actionability" => {
                    // Add actionable details from suggestions
                    fixes_applied +=
                        self.enhance_actionability(skills, agents, rules, &judgment.suggestions);
                }
                _ => {
                    // Unknown issue type - log and skip
                    tracing::trace!(code = %issue.code, "Unknown critique issue code");
                }
            }
        }

        Ok(fixes_applied)
    }

    fn mark_for_evidence_fix(
        &self,
        skills: &[Skill],
        _agents: &[Agent],
        _rules: &[Rule],
        issue: &QualityIssue,
    ) -> usize {
        // Evidence issues are handled by EvidenceStrategy in the main refinement loop.
        // Here we just log which artifacts need attention.
        for skill in skills.iter() {
            if issue.message.contains(&skill.name)
                || issue.evidence.iter().any(|e| e.contains(&skill.name))
            {
                tracing::debug!(
                    skill = %skill.name,
                    issue = %issue.code,
                    "Skill flagged for evidence enhancement (will be handled by EvidenceStrategy)"
                );
            }
        }
        // Return 0 - actual fixes deferred to EvidenceStrategy
        0
    }

    fn remove_generic_content(
        &self,
        skills: &mut Vec<Skill>,
        agents: &mut Vec<Agent>,
        rules: &mut Vec<Rule>,
        issue: &QualityIssue,
    ) -> usize {
        let mut removed = 0;

        // Remove skills flagged as generic
        let initial_skills = skills.len();
        skills.retain(|s| {
            let is_generic = issue.message.contains(&s.name)
                || issue.evidence.iter().any(|e| e.contains(&s.name));
            if is_generic {
                tracing::debug!(skill = %s.name, "Removing generic skill");
            }
            !is_generic
        });
        removed += initial_skills - skills.len();

        // Remove agents flagged as generic
        let initial_agents = agents.len();
        agents.retain(|a| {
            let is_generic = issue.message.contains(&a.name)
                || issue.evidence.iter().any(|e| e.contains(&a.name));
            !is_generic
        });
        removed += initial_agents - agents.len();

        // Remove rules flagged as generic
        let initial_rules = rules.len();
        rules.retain(|r| {
            let is_generic = issue.message.contains(&r.name)
                || issue.evidence.iter().any(|e| e.contains(&r.name));
            !is_generic
        });
        removed += initial_rules - rules.len();

        removed
    }

    fn enhance_actionability(
        &self,
        skills: &mut [Skill],
        agents: &mut [Agent],
        _rules: &mut [Rule],
        suggestions: &[super::quality::Suggestion],
    ) -> usize {
        let mut enhanced = 0;

        for suggestion in suggestions {
            if suggestion.action.contains("actionable") || suggestion.action.contains("specific") {
                // Find and enhance matching skills
                for skill in skills.iter_mut() {
                    if suggestion.rationale.contains(&skill.name) {
                        skill
                            .body
                            .push_str(&format!("\n\n### Action Items\n{}", suggestion.action));
                        enhanced += 1;
                    }
                }

                // Find and enhance matching agents
                for agent in agents.iter_mut() {
                    if suggestion.rationale.contains(&agent.name) {
                        agent
                            .prompt
                            .push_str(&format!("\n\n### Action Items\n{}", suggestion.action));
                        enhanced += 1;
                    }
                }
            }
        }

        enhanced
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
        let validators = Validators::new(
            &self.config,
            &self.provider,
            &self.project_root,
            &self.llm_modules,
        );
        let file_registry = match &self.file_registry {
            Some(r) => r.clone(),
            None => VerifiedFileRegistry::build(&self.project_root).await?,
        };
        let _project_context = format!("Project root: {}", self.project_root.display());
        let _claude_md_content = claude_md.to_markdown();
        let mut state = RefinementState::new();
        let mut thinking = IterationState::new(cfg.base_iterations, cfg.max_extension);

        while thinking.should_continue() {
            let iteration = thinking.iteration;
            state.decay_strategy_failures();

            // Phase 1: Tier filtering (simplified - actual filtering done by LLM Judge)
            let _tier_result = TierFilterResult::check(&skills, &agents, &rules);
            // Keep all artifacts - filtering decision moved to quality validation via LLM Judge

            // Phase 2: Run all validations
            let cv_result = self.assess_quality(&skills, &agents, &rules, claude_md, output_plan);
            let artifacts = Artifacts {
                skills: skills.clone(),
                agents: agents.clone(),
                rules: rules.clone(),
            };
            let judgment = self.judge.evaluate_artifacts(&artifacts).await?;
            state.last_judgment = Some(judgment.clone());

            let structural_result = validators
                .run_structural(&skills, &agents, &rules, claude_md)
                .await?;
            if structural_result.is_some() {
                state.last_structural_result = structural_result.clone();
            }

            // Phase 3: Aggregate feedback
            let aggregated_feedback = self.feedback_aggregator.aggregate(
                &judgment,
                structural_result.as_ref(),
                Some(&cv_result),
            );

            let surface_quality = if cv_result.passed { 0.8 } else { 0.5 };
            let quality_score = judgment.overall_score;
            let combined_quality = aggregated_feedback.overall_score;

            thinking.record_quality(combined_quality);

            // Phase 4: Rollback logic
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
                    if degradation > cfg.rollback_threshold
                        && state.rollback_count < cfg.max_rollbacks
                    {
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

            let tier_status = format!("{:?}", judgment.tier);
            let issues_count = judgment.issues.len();

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
                quality = format!("{:.1}%", quality_score * 100.0),
                combined = format!("{:.1}%", combined_quality * 100.0),
                tier = tier_status,
                issues = issues_count,
                structural = structural_status,
                target = format!("{:.1}%", cfg.target_quality * 100.0),
                "Quality assessment"
            );

            // Phase 7: Termination decision with uncertainty
            let dimensions_for_check = build_dimensions_status(
                &judgment,
                structural_result.as_ref(),
                &self.config.refinement().dimension_thresholds,
            );

            let convergence_checker =
                QualityAssessor::new(cfg.target_quality, cfg.require_all_dimensions);
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
                let oscillation_override = state.total_convergence_detections
                    > cfg.max_convergence_detections
                    && thinking.uncertainty < cfg.high_uncertainty_threshold;

                if verification_passed || oscillation_override {
                    let (convergence_path, decision_rationale) = match &termination_decision {
                        TerminationDecision::Terminate(TerminationReason::EarlyExit {
                            quality,
                            uncertainty,
                        }) => (
                            AssessmentPath::EarlyExit,
                            format!(
                                "Early exit: quality={:.1}%, uncertainty={:.2}",
                                quality * 100.0,
                                uncertainty
                            ),
                        ),
                        TerminationDecision::Terminate(TerminationReason::Converged(path)) => {
                            (*path, format!("Converged via {}", path.as_str()))
                        }
                        TerminationDecision::Terminate(TerminationReason::Satisfied) => (
                            AssessmentPath::QualityTargetMet,
                            "Satisfied all requirements".to_string(),
                        ),
                        _ => (
                            if oscillation_override {
                                AssessmentPath::OscillationSettled
                            } else {
                                AssessmentPath::QualityTargetMet
                            },
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
                                (
                                    best.skills.clone(),
                                    best.agents.clone(),
                                    best.rules.clone(),
                                    best.quality,
                                )
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
                        ValidationContext {
                            judgment: &judgment,
                            structural: structural_result.as_ref(),
                        },
                    );
                    self.persist_learning().await;

                    return Ok(RefinementResult {
                        skills: final_skills,
                        agents: final_agents,
                        rules: final_rules,
                        iterations: iteration + 1,
                        converged: true,
                        final_quality,
                        judgment: Some(judgment),
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
                let improved =
                    combined_quality > prev + refinement_cfg.min_improvement_per_iteration;

                let is_oscillating = refinement_cfg.detect_oscillation
                    && thinking.quality_trajectory.len() >= refinement_cfg.oscillation_window
                    && {
                        let window: Vec<f32> = thinking
                            .quality_trajectory
                            .iter()
                            .copied()
                            .skip(
                                thinking
                                    .quality_trajectory
                                    .len()
                                    .saturating_sub(refinement_cfg.oscillation_window),
                            )
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

            let mut issues =
                self.identify_issues_with_judgment(&cv_result, &skills, &agents, &judgment);

            // Add structural issues (missing modules, partial coverage)
            self.add_structural_issues(&mut issues, &structural_result);

            // Re-sort issues after adding structural ones
            issues.sort_by(|a, b| b.severity.cmp(&a.severity));

            // Phase 9: Check for no issues remaining
            if issues.is_empty() && iteration >= cfg.min_iterations_for_exit {
                let quality_acceptable = meets_target
                    || (!cfg.require_all_dimensions
                        && combined_quality >= cfg.target_quality * 0.9);

                if (converged || !cfg.require_all_dimensions) && quality_acceptable {
                    let no_issues_verification_passed = !cfg.post_convergence_verification
                        || state.consecutive_convergence_passes
                            >= cfg.post_convergence_passes_required;

                    if no_issues_verification_passed {
                        tracing::info!(
                            iteration = iteration + 1,
                            quality = format!("{:.1}%", combined_quality * 100.0),
                            consecutive_passes = state.consecutive_convergence_passes,
                            "No issues found, refinement complete with verification"
                        );

                        let report = self.build_success_report(
                            AssessmentPath::NoIssuesRemaining,
                            iteration + 1,
                            thinking.quality_trajectory_vec(),
                            state.critical_improvements.clone(),
                            ValidationContext {
                                judgment: &judgment,
                                structural: structural_result.as_ref(),
                            },
                        );
                        self.persist_learning().await;

                        return Ok(RefinementResult {
                            skills,
                            agents,
                            rules,
                            iterations: iteration + 1,
                            converged: meets_target,
                            final_quality: combined_quality,
                            judgment: Some(judgment),
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
                    &judgment,
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

            // Phase 10.5: Self-critique inner loop
            let critique_result = self
                .self_critique_loop(&mut skills, &mut agents, &mut rules)
                .await?;
            match &critique_result {
                CritiqueResult::Fixed {
                    iterations: crit_iters,
                    final_quality,
                } => {
                    tracing::debug!(
                        iteration = iteration + 1,
                        critique_iterations = crit_iters,
                        quality = format!("{:.1}%", final_quality * 100.0),
                        "Self-critique applied fixes"
                    );
                }
                CritiqueResult::MaxIterationsReached {
                    remaining_issues, ..
                } if *remaining_issues > 0 => {
                    tracing::debug!(
                        iteration = iteration + 1,
                        remaining_issues,
                        "Self-critique reached max iterations with remaining issues"
                    );
                }
                _ => {}
            }

            // Extract changes made from improvements (before moving iter_improvements)
            let changes_made: Vec<String> = iter_improvements
                .iter()
                .map(|imp| imp.description.clone())
                .collect();

            state.critical_improvements.extend(iter_improvements);

            // Check if iteration extension is warranted
            if thinking.iteration >= thinking.estimated_total {
                let extended_quality =
                    thinking.maybe_extend(BudgetExtensionTrigger::QualityImproving {
                        min_delta: cfg.quality_improving_delta,
                    });
                let extended_uncertainty =
                    thinking.maybe_extend(BudgetExtensionTrigger::HighUncertainty {
                        threshold: cfg.high_uncertainty_threshold,
                    });

                if extended_quality || extended_uncertainty {
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
                format!(
                    "Converged: quality={:.1}%, uncertainty={:.2}",
                    combined_quality * 100.0,
                    thinking.uncertainty
                )
            } else if thinking.uncertainty > cfg.high_uncertainty_threshold {
                format!(
                    "Continue: high uncertainty ({:.2} > {:.2})",
                    thinking.uncertainty, cfg.high_uncertainty_threshold
                )
            } else if thinking.is_quality_improving(cfg.quality_improving_delta) {
                "Continue: quality improving".to_string()
            } else {
                format!("Continue: iteration {}/{}", iteration + 1, cfg.max_total())
            };

            // Extract issues addressed in this iteration
            let issues_addressed: Vec<String> = issues
                .iter()
                .take(cfg.issues_per_iteration)
                .map(|i| format!("{}:{}", i.item_type, i.item_name))
                .collect();

            let mut thinking_record = IterationRecord::new(iteration, quality_before)
                .with_quality_after(combined_quality)
                .with_uncertainty(thinking.uncertainty)
                .with_strategies(strategies_applied)
                .with_rationale(&decision_rationale)
                .with_issues(issues_addressed)
                .with_changes(changes_made)
                .needs_continuation(should_continue);

            // Include revision info if this iteration follows a rollback
            if let Some(ref revision) = thinking.revision {
                thinking_record =
                    thinking_record.with_revision(revision.revises_iteration, &revision.reason);
                thinking.end_revision();
            }

            thinking.record(thinking_record);
        }

        // Final quality assessment after max iterations
        let final_cv = self.assess_quality(&skills, &agents, &rules, claude_md, output_plan);
        let final_artifacts = Artifacts {
            skills: skills.clone(),
            agents: agents.clone(),
            rules: rules.clone(),
        };
        let final_judgment = self.judge.evaluate_artifacts(&final_artifacts).await?;

        let final_aggregated = self.feedback_aggregator.aggregate(
            &final_judgment,
            state.last_structural_result.as_ref(),
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
                    (
                        best.skills.clone(),
                        best.agents.clone(),
                        best.rules.clone(),
                        best.quality,
                    )
                } else {
                    (skills, agents, rules, final_quality)
                }
            } else {
                (skills, agents, rules, final_quality)
            };

        tracing::warn!(
            iterations = cfg.max_total(),
            quality = format!("{:.1}%", output_quality * 100.0),
            judgment_score = format!("{:.1}%", final_judgment.overall_score * 100.0),
            tier = ?final_judgment.tier,
            issues = final_judgment.issues.len(),
            "Max iterations reached"
        );

        let dimensions_status = build_dimensions_status(
            &final_judgment,
            state.last_structural_result.as_ref(),
            &self.config.refinement().dimension_thresholds,
        );

        let remaining_issues: Vec<super::quality_assessment::RemainingIssue> = final_judgment
            .issues
            .iter()
            .map(|issue| super::quality_assessment::RemainingIssue {
                target: issue.code.clone(),
                category: "quality".to_string(),
                severity: format!("{:?}", issue.severity),
                description: issue.message.clone(),
                attempts: cfg.max_total(),
            })
            .collect();

        let report = QualityAssessment::failure(
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
            judgment: state.last_judgment.or(Some(final_judgment)),
            structural_quality: state.last_structural_result,
            aggregated_feedback: None,
            learning_summary: Some(self.learning_history.get_progress_summary()),
            convergence_report: Some(report),
        })
    }

    fn build_success_report(
        &self,
        path: AssessmentPath,
        iteration: usize,
        quality_trajectory: Vec<f32>,
        critical_improvements: Vec<Improvement>,
        validation: ValidationContext<'_>,
    ) -> QualityAssessment {
        let dimensions_status = build_dimensions_status(
            validation.judgment,
            validation.structural,
            &self.config.refinement().dimension_thresholds,
        );
        QualityAssessment::success(
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
        let empty_registry = VerifiedFileRegistry::empty();
        let registry = self.file_registry.as_ref().unwrap_or(&empty_registry);
        CrossValidationResult::check(skills, agents, rules, claude_md, registry)
    }

    fn identify_issues_with_judgment(
        &self,
        cv_result: &CrossValidationResult,
        skills: &[Skill],
        agents: &[Agent],
        judgment: &JudgmentResult,
    ) -> Vec<DetectedArtifactIssue> {
        use super::quality::IssueSeverity;

        let mut issues = Vec::new();
        let quality_cfg = self.config.quality();

        for skill in skills {
            issues.extend(self.check_skill_quality(skill, &quality_cfg.skill));
        }

        for agent in agents {
            issues.extend(self.check_agent_quality(agent, &quality_cfg.agent));
        }

        for missing in &cv_result.plan_consistency.missing_coverage {
            let (item_type, name) = parse_missing_item(missing);
            issues.push(DetectedArtifactIssue {
                item_type,
                item_name: name,
                issue: DetectedIssue::PlanMismatch,
                severity: DiagnosticLevel::Error,
            });
        }

        for quality_issue in &judgment.issues {
            let severity = match quality_issue.severity {
                IssueSeverity::Critical => DiagnosticLevel::Error,
                IssueSeverity::Major => DiagnosticLevel::Warning,
                IssueSeverity::Minor => DiagnosticLevel::Info,
            };

            let issue_kind = if quality_issue.code.contains("GENERIC")
                || quality_issue.code.contains("TIER1")
            {
                DetectedIssue::TooGeneric {
                    description: quality_issue.message.clone(),
                }
            } else if quality_issue.code.contains("EVIDENCE") || quality_issue.code.contains("REF")
            {
                DetectedIssue::WeakEvidence {
                    description: quality_issue.message.clone(),
                }
            } else if quality_issue.code.contains("REDUNDANT") {
                DetectedIssue::Redundant {
                    description: quality_issue.message.clone(),
                }
            } else if quality_issue.code.contains("SHALLOW") || quality_issue.code.contains("SHORT")
            {
                DetectedIssue::Shallow {
                    description: quality_issue.message.clone(),
                }
            } else {
                DetectedIssue::TooGeneric {
                    description: quality_issue.message.clone(),
                }
            };

            issues.push(DetectedArtifactIssue {
                item_type: ItemType::ClaudeMd,
                item_name: quality_issue.code.clone(),
                issue: issue_kind,
                severity,
            });
        }

        issues.sort_by(|a, b| b.severity.cmp(&a.severity));
        issues
    }

    fn add_structural_issues(
        &self,
        issues: &mut Vec<DetectedArtifactIssue>,
        structural_result: &Option<StructuralValidationResult>,
    ) {
        let Some(structural) = structural_result else {
            return;
        };

        // Add issues for missing modules
        for missing in &structural.coverage_report.missing_modules {
            issues.push(DetectedArtifactIssue {
                item_type: ItemType::ClaudeMd,
                item_name: format!("module:{}", missing.name),
                issue: DetectedIssue::MissingModule {
                    module_name: missing.name.clone(),
                    file_count: 0,         // ModuleCoverage doesn't track file count
                    key_files: Vec::new(), // ModuleCoverage doesn't track key files
                },
                severity: DiagnosticLevel::Error,
            });
        }

        // Add issues for partially covered modules
        for partial in &structural.coverage_report.partially_covered {
            if partial.coverage_score < 0.5 {
                issues.push(DetectedArtifactIssue {
                    item_type: ItemType::ClaudeMd,
                    item_name: format!("module:{}", partial.name),
                    issue: DetectedIssue::PartialModuleCoverage {
                        module_name: partial.name.clone(),
                        coverage: partial.coverage_score,
                    },
                    severity: DiagnosticLevel::Warning,
                });
            }
        }
    }

    /// Check skill quality - informational only, not blocking
    /// LLM judge makes final quality decisions
    fn check_skill_quality(
        &self,
        skill: &Skill,
        cfg: &crate::config::SkillQualityConfig,
    ) -> Vec<DetectedArtifactIssue> {
        let mut issues = Vec::new();

        // File references are informational hints, not requirements
        let ref_count = count_file_references(&skill.body);
        if ref_count == 0 && cfg.min_file_refs > 0 {
            issues.push(DetectedArtifactIssue {
                item_type: ItemType::Skill,
                item_name: skill.name.clone(),
                issue: DetectedIssue::MissingReferences {
                    expected: cfg.min_file_refs,
                    actual: ref_count,
                },
                severity: DiagnosticLevel::Info,
            });
        }

        issues
    }

    /// Check agent quality - informational only, not blocking
    /// LLM judge makes final quality decisions
    fn check_agent_quality(
        &self,
        _agent: &Agent,
        _cfg: &crate::config::AgentQualityConfig,
    ) -> Vec<DetectedArtifactIssue> {
        // Agent quality is evaluated by LLM judge, not programmatic checks
        // Removed: min_chars, min_sections (format-specific assumptions)
        Vec::new()
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_refinements_with_strategies(
        &mut self,
        mut skills: Vec<Skill>,
        mut agents: Vec<Agent>,
        mut rules: Vec<Rule>,
        issues: &[DetectedArtifactIssue],
        judgment: &JudgmentResult,
        aggregated_feedback: &AggregatedFeedback,
        file_registry: &VerifiedFileRegistry,
        iteration: usize,
        issues_per_iteration: usize,
        strategy_retry_limit: usize,
        strategy_failures: &mut std::collections::HashMap<String, usize>,
        combined_quality: f32, // For learning-based strategy recommendation
    ) -> Result<(
        Vec<Skill>,
        Vec<Agent>,
        Vec<Rule>,
        Vec<super::quality_assessment::Improvement>,
        Vec<String>,
    )> {
        let mut improvements = Vec::new();
        let mut strategies_used = Vec::new();
        let error_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == DiagnosticLevel::Error)
            .collect();

        for issue in error_issues {
            if matches!(issue.issue, DetectedIssue::PlanMismatch) {
                match issue.item_type {
                    ItemType::Skill => {
                        if let Some(skill) =
                            self.regenerate_skill(&issue.item_name, file_registry).await
                        {
                            skills.push(skill);
                        }
                    }
                    ItemType::Agent => {
                        if let Some(agent) =
                            self.regenerate_agent(&issue.item_name, file_registry).await
                        {
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
                    DetectedIssue::LowActionability { .. }
                        | DetectedIssue::TooGeneric { .. }
                        | DetectedIssue::WeakEvidence { .. }
                        | DetectedIssue::Shallow { .. }
                        | DetectedIssue::MissingReferences { .. }
                        | DetectedIssue::TooShort { .. }
                        | DetectedIssue::MissingSections { .. }
                        | DetectedIssue::MissingModule { .. }
                        | DetectedIssue::PartialModuleCoverage { .. }
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
            let strategy = self
                .learning_history
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
                    self.strategy_rotator
                        .select_strategy(&issue.item_name, &strategy_issue_kind)
                });

            // Build suggestions with key_files from MissingModule issues
            let mut suggestions: Vec<String> = judgment
                .suggestions
                .iter()
                .map(|s| s.action.clone())
                .collect();
            if let DetectedIssue::MissingModule { key_files, .. } = &issue.issue {
                for file in key_files {
                    suggestions.push(format!("Key file: {}", file));
                }
            }

            // Extract validation feedback from aggregated feedback for targeted refinement
            let validation_feedback = Some(super::strategy::ValidationFeedback {
                missing_modules: aggregated_feedback
                    .analysis_feedback
                    .missing_modules
                    .clone(),
                weak_coverage_areas: aggregated_feedback
                    .analysis_feedback
                    .weak_coverage_areas
                    .clone(),
                module_constraints: aggregated_feedback
                    .analysis_feedback
                    .refinement_hints
                    .clone(),
            });

            let context = StrategyContext::new(file_registry)
                .with_issues(vec![issue.issue.to_strategy_issue()])
                .with_suggestions(suggestions)
                .with_acceptance_delta(self.config.refinement().quality_acceptance_delta);
            let context = if let Some(feedback) = validation_feedback {
                context.with_validation_feedback(feedback)
            } else {
                context
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
                    StrategyAttempt {
                        strategy_name: strategy.name().to_string(),
                        success,
                        quality_delta: result.quality_delta,
                        iteration,
                    },
                );

                // Record to learning history for cross-session pattern learning
                self.learning_history.record_outcome(&LearningOutcome {
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

    async fn regenerate_skill(
        &self,
        name: &str,
        file_registry: &VerifiedFileRegistry,
    ) -> Option<Skill> {
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
                    tracing::warn!(
                        skill = name,
                        body_len = body.len(),
                        "Generated skill too short"
                    );
                    None
                }
            }
            Err(e) => {
                tracing::error!(skill = name, error = %e, "Failed to regenerate skill");
                None
            }
        }
    }

    async fn regenerate_agent(
        &self,
        name: &str,
        file_registry: &VerifiedFileRegistry,
    ) -> Option<Agent> {
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
                    tracing::warn!(
                        agent = name,
                        prompt_len = prompt_text.len(),
                        "Generated agent too short"
                    );
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

fn build_dimensions_status(
    judgment: &JudgmentResult,
    structural: Option<&StructuralValidationResult>,
    thresholds: &crate::config::DimensionThresholds,
) -> super::quality_assessment::DimensionsStatus {
    use super::quality_assessment::{DimensionScore, DimensionsStatus};

    let semantic_threshold = thresholds.semantic;

    DimensionsStatus {
        actionability: DimensionScore::new(
            "actionability",
            judgment.overall_score,
            semantic_threshold,
        ),
        specificity: DimensionScore::new(
            "specificity",
            judgment.overall_score,
            semantic_threshold * 0.8,
        ),
        evidence_quality: DimensionScore::new(
            "evidence_quality",
            judgment.overall_score,
            thresholds.evidence,
        ),
        depth: DimensionScore::new("depth", judgment.overall_score, semantic_threshold * 0.8),
        redundancy: DimensionScore::new_inverted("redundancy", 1.0 - judgment.overall_score, 0.3),
        structural_coverage: structural.map(|s| {
            DimensionScore::new(
                "structural_coverage",
                s.coverage_report.coverage,
                thresholds.surface,
            )
        }),
        cross_artifact: None,
        usability: None,
    }
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
            && prev != current_direction
        {
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
    fn test_detected_issue_to_strategy_issue() {
        let issue = DetectedIssue::LowActionability {
            score: 0.3,
            threshold: 0.6,
        };
        let strategy_issue = issue.to_strategy_issue();
        assert!(strategy_issue.message.contains("30%"));
        assert!(strategy_issue.message.contains("60%"));
        assert_eq!(strategy_issue.kind, StrategyIssueKind::LowActionability);

        let issue = DetectedIssue::TooShort {
            actual: 100,
            min: 300,
        };
        let strategy_issue = issue.to_strategy_issue();
        assert!(strategy_issue.message.contains("100"));
        assert!(strategy_issue.message.contains("300"));
        assert_eq!(strategy_issue.kind, StrategyIssueKind::TooShort);
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
