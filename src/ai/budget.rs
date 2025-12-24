//! Global Token Budget Management
//!
//! Thread-safe token budget tracking across the entire documentation pipeline.
//! Implements hard limits to prevent budget overruns with configurable enforcement.
//!
//! ## Design Principles
//!
//! - **Atomic operations**: Safe for concurrent access from multiple phases
//! - **Phase-aware limits**: Each phase has its own allocation within global budget
//! - **Hard limit enforcement**: Phase budgets are enforced, not just warnings
//! - **Reservation system**: Pre-allocate budget for predictable consumption
//! - **Graceful degradation**: Configurable behavior when limits are reached

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::constants::budget as budget_constants;
use crate::types::{Result, WeaveError};

// =============================================================================
// Error Types
// =============================================================================

/// Budget-specific error type with detailed context
#[derive(Debug, Clone)]
pub enum BudgetError {
    /// Global budget exceeded
    GlobalExceeded {
        consumed: u64,
        budget: u64,
        requested: u64,
    },
    /// Phase-specific budget exceeded
    PhaseExceeded {
        phase: u8,
        phase_name: &'static str,
        consumed: u64,
        limit: u64,
        requested: u64,
    },
    /// Invalid phase number
    InvalidPhase { phase: u8 },
    /// Reservation failed (not enough budget)
    ReservationFailed {
        phase: u8,
        requested: u64,
        available: u64,
    },
}

impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GlobalExceeded {
                consumed,
                budget,
                requested,
            } => {
                write!(
                    f,
                    "Global token budget exceeded: {consumed}/{budget} tokens (requested: {requested})"
                )
            }
            Self::PhaseExceeded {
                phase,
                phase_name,
                consumed,
                limit,
                requested,
            } => {
                write!(
                    f,
                    "Phase {phase} ({phase_name}) budget exceeded: {consumed}/{limit} tokens (requested: {requested})"
                )
            }
            Self::InvalidPhase { phase } => {
                write!(f, "Invalid phase number: {phase} (valid: 0-4)")
            }
            Self::ReservationFailed {
                phase,
                requested,
                available,
            } => {
                write!(
                    f,
                    "Budget reservation failed for phase {phase}: requested {requested}, available {available}"
                )
            }
        }
    }
}

impl std::error::Error for BudgetError {}

// =============================================================================
// Budget Enforcement Mode
// =============================================================================

/// How phase budget limits are enforced
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BudgetEnforcement {
    /// Soft limit: warn when exceeded but allow using global budget
    #[default]
    Soft,
    /// Hard limit: return error when phase budget is exceeded
    Hard,
}

impl std::fmt::Display for BudgetEnforcement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Soft => write!(f, "soft"),
            Self::Hard => write!(f, "hard"),
        }
    }
}

// =============================================================================
// Phase Names
// =============================================================================

/// Phase number to name mapping
const PHASE_NAMES: [&str; 5] = [
    "Characterization",
    "BottomUp",
    "TopDown",
    "Consolidation",
    "Refinement",
];

/// Get phase name from number
fn phase_name(phase: u8) -> &'static str {
    PHASE_NAMES
        .get(phase as usize)
        .copied()
        .unwrap_or("Unknown")
}

// =============================================================================
// Configuration
// =============================================================================

/// TALE (Token-Budget-Aware Learning and Estimation) Configuration
#[derive(Debug, Clone)]
pub struct TaleConfig {
    /// Total token budget for the session
    pub total_budget: u64,
    /// Phase allocation percentages (must sum to 100)
    pub phase_allocations: PhaseAllocations,
    /// Warning threshold (0.0-1.0) - emit warning when exceeded
    pub warning_threshold: f64,
    /// Critical threshold (0.0-1.0) - start degradation when exceeded
    pub critical_threshold: f64,
    /// Enable adaptive reallocation when phases complete early
    pub adaptive_reallocation: bool,
    /// Reserve buffer percentage for retries and repairs
    pub reserve_buffer_pct: f64,
    /// Budget enforcement mode
    pub enforcement: BudgetEnforcement,
}

impl Default for TaleConfig {
    fn default() -> Self {
        Self {
            total_budget: budget_constants::DEFAULT_BUDGET,
            phase_allocations: PhaseAllocations::default(),
            warning_threshold: budget_constants::WARNING_THRESHOLD,
            critical_threshold: budget_constants::CRITICAL_THRESHOLD,
            adaptive_reallocation: true,
            reserve_buffer_pct: budget_constants::RESERVE_BUFFER_PCT,
            enforcement: BudgetEnforcement::Hard, // Default to hard limit
        }
    }
}

impl TaleConfig {
    /// Create a new TALE config with specified budget
    pub fn with_budget(total_budget: u64) -> Self {
        Self {
            total_budget,
            ..Default::default()
        }
    }

    /// Create config with soft enforcement (legacy behavior)
    pub fn soft(total_budget: u64) -> Self {
        Self {
            total_budget,
            enforcement: BudgetEnforcement::Soft,
            ..Default::default()
        }
    }

