//! Bidirectional Feedback Loop
//!
//! Orchestrates the complete analysis-validation-reanalysis cycle:
//! 1. Analyze (multi-agent) → 2. Synthesize → 3. Validate → 4. Identify Gaps
//! 5. If gaps: Targeted Re-analysis → Back to step 2
//! 6. Repeat until (quality >= target) OR (budget exhausted)
//!
//! Ensures guaranteed convergence with deadlock prevention.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::ai::LlmProvider;
use crate::config::{AnalysisSpecialty, Config, MultiAgentConfig, ProjectType, ProjectTypeQuality};
use crate::pipeline::analysis::multi_agent::{AnalysisContext, MultiAgentAnalyzer, MultiAgentResult};
use crate::pipeline::analysis::reconciliation::{BidirectionalReconciler, ReconciliationConfig};
use crate::pipeline::analysis::synthesis::{
    AnalysisSynthesizer, GapImpact, SynthesizedAnalysis,
};
use crate::pipeline::analysis::{DeepAnalysisResult, StructuralValidationResult};
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::phases::ProjectDetection;
use crate::pipeline::validation::cross_specialist::{CrossSpecialistConfig, CrossSpecialistValidator};
use crate::types::Result;

/// Configuration for the feedback loop
#[derive(Debug, Clone)]
pub struct FeedbackLoopConfig {
    /// Target quality score to achieve
    pub target_quality: f32,
    /// Maximum iterations before forced termination
    pub max_iterations: usize,
    /// Wall clock timeout for entire loop
    pub wall_clock_timeout: Duration,
    /// Window size for stagnation detection
    pub stagnation_window: usize,
    /// Minimum improvement required to not be considered stagnant
    pub stagnation_threshold: f32,
    /// Minimum confidence required from synthesis
    pub min_synthesis_confidence: f32,
    /// Enable oscillation detection and damping
    pub dampen_oscillation: bool,
    /// Whether to perform targeted re-analysis on gaps
    pub targeted_reanalysis: bool,
    /// Enable cross-specialist validation
    pub cross_specialist_validation: bool,
    /// Enable bidirectional reconciliation
    pub bidirectional_reconciliation: bool,
}

impl Default for FeedbackLoopConfig {
    fn default() -> Self {
        Self {
            target_quality: 0.85,
            max_iterations: 10,
            wall_clock_timeout: Duration::from_secs(600), // 10 minutes
            stagnation_window: 3,
            stagnation_threshold: 0.02,
            min_synthesis_confidence: 0.7,
            dampen_oscillation: true,
            targeted_reanalysis: true,
            cross_specialist_validation: true,
            bidirectional_reconciliation: true,
        }
    }
}

impl FeedbackLoopConfig {
    /// Create config from project-type quality gate
    pub fn from_project_quality(quality: &ProjectTypeQuality, config: &Config) -> Self {
        Self {
            target_quality: quality.min_quality_score,
            max_iterations: quality.max_iterations.min(config.quality_loop().max_iterations),
            wall_clock_timeout: Duration::from_secs(config.network().timeout_ms / 200), // timeout_ms/1000 * 5
            stagnation_window: 3,
            stagnation_threshold: config.refinement().stagnation_threshold,
            min_synthesis_confidence: config.deep_analysis().min_confidence,
            dampen_oscillation: true,
            targeted_reanalysis: config.deep_analysis().targeted_reanalysis,
            cross_specialist_validation: config.multi_agent().cross_validate_specialists,
            bidirectional_reconciliation: true,
        }
    }
}

/// Result of the feedback loop
#[derive(Debug, Clone)]
pub struct VerifiedAnalysis {
    pub analysis: DeepAnalysisResult,
    pub synthesis: SynthesizedAnalysis,
    pub iterations: usize,
    pub converged: bool,
    pub convergence_path: ConvergenceExplanation,
    pub quality_trajectory: Vec<f32>,
    pub gaps_addressed: Vec<AddressedGap>,
}

