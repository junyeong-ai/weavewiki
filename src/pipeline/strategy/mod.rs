//! Refinement Strategy Module
//!
//! Provides pluggable strategies for iterative quality improvement.
//! Strategies are selected based on issue type and historical success rates.

mod evidence;
mod regeneration;
mod semantic;
mod verification;

pub use evidence::EvidenceStrategy;
pub use regeneration::RegenerationStrategy;
pub use semantic::SemanticStrategy;
pub use verification::{PostStrategyVerifier, VerificationMetrics, VerificationResult};

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::ai::LlmProvider;
use crate::config::RefinementStrategyType;
use crate::types::{Agent, Result, Rule, Skill};

use super::context::VerifiedFileRegistry;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IssueKind {
    LowActionability,
    TooGeneric,
    WeakEvidence,
    MissingReferences,
    Shallow,
    TooShort,
    MissingSections,
    Redundant,
    Tier1Content,
    PlanMismatch,
    MissingModule,
    PartialModuleCoverage,
}

impl IssueKind {
    pub fn from_refinement_issue(issue: &super::refinement::IssueKind) -> Self {
        use super::refinement::IssueKind as RI;
        match issue {
            RI::LowActionability { .. } => Self::LowActionability,
            RI::TooGeneric { .. } => Self::TooGeneric,
            RI::WeakEvidence { .. } => Self::WeakEvidence,
            RI::MissingReferences { .. } => Self::MissingReferences,
            RI::Shallow { .. } => Self::Shallow,
            RI::TooShort { .. } => Self::TooShort,
            RI::MissingSections { .. } => Self::MissingSections,
            RI::Redundant { .. } => Self::Redundant,
            RI::Tier1Content { .. } => Self::Tier1Content,
            RI::PlanMismatch => Self::PlanMismatch,
            RI::MissingModule { .. } => Self::MissingModule,
            RI::PartialModuleCoverage { .. } => Self::PartialModuleCoverage,
        }
    }
}