    /// Create a strict config for cost-sensitive usage
    pub fn strict() -> Self {
        Self {
            total_budget: 500_000,
            warning_threshold: 0.60,
            critical_threshold: 0.80,
            adaptive_reallocation: true,
            reserve_buffer_pct: 0.10,
            enforcement: BudgetEnforcement::Hard,
            ..Default::default()
        }
    }

    /// Create a generous config for comprehensive documentation
    pub fn generous() -> Self {
        Self {
            total_budget: 2_000_000,
            warning_threshold: 0.85,
            critical_threshold: 0.95,
            adaptive_reallocation: true,
            reserve_buffer_pct: 0.03,
            enforcement: BudgetEnforcement::Hard,
            ..Default::default()
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        self.phase_allocations.validate()?;

        if self.warning_threshold <= 0.0 || self.warning_threshold >= 1.0 {
            return Err(WeaveError::Config(format!(
                "Warning threshold must be between 0.0 and 1.0, got {}",
                self.warning_threshold
            )));
        }

        if self.critical_threshold <= self.warning_threshold || self.critical_threshold >= 1.0 {
            return Err(WeaveError::Config(format!(
                "Critical threshold must be between warning threshold ({}) and 1.0, got {}",
                self.warning_threshold, self.critical_threshold
            )));
        }

        if self.reserve_buffer_pct < 0.0 || self.reserve_buffer_pct >= 0.5 {
            return Err(WeaveError::Config(format!(
                "Reserve buffer must be between 0.0 and 0.5, got {}",
                self.reserve_buffer_pct
            )));
        }

        Ok(())
    }

    /// Create phase limits from this config
    pub fn to_phase_limits(&self) -> PhaseLimits {
        let effective_budget = (self.total_budget as f64 * (1.0 - self.reserve_buffer_pct)) as u64;
        self.phase_allocations.to_limits(effective_budget)
    }
}

// =============================================================================
// Phase Allocations
// =============================================================================

/// Phase allocation percentages
#[derive(Debug, Clone)]
pub struct PhaseAllocations {
    pub characterization_pct: u8,
    pub bottom_up_pct: u8,
    pub top_down_pct: u8,
    pub consolidation_pct: u8,
    pub refinement_pct: u8,
}

impl Default for PhaseAllocations {
    fn default() -> Self {
        use crate::constants::budget::phase;
        Self {
            characterization_pct: phase::CHARACTERIZATION_PCT,
            bottom_up_pct: phase::BOTTOM_UP_PCT,
            top_down_pct: phase::TOP_DOWN_PCT,
            consolidation_pct: phase::CONSOLIDATION_PCT,
            refinement_pct: phase::REFINEMENT_PCT,
        }
    }
}

impl PhaseAllocations {
    /// Validate that allocations sum to 100
    pub fn validate(&self) -> Result<()> {
        let total = self.characterization_pct as u16
            + self.bottom_up_pct as u16
            + self.top_down_pct as u16
            + self.consolidation_pct as u16
            + self.refinement_pct as u16;

        if total != 100 {
            return Err(WeaveError::Config(format!(
                "Phase allocations must sum to 100, got {total}"
            )));
        }
        Ok(())
    }

    /// Convert percentages to absolute limits
    pub fn to_limits(&self, total_budget: u64) -> PhaseLimits {
        PhaseLimits::new(
            total_budget * self.characterization_pct as u64 / 100,
            total_budget * self.bottom_up_pct as u64 / 100,
            total_budget * self.top_down_pct as u64 / 100,
            total_budget * self.consolidation_pct as u64 / 100,
            total_budget * self.refinement_pct as u64 / 100,
        )
    }

    /// Create allocations optimized for large codebases
    pub fn for_large_codebase() -> Self {
        Self {
            characterization_pct: 3,
            bottom_up_pct: 60,
            top_down_pct: 8,
            consolidation_pct: 17,
            refinement_pct: 12,
        }
    }

    /// Create allocations optimized for small projects
    pub fn for_small_project() -> Self {
        Self {
            characterization_pct: 8,
            bottom_up_pct: 40,
            top_down_pct: 15,
            consolidation_pct: 20,
            refinement_pct: 17,
        }
    }
}

// =============================================================================
// Phase Limits
// =============================================================================

/// Phase-specific token allocations with atomic support for reallocation
#[derive(Debug)]
pub struct PhaseLimits {
    characterization: AtomicU64,
    bottom_up: AtomicU64,
    top_down: AtomicU64,
    consolidation: AtomicU64,
    refinement: AtomicU64,
}

impl Clone for PhaseLimits {
    fn clone(&self) -> Self {
        Self {
            characterization: AtomicU64::new(self.characterization.load(Ordering::Relaxed)),
            bottom_up: AtomicU64::new(self.bottom_up.load(Ordering::Relaxed)),
            top_down: AtomicU64::new(self.top_down.load(Ordering::Relaxed)),
            consolidation: AtomicU64::new(self.consolidation.load(Ordering::Relaxed)),
            refinement: AtomicU64::new(self.refinement.load(Ordering::Relaxed)),
        }
    }
}

impl PhaseLimits {
    /// Create phase limits from individual values
    pub fn new(
        characterization: u64,
        bottom_up: u64,
        top_down: u64,
        consolidation: u64,
        refinement: u64,
    ) -> Self {
        Self {
            characterization: AtomicU64::new(characterization),
            bottom_up: AtomicU64::new(bottom_up),
            top_down: AtomicU64::new(top_down),
            consolidation: AtomicU64::new(consolidation),
            refinement: AtomicU64::new(refinement),
        }
    }