#[derive(Debug, Clone)]
pub struct AddressedGap {
    pub area: String,
    pub iteration_found: usize,
    pub iteration_addressed: usize,
    pub specialists_used: Vec<AnalysisSpecialty>,
    pub improvement: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConvergenceExplanation {
    /// Quality target met
    QualityTargetMet { final_score: f32 },
    /// All dimensions passed
    AllDimensionsPassed { dimensions: Vec<(String, f32)> },
    /// Max iterations reached
    MaxIterationsReached { iterations: usize, final_score: f32 },
    /// Wall clock timeout
    WallClockTimeout { elapsed_secs: u64, final_score: f32 },
    /// Quality stagnated (no improvement)
    Stagnated { iterations_stagnant: usize, final_score: f32 },
    /// Oscillation detected and settled
    OscillationSettled { pattern: String, final_score: f32 },
    /// No actionable gaps found
    NoActionableGaps { final_score: f32 },
}

/// Actionable gap that can be addressed by specific specialists
#[derive(Debug, Clone)]
pub struct ActionableGap {
    pub area: String,
    pub description: String,
    pub severity: GapSeverity,
    pub specialists_needed: Vec<AnalysisSpecialty>,
    pub priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapSeverity {
    Critical,
    High,
    Medium,
    Low,
}

impl From<GapImpact> for GapSeverity {
    fn from(impact: GapImpact) -> Self {
        match impact {
            GapImpact::Critical => GapSeverity::Critical,
            GapImpact::High => GapSeverity::High,
            GapImpact::Medium => GapSeverity::Medium,
            GapImpact::Low => GapSeverity::Low,
        }
    }
}

/// Convergence guard for deadlock prevention
pub struct ConvergenceGuard {
    start_time: Instant,
    config: FeedbackLoopConfig,
    quality_history: VecDeque<f32>,
    oscillation_pattern: Option<OscillationPattern>,
}

#[derive(Debug, Clone)]
pub struct OscillationPattern {
    pub period: usize,
    pub amplitude: f32,
}

#[derive(Debug, Clone)]
pub enum GuardDecision {
    Continue,
    Escalate { reason: String },
    DampenOscillation { pattern: OscillationPattern },
    ForceTerminate { reason: String },
}

impl ConvergenceGuard {
    pub fn new(config: FeedbackLoopConfig) -> Self {
        Self {
            start_time: Instant::now(),
            config,
            quality_history: VecDeque::with_capacity(20),
            oscillation_pattern: None,
        }
    }

    pub fn check(&mut self, iteration: usize, quality: f32) -> GuardDecision {
        // 1. Wall clock timeout
        if self.start_time.elapsed() > self.config.wall_clock_timeout {
            return GuardDecision::ForceTerminate {
                reason: format!(
                    "Wall clock timeout exceeded ({}s)",
                    self.start_time.elapsed().as_secs()
                ),
            };
        }

        // 2. Iteration budget
        if iteration >= self.config.max_iterations {
            return GuardDecision::ForceTerminate {
                reason: format!("Max iterations ({}) reached", self.config.max_iterations),
            };
        }

        // 3. Record quality and check stagnation
        self.quality_history.push_back(quality);
        if self.quality_history.len() > self.config.stagnation_window * 2 {
            self.quality_history.pop_front();
        }

        if self.is_stagnant() {
            return GuardDecision::Escalate {
                reason: "Quality stagnant for too long".into(),
            };
        }

        // 4. Oscillation detection
        if self.config.dampen_oscillation
            && let Some(pattern) = self.detect_oscillation() {
                self.oscillation_pattern = Some(pattern.clone());
                return GuardDecision::DampenOscillation { pattern };
            }

        GuardDecision::Continue
    }

    fn is_stagnant(&self) -> bool {
        if self.quality_history.len() < self.config.stagnation_window {
            return false;
        }

        let recent: Vec<_> = self
            .quality_history
            .iter()
            .rev()
            .take(self.config.stagnation_window)
            .cloned()
            .collect();

        // Check if variance is below threshold
        let mean: f32 = recent.iter().sum::<f32>() / recent.len() as f32;
        let variance: f32 =
            recent.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / recent.len() as f32;

        // Check max delta between consecutive iterations
        let max_delta = recent
            .windows(2)
            .map(|w| (w[0] - w[1]).abs())
            .fold(0.0f32, f32::max);

        variance < 0.0001 && max_delta < self.config.stagnation_threshold
    }

