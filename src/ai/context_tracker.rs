//! Context Window Tracker
//!
//! Budget allocation for LLM context windows.
//! Ensures generation context stays within model limits by:
//! - Tracking token allocation per section
//! - Enforcing budget limits (not just guidance)
//! - Supporting 3-tier progressive context loading

use std::collections::HashMap;

use crate::constants::context::MODEL_CONTEXT_LIMIT;
use crate::constants::token_estimation::{CHARS_PER_TOKEN, NON_ASCII_CHARS_PER_TOKEN};

/// Default input ratio matching ContextWindowConfig.input_ratio (0.90).
/// This means 90% of context is for input, 10% reserved for output.
const DEFAULT_INPUT_RATIO: f64 = 0.90;

/// Approximate tokens from character count.
///
/// Uses different ratios for ASCII (~4 chars/token) vs non-ASCII (~2 chars/token)
/// to avoid under-estimating tokens for CJK, Cyrillic, and other non-Latin scripts.
/// Conservative: prefers over-estimation per project defaults philosophy.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let mut ascii_count: usize = 0;
    let mut non_ascii_count: usize = 0;
    for ch in text.chars() {
        if ch.is_ascii() {
            ascii_count += 1;
        } else {
            non_ascii_count += 1;
        }
    }
    let estimate = (ascii_count as f64 / CHARS_PER_TOKEN as f64)
        + (non_ascii_count as f64 / NON_ASCII_CHARS_PER_TOKEN as f64);
    // Minimum 1 token for non-empty text
    (estimate as usize).max(1)
}

/// Budget allocation for context window sections.
///
/// Ensures the total context sent to LLM stays within model limits.
/// Uses 3-tier progressive loading with priority-based allocation:
/// - **Tier 1 (Essential):** Always gets guaranteed minimum (80% of budget)
/// - **Tier 3 (Reference):** Reserved space secured before tier 2 allocation
/// - **Tier 2 (Relevant):** Gets remaining budget after tier 1 and tier 3 reserves
#[derive(Debug, Clone)]
pub struct ContextBudget {
    /// Total tokens available for input (model limit * input_ratio)
    pub total_tokens: usize,
    /// Tokens reserved for output generation
    pub output_reserve: usize,
    /// Per-section token allocations
    pub allocated: HashMap<String, usize>,
    /// Tokens reserved for tier 3 (secured before tier 2 gets its share)
    tier3_reserve: usize,
}

/// Fraction of total budget guaranteed for tier 1 (essential) content.
const TIER1_GUARANTEED_RATIO: f64 = 0.80;

/// Fraction of total budget reserved for tier 3 (reference) content.
const TIER3_RESERVE_RATIO: f64 = 0.05;

impl ContextBudget {
    pub fn new(model_limit: usize) -> Self {
        Self::input_ratio(model_limit, DEFAULT_INPUT_RATIO)
    }

    /// Create a budget with a custom input ratio (0.0-1.0).
    /// The ratio determines what fraction of the context window is for input.
    pub fn input_ratio(model_limit: usize, input_ratio: f64) -> Self {
        let total_tokens = (model_limit as f64 * input_ratio) as usize;
        let output_reserve = model_limit - total_tokens;
        let tier3_reserve = (total_tokens as f64 * TIER3_RESERVE_RATIO) as usize;
        Self {
            total_tokens,
            output_reserve,
            allocated: HashMap::new(),
            tier3_reserve,
        }
    }

    /// Allocate tokens for a named section. Returns actual allocated amount
    /// (may be less than requested if budget is tight).
    ///
    /// Respects tier 3 reserve: non-tier3 allocations cannot consume reserved space.
    pub fn allocate(&mut self, section: &str, requested: usize) -> usize {
        let used: usize = self.allocated.values().sum();
        let effective_ceiling = if section.starts_with("tier3") {
            // Tier 3 sections can use the full remaining budget (including their reserve)
            self.total_tokens
        } else {
            // Non-tier3 sections respect the tier3 reserve
            self.total_tokens.saturating_sub(self.tier3_reserve)
        };
        let remaining = effective_ceiling.saturating_sub(used);
        let actual = requested.min(remaining);
        self.allocated.insert(section.to_string(), actual);
        actual
    }

    /// Allocate with guaranteed minimum. Used for tier 1 essential sections.
    /// Returns the full requested amount as long as it fits within the tier 1 guarantee.
    pub fn allocate_guaranteed(&mut self, section: &str, requested: usize) -> usize {
        let tier1_limit = (self.total_tokens as f64 * TIER1_GUARANTEED_RATIO) as usize;
        let tier1_used: usize = self
            .allocated
            .iter()
            .filter(|(k, _)| k.starts_with("tier1") || k.starts_with("system"))
            .map(|(_, v)| *v)
            .sum();
        let tier1_remaining = tier1_limit.saturating_sub(tier1_used);
        let actual = requested.min(tier1_remaining);
        self.allocated.insert(section.to_string(), actual);
        actual
    }