    /// Create phase limits for a given total budget
    pub fn from_total(total: u64) -> Self {
        Self::new(
            total * 5 / 100,
            total * 50 / 100,
            total * 10 / 100,
            total * 20 / 100,
            total * 15 / 100,
        )
    }

    /// Get limit for a specific phase number (0-4)
    pub fn for_phase(&self, phase: u8) -> u64 {
        match phase {
            0 => self.characterization.load(Ordering::Relaxed),
            1 => self.bottom_up.load(Ordering::Relaxed),
            2 => self.top_down.load(Ordering::Relaxed),
            3 => self.consolidation.load(Ordering::Relaxed),
            4 => self.refinement.load(Ordering::Relaxed),
            _ => 0,
        }
    }

    /// Get mutable reference to phase limit atomic
    fn phase_limit_atomic(&self, phase: u8) -> Option<&AtomicU64> {
        match phase {
            0 => Some(&self.characterization),
            1 => Some(&self.bottom_up),
            2 => Some(&self.top_down),
            3 => Some(&self.consolidation),
            4 => Some(&self.refinement),
            _ => None,
        }
    }

    /// Add tokens to a phase limit (for reallocation)
    pub fn add_to_phase(&self, phase: u8, tokens: u64) -> Result<u64> {
        if let Some(limit) = self.phase_limit_atomic(phase) {
            let new_value = limit.fetch_add(tokens, Ordering::SeqCst) + tokens;
            Ok(new_value)
        } else {
            Err(BudgetError::InvalidPhase { phase }.into())
        }
    }

    /// Total of all phase limits
    pub fn total(&self) -> u64 {
        self.characterization.load(Ordering::Relaxed)
            + self.bottom_up.load(Ordering::Relaxed)
            + self.top_down.load(Ordering::Relaxed)
            + self.consolidation.load(Ordering::Relaxed)
            + self.refinement.load(Ordering::Relaxed)
    }
}

impl Default for PhaseLimits {
    fn default() -> Self {
        Self::from_total(budget_constants::DEFAULT_BUDGET)
    }
}

// =============================================================================
// Global Token Budget
// =============================================================================

/// Number of pipeline phases (0-4)
const PHASE_COUNT: usize = 5;

/// Thread-safe global token budget with hard limit enforcement
#[derive(Debug)]
pub struct GlobalTokenBudget {
    /// Total budget for the session
    total_budget: u64,
    /// Consumed tokens (atomic for thread-safety)
    consumed: AtomicU64,
    /// Per-phase consumption tracking
    phase_consumed: [AtomicU64; PHASE_COUNT],
    /// Phase limits
    phase_limits: PhaseLimits,
    /// Budget enforcement mode
    enforcement: BudgetEnforcement,
    /// Warning threshold (0.0-1.0)
    warning_threshold: f64,
    /// Critical threshold (0.0-1.0)
    critical_threshold: f64,
    /// Whether warnings have been emitted
    warning_emitted: AtomicBool,
    critical_emitted: AtomicBool,
}

impl GlobalTokenBudget {
    /// Create zero-initialized phase consumption array
    const fn zero_phase_consumed() -> [AtomicU64; PHASE_COUNT] {
        [
            AtomicU64::new(0),
            AtomicU64::new(0),
            AtomicU64::new(0),
            AtomicU64::new(0),
            AtomicU64::new(0),
        ]
    }

    /// Create a new global budget with specified total (hard limit by default)
    pub fn new(total_budget: u64) -> Self {
        Self {
            total_budget,
            consumed: AtomicU64::new(0),
            phase_consumed: Self::zero_phase_consumed(),
            phase_limits: PhaseLimits::from_total(total_budget),
            enforcement: BudgetEnforcement::Hard,
            warning_threshold: budget_constants::WARNING_THRESHOLD,
            critical_threshold: budget_constants::CRITICAL_THRESHOLD,
            warning_emitted: AtomicBool::new(false),
            critical_emitted: AtomicBool::new(false),
        }
    }

    /// Create with soft enforcement (legacy behavior)
    pub fn new_soft(total_budget: u64) -> Self {
        Self {
            enforcement: BudgetEnforcement::Soft,
            ..Self::new(total_budget)
        }
    }

    /// Create with custom phase limits
    pub fn with_limits(total_budget: u64, phase_limits: PhaseLimits) -> Self {
        Self {
            total_budget,
            consumed: AtomicU64::new(0),
            phase_consumed: Self::zero_phase_consumed(),
            phase_limits,
            enforcement: BudgetEnforcement::Hard,
            warning_threshold: budget_constants::WARNING_THRESHOLD,
            critical_threshold: budget_constants::CRITICAL_THRESHOLD,
            warning_emitted: AtomicBool::new(false),
            critical_emitted: AtomicBool::new(false),
        }
    }