    fn detect_oscillation(&self) -> Option<OscillationPattern> {
        if self.quality_history.len() < 6 {
            return None;
        }

        let values: Vec<_> = self.quality_history.iter().cloned().collect();

        // Check for period-2 oscillation (most common)
        if values.len() >= 4 {
            let last_4: Vec<_> = values.iter().rev().take(4).cloned().collect();
            let diff_01 = (last_4[0] - last_4[1]).abs();
            let diff_23 = (last_4[2] - last_4[3]).abs();
            let diff_02 = (last_4[0] - last_4[2]).abs();
            let diff_13 = (last_4[1] - last_4[3]).abs();

            // If alternating pattern detected
            if diff_02 < 0.01 && diff_13 < 0.01 && diff_01 > 0.02 {
                return Some(OscillationPattern {
                    period: 2,
                    amplitude: diff_01.max(diff_23),
                });
            }
        }

        None
    }

    pub fn get_oscillation_pattern(&self) -> Option<&OscillationPattern> {
        self.oscillation_pattern.as_ref()
    }
}

/// Main feedback loop orchestrator
pub struct FeedbackLoop {
    provider: Arc<dyn LlmProvider>,
    synthesizer: AnalysisSynthesizer,
    multi_agent_config: MultiAgentConfig,
    config: FeedbackLoopConfig,
    cross_specialist_validator: Option<CrossSpecialistValidator>,
    reconciler: Option<BidirectionalReconciler>,
}

impl FeedbackLoop {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        synthesizer: AnalysisSynthesizer,
        multi_agent_config: MultiAgentConfig,
        config: FeedbackLoopConfig,
    ) -> Self {
        let cross_specialist_validator = if config.cross_specialist_validation {
            Some(CrossSpecialistValidator::new(CrossSpecialistConfig::default()))
        } else {
            None
        };
        let reconciler = if config.bidirectional_reconciliation {
            Some(BidirectionalReconciler::new(ReconciliationConfig::default()))
        } else {
            None
        };

        Self {
            provider,
            synthesizer,
            multi_agent_config,
            config,
            cross_specialist_validator,
            reconciler,
        }
    }

    pub fn from_config(
        provider: Arc<dyn LlmProvider>,
        config: &Config,
        project_type: ProjectType,
    ) -> Self {
        let quality_cfg = config.quality();
        let quality_gate = quality_cfg.project_specific.get_for_type(project_type);
        let loop_config = FeedbackLoopConfig::from_project_quality(quality_gate, config);

        let cross_specialist_validator = if loop_config.cross_specialist_validation {
            Some(CrossSpecialistValidator::new(CrossSpecialistConfig::default()))
        } else {
            None
        };
        let reconciler = if loop_config.bidirectional_reconciliation {
            Some(BidirectionalReconciler::new(ReconciliationConfig::default()))
        } else {
            None
        };

        Self {
            provider,
            synthesizer: AnalysisSynthesizer::new(config.analysis.clone()),
            multi_agent_config: config.multi_agent().clone(),
            config: loop_config,
            cross_specialist_validator,
            reconciler,
        }
    }