    /// Flexible allocation for tier2/tier3 sections.
    ///
    /// Returns `(actual_tokens, summarization_needed)`:
    /// - `actual_tokens`: tokens actually allocated (may be less than requested)
    /// - `summarization_needed`: true if the full request could not be satisfied
    ///
    /// For tier3 sections (prefix "tier3"): draws from reserved space + remaining.
    /// For tier2 sections: draws from remaining budget after tier1 and tier3 reserves.
    pub fn allocate_flexible(&mut self, section: &str, requested: usize) -> (usize, bool) {
        let actual = self.allocate(section, requested);
        let summarization_needed = actual < requested;
        (actual, summarization_needed)
    }

    /// Check remaining budget (respecting tier 3 reserve for non-tier3 callers).
    pub fn remaining(&self) -> usize {
        let used: usize = self.allocated.values().sum();
        let effective = self.total_tokens.saturating_sub(self.tier3_reserve);
        effective.saturating_sub(used)
    }

    /// Total remaining including tier 3 reserve.
    pub fn remaining_total(&self) -> usize {
        let used: usize = self.allocated.values().sum();
        self.total_tokens.saturating_sub(used)
    }

    /// Check if a section can fit within remaining budget.
    pub fn can_fit(&self, tokens: usize) -> bool {
        self.remaining() >= tokens
    }

    /// Get utilization ratio.
    pub fn utilization(&self) -> f32 {
        if self.total_tokens == 0 {
            return 0.0;
        }
        let used: usize = self.allocated.values().sum();
        used as f32 / self.total_tokens as f32
    }