#[derive(Debug)]
pub enum ItemContent<'a> {
    Skill(&'a mut Skill),
    Agent(&'a mut Agent),
    Rule(&'a mut Rule),
}

#[derive(Debug, Clone)]
pub struct StrategyContext<'a> {
    pub file_registry: &'a VerifiedFileRegistry,
    pub issue_description: String,
    pub suggestions: Vec<String>,
    pub validation_feedback: Option<ValidationFeedback>,
}

/// Feedback from validation to guide targeted refinement
#[derive(Debug, Clone, Default)]
pub struct ValidationFeedback {
    pub missing_modules: Vec<String>,
    pub weak_coverage_areas: Vec<String>,
    pub module_constraints: std::collections::HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct StrategyResult {
    pub success: bool,
    pub quality_delta: f32,
    pub changes_made: Vec<String>,
}

impl Default for StrategyResult {
    fn default() -> Self {
        Self {
            success: false,
            quality_delta: 0.0,
            changes_made: Vec::new(),
        }
    }
}

#[async_trait]
pub trait RefinementStrategy: Send + Sync {
    fn name(&self) -> &str;

    fn applicable_to(&self, issue: &IssueKind) -> bool;

    fn priority(&self) -> u8 {
        50
    }

    async fn refine_skill(
        &self,
        skill: &mut Skill,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult>;

    async fn refine_agent(
        &self,
        agent: &mut Agent,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult>;

    async fn refine_rule(
        &self,
        _rule: &mut Rule,
        _context: &StrategyContext<'_>,
    ) -> Result<StrategyResult> {
        Ok(StrategyResult::default())
    }
}

pub struct StrategyRotator {
    strategies: Vec<Arc<dyn RefinementStrategy>>,
    history: HashMap<(String, IssueKind), Vec<StrategyOutcome>>,
    max_history_per_item: usize,
    escalation_level: usize,
}

#[derive(Debug, Clone)]
pub struct StrategyOutcome {
    pub strategy_name: String,
    pub success: bool,
    pub quality_delta: f32,
    pub iteration: usize,
}

impl StrategyRotator {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self::with_strategies(provider, &[
            RefinementStrategyType::Semantic,
            RefinementStrategyType::Evidence,
            RefinementStrategyType::Regeneration,
        ])
    }

    pub fn with_strategies(
        provider: Arc<dyn LlmProvider>,
        strategy_types: &[RefinementStrategyType],
    ) -> Self {
        let strategies: Vec<Arc<dyn RefinementStrategy>> = strategy_types
            .iter()
            .map(|strategy_type| Self::create_strategy(&provider, *strategy_type))
            .collect();

        assert!(
            !strategies.is_empty(),
            "StrategyRotator requires at least one strategy"
        );

        Self {
            strategies,
            history: HashMap::new(),
            max_history_per_item: 10,
            escalation_level: 0,
        }
    }

    fn create_strategy(
        provider: &Arc<dyn LlmProvider>,
        strategy_type: RefinementStrategyType,
    ) -> Arc<dyn RefinementStrategy> {
        match strategy_type {
            RefinementStrategyType::Semantic => {
                Arc::new(SemanticStrategy::new(Arc::clone(provider)))
            }
            RefinementStrategyType::Evidence => {
                Arc::new(EvidenceStrategy::new(Arc::clone(provider)))
            }
            RefinementStrategyType::Regeneration => {
                Arc::new(RegenerationStrategy::new(Arc::clone(provider)))
            }
        }
    }

    pub fn escalate(&mut self) {
        self.escalation_level =
            (self.escalation_level + 1).min(self.strategies.len().saturating_sub(1));
        self.history.clear();
        tracing::debug!(
            escalation_level = self.escalation_level,
            "Strategy escalation applied"
        );
    }

    pub fn force_regeneration(&mut self) {
        self.escalation_level = self.strategies.len().saturating_sub(1);
        self.history.clear();
        tracing::info!("Forced regeneration mode - using most aggressive strategy");
    }

    pub fn reset_escalation(&mut self) {
        self.escalation_level = 0;
    }

    pub fn select_strategy(
        &self,
        item_name: &str,
        issue: &IssueKind,
    ) -> Arc<dyn RefinementStrategy> {
        let key = (item_name.to_string(), issue.clone());
        let history = self.history.get(&key);

        let mut candidates: Vec<_> = self
            .strategies
            .iter()
            .filter(|s| s.applicable_to(issue))
            .filter(|s| !self.recently_failed(history, s.name()))
            .cloned()
            .collect();

        candidates.sort_by_key(|s| std::cmp::Reverse(s.priority()));

        if self.escalation_level > 0 && candidates.len() > self.escalation_level {
            candidates = candidates.into_iter().skip(self.escalation_level).collect();
        }

        if let Some(best_strategy) = self.best_historical_strategy(history)
            && let Some(strategy) = candidates
                .iter()
                .find(|s| s.name() == best_strategy.as_str())
            {
                return Arc::clone(strategy);
            }

        candidates
            .into_iter()
            .next()
            .or_else(|| self.strategies.last().cloned())
            .expect("StrategyRotator must have at least one strategy")
    }

    fn recently_failed(&self, history: Option<&Vec<StrategyOutcome>>, strategy_name: &str) -> bool {
        let Some(outcomes) = history else {
            return false;
        };

        let recent_failures = outcomes
            .iter()
            .rev()
            .take(3)
            .filter(|o| o.strategy_name == strategy_name && !o.success)
            .count();

        recent_failures >= 2
    }

    fn best_historical_strategy(&self, history: Option<&Vec<StrategyOutcome>>) -> Option<String> {
        let outcomes = history?;

        let mut success_rates: HashMap<String, (usize, usize)> = HashMap::new();

        for outcome in outcomes {
            let entry = success_rates
                .entry(outcome.strategy_name.clone())
                .or_insert((0, 0));
            entry.1 += 1;
            if outcome.success {
                entry.0 += 1;
            }
        }

        success_rates
            .into_iter()
            .filter(|(_, (successes, total))| *total >= 2 && *successes > 0)
            .max_by(|(_, (s1, t1)), (_, (s2, t2))| {
                let rate1 = *s1 as f32 / *t1 as f32;
                let rate2 = *s2 as f32 / *t2 as f32;
                rate1
                    .partial_cmp(&rate2)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(name, _)| name)
    }

    pub fn record_outcome(&mut self, item_name: &str, issue: &IssueKind, outcome: StrategyOutcome) {
        let key = (item_name.to_string(), issue.clone());
        let history = self.history.entry(key).or_default();

        history.push(outcome);

        if history.len() > self.max_history_per_item {
            history.remove(0);
        }
    }

    pub fn get_all_strategies(&self) -> &[Arc<dyn RefinementStrategy>] {
        &self.strategies
    }

    pub fn get_strategy_by_name(&self, name: &str) -> Option<Arc<dyn RefinementStrategy>> {
        self.strategies.iter().find(|s| s.name() == name).cloned()
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

pub fn calculate_quick_quality(content: &str) -> f32 {
    use super::patterns::{
        count_generic_patterns, count_value_indicators, ACTIONABLE_PATTERN, FILE_LINE_REF, FILE_REF,
        GENERIC_PATTERN,
    };

    if content.trim().is_empty() {
        return 0.0;
    }

    let total_lines: Vec<&str> = content
        .lines()
        .filter(|l| {
            let trimmed = l.trim();
            trimmed.len() >= 10 && !trimmed.starts_with('#') && !trimmed.starts_with("```")
        })
        .collect();

    let actionable_lines = total_lines
        .iter()
        .filter(|l| ACTIONABLE_PATTERN.is_match(l))
        .count();

    let actionability = if !total_lines.is_empty() {
        (actionable_lines as f32 / total_lines.len() as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let generic_count = content
        .lines()
        .filter(|l| GENERIC_PATTERN.is_match(l))
        .count();
    let specific_segments = count_value_indicators(content);
    let generic_pattern_count = count_generic_patterns(content);

    let specificity = if !total_lines.is_empty() {
        let generic_ratio = generic_count as f32 / total_lines.len() as f32;
        let specific_ratio = (specific_segments as f32 / 10.0).min(1.0);
        let generic_penalty = (generic_pattern_count as f32 * 0.1).min(0.5);
        ((1.0 - generic_ratio) * 0.5 + specific_ratio * 0.5 - generic_penalty).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let file_refs = FILE_REF.captures_iter(content).count();
    let file_line_refs = FILE_LINE_REF.captures_iter(content).count();
    let evidence = if file_refs > 0 {
        let line_ratio = file_line_refs as f32 / file_refs as f32;
        (file_refs.min(5) as f32 / 5.0 * 0.5 + line_ratio * 0.5).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let section_count = content.matches("##").count();
    let example_count = content.matches("```").count() / 2;
    let depth = ((section_count.min(5) as f32 / 5.0) * 0.5
        + (example_count.min(3) as f32 / 3.0) * 0.5)
        .clamp(0.0, 1.0);

    let unique_words: std::collections::HashSet<&str> = content
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .collect();
    let total_words = content.split_whitespace().filter(|w| w.len() > 3).count();
    let redundancy = if total_words > 0 {
        1.0 - (unique_words.len() as f32 / total_words as f32)
    } else {
        0.0
    };

    let w = crate::config::SemanticDimensionWeights::default();
    let combined = actionability * w.actionability
        + specificity * w.specificity
        + evidence * w.evidence
        + (1.0 - redundancy) * w.redundancy
        + depth * w.depth;

    combined.clamp(0.0, 1.0)
}

pub fn calculate_validated_quality(content: &str, registry: &VerifiedFileRegistry) -> f32 {
    use super::patterns::FILE_REF;

    let base_quality = calculate_quick_quality(content);

    let file_refs: Vec<&str> = FILE_REF
        .captures_iter(content)
        .filter_map(|c| c.get(1).map(|m| m.as_str()))
        .collect();

    if file_refs.is_empty() {
        return base_quality;
    }

    let valid_refs = file_refs
        .iter()
        .filter(|path| registry.contains(path))
        .count();

    let validity_ratio = valid_refs as f32 / file_refs.len() as f32;
    let evidence_adjustment = (validity_ratio - 0.5) * 0.1;

    (base_quality + evidence_adjustment).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_kind_conversion() {
        use super::super::refinement::IssueKind as RI;

        let issue = RI::LowActionability {
            score: 0.3,
            threshold: 0.6,
        };
        assert_eq!(
            IssueKind::from_refinement_issue(&issue),
            IssueKind::LowActionability
        );

        let issue = RI::TooGeneric {
            description: "test".to_string(),
        };
        assert_eq!(
            IssueKind::from_refinement_issue(&issue),
            IssueKind::TooGeneric
        );
    }

    #[test]
    fn test_quick_quality_calculation() {
        let content = "You must use @src/main.rs:10 and should avoid direct access.
## Example
```rust
// good code
```";
        let score = calculate_quick_quality(content);
        assert!(score > 0.2, "Expected score > 0.2, got {}", score);

        let rich_content = "You must always prefer @src/main.rs:10 over alternatives.
You should use @src/lib.rs:20 and must avoid direct access.
## Overview
Documentation section.
## Requirements
- First step
## Example
```rust
fn main() {}
```
## Gotchas
Things to avoid.";
        let rich_score = calculate_quick_quality(rich_content);
        assert!(
            rich_score > 0.5,
            "Expected rich_score > 0.5, got {}",
            rich_score
        );

        let empty_content = "";
        assert_eq!(calculate_quick_quality(empty_content), 0.0);
    }
}