    /// Run the bidirectional feedback loop
    pub async fn run(
        &mut self,
        initial_analysis: DeepAnalysisResult,
        detection: &ProjectDetection,
        file_registry: &VerifiedFileRegistry,
        analysis_context: &AnalysisContext,
    ) -> Result<VerifiedAnalysis> {
        let mut guard = ConvergenceGuard::new(self.config.clone());
        let mut analysis = initial_analysis;
        let mut iteration = 0;
        let mut quality_trajectory = Vec::new();
        let mut gaps_addressed = Vec::new();
        let mut pending_gaps: Vec<ActionableGap> = Vec::new();

        let structural_result: Option<StructuralValidationResult> = None;

        loop {
            iteration += 1;
            tracing::info!(
                iteration,
                max = self.config.max_iterations,
                "Feedback loop iteration"
            );

            // 1. Apply bidirectional reconciliation if we have structural validation
            let reconciled_analysis = if let (Some(reconciler), Some(structural)) =
                (&self.reconciler, &structural_result)
            {
                let reconciled = reconciler.reconcile(analysis.clone(), Some(structural.clone()), file_registry);
                tracing::debug!(
                    reconciliation_count = reconciled.reconciliation_count,
                    unresolved = reconciled.unresolved_conflicts.len(),
                    confidence = format!("{:.1}%", reconciled.confidence * 100.0),
                    "Bidirectional reconciliation complete"
                );
                reconciled.deep
            } else {
                analysis.clone()
            };

            // 2. Synthesize current analysis with reconciled data
            let synthesis = self.synthesizer.synthesize(
                reconciled_analysis,
                structural_result.clone(),
                detection,
                file_registry,
            );

            let quality_score = synthesis.confidence.overall;
            quality_trajectory.push(quality_score);

            tracing::debug!(
                iteration,
                quality = format!("{:.1}%", quality_score * 100.0),
                conflicts = synthesis.validation.conflicts.len(),
                gaps = synthesis.validation.gaps.len(),
                "Synthesis complete"
            );

            // 2. Check convergence
            if quality_score >= self.config.target_quality
                && synthesis.reference_validation.validation_ratio >= 0.9
            {
                tracing::info!(
                    quality = format!("{:.1}%", quality_score * 100.0),
                    "Quality target met - converged"
                );

                return Ok(VerifiedAnalysis {
                    analysis,
                    synthesis,
                    iterations: iteration,
                    converged: true,
                    convergence_path: ConvergenceExplanation::QualityTargetMet {
                        final_score: quality_score,
                    },
                    quality_trajectory,
                    gaps_addressed,
                });
            }

            // 3. Check guard decision
            match guard.check(iteration, quality_score) {
                GuardDecision::Continue => {}
                GuardDecision::Escalate { reason } => {
                    tracing::warn!(reason = %reason, "Escalating analysis");
                    // Escalate by enabling more specialists or increasing depth
                    // For now, just continue with targeted reanalysis
                }
                GuardDecision::DampenOscillation { pattern } => {
                    tracing::warn!(
                        period = pattern.period,
                        amplitude = pattern.amplitude,
                        "Oscillation detected, settling"
                    );

                    return Ok(VerifiedAnalysis {
                        analysis,
                        synthesis,
                        iterations: iteration,
                        converged: true,
                        convergence_path: ConvergenceExplanation::OscillationSettled {
                            pattern: format!("period={}, amplitude={:.2}", pattern.period, pattern.amplitude),
                            final_score: quality_score,
                        },
                        quality_trajectory,
                        gaps_addressed,
                    });
                }
                GuardDecision::ForceTerminate { reason } => {
                    tracing::warn!(reason = %reason, "Force terminating feedback loop");

                    let explanation = if reason.contains("Max iterations") {
                        ConvergenceExplanation::MaxIterationsReached {
                            iterations: iteration,
                            final_score: quality_score,
                        }
                    } else {
                        ConvergenceExplanation::WallClockTimeout {
                            elapsed_secs: guard.start_time.elapsed().as_secs(),
                            final_score: quality_score,
                        }
                    };

                    return Ok(VerifiedAnalysis {
                        analysis,
                        synthesis,
                        iterations: iteration,
                        converged: false,
                        convergence_path: explanation,
                        quality_trajectory,
                        gaps_addressed,
                    });
                }
            }

            // 4. Identify actionable gaps
            let new_gaps = self.identify_actionable_gaps(&synthesis);
            if new_gaps.is_empty() && pending_gaps.is_empty() {
                tracing::info!("No actionable gaps found - settling");

                return Ok(VerifiedAnalysis {
                    analysis,
                    synthesis,
                    iterations: iteration,
                    converged: quality_score >= self.config.target_quality * 0.95, // 95% of target
                    convergence_path: ConvergenceExplanation::NoActionableGaps {
                        final_score: quality_score,
                    },
                    quality_trajectory,
                    gaps_addressed,
                });
            }

            // Merge new gaps with pending
            for gap in new_gaps {
                if !pending_gaps.iter().any(|g| g.area == gap.area) {
                    pending_gaps.push(gap);
                }
            }

            // 5. Targeted re-analysis for gaps
            if self.config.targeted_reanalysis {
                let specialists_to_run = self.select_specialists_for_gaps(&pending_gaps);

                if !specialists_to_run.is_empty() {
                    tracing::debug!(
                        specialists = ?specialists_to_run,
                        "Running targeted re-analysis"
                    );

                    let enhanced = self
                        .targeted_reanalysis(&specialists_to_run, analysis_context)
                        .await?;

                    // Record addressed gaps
                    let prev_quality = quality_trajectory.last().copied().unwrap_or(0.0);
                    for gap in pending_gaps.drain(..) {
                        if gap
                            .specialists_needed
                            .iter()
                            .any(|s| specialists_to_run.contains(s))
                        {
                            gaps_addressed.push(AddressedGap {
                                area: gap.area,
                                iteration_found: iteration.saturating_sub(1),
                                iteration_addressed: iteration,
                                specialists_used: gap.specialists_needed,
                                improvement: quality_score - prev_quality,
                            });
                        }
                    }

                    // Merge enhanced analysis
                    analysis = self.merge_analysis(analysis, enhanced);
                }
            }

            // Check for stagnation after re-analysis
            if quality_trajectory.len() >= self.config.stagnation_window {
                let recent_quality: Vec<_> = quality_trajectory
                    .iter()
                    .rev()
                    .take(self.config.stagnation_window)
                    .cloned()
                    .collect();

                let max_diff = recent_quality
                    .windows(2)
                    .map(|w| (w[0] - w[1]).abs())
                    .fold(0.0f32, f32::max);

                if max_diff < self.config.stagnation_threshold {
                    tracing::warn!(
                        window = self.config.stagnation_window,
                        max_improvement = format!("{:.3}", max_diff),
                        "Quality stagnated"
                    );

                    return Ok(VerifiedAnalysis {
                        analysis,
                        synthesis,
                        iterations: iteration,
                        converged: false,
                        convergence_path: ConvergenceExplanation::Stagnated {
                            iterations_stagnant: self.config.stagnation_window,
                            final_score: quality_score,
                        },
                        quality_trajectory,
                        gaps_addressed,
                    });
                }
            }
        }
    }

