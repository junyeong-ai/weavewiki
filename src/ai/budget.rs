//! Global Token Budget Management
//!
//! Thread-safe token budget tracking with hard limit enforcement.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::constants::budget as budget_constants;
use crate::types::{Result, WeaveError};

// =============================================================================
// Error Types
// =============================================================================

#[derive(Debug, Clone)]
pub enum BudgetError {
    GlobalExceeded {
        consumed: u64,
        budget: u64,
        requested: u64,
    },
    PhaseExceeded {
        phase: u8,
        phase_name: &'static str,
        consumed: u64,
        limit: u64,
        requested: u64,
    },
    InvalidPhase {
        phase: u8,
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
                    "Global token budget exceeded: {consumed}/{budget} (requested: {requested})"
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
                    "Phase {phase} ({phase_name}) exceeded: {consumed}/{limit} (requested: {requested})"
                )
            }
            Self::InvalidPhase { phase } => write!(f, "Invalid phase: {phase}"),
        }
    }
}

impl std::error::Error for BudgetError {}

// =============================================================================
// Phase Names
// =============================================================================

const PHASE_NAMES: [&str; 5] = [
    "Characterization",
    "BottomUp",
    "TopDown",
    "Consolidation",
    "Refinement",
];
const PHASE_COUNT: usize = 5;

fn phase_name(phase: u8) -> &'static str {
    PHASE_NAMES
        .get(phase as usize)
        .copied()
        .unwrap_or("Unknown")
}

// =============================================================================
// Configuration
// =============================================================================

#[derive(Debug, Clone)]
pub struct TaleConfig {
    pub total_budget: u64,
    pub phase_allocations: PhaseAllocations,
    pub warning_threshold: f64,
    pub critical_threshold: f64,
    pub reserve_buffer_pct: f64,
}

impl Default for TaleConfig {
    fn default() -> Self {
        Self {
            total_budget: budget_constants::DEFAULT_BUDGET,
            phase_allocations: PhaseAllocations::default(),
            warning_threshold: budget_constants::WARNING_THRESHOLD,
            critical_threshold: budget_constants::CRITICAL_THRESHOLD,
            reserve_buffer_pct: budget_constants::RESERVE_BUFFER_PCT,
        }
    }
}

impl TaleConfig {
    pub fn with_budget(total_budget: u64) -> Self {
        Self {
            total_budget,
            ..Default::default()
        }
    }

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
                "Critical threshold must be > warning ({}) and < 1.0, got {}",
                self.warning_threshold, self.critical_threshold
            )));
        }
        Ok(())
    }

    pub fn to_phase_limits(&self) -> PhaseLimits {
        let effective = (self.total_budget as f64 * (1.0 - self.reserve_buffer_pct)) as u64;
        self.phase_allocations.to_limits(effective)
    }
}

// =============================================================================
// Phase Allocations
// =============================================================================

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

    pub fn to_limits(&self, total_budget: u64) -> PhaseLimits {
        PhaseLimits::new(
            total_budget * self.characterization_pct as u64 / 100,
            total_budget * self.bottom_up_pct as u64 / 100,
            total_budget * self.top_down_pct as u64 / 100,
            total_budget * self.consolidation_pct as u64 / 100,
            total_budget * self.refinement_pct as u64 / 100,
        )
    }
}

// =============================================================================
// Phase Limits
// =============================================================================

#[derive(Debug)]
pub struct PhaseLimits {
    limits: [AtomicU64; PHASE_COUNT],
}

impl Clone for PhaseLimits {
    fn clone(&self) -> Self {
        Self {
            limits: std::array::from_fn(|i| AtomicU64::new(self.limits[i].load(Ordering::Relaxed))),
        }
    }
}

impl PhaseLimits {
    pub fn new(char: u64, bottom: u64, top: u64, consol: u64, refine: u64) -> Self {
        Self {
            limits: [
                AtomicU64::new(char),
                AtomicU64::new(bottom),
                AtomicU64::new(top),
                AtomicU64::new(consol),
                AtomicU64::new(refine),
            ],
        }
    }

    pub fn from_total(total: u64) -> Self {
        Self::new(
            total * 5 / 100,
            total * 50 / 100,
            total * 10 / 100,
            total * 20 / 100,
            total * 15 / 100,
        )
    }