    /// Create from TaleConfig
    pub fn from_config(config: &TaleConfig) -> Self {
        Self {
            total_budget: config.total_budget,
            consumed: AtomicU64::new(0),
            phase_consumed: Self::zero_phase_consumed(),
            phase_limits: config.to_phase_limits(),
            enforcement: config.enforcement,
            warning_threshold: config.warning_threshold,
            critical_threshold: config.critical_threshold,
            warning_emitted: AtomicBool::new(false),
            critical_emitted: AtomicBool::new(false),
        }
    }

    /// Get enforcement mode
    pub fn enforcement(&self) -> BudgetEnforcement {
        self.enforcement
    }

    /// Check if tokens can be consumed without exceeding global budget
    pub fn can_consume(&self, tokens: u64) -> bool {
        let current = self.consumed.load(Ordering::Relaxed);
        current + tokens <= self.total_budget
    }

    /// Check if tokens can be consumed for a specific phase
    pub fn can_consume_for_phase(&self, tokens: u64, phase: u8) -> bool {
        if phase >= PHASE_COUNT as u8 {
            return false;
        }

        // Check global budget
        if !self.can_consume(tokens) {
            return false;
        }

        // Check phase budget (only matters in hard mode)
        if self.enforcement == BudgetEnforcement::Hard {
            let phase_limit = self.phase_limits.for_phase(phase);
            let phase_consumed = self.phase_consumed[phase as usize].load(Ordering::Relaxed);
            return phase_consumed + tokens <= phase_limit;
        }

        true
    }

    /// Consume tokens from the global budget
    ///
    /// Uses compare_exchange to avoid ABA race conditions.
    pub fn consume(&self, tokens: u64) -> Result<()> {
        loop {
            let current = self.consumed.load(Ordering::Acquire);
            let new_total = current + tokens;

            if new_total > self.total_budget {
                return Err(BudgetError::GlobalExceeded {
                    consumed: current,
                    budget: self.total_budget,
                    requested: tokens,
                }
                .into());
            }

            match self.consumed.compare_exchange_weak(
                current,
                new_total,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.check_thresholds(new_total);
                    return Ok(());
                }
                Err(_) => continue,
            }
        }
    }

    /// Consume tokens for a specific phase with hard limit enforcement
    ///
    /// In Hard mode: returns error if phase budget is exceeded
    /// In Soft mode: warns but allows using global budget
    pub fn consume_for_phase(&self, tokens: u64, phase: u8) -> Result<()> {
        if phase >= PHASE_COUNT as u8 {
            return Err(BudgetError::InvalidPhase { phase }.into());
        }

        let phase_limit = self.phase_limits.for_phase(phase);
        let phase_consumed = self.phase_consumed[phase as usize].load(Ordering::Relaxed);

        // Check phase limit
        if phase_consumed + tokens > phase_limit {
            match self.enforcement {
                BudgetEnforcement::Hard => {
                    return Err(BudgetError::PhaseExceeded {
                        phase,
                        phase_name: phase_name(phase),
                        consumed: phase_consumed,
                        limit: phase_limit,
                        requested: tokens,
                    }
                    .into());
                }
                BudgetEnforcement::Soft => {
                    tracing::warn!(
                        phase = phase,
                        phase_name = phase_name(phase),
                        consumed = phase_consumed,
                        limit = phase_limit,
                        requested = tokens,
                        "Phase budget exceeded (soft limit - continuing with global budget)"
                    );
                }
            }
        }

        // Consume from global budget
        self.consume(tokens)?;

        // Track phase consumption (even if over limit in soft mode)
        self.phase_consumed[phase as usize].fetch_add(tokens, Ordering::Relaxed);

        Ok(())
    }

    /// Try to reserve budget for a phase operation
    ///
    /// Returns the amount that can actually be reserved (may be less than requested)
    pub fn try_reserve_for_phase(&self, tokens: u64, phase: u8) -> Result<u64> {
        if phase >= PHASE_COUNT as u8 {
            return Err(BudgetError::InvalidPhase { phase }.into());
        }

        let phase_remaining = self.remaining_for_phase(phase);
        let global_remaining = self.remaining();

        let available = phase_remaining.min(global_remaining);

        if available == 0 {
            return Err(BudgetError::ReservationFailed {
                phase,
                requested: tokens,
                available: 0,
            }
            .into());
        }

        Ok(available.min(tokens))
    }

    /// Get current consumption statistics
    pub fn stats(&self) -> BudgetStats {
        let consumed = self.consumed.load(Ordering::Relaxed);
        let remaining = self.total_budget.saturating_sub(consumed);
        let utilization = if self.total_budget > 0 {
            consumed as f64 / self.total_budget as f64
        } else {
            0.0
        };

        let mut phase_stats = Vec::with_capacity(PHASE_COUNT);
        for phase in 0..PHASE_COUNT {
            let phase_consumed = self.phase_consumed[phase].load(Ordering::Relaxed);
            let phase_limit = self.phase_limits.for_phase(phase as u8);
            if phase_limit > 0 {
                phase_stats.push(PhaseStats {
                    phase: phase as u8,
                    phase_name: phase_name(phase as u8),
                    consumed: phase_consumed,
                    limit: phase_limit,
                    utilization: phase_consumed as f64 / phase_limit as f64,
                });
            }
        }

        BudgetStats {
            total_budget: self.total_budget,
            consumed,
            remaining,
            utilization,
            enforcement: self.enforcement,
            is_warning: utilization >= self.warning_threshold,
            is_critical: utilization >= self.critical_threshold,
            phase_stats,
        }
    }