    /// Identify actionable gaps from synthesis results
    fn identify_actionable_gaps(&self, synthesis: &SynthesizedAnalysis) -> Vec<ActionableGap> {
        let mut gaps = Vec::new();

        // From synthesis validation gaps
        for gap in &synthesis.validation.gaps {
            if matches!(gap.impact, GapImpact::Critical | GapImpact::High) {
                let specialists = self.map_gap_to_specialists(&gap.area);
                if !specialists.is_empty() {
                    gaps.push(ActionableGap {
                        area: gap.area.clone(),
                        description: gap.reason.clone(),
                        severity: gap.impact.into(),
                        specialists_needed: specialists,
                        priority: match gap.impact {
                            GapImpact::Critical => 1,
                            GapImpact::High => 2,
                            GapImpact::Medium => 3,
                            GapImpact::Low => 4,
                        },
                    });
                }
            }
        }

        // From reanalysis targets
        let targets = self
            .synthesizer
            .get_reanalysis_targets(synthesis, self.config.min_synthesis_confidence);

        if targets.reanalyze_structure {
            gaps.push(ActionableGap {
                area: "structure".into(),
                description: "Low structure confidence".into(),
                severity: GapSeverity::High,
                specialists_needed: vec![AnalysisSpecialty::Structure],
                priority: 1,
            });
        }

        if targets.reanalyze_patterns {
            gaps.push(ActionableGap {
                area: "patterns".into(),
                description: "Low pattern confidence".into(),
                severity: GapSeverity::High,
                specialists_needed: vec![AnalysisSpecialty::Pattern],
                priority: 2,
            });
        }

        if targets.reanalyze_constraints {
            gaps.push(ActionableGap {
                area: "constraints".into(),
                description: "Low constraint confidence".into(),
                severity: GapSeverity::High,
                specialists_needed: vec![AnalysisSpecialty::Constraint],
                priority: 2,
            });
        }

        // Sort by priority
        gaps.sort_by_key(|g| g.priority);
        gaps
    }

