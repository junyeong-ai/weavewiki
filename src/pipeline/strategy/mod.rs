//! Refinement Strategy Module
//!
//! Provides pluggable strategies for iterative quality improvement.
//! Strategies operate on artifacts with full GenerationContext for
//! context-aware refinement based on source insights.

mod evidence;
mod regeneration;
mod semantic;

pub use evidence::{EvidenceResult, EvidenceStrategy};
pub use regeneration::RegenerationStrategy;
pub use semantic::SemanticStrategy;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::ai::LlmProvider;
use crate::config::RefinementStrategyType;
use crate::types::{Agent, DiagnosticLevel, Result, Rule, Skill};

use super::context::VerifiedFileRegistry;

/// Issue kinds that can trigger refinement
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

impl From<&super::refinement::DetectedIssue> for IssueKind {
    fn from(issue: &super::refinement::DetectedIssue) -> Self {
        use super::refinement::DetectedIssue as DI;
        match issue {
            DI::LowActionability { .. } => Self::LowActionability,
            DI::TooGeneric { .. } => Self::TooGeneric,
            DI::WeakEvidence { .. } => Self::WeakEvidence,
            DI::MissingReferences { .. } => Self::MissingReferences,
            DI::Shallow { .. } => Self::Shallow,
            DI::TooShort { .. } => Self::TooShort,
            DI::MissingSections { .. } => Self::MissingSections,
            DI::Redundant { .. } => Self::Redundant,
            DI::Tier1Content { .. } => Self::Tier1Content,
            DI::PlanMismatch => Self::PlanMismatch,
            DI::MissingModule { .. } => Self::MissingModule,
            DI::PartialModuleCoverage { .. } => Self::PartialModuleCoverage,
        }
    }
}

/// A refinement issue detected in an artifact
#[derive(Debug, Clone)]
pub struct StrategyIssue {
    pub kind: IssueKind,
    pub severity: DiagnosticLevel,
    pub message: String,
    pub suggestion: Option<String>,
}

impl StrategyIssue {
    pub fn new(kind: IssueKind, severity: DiagnosticLevel, message: impl Into<String>) -> Self {
        Self {
            kind,
            severity,
            message: message.into(),
            suggestion: None,
        }
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn error(kind: IssueKind, message: impl Into<String>) -> Self {
        Self::new(kind, DiagnosticLevel::Error, message)
    }

    pub fn warning(kind: IssueKind, message: impl Into<String>) -> Self {
        Self::new(kind, DiagnosticLevel::Warning, message)
    }
}

/// Rich context for refinement strategies
///
/// Provides file registry and issue context for strategy-based refinement.
#[derive(Debug, Clone)]
pub struct StrategyContext<'a> {
    /// Verified file registry for reference validation
    pub file_registry: &'a VerifiedFileRegistry,
    /// Current refinement issues to address
    pub issues: Vec<StrategyIssue>,
    /// Pre-computed suggestions for improvement
    pub suggestions: Vec<String>,
    /// Validation feedback from previous passes
    pub validation_feedback: Option<ValidationFeedback>,
    /// Minimum quality improvement required for acceptance
    pub quality_acceptance_delta: f32,
}

impl<'a> StrategyContext<'a> {
    /// Create a new strategy context with minimal requirements
    pub fn new(file_registry: &'a VerifiedFileRegistry) -> Self {
        Self {
            file_registry,
            issues: Vec::new(),
            suggestions: Vec::new(),
            validation_feedback: None,
            quality_acceptance_delta: 0.02,
        }
    }

    /// Add refinement issues to address
    pub fn with_issues(mut self, issues: Vec<StrategyIssue>) -> Self {
        self.issues = issues;
        self
    }