    /// Get remaining budget
    pub fn remaining(&self) -> u64 {
        self.total_budget
            .saturating_sub(self.consumed.load(Ordering::Relaxed))
    }

    /// Get remaining budget for a phase
    pub fn remaining_for_phase(&self, phase: u8) -> u64 {
        if phase >= PHASE_COUNT as u8 {
            return 0;
        }

        let phase_limit = self.phase_limits.for_phase(phase);
        let phase_consumed = self.phase_consumed[phase as usize].load(Ordering::Relaxed);
        phase_limit.saturating_sub(phase_consumed)
    }

    /// Get utilization percentage (0.0-1.0)
    pub fn utilization(&self) -> f64 {
        if self.total_budget == 0 {
            return 0.0;
        }
        self.consumed.load(Ordering::Relaxed) as f64 / self.total_budget as f64
    }

    /// Check and emit threshold warnings
    fn check_thresholds(&self, consumed: u64) {
        let utilization = consumed as f64 / self.total_budget as f64;

        if utilization >= self.critical_threshold
            && !self.critical_emitted.swap(true, Ordering::Relaxed)
        {
            tracing::error!(
                consumed = consumed,
                total = self.total_budget,
                utilization_pct = utilization * 100.0,
                "CRITICAL: Token budget at critical threshold. Consider reducing scope."
            );
        } else if utilization >= self.warning_threshold
            && !self.warning_emitted.swap(true, Ordering::Relaxed)
        {
            tracing::warn!(
                consumed = consumed,
                total = self.total_budget,
                utilization_pct = utilization * 100.0,
                "Token budget approaching limit"
            );
        }
    }

    /// Reallocate unused budget from completed phase to another phase
    ///
    /// Atomically transfers unused tokens from source phase to destination.
    /// The source phase's consumed amount is "frozen" (marked as fully used)
    /// and the remaining budget is added to the target phase's limit.
    ///
    /// Returns the actual amount reallocated.
    pub fn reallocate_from_phase(&self, from_phase: u8, to_phase: u8) -> Result<u64> {
        if from_phase >= PHASE_COUNT as u8 || to_phase >= PHASE_COUNT as u8 {
            return Err(BudgetError::InvalidPhase {
                phase: from_phase.max(to_phase),
            }
            .into());
        }

        if from_phase == to_phase {
            return Ok(0);
        }

        let unused = self.remaining_for_phase(from_phase);
        if unused == 0 {
            return Ok(0);
        }

        // Mark source phase as fully consumed by adding unused to consumed
        // This prevents double-spending of the reallocated budget
        self.phase_consumed[from_phase as usize].fetch_add(unused, Ordering::SeqCst);

        // Add the unused amount to target phase's limit
        self.phase_limits.add_to_phase(to_phase, unused)?;

        tracing::info!(
            from_phase = phase_name(from_phase),
            to_phase = phase_name(to_phase),
            amount = unused,
            new_target_limit = self.phase_limits.for_phase(to_phase),
            "Budget reallocated between phases"
        );

        Ok(unused)
    }

    /// Reset budget for a new session
    pub fn reset(&self) {
        self.consumed.store(0, Ordering::Relaxed);
        for phase in &self.phase_consumed {
            phase.store(0, Ordering::Relaxed);
        }
        self.warning_emitted.store(false, Ordering::Relaxed);
        self.critical_emitted.store(false, Ordering::Relaxed);
    }
}

/// Thread-safe handle to global budget
pub type SharedBudget = Arc<GlobalTokenBudget>;

/// Create a shared budget handle with hard limits
pub fn create_shared_budget(total_budget: u64) -> SharedBudget {
    Arc::new(GlobalTokenBudget::new(total_budget))
}

/// Create a shared budget with soft limits (legacy behavior)
pub fn create_shared_budget_soft(total_budget: u64) -> SharedBudget {
    Arc::new(GlobalTokenBudget::new_soft(total_budget))
}

// =============================================================================
// Budget Statistics
// =============================================================================

/// Budget statistics
#[derive(Debug, Clone)]
pub struct BudgetStats {
    pub total_budget: u64,
    pub consumed: u64,
    pub remaining: u64,
    pub utilization: f64,
    pub enforcement: BudgetEnforcement,
    pub is_warning: bool,
    pub is_critical: bool,
    pub phase_stats: Vec<PhaseStats>,
}

impl BudgetStats {
    /// Format as a human-readable summary
    pub fn summary(&self) -> String {
        let status = if self.is_critical {
            " [CRITICAL]"
        } else if self.is_warning {
            " [WARNING]"
        } else {
            ""
        };

        format!(
            "Budget: {}/{} tokens ({:.1}%) [{}]{} | Remaining: {}",
            self.consumed,
            self.total_budget,
            self.utilization * 100.0,
            self.enforcement,
            status,
            self.remaining
        )
    }