    /// Determine if content needs summarization based on available budget.
    pub fn needs_summarization(&self, content_tokens: usize) -> bool {
        !self.can_fit(content_tokens)
    }
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self::new(MODEL_CONTEXT_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_budget_allocation() {
        let mut budget = ContextBudget::new(200_000);
        // Total available = 200K * 90% = 180K
        assert_eq!(budget.total_tokens, 180_000);

        let actual = budget.allocate("tier1_essentials", 20_000);
        assert_eq!(actual, 20_000);
        // remaining() subtracts tier3 reserve (5% of 180K = 9K)
        // so remaining = 180K - 9K - 20K = 151K
        assert_eq!(budget.remaining(), 151_000);

        let actual = budget.allocate("tier2_modules", 50_000);
        assert_eq!(actual, 50_000);
        assert_eq!(budget.remaining(), 101_000);
    }

    #[test]
    fn test_context_budget_overflow_protection() {
        let mut budget = ContextBudget::new(100_000);
        // Total = 90K (90% of 100K), tier3 reserve = 5% of 90K = 4500
        // Non-tier3 ceiling = 90K - 4500 = 85500
        budget.allocate("section1", 80_000);
        // Effective remaining for non-tier3: 85500 - 80000 = 5500
        let actual = budget.allocate("section2", 20_000);
        assert_eq!(actual, 5_500);
    }

    #[test]
    fn test_context_budget_needs_summarization() {
        let mut budget = ContextBudget::new(100_000);
        // Total = 90K, tier3 reserve = 4500, non-tier3 ceiling = 85500
        budget.allocate("section1", 83_000);
        // remaining = 85500 - 83000 = 2500
        assert!(budget.needs_summarization(5_000));
        assert!(!budget.needs_summarization(2_000));
    }

    #[test]
    fn test_context_budget_with_custom_input_ratio() {
        let budget = ContextBudget::input_ratio(200_000, 0.80);
        assert_eq!(budget.total_tokens, 160_000);
        assert_eq!(budget.output_reserve, 40_000);
    }

    #[test]
    fn test_tier3_can_use_reserved_space() {
        let mut budget = ContextBudget::new(100_000);
        // Total = 90K, tier3 reserve = 4500
        budget.allocate("tier1_identity", 80_000);
        // Non-tier3 remaining = 85500 - 80000 = 5500
        // But tier3 gets full remaining = 90000 - 80000 = 10000
        let actual = budget.allocate("tier3_domain", 8_000);
        assert_eq!(actual, 8_000);
    }

    #[test]
    fn test_guaranteed_allocation() {
        let mut budget = ContextBudget::new(100_000);
        // Total = 90K, guaranteed tier1 = 80% of 90K = 72K
        let actual = budget.allocate_guaranteed("tier1_identity", 70_000);
        assert_eq!(actual, 70_000);

        // Only 2K left of tier1 guarantee
        let actual = budget.allocate_guaranteed("tier1_conventions", 5_000);
        assert_eq!(actual, 2_000);
    }

    #[test]
    fn test_estimate_tokens_ascii() {
        let text = "Hello, world! This is a test."; // 29 ASCII chars
        assert_eq!(estimate_tokens(text), 7); // 29 / 4.0 = 7.25 → 7
    }

    #[test]
    fn test_estimate_tokens_cjk() {
        // 6 CJK characters → 6 / 2.0 = 3 tokens
        let text = "\u{4F60}\u{597D}\u{4E16}\u{754C}\u{6D4B}\u{8BD5}"; // 你好世界测试
        assert_eq!(estimate_tokens(text), 3);
    }

    #[test]
    fn test_estimate_tokens_mixed() {
        // "Hello世界" = 5 ASCII + 2 CJK
        let text = "Hello\u{4E16}\u{754C}";
        // 5 / 4.0 + 2 / 2.0 = 1.25 + 1.0 = 2.25 → 2
        assert_eq!(estimate_tokens(text), 2);
    }

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_single_char() {
        // Single ASCII char: 1 / 4.0 = 0.25 → max(0, 1) = 1
        assert_eq!(estimate_tokens("a"), 1);
        // Single CJK char: 0 / 4.0 + 1 / 2.0 = 0.5 → max(0, 1) = 1
        assert_eq!(estimate_tokens("\u{4F60}"), 1);
    }

    #[test]
    fn test_allocate_flexible_fits() {
        let mut budget = ContextBudget::new(200_000);
        // Total = 180K, plenty of room
        let (actual, needs_summary) = budget.allocate_flexible("tier2_modules", 10_000);
        assert_eq!(actual, 10_000);
        assert!(!needs_summary);
    }

    #[test]
    fn test_allocate_flexible_overflow() {
        let mut budget = ContextBudget::new(100_000);
        // Total = 90K, tier3 reserve = 4500, non-tier3 ceiling = 85500
        budget.allocate("tier1_identity", 80_000);
        // remaining for non-tier3 = 85500 - 80000 = 5500
        let (actual, needs_summary) = budget.allocate_flexible("tier2_modules", 20_000);
        assert_eq!(actual, 5_500);
        assert!(needs_summary);
    }

    #[test]
    fn test_allocate_flexible_tier3_uses_reserve() {
        let mut budget = ContextBudget::new(100_000);
        // Total = 90K, tier3 reserve = 4500
        budget.allocate("tier1_identity", 80_000);
        // tier3 can access full remaining (90K - 80K = 10K)
        let (actual, needs_summary) = budget.allocate_flexible("tier3_domain", 8_000);
        assert_eq!(actual, 8_000);
        assert!(!needs_summary);
    }

    #[test]
    fn test_allocation_order_tier1_tier3_tier2() {
        let mut budget = ContextBudget::new(100_000);
        // Total = 90K

        // Tier1: guaranteed 80% = 72K
        let t1 = budget.allocate_guaranteed("tier1_identity", 50_000);
        assert_eq!(t1, 50_000);

        // Tier3: uses reserved space (can access 90K - 50K = 40K)
        let (t3, t3_summary) = budget.allocate_flexible("tier3_domain", 5_000);
        assert_eq!(t3, 5_000);
        assert!(!t3_summary);

        // Tier2: gets remaining after tier1+tier3 and respecting reserve
        // Non-tier3 ceiling = 90K - 4500 = 85500
        // Used = 50K + 5K = 55K
        // Remaining for tier2 = 85500 - 55000 = 30500
        let (t2, t2_summary) = budget.allocate_flexible("tier2_modules", 30_000);
        assert_eq!(t2, 30_000);
        assert!(!t2_summary);
    }

    #[test]
    fn test_context_limit_for_model() {
        use crate::constants::context::context_limit_for_model;

        assert_eq!(context_limit_for_model("claude-sonnet-4-5-20250929"), 200_000);
        assert_eq!(context_limit_for_model("claude-opus-4-6"), 200_000);
        assert_eq!(context_limit_for_model("gpt-4o"), 128_000);
        assert_eq!(context_limit_for_model("gpt-4-turbo"), 128_000);
        assert_eq!(context_limit_for_model("o1-preview"), 200_000);
        assert_eq!(context_limit_for_model("gemini-pro"), 200_000);
        // Unknown model gets full window (conservative: over-estimate rather than under-estimate)
        assert_eq!(context_limit_for_model("custom-model"), 200_000);
    }
}