    pub fn for_phase(&self, phase: u8) -> u64 {
        self.limits
            .get(phase as usize)
            .map(|a| a.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub fn add_to_phase(&self, phase: u8, tokens: u64) -> Result<u64> {
        if let Some(limit) = self.limits.get(phase as usize) {
            Ok(limit.fetch_add(tokens, Ordering::SeqCst) + tokens)
        } else {
            Err(BudgetError::InvalidPhase { phase }.into())
        }
    }

    pub fn total(&self) -> u64 {
        self.limits.iter().map(|a| a.load(Ordering::Relaxed)).sum()
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

#[derive(Debug)]
pub struct GlobalTokenBudget {
    total_budget: u64,
    consumed: AtomicU64,
    phase_consumed: [AtomicU64; PHASE_COUNT],
    phase_limits: PhaseLimits,
    warning_threshold: f64,
    critical_threshold: f64,
    warning_emitted: AtomicBool,
    critical_emitted: AtomicBool,
}

impl GlobalTokenBudget {
    const fn zero_phase_consumed() -> [AtomicU64; PHASE_COUNT] {
        [
            AtomicU64::new(0),
            AtomicU64::new(0),
            AtomicU64::new(0),
            AtomicU64::new(0),
            AtomicU64::new(0),
        ]
    }

    pub fn new(total_budget: u64) -> Self {
        Self {
            total_budget,
            consumed: AtomicU64::new(0),
            phase_consumed: Self::zero_phase_consumed(),
            phase_limits: PhaseLimits::from_total(total_budget),
            warning_threshold: budget_constants::WARNING_THRESHOLD,
            critical_threshold: budget_constants::CRITICAL_THRESHOLD,
            warning_emitted: AtomicBool::new(false),
            critical_emitted: AtomicBool::new(false),
        }
    }

    pub fn with_limits(total_budget: u64, phase_limits: PhaseLimits) -> Self {
        Self {
            phase_limits,
            ..Self::new(total_budget)
        }
    }

    pub fn from_config(config: &TaleConfig) -> Self {
        Self {
            total_budget: config.total_budget,
            consumed: AtomicU64::new(0),
            phase_consumed: Self::zero_phase_consumed(),
            phase_limits: config.to_phase_limits(),
            warning_threshold: config.warning_threshold,
            critical_threshold: config.critical_threshold,
            warning_emitted: AtomicBool::new(false),
            critical_emitted: AtomicBool::new(false),
        }
    }

    pub fn can_consume(&self, tokens: u64) -> bool {
        self.consumed.load(Ordering::Relaxed) + tokens <= self.total_budget
    }

    pub fn can_consume_for_phase(&self, tokens: u64, phase: u8) -> bool {
        if phase >= PHASE_COUNT as u8 || !self.can_consume(tokens) {
            return false;
        }
        let limit = self.phase_limits.for_phase(phase);
        let consumed = self.phase_consumed[phase as usize].load(Ordering::Relaxed);
        consumed + tokens <= limit
    }

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
            if self
                .consumed
                .compare_exchange_weak(current, new_total, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                self.check_thresholds(new_total);
                return Ok(());
            }
        }
    }

    pub fn consume_for_phase(&self, tokens: u64, phase: u8) -> Result<()> {
        if phase >= PHASE_COUNT as u8 {
            return Err(BudgetError::InvalidPhase { phase }.into());
        }
        let limit = self.phase_limits.for_phase(phase);
        let consumed = self.phase_consumed[phase as usize].load(Ordering::Relaxed);
        if consumed + tokens > limit {
            return Err(BudgetError::PhaseExceeded {
                phase,
                phase_name: phase_name(phase),
                consumed,
                limit,
                requested: tokens,
            }
            .into());
        }
        self.consume(tokens)?;
        self.phase_consumed[phase as usize].fetch_add(tokens, Ordering::Relaxed);
        Ok(())
    }

    pub fn try_reserve_for_phase(&self, tokens: u64, phase: u8) -> Result<u64> {
        if phase >= PHASE_COUNT as u8 {
            return Err(BudgetError::InvalidPhase { phase }.into());
        }
        let available = self.remaining_for_phase(phase).min(self.remaining());
        if available == 0 {
            return Err(BudgetError::GlobalExceeded {
                consumed: self.consumed.load(Ordering::Relaxed),
                budget: self.total_budget,
                requested: tokens,
            }
            .into());
        }
        Ok(available.min(tokens))
    }

    pub fn stats(&self) -> BudgetStats {
        let consumed = self.consumed.load(Ordering::Relaxed);
        let remaining = self.total_budget.saturating_sub(consumed);
        let utilization = if self.total_budget > 0 {
            consumed as f64 / self.total_budget as f64
        } else {
            0.0
        };

        let phase_stats: Vec<_> = (0..PHASE_COUNT)
            .filter_map(|p| {
                let limit = self.phase_limits.for_phase(p as u8);
                (limit > 0).then(|| PhaseStats {
                    phase: p as u8,
                    phase_name: phase_name(p as u8),
                    consumed: self.phase_consumed[p].load(Ordering::Relaxed),
                    limit,
                    utilization: self.phase_consumed[p].load(Ordering::Relaxed) as f64
                        / limit as f64,
                })
            })
            .collect();

        BudgetStats {
            total_budget: self.total_budget,
            consumed,
            remaining,
            utilization,
            is_warning: utilization >= self.warning_threshold,
            is_critical: utilization >= self.critical_threshold,
            phase_stats,
        }
    }

    pub fn remaining(&self) -> u64 {
        self.total_budget
            .saturating_sub(self.consumed.load(Ordering::Relaxed))
    }

    pub fn remaining_for_phase(&self, phase: u8) -> u64 {
        if phase >= PHASE_COUNT as u8 {
            return 0;
        }
        let limit = self.phase_limits.for_phase(phase);
        limit.saturating_sub(self.phase_consumed[phase as usize].load(Ordering::Relaxed))
    }

    pub fn utilization(&self) -> f64 {
        if self.total_budget == 0 {
            return 0.0;
        }
        self.consumed.load(Ordering::Relaxed) as f64 / self.total_budget as f64
    }

    fn check_thresholds(&self, consumed: u64) {
        let util = consumed as f64 / self.total_budget as f64;
        if util >= self.critical_threshold && !self.critical_emitted.swap(true, Ordering::Relaxed) {
            tracing::error!(
                consumed,
                total = self.total_budget,
                "CRITICAL: Token budget at critical threshold"
            );
        } else if util >= self.warning_threshold
            && !self.warning_emitted.swap(true, Ordering::Relaxed)
        {
            tracing::warn!(
                consumed,
                total = self.total_budget,
                "Token budget approaching limit"
            );
        }
    }

    pub fn reallocate_from_phase(&self, from: u8, to: u8) -> Result<u64> {
        if from >= PHASE_COUNT as u8 || to >= PHASE_COUNT as u8 {
            return Err(BudgetError::InvalidPhase {
                phase: from.max(to),
            }
            .into());
        }
        if from == to {
            return Ok(0);
        }
        let unused = self.remaining_for_phase(from);
        if unused == 0 {
            return Ok(0);
        }
        self.phase_consumed[from as usize].fetch_add(unused, Ordering::SeqCst);
        self.phase_limits.add_to_phase(to, unused)?;
        tracing::info!(
            from = phase_name(from),
            to = phase_name(to),
            amount = unused,
            "Budget reallocated"
        );
        Ok(unused)
    }

    pub fn reset(&self) {
        self.consumed.store(0, Ordering::Relaxed);
        for phase in &self.phase_consumed {
            phase.store(0, Ordering::Relaxed);
        }
        self.warning_emitted.store(false, Ordering::Relaxed);
        self.critical_emitted.store(false, Ordering::Relaxed);
    }
}

pub type SharedBudget = Arc<GlobalTokenBudget>;

pub fn create_shared_budget(total_budget: u64) -> SharedBudget {
    Arc::new(GlobalTokenBudget::new(total_budget))
}

// =============================================================================
// Budget Statistics
// =============================================================================

#[derive(Debug, Clone)]
pub struct BudgetStats {
    pub total_budget: u64,
    pub consumed: u64,
    pub remaining: u64,
    pub utilization: f64,
    pub is_warning: bool,
    pub is_critical: bool,
    pub phase_stats: Vec<PhaseStats>,
}

impl BudgetStats {
    pub fn summary(&self) -> String {
        let status = if self.is_critical {
            " [CRITICAL]"
        } else if self.is_warning {
            " [WARNING]"
        } else {
            ""
        };
        format!(
            "Budget: {}/{} ({:.1}%){} | Remaining: {}",
            self.consumed,
            self.total_budget,
            self.utilization * 100.0,
            status,
            self.remaining
        )
    }

    pub fn phase_summary(&self) -> String {
        self.phase_stats
            .iter()
            .map(|p| format!("{}: {:.0}%", p.phase_name, p.utilization * 100.0))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

#[derive(Debug, Clone)]
pub struct PhaseStats {
    pub phase: u8,
    pub phase_name: &'static str,
    pub consumed: u64,
    pub limit: u64,
    pub utilization: f64,
}

// =============================================================================
// Complexity Estimation (for pre-flight)
// =============================================================================

#[derive(Debug, Clone)]
pub struct ComplexityEstimate {
    pub total_tokens: u64,
    pub phase_estimates: PhaseEstimates,
    pub confidence: f32,
    pub tier_breakdown: TierBreakdown,
}

#[derive(Debug, Clone)]
pub struct PhaseEstimates {
    pub characterization: u64,
    pub bottom_up: u64,
    pub top_down: u64,
    pub consolidation: u64,
    pub refinement: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TierBreakdown {
    pub leaf_count: usize,
    pub standard_count: usize,
    pub important_count: usize,
    pub core_count: usize,
}

pub fn estimate_complexity(file_count: usize, tiers: TierBreakdown) -> ComplexityEstimate {
    const TOKENS_LEAF: u64 = 800;
    const TOKENS_STANDARD: u64 = 1500;
    const TOKENS_IMPORTANT: u64 = 4000;
    const TOKENS_CORE: u64 = 6000;

    let bottom_up = tiers.leaf_count as u64 * TOKENS_LEAF
        + tiers.standard_count as u64 * TOKENS_STANDARD
        + tiers.important_count as u64 * TOKENS_IMPORTANT * 3
        + tiers.core_count as u64 * TOKENS_CORE * 4;

    let characterization = 7 * 2000;
    let top_down = 4 * 3000;
    let domains = (file_count / 20).clamp(3, 30) as u64;
    let consolidation = domains * 3000;
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
        tier_breakdown: tiers,
    }
}

pub fn estimate_complexity_simple(file_count: usize) -> ComplexityEstimate {
    let tiers = TierBreakdown {
        leaf_count: file_count * 60 / 100,
        standard_count: file_count * 25 / 100,
        important_count: file_count * 12 / 100,
        core_count: file_count * 3 / 100,
    };
    estimate_complexity(file_count, tiers)
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
        assert!(budget.consume(600).is_err());
        assert_eq!(budget.remaining(), 500);
    }

    #[test]
    fn test_phase_limit_enforced() {
        let budget = GlobalTokenBudget::new(100_000);
        budget.consume_for_phase(4000, 0).unwrap();
        assert_eq!(budget.remaining_for_phase(0), 1000);
        assert!(budget.consume_for_phase(2000, 0).is_err());
        assert_eq!(budget.remaining(), 96_000);
    }

    #[test]
    fn test_can_consume_for_phase() {
        let budget = GlobalTokenBudget::new(100_000);
        assert!(budget.can_consume_for_phase(40_000, 1));
        assert!(budget.can_consume_for_phase(50_000, 1));
        assert!(!budget.can_consume_for_phase(51_000, 1));
    }

    #[test]
    fn test_phase_limits() {
        let limits = PhaseLimits::from_total(1_000_000);
        assert_eq!(limits.for_phase(0), 50_000);
        assert_eq!(limits.for_phase(1), 500_000);
        assert_eq!(limits.for_phase(2), 100_000);
        assert_eq!(limits.for_phase(3), 200_000);
        assert_eq!(limits.for_phase(4), 150_000);
    }

    #[test]
    fn test_budget_reallocation() {
        let budget = GlobalTokenBudget::new(100_000);
        budget.consume_for_phase(2_000, 0).unwrap();
        let reallocated = budget.reallocate_from_phase(0, 1).unwrap();
        assert_eq!(reallocated, 3_000);
        assert_eq!(budget.remaining_for_phase(0), 0);
        assert_eq!(budget.phase_limits.for_phase(1), 53_000);
    }

    #[test]
    fn test_shared_budget() {
        let budget = create_shared_budget(10_000);
        let budget2 = Arc::clone(&budget);
        budget.consume(5000).unwrap();
        assert_eq!(budget2.remaining(), 5000);
    }

    #[test]
    fn test_budget_stats() {
        let budget = GlobalTokenBudget::new(10_000);
        budget.consume(7500).unwrap();
        let stats = budget.stats();
        assert_eq!(stats.consumed, 7500);
        assert_eq!(stats.remaining, 2500);
        assert!(stats.is_warning);
        assert!(!stats.is_critical);
    }

    #[test]
    fn test_budget_reset() {
        let budget = GlobalTokenBudget::new(10_000);
        budget.consume(5000).unwrap();
        budget.reset();
        assert_eq!(budget.remaining(), 10_000);
    }

    #[test]
    fn test_tale_config_validation() {
        assert!(TaleConfig::default().validate().is_ok());
        let bad = TaleConfig {
            warning_threshold: 1.5,
            ..Default::default()
        };
        assert!(bad.validate().is_err());
    }
}