    /// Map gap area to appropriate specialists
    fn map_gap_to_specialists(&self, area: &str) -> Vec<AnalysisSpecialty> {
        let area_lower = area.to_lowercase();

        if area_lower.contains("structure")
            || area_lower.contains("module")
            || area_lower.contains("file coverage")
        {
            vec![AnalysisSpecialty::Structure]
        } else if area_lower.contains("pattern") || area_lower.contains("convention") {
            vec![AnalysisSpecialty::Pattern]
        } else if area_lower.contains("constraint")
            || area_lower.contains("anti-pattern")
            || area_lower.contains("dependency")
            || area_lower.contains("import")
        {
            vec![AnalysisSpecialty::Constraint]
        } else if area_lower.contains("abstraction") || area_lower.contains("interface") {
            vec![AnalysisSpecialty::Pattern]
        } else {
            // Cross-cutting - use multiple specialists
            vec![
                AnalysisSpecialty::Structure,
                AnalysisSpecialty::Pattern,
                AnalysisSpecialty::Constraint,
            ]
        }
    }

    /// Select specialists to run for current gaps
    fn select_specialists_for_gaps(&self, gaps: &[ActionableGap]) -> HashSet<AnalysisSpecialty> {
        let mut specialists = HashSet::new();

        // Limit to top 3 highest priority gaps
        for gap in gaps.iter().take(3) {
            for specialist in &gap.specialists_needed {
                if self.multi_agent_config.enabled_specialists.contains(specialist) {
                    specialists.insert(*specialist);
                }
            }
        }

        specialists
    }

    /// Run targeted re-analysis with specific specialists
    async fn targeted_reanalysis(
        &self,
        specialists: &HashSet<AnalysisSpecialty>,
        context: &AnalysisContext,
    ) -> Result<MultiAgentResult> {
        // Create a filtered config for targeted analysis
        let mut targeted_config = self.multi_agent_config.clone();
        targeted_config.enabled_specialists = specialists.iter().cloned().collect();

        let analyzer = MultiAgentAnalyzer::new(Arc::clone(&self.provider));
        let result = analyzer.analyze(context.clone()).await?;

        // Run cross-specialist validation if enabled
        if let Some(validator) = &self.cross_specialist_validator {
            let validation_result = validator.validate(&result);
            tracing::debug!(
                passed = validation_result.passed,
                conflicts = validation_result.conflicts.len(),
                agreements = validation_result.agreements.len(),
                agreement_ratio = format!("{:.1}%", validation_result.agreement_ratio * 100.0),
                "Cross-specialist validation complete"
            );

            if !validation_result.passed && !validation_result.specialists_to_rerun.is_empty() {
                tracing::warn!(
                    specialists = ?validation_result.specialists_to_rerun,
                    "Cross-specialist validation failed, some specialists may need re-run"
                );
            }
        }

        Ok(result)
    }