    /// Set pre-computed suggestions
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
        self
    }

    /// Set validation feedback
    pub fn with_validation_feedback(mut self, feedback: ValidationFeedback) -> Self {
        self.validation_feedback = Some(feedback);
        self
    }

    /// Set quality acceptance delta
    pub fn with_acceptance_delta(mut self, delta: f32) -> Self {
        self.quality_acceptance_delta = delta;
        self
    }

    /// Format source insights for inclusion in prompts (reserved for future use)
    pub fn format_source_insights(&self) -> String {
        String::new()
    }

    /// Format current issues for prompt inclusion
    pub fn format_issues(&self) -> String {
        if self.issues.is_empty() {
            return String::new();
        }

        self.issues
            .iter()
            .map(|i| {
                if let Some(ref suggestion) = i.suggestion {
                    format!("[{}] {}\n  Suggestion: {}", i.severity, i.message, suggestion)
                } else {
                    format!("[{}] {}", i.severity, i.message)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format project context for prompts (reserved for future use)
    pub fn format_project_context(&self) -> String {
        String::new()
    }

    /// Generate suggestions section for prompts
    pub fn suggestions_section(&self, default: &str) -> String {
        if self.suggestions.is_empty() {
            default.to_string()
        } else {
            self.suggestions
                .iter()
                .map(|s| format!("- {}", s))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    /// Generate feedback section for prompts
    pub fn feedback_section(&self) -> String {
        self.validation_feedback
            .as_ref()
            .map(|f| f.to_prompt_section())
            .unwrap_or_default()
    }
}

/// Feedback from validation to guide targeted refinement
#[derive(Debug, Clone, Default)]
pub struct ValidationFeedback {
    pub missing_modules: Vec<String>,
    pub weak_coverage_areas: Vec<String>,
    pub module_constraints: HashMap<String, Vec<String>>,
}

impl ValidationFeedback {
    pub fn to_prompt_section(&self) -> String {
        let mut parts = Vec::new();

        if !self.missing_modules.is_empty() {
            parts.push(format!(
                "MISSING MODULE COVERAGE:\n{}",
                self.missing_modules
                    .iter()
                    .map(|m| format!("- {}", m))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.weak_coverage_areas.is_empty() {
            parts.push(format!(
                "WEAK COVERAGE AREAS:\n{}",
                self.weak_coverage_areas
                    .iter()
                    .map(|a| format!("- {}", a))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.module_constraints.is_empty() {
            let constraints: Vec<String> = self
                .module_constraints
                .iter()
                .flat_map(|(module, constraints)| {
                    constraints
                        .iter()
                        .map(move |c| format!("- {}: {}", module, c))
                })
                .collect();
            parts.push(format!("MODULE CONSTRAINTS:\n{}", constraints.join("\n")));
        }

        parts.join("\n\n")
    }
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
    history: HashMap<(String, IssueKind), Vec<StrategyAttempt>>,
    max_history_per_item: usize,
    escalation_level: usize,
}

#[derive(Debug, Clone)]
pub struct StrategyAttempt {
    pub strategy_name: String,
    pub success: bool,
    pub quality_delta: f32,
    pub iteration: usize,
}

impl StrategyRotator {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self::with_strategies(
            provider,
            &[
                RefinementStrategyType::Semantic,
                RefinementStrategyType::Evidence,
                RefinementStrategyType::Regeneration,
            ],
        )
    }

    pub fn with_strategies(
        provider: Arc<dyn LlmProvider>,
        strategy_types: &[RefinementStrategyType],
    ) -> Self {
        let mut strategies: Vec<Arc<dyn RefinementStrategy>> = strategy_types
            .iter()
            .map(|strategy_type| Self::create_strategy(&provider, *strategy_type))
            .collect();

        // Ensure at least one strategy exists - add default if empty
        if strategies.is_empty() {
            tracing::warn!(
                "StrategyRotator configured with zero strategies - adding default SemanticStrategy"
            );
            strategies.push(Self::create_strategy(
                &provider,
                RefinementStrategyType::Semantic,
            ));
        }

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
            .expect("StrategyRotator must have at least one strategy (enforced at construction)")
    }

    fn recently_failed(&self, history: Option<&Vec<StrategyAttempt>>, strategy_name: &str) -> bool {
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

    fn best_historical_strategy(&self, history: Option<&Vec<StrategyAttempt>>) -> Option<String> {
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

    pub fn record_outcome(&mut self, item_name: &str, issue: &IssueKind, outcome: StrategyAttempt) {
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
    use super::file_reference;
    use super::patterns::{
        ACTIONABLE_PATTERN, GENERIC_PATTERN, count_generic_patterns, count_value_indicators,
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

    let file_refs = file_reference::count_references(content);
    let file_line_refs = file_reference::count_references_with_lines(content);
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

    let unique_words: std::collections::HashSet<&str> =
        content.split_whitespace().filter(|w| w.len() > 3).collect();
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
    let base_quality = calculate_quick_quality(content);

    let refs = super::file_reference::extract_references(content);
    if refs.is_empty() {
        return base_quality;
    }

    let valid_refs = refs.iter().filter(|r| registry.contains(&r.path)).count();

    let validity_ratio = valid_refs as f32 / refs.len() as f32;
    let evidence_adjustment = (validity_ratio - 0.5) * 0.1;

    (base_quality + evidence_adjustment).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_kind_conversion() {
        use super::super::refinement::DetectedIssue as DI;

        let issue = DI::LowActionability {
            score: 0.3,
            threshold: 0.6,
        };
        assert_eq!(IssueKind::from(&issue), IssueKind::LowActionability);

        let issue = DI::TooGeneric {
            description: "test".to_string(),
        };
        assert_eq!(IssueKind::from(&issue), IssueKind::TooGeneric);
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