    /// Get phase summary
    pub fn phase_summary(&self) -> String {
        self.phase_stats
            .iter()
            .map(|p| format!("{}: {:.0}%", p.phase_name, p.utilization * 100.0))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// Per-phase statistics
#[derive(Debug, Clone)]
pub struct PhaseStats {
    pub phase: u8,
    pub phase_name: &'static str,
    pub consumed: u64,
    pub limit: u64,
    pub utilization: f64,
}

// =============================================================================
// TALE Framework: Complexity Estimation
// =============================================================================

/// Project complexity estimation for dynamic budget allocation
#[derive(Debug, Clone)]
pub struct ComplexityEstimate {
    pub total_tokens: u64,
    pub phase_estimates: PhaseEstimates,
    pub confidence: f32,
    pub tier_breakdown: TierBreakdown,
}

/// Phase-level token estimates
#[derive(Debug, Clone)]
pub struct PhaseEstimates {
    pub characterization: u64,
    pub bottom_up: u64,
    pub top_down: u64,
    pub consolidation: u64,
    pub refinement: u64,
}

/// File tier breakdown
#[derive(Debug, Clone, Default)]
pub struct TierBreakdown {
    pub leaf_count: usize,
    pub standard_count: usize,
    pub important_count: usize,
    pub core_count: usize,
}

/// Complexity estimator for pre-analysis budget prediction
pub struct ComplexityEstimator;

impl ComplexityEstimator {
    const TOKENS_LEAF: u64 = 800;
    const TOKENS_STANDARD: u64 = 1500;
    const TOKENS_IMPORTANT: u64 = 4000;
    const TOKENS_CORE: u64 = 6000;

    /// Estimate project complexity based on file count and structure
    pub fn estimate(file_count: usize, tier_breakdown: TierBreakdown) -> ComplexityEstimate {
        let bottom_up = tier_breakdown.leaf_count as u64 * Self::TOKENS_LEAF
            + tier_breakdown.standard_count as u64 * Self::TOKENS_STANDARD
            + tier_breakdown.important_count as u64 * Self::TOKENS_IMPORTANT * 3
            + tier_breakdown.core_count as u64 * Self::TOKENS_CORE * 4;

        let characterization = 7 * 2000;
        let top_down = 4 * 3000;
        let estimated_domains = (file_count / 20).clamp(3, 30);
        let consolidation = estimated_domains as u64 * 3000;
        let refinement = 5 * 2000;

        let total_tokens = characterization + bottom_up + top_down + consolidation + refinement;
        let confidence = ((file_count as f32 / 100.0).min(1.0) * 0.6 + 0.4).min(0.95);

        ComplexityEstimate {
            total_tokens,
            phase_estimates: PhaseEstimates {
                characterization,
                bottom_up,
                top_down,
                consolidation,
                refinement,
            },
            confidence,
            tier_breakdown,
        }
    }

    /// Quick estimate from file count only
    pub fn estimate_simple(file_count: usize) -> ComplexityEstimate {
        let tier_breakdown = TierBreakdown {
            leaf_count: (file_count * 60) / 100,
            standard_count: (file_count * 25) / 100,
            important_count: (file_count * 12) / 100,
            core_count: (file_count * 3) / 100,
        };
        Self::estimate(file_count, tier_breakdown)
    }
}

/// Dynamic budget allocator based on complexity estimates
pub struct DynamicAllocator;

impl DynamicAllocator {
    /// Allocate phase budgets based on complexity estimate
    pub fn allocate(total_budget: u64, estimate: &ComplexityEstimate) -> PhaseLimits {
        let estimated_total = estimate.total_tokens;

        if estimated_total <= total_budget {
            let buffer = total_budget - estimated_total;
            Self::allocate_with_buffer(&estimate.phase_estimates, buffer)
        } else {
            let scale = total_budget as f64 / estimated_total as f64;
            Self::scale_allocation(&estimate.phase_estimates, scale)
        }
    }

    fn allocate_with_buffer(estimates: &PhaseEstimates, buffer: u64) -> PhaseLimits {
        let bottom_up_extra = buffer * 50 / 100;
        let consolidation_extra = buffer * 30 / 100;
        let refinement_extra = buffer * 20 / 100;

        PhaseLimits::new(
            estimates.characterization,
            estimates.bottom_up + bottom_up_extra,
            estimates.top_down,
            estimates.consolidation + consolidation_extra,
            estimates.refinement + refinement_extra,
        )
    }

    fn scale_allocation(estimates: &PhaseEstimates, scale: f64) -> PhaseLimits {
        PhaseLimits::new(
            ((estimates.characterization as f64 * scale.max(0.8)) as u64)
                .max(estimates.characterization / 2),
            (estimates.bottom_up as f64 * scale) as u64,
            ((estimates.top_down as f64 * scale.max(0.7)) as u64).max(estimates.top_down / 2),
            (estimates.consolidation as f64 * scale) as u64,
            ((estimates.refinement as f64 * scale.min(0.5)) as u64).max(2000),
        )
    }
}

// =============================================================================
// Runtime Monitoring
// =============================================================================

/// Budget status for runtime monitoring
#[derive(Debug, Clone)]
pub enum BudgetStatus {
    OnTrack,
    SlightlyOver {
        deviation_percent: f32,
    },
    SignificantlyOver {
        deviation_percent: f32,
        recommended_action: DegradationAction,
    },
    Critical {
        remaining: u64,
    },
}

/// Actions to take when budget is exceeded
#[derive(Debug, Clone)]
pub enum DegradationAction {
    ReduceConcurrency { new_limit: usize },
    SkipDeepResearch,
    TruncateLargeFiles { max_lines: usize },
    ReduceRefinement { max_iterations: usize },
    LowerQualityTarget { new_target: f32 },
    SkipDiagrams,
}

/// Runtime budget monitor for checkpoints
#[derive(Debug)]
pub struct RuntimeMonitor {
    expected_per_phase: [u64; PHASE_COUNT],
}

impl RuntimeMonitor {
    /// Create monitor from complexity estimate
    pub fn from_estimate(estimate: &ComplexityEstimate) -> Self {
        Self {
            expected_per_phase: [
                estimate.phase_estimates.characterization,
                estimate.phase_estimates.bottom_up,
                estimate.phase_estimates.top_down,
                estimate.phase_estimates.consolidation,
                estimate.phase_estimates.refinement,
            ],
        }
    }

    /// Check budget status at a phase checkpoint
    pub fn check_at_phase(&self, phase: u8, actual_consumed: u64) -> BudgetStatus {
        let expected = self.expected_for_phase(phase);
        if expected == 0 {
            return BudgetStatus::OnTrack;
        }

        let deviation = if actual_consumed > expected {
            ((actual_consumed - expected) as f64 / expected as f64 * 100.0) as f32
        } else {
            0.0
        };

        match deviation {
            d if d < 10.0 => BudgetStatus::OnTrack,
            d if d < 25.0 => BudgetStatus::SlightlyOver {
                deviation_percent: d,
            },
            d if d < 50.0 => BudgetStatus::SignificantlyOver {
                deviation_percent: d,
                recommended_action: self.recommend_action(phase, d),
            },
            _ => BudgetStatus::Critical { remaining: 0 },
        }
    }

    fn expected_for_phase(&self, phase: u8) -> u64 {
        let max_phase = (PHASE_COUNT - 1) as u8;
        self.expected_per_phase[..=phase.min(max_phase) as usize]
            .iter()
            .sum()
    }

    fn recommend_action(&self, phase: u8, deviation: f32) -> DegradationAction {
        match phase {
            1 if deviation > 40.0 => DegradationAction::SkipDeepResearch,
            1 => DegradationAction::TruncateLargeFiles { max_lines: 500 },
            3 => DegradationAction::ReduceConcurrency { new_limit: 2 },
            4 => DegradationAction::ReduceRefinement { max_iterations: 2 },
            _ => DegradationAction::LowerQualityTarget { new_target: 0.6 },
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_budget_creation() {
        let budget = GlobalTokenBudget::new(1_000_000);
        assert_eq!(budget.remaining(), 1_000_000);
        assert_eq!(budget.utilization(), 0.0);
        assert_eq!(budget.enforcement(), BudgetEnforcement::Hard);
    }

    #[test]
    fn test_consume_within_budget() {
        let budget = GlobalTokenBudget::new(10_000);

        budget.consume(1000).unwrap();
        assert_eq!(budget.remaining(), 9000);

        budget.consume(4000).unwrap();
        assert_eq!(budget.remaining(), 5000);
    }

    #[test]
    fn test_consume_exceeds_budget() {
        let budget = GlobalTokenBudget::new(1000);

        budget.consume(500).unwrap();
        let result = budget.consume(600);

        assert!(result.is_err());
        assert_eq!(budget.remaining(), 500);
    }

    #[test]
    fn test_phase_hard_limit_enforced() {
        let budget = GlobalTokenBudget::new(100_000);

        // Phase 0 (characterization) has 5% = 5,000 tokens
        budget.consume_for_phase(4000, 0).unwrap();
        assert_eq!(budget.remaining_for_phase(0), 1000);

        // Exceeding phase limit should fail in hard mode
        let result = budget.consume_for_phase(2000, 0);
        assert!(result.is_err());

        // Global budget should not have been consumed
        assert_eq!(budget.remaining(), 96_000);
    }

    #[test]
    fn test_phase_soft_limit_allows_overflow() {
        let budget = GlobalTokenBudget::new_soft(100_000);

        // Phase 0 (characterization) has 5% = 5,000 tokens
        budget.consume_for_phase(4000, 0).unwrap();

        // Exceeding phase limit should warn but succeed in soft mode
        let result = budget.consume_for_phase(2000, 0);
        assert!(result.is_ok());

        // Global budget should have been consumed
        assert_eq!(budget.remaining(), 94_000);
    }

    #[test]
    fn test_can_consume_for_phase() {
        let budget = GlobalTokenBudget::new(100_000);

        // Phase 1 (bottom_up) has 50% = 50,000 tokens
        assert!(budget.can_consume_for_phase(40_000, 1));
        assert!(budget.can_consume_for_phase(50_000, 1));
        assert!(!budget.can_consume_for_phase(51_000, 1));
    }

    #[test]
    fn test_try_reserve_for_phase() {
        let budget = GlobalTokenBudget::new(100_000);

        // Phase 0 has 5,000 tokens
        let reserved = budget.try_reserve_for_phase(10_000, 0).unwrap();
        assert_eq!(reserved, 5_000); // Capped at phase limit

        // Consume some
        budget.consume_for_phase(3_000, 0).unwrap();

        // Now only 2,000 available
        let reserved = budget.try_reserve_for_phase(10_000, 0).unwrap();
        assert_eq!(reserved, 2_000);
    }

    #[test]
    fn test_phase_limits() {
        let limits = PhaseLimits::from_total(1_000_000);

        assert_eq!(limits.for_phase(0), 50_000); // characterization
        assert_eq!(limits.for_phase(1), 500_000); // bottom_up
        assert_eq!(limits.for_phase(2), 100_000); // top_down
        assert_eq!(limits.for_phase(3), 200_000); // consolidation
        assert_eq!(limits.for_phase(4), 150_000); // refinement
    }

    #[test]
    fn test_budget_reallocation() {
        let budget = GlobalTokenBudget::new(100_000);

        // Phase 0 (characterization) has 5% = 5,000 tokens
        // Consume only 2,000 of it
        budget.consume_for_phase(2_000, 0).unwrap();
        assert_eq!(budget.remaining_for_phase(0), 3_000);

        // Phase 1 (bottom_up) starts with 50% = 50,000 tokens
        let initial_phase1_limit = budget.phase_limits.for_phase(1);
        assert_eq!(initial_phase1_limit, 50_000);

        // Reallocate unused from phase 0 to phase 1
        let reallocated = budget.reallocate_from_phase(0, 1).unwrap();
        assert_eq!(reallocated, 3_000);

        // Phase 0 should now have 0 remaining (fully consumed)
        assert_eq!(budget.remaining_for_phase(0), 0);

        // Phase 1 should have increased limit
        assert_eq!(budget.phase_limits.for_phase(1), 53_000);
    }

    #[test]
    fn test_reallocation_invalid_phase() {
        let budget = GlobalTokenBudget::new(100_000);

        // Invalid source phase
        let result = budget.reallocate_from_phase(10, 1);
        assert!(result.is_err());

        // Invalid target phase
        let result = budget.reallocate_from_phase(0, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_reallocation_same_phase() {
        let budget = GlobalTokenBudget::new(100_000);

        // Reallocating to same phase should return 0
        let result = budget.reallocate_from_phase(0, 0).unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_shared_budget() {
        let budget = create_shared_budget(10_000);
        let budget2 = Arc::clone(&budget);

        budget.consume(5000).unwrap();
        assert_eq!(budget2.remaining(), 5000);

        budget2.consume(3000).unwrap();
        assert_eq!(budget.remaining(), 2000);
    }

    #[test]
    fn test_budget_stats() {
        let budget = GlobalTokenBudget::new(10_000);
        budget.consume(7500).unwrap();

        let stats = budget.stats();
        assert_eq!(stats.consumed, 7500);
        assert_eq!(stats.remaining, 2500);
        assert!((stats.utilization - 0.75).abs() < 0.001);
        assert!(stats.is_warning);
        assert!(!stats.is_critical);
        assert_eq!(stats.enforcement, BudgetEnforcement::Hard);
    }

    #[test]
    fn test_budget_reset() {
        let budget = GlobalTokenBudget::new(10_000);
        budget.consume(5000).unwrap();
        budget.consume_for_phase(2000, 1).unwrap();

        budget.reset();

        assert_eq!(budget.remaining(), 10_000);
        assert_eq!(budget.remaining_for_phase(1), 5_000);
    }

    #[test]
    fn test_invalid_phase() {
        let budget = GlobalTokenBudget::new(10_000);
        let result = budget.consume_for_phase(100, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_budget_error_display() {
        let err = BudgetError::PhaseExceeded {
            phase: 1,
            phase_name: "BottomUp",
            consumed: 45000,
            limit: 50000,
            requested: 10000,
        };

        let msg = err.to_string();
        assert!(msg.contains("BottomUp"));
        assert!(msg.contains("45000"));
        assert!(msg.contains("50000"));
    }

    #[test]
    fn test_tale_config_validation() {
        let config = TaleConfig::default();
        assert!(config.validate().is_ok());

        let bad_config = TaleConfig {
            warning_threshold: 1.5,
            ..Default::default()
        };
        assert!(bad_config.validate().is_err());
    }

    #[test]
    fn test_from_config() {
        let config = TaleConfig::strict();
        let budget = GlobalTokenBudget::from_config(&config);

        assert_eq!(budget.enforcement(), BudgetEnforcement::Hard);
        assert_eq!(budget.remaining(), 500_000);
    }
}