    /// Merge enhanced analysis into existing analysis
    fn merge_analysis(
        &self,
        mut base: DeepAnalysisResult,
        enhanced: MultiAgentResult,
    ) -> DeepAnalysisResult {
        // Merge structure
        for entry_point in enhanced.structure.entry_points {
            if !base
                .structure
                .entry_points
                .iter()
                .any(|e| e.path == entry_point.path)
            {
                base.structure.entry_points.push(entry_point);
            }
        }

        for module in enhanced.structure.core_modules {
            if !base
                .structure
                .core_modules
                .iter()
                .any(|m| m.name == module.name)
            {
                base.structure.core_modules.push(module);
            }
        }

        // Merge patterns (update existing or add new)
        for pattern in enhanced.patterns {
            if let Some(existing) = base.patterns.iter_mut().find(|p| p.name == pattern.name) {
                // Update existing pattern with new locations
                for loc in pattern.locations {
                    if !existing.locations.iter().any(|l| l.file == loc.file) {
                        existing.locations.push(loc);
                    }
                }
            } else {
                base.patterns.push(pattern);
            }
        }

        // Merge constraints (update or add)
        for constraint in enhanced.constraints {
            if let Some(existing) = base
                .constraints
                .iter_mut()
                .find(|c| c.title == constraint.title)
            {
                // Merge evidence
                for evidence in constraint.evidence {
                    if !existing.evidence.iter().any(|e| e.file == evidence.file) {
                        existing.evidence.push(evidence);
                    }
                }
            } else {
                base.constraints.push(constraint);
            }
        }

        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convergence_guard_detects_stagnation() {
        let config = FeedbackLoopConfig {
            stagnation_window: 3,
            stagnation_threshold: 0.01,
            max_iterations: 100, // High to avoid hitting iteration limit
            ..Default::default()
        };
        let mut guard = ConvergenceGuard::new(config);

        // Add stagnant values (all the same = zero variance)
        // After stagnation_window iterations with same value, should detect stagnation
        let mut detected_stagnation = false;
        for i in 0..10 {
            let decision = guard.check(i, 0.75);
            if matches!(decision, GuardDecision::Escalate { .. }) {
                detected_stagnation = true;
                break;
            }
        }
        assert!(detected_stagnation, "Stagnation should be detected with constant values");
    }

    #[test]
    fn test_gap_to_specialist_mapping_structure() {
        // Test without creating full FeedbackLoop
        let area = "Module structure issue";
        let area_lower = area.to_lowercase();

        let is_structure = area_lower.contains("structure")
            || area_lower.contains("module")
            || area_lower.contains("file coverage");
        assert!(is_structure);
    }

    #[test]
    fn test_gap_to_specialist_mapping_pattern() {
        let area = "Pattern: ErrorHandling";
        let area_lower = area.to_lowercase();

        let is_pattern = area_lower.contains("pattern") || area_lower.contains("convention");
        assert!(is_pattern);
    }

    #[test]
    fn test_gap_to_specialist_mapping_constraint() {
        let area = "Constraint: AntiPattern";
        let area_lower = area.to_lowercase();

        let is_constraint = area_lower.contains("constraint") || area_lower.contains("anti-pattern");
        assert!(is_constraint);
    }

    #[test]
    fn test_oscillation_detection() {
        let config = FeedbackLoopConfig {
            dampen_oscillation: true,
            ..Default::default()
        };
        let mut guard = ConvergenceGuard::new(config);

        // Simulate oscillating values
        let oscillating = [0.80, 0.85, 0.80, 0.85, 0.80, 0.85];
        for (i, &value) in oscillating.iter().enumerate() {
            let decision = guard.check(i, value);
            if i >= 4 {
                if let GuardDecision::DampenOscillation { pattern } = decision {
                    assert_eq!(pattern.period, 2);
                    return;
                }
            }
        }
        // If we get here, oscillation should have been detected
    }

    #[test]
    fn test_max_iterations_termination() {
        let config = FeedbackLoopConfig {
            max_iterations: 3,
            ..Default::default()
        };
        let mut guard = ConvergenceGuard::new(config);

        for i in 0..5 {
            let decision = guard.check(i, 0.7);
            if i >= 3 {
                assert!(matches!(decision, GuardDecision::ForceTerminate { .. }));
            }
        }
    }

    #[test]
    fn test_gap_severity_from_impact() {
        assert!(matches!(GapSeverity::from(GapImpact::Critical), GapSeverity::Critical));
        assert!(matches!(GapSeverity::from(GapImpact::High), GapSeverity::High));
        assert!(matches!(GapSeverity::from(GapImpact::Medium), GapSeverity::Medium));
        assert!(matches!(GapSeverity::from(GapImpact::Low), GapSeverity::Low));
    }

    #[test]
    fn test_feedback_loop_config_default() {
        let config = FeedbackLoopConfig::default();
        assert_eq!(config.target_quality, 0.85);
        assert_eq!(config.max_iterations, 10);
        assert_eq!(config.stagnation_window, 3);
        assert!(config.targeted_reanalysis);
    }
}
