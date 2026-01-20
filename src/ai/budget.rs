//! Token Budget Management

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::types::{ClaudegenError, Result};

const DEFAULT_BUDGET: u64 = 1_000_000;
const WARNING_THRESHOLD: f64 = 0.75;
const CRITICAL_THRESHOLD: f64 = 0.90;

#[derive(Debug)]
pub struct GlobalTokenBudget {
    total_budget: u64,
    consumed: AtomicU64,
    warning_emitted: AtomicBool,
    critical_emitted: AtomicBool,
}

impl GlobalTokenBudget {
    pub fn new(total_budget: u64) -> Self {
        Self {
            total_budget,
            consumed: AtomicU64::new(0),
            warning_emitted: AtomicBool::new(false),
            critical_emitted: AtomicBool::new(false),
        }
    }

    pub fn can_consume(&self, tokens: u64) -> bool {
        self.consumed.load(Ordering::Relaxed) + tokens <= self.total_budget
    }

    pub fn consume(&self, tokens: u64) -> Result<()> {
        loop {
            let current = self.consumed.load(Ordering::Acquire);
            let new_total = current + tokens;
            if new_total > self.total_budget {
                return Err(ClaudegenError::Budget {
                    consumed: current,
                    budget: self.total_budget,
                    requested: tokens,
                });
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

    pub fn remaining(&self) -> u64 {
        self.total_budget
            .saturating_sub(self.consumed.load(Ordering::Relaxed))
    }

    pub fn utilization(&self) -> f64 {
        if self.total_budget == 0 {
            return 0.0;
        }
        self.consumed.load(Ordering::Relaxed) as f64 / self.total_budget as f64
    }

    pub fn stats(&self) -> BudgetStats {
        let consumed = self.consumed.load(Ordering::Relaxed);
        let remaining = self.total_budget.saturating_sub(consumed);
        let utilization = if self.total_budget > 0 {
            consumed as f64 / self.total_budget as f64
        } else {
            0.0
        };

        BudgetStats {
            total_budget: self.total_budget,
            consumed,
            remaining,
            utilization,
            is_warning: utilization >= WARNING_THRESHOLD,
            is_critical: utilization >= CRITICAL_THRESHOLD,
        }
    }

    pub fn reset(&self) {
        self.consumed.store(0, Ordering::Relaxed);
        self.warning_emitted.store(false, Ordering::Relaxed);
        self.critical_emitted.store(false, Ordering::Relaxed);
    }

    fn check_thresholds(&self, consumed: u64) {
        let util = consumed as f64 / self.total_budget as f64;
        if util >= CRITICAL_THRESHOLD && !self.critical_emitted.swap(true, Ordering::Relaxed) {
            tracing::error!(consumed, total = self.total_budget, "Token budget critical");
        } else if util >= WARNING_THRESHOLD && !self.warning_emitted.swap(true, Ordering::Relaxed) {
            tracing::warn!(consumed, total = self.total_budget, "Token budget warning");
        }
    }
}

impl Default for GlobalTokenBudget {
    fn default() -> Self {
        Self::new(DEFAULT_BUDGET)
    }
}

pub type SharedBudget = Arc<GlobalTokenBudget>;

pub fn create_shared_budget(total_budget: u64) -> SharedBudget {
    Arc::new(GlobalTokenBudget::new(total_budget))
}

#[derive(Debug, Clone)]
pub struct BudgetStats {
    pub total_budget: u64,
    pub consumed: u64,
    pub remaining: u64,
    pub utilization: f64,
    pub is_warning: bool,
    pub is_critical: bool,
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
            "Budget: {}/{} ({:.1}%){}",
            self.consumed,
            self.total_budget,
            self.utilization * 100.0,
            status
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_creation() {
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
}
