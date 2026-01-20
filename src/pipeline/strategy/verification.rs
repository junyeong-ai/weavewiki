//! Post-Strategy Verification Module
//!
//! Verifies that a strategy actually resolved the targeted issue immediately
//! after application, rather than waiting for the next iteration.

use super::{IssueKind, calculate_quick_quality};
use crate::config::SemanticDimensionWeights;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::patterns::{
    ACTIONABLE_PATTERN, FILE_LINE_REF, FILE_REF, GENERIC_PATTERN,
    count_generic_patterns, count_value_indicators,
};

/// Result of verifying whether a strategy resolved its targeted issue
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Whether the issue appears to be resolved
    pub resolved: bool,
    /// Quality improvement (positive) or degradation (negative)
    pub quality_delta: f32,
    /// Metric-specific details for logging/debugging
    pub details: String,
    /// Specific metric before/after values
    pub metrics: VerificationMetrics,
}

impl VerificationResult {
    pub fn success(quality_delta: f32, details: impl Into<String>) -> Self {
        Self {
            resolved: true,
            quality_delta,
            details: details.into(),
            metrics: VerificationMetrics::default(),
        }
    }

    pub fn failure(details: impl Into<String>) -> Self {
        Self {
            resolved: false,
            quality_delta: 0.0,
            details: details.into(),
            metrics: VerificationMetrics::default(),
        }
    }

    pub fn with_metrics(mut self, metrics: VerificationMetrics) -> Self {
        self.metrics = metrics;
        self
    }
}

/// Detailed metrics for verification
#[derive(Debug, Clone, Default)]
pub struct VerificationMetrics {
    pub before_value: f32,
    pub after_value: f32,
    pub threshold: f32,
    pub metric_name: String,
}

impl VerificationMetrics {
    pub fn new(metric_name: impl Into<String>, before: f32, after: f32, threshold: f32) -> Self {
        Self {
            metric_name: metric_name.into(),
            before_value: before,
            after_value: after,
            threshold,
        }
    }
}

/// Verifies strategy outcomes immediately after application
pub struct PostStrategyVerifier {
    thresholds: SemanticDimensionWeights,
}

impl PostStrategyVerifier {
    pub fn new(thresholds: SemanticDimensionWeights) -> Self {
        Self { thresholds }
    }

    /// Verify that a strategy actually resolved the targeted issue
    pub fn verify_issue_resolution(
        &self,
        issue: &IssueKind,
        before: &str,
        after: &str,
        file_registry: Option<&VerifiedFileRegistry>,
    ) -> VerificationResult {
        match issue {
            IssueKind::WeakEvidence | IssueKind::MissingReferences => {
                self.verify_evidence_improvement(before, after, file_registry)
            }
            IssueKind::LowActionability => {
                self.verify_actionability_improvement(before, after)
            }
            IssueKind::TooGeneric => {
                self.verify_specificity_improvement(before, after)
            }
            IssueKind::Shallow => {
                self.verify_depth_improvement(before, after)
            }
            IssueKind::TooShort => {
                self.verify_length_improvement(before, after)
            }
            IssueKind::MissingSections => {
                self.verify_sections_improvement(before, after)
            }
            IssueKind::Redundant => {
                self.verify_redundancy_improvement(before, after)
            }
            IssueKind::Tier1Content => {
                self.verify_tier1_removal(before, after)
            }
            IssueKind::MissingModule | IssueKind::PartialModuleCoverage => {
                // Module coverage is verified at a higher level
                self.verify_general_quality_improvement(before, after)
            }
            IssueKind::PlanMismatch => {
                // Plan alignment is verified elsewhere
                self.verify_general_quality_improvement(before, after)
            }
        }
    }

    /// Verify evidence/reference improvement
    fn verify_evidence_improvement(
        &self,
        before: &str,
        after: &str,
        file_registry: Option<&VerifiedFileRegistry>,
    ) -> VerificationResult {
        let before_refs = self.count_file_refs(before);
        let after_refs = self.count_file_refs(after);

        // If registry available, only count valid refs
        let (before_valid, after_valid) = if let Some(registry) = file_registry {
            (
                self.count_valid_refs(before, registry),
                self.count_valid_refs(after, registry),
            )
        } else {
            (before_refs, after_refs)
        };

        let before_line_refs = self.count_line_refs(before);
        let after_line_refs = self.count_line_refs(after);

        let improvement = (after_valid as i32 - before_valid as i32) as f32 * 0.1
            + (after_line_refs as i32 - before_line_refs as i32) as f32 * 0.05;

        let metrics = VerificationMetrics::new(
            "valid_refs",
            before_valid as f32,
            after_valid as f32,
            2.0, // Minimum 2 valid refs expected
        );

        if after_valid > before_valid {
            VerificationResult {
                resolved: true,
                quality_delta: improvement,
                details: format!(
                    "Refs improved: {} → {} (line refs: {} → {})",
                    before_valid, after_valid, before_line_refs, after_line_refs
                ),
                metrics,
            }
        } else {
            VerificationResult {
                resolved: false,
                quality_delta: 0.0,
                details: format!(
                    "Refs unchanged: {} → {} (expected increase)",
                    before_valid, after_valid
                ),
                metrics,
            }
        }
    }

    /// Verify actionability improvement
    fn verify_actionability_improvement(&self, before: &str, after: &str) -> VerificationResult {
        let before_score = self.calculate_actionability(before);
        let after_score = self.calculate_actionability(after);

        let threshold = 0.6; // Standard actionability threshold
        let improvement = after_score - before_score;

        let metrics = VerificationMetrics::new("actionability", before_score, after_score, threshold);

        if after_score >= threshold || improvement > 0.05 {
            VerificationResult {
                resolved: after_score >= threshold,
                quality_delta: improvement,
                details: format!("Actionability: {:.2} → {:.2} (threshold: {:.2})", before_score, after_score, threshold),
                metrics,
            }
        } else {
            VerificationResult {
                resolved: false,
                quality_delta: 0.0,
                details: format!(
                    "Actionability insufficient: {:.2} → {:.2} (need {:.2})",
                    before_score, after_score, threshold
                ),
                metrics,
            }
        }
    }

    /// Verify specificity improvement (reduced generic content)
    fn verify_specificity_improvement(&self, before: &str, after: &str) -> VerificationResult {
        let before_generic = self.count_generic_phrases(before);
        let after_generic = self.count_generic_phrases(after);

        let before_specific = count_value_indicators(before);
        let after_specific = count_value_indicators(after);

        let generic_reduction = before_generic.saturating_sub(after_generic);
        let specific_increase = after_specific.saturating_sub(before_specific);

        let improvement = generic_reduction as f32 * 0.05 + specific_increase as f32 * 0.03;

        let metrics = VerificationMetrics::new(
            "generic_count",
            before_generic as f32,
            after_generic as f32,
            0.0, // Lower is better
        );

        if after_generic < before_generic || after_specific > before_specific {
            VerificationResult {
                resolved: true,
                quality_delta: improvement,
                details: format!(
                    "Generic: {} → {}, Specific indicators: {} → {}",
                    before_generic, after_generic, before_specific, after_specific
                ),
                metrics,
            }
        } else {
            VerificationResult {
                resolved: false,
                quality_delta: 0.0,
                details: format!(
                    "Specificity unchanged: generic {} → {}, specific {} → {}",
                    before_generic, after_generic, before_specific, after_specific
                ),
                metrics,
            }
        }
    }

    /// Verify depth improvement
    fn verify_depth_improvement(&self, before: &str, after: &str) -> VerificationResult {
        let before_depth = self.calculate_depth_indicators(before);
        let after_depth = self.calculate_depth_indicators(after);

        let improvement = (after_depth as f32 - before_depth as f32) * 0.05;

        let metrics = VerificationMetrics::new(
            "depth_indicators",
            before_depth as f32,
            after_depth as f32,
            3.0, // Minimum 3 depth indicators expected
        );

        if after_depth > before_depth {
            VerificationResult {
                resolved: true,
                quality_delta: improvement,
                details: format!("Depth indicators: {} → {}", before_depth, after_depth),
                metrics,
            }
        } else {
            VerificationResult {
                resolved: false,
                quality_delta: 0.0,
                details: format!(
                    "Depth unchanged: {} → {} (expected increase)",
                    before_depth, after_depth
                ),
                metrics,
            }
        }
    }

    /// Verify length improvement
    fn verify_length_improvement(&self, before: &str, after: &str) -> VerificationResult {
        let before_lines = before.lines().filter(|l| !l.trim().is_empty()).count();
        let after_lines = after.lines().filter(|l| !l.trim().is_empty()).count();

        let min_lines = self.thresholds.min_substantive_lines_deep;
        let metrics = VerificationMetrics::new(
            "line_count",
            before_lines as f32,
            after_lines as f32,
            min_lines as f32,
        );

        if after_lines >= min_lines && after_lines > before_lines {
            VerificationResult {
                resolved: true,
                quality_delta: 0.1,
                details: format!(
                    "Lines: {} → {} (min: {})",
                    before_lines, after_lines, min_lines
                ),
                metrics,
            }
        } else if after_lines >= min_lines {
            VerificationResult {
                resolved: true,
                quality_delta: 0.05,
                details: format!("Lines meet minimum: {} (min: {})", after_lines, min_lines),
                metrics,
            }
        } else {
            VerificationResult {
                resolved: false,
                quality_delta: 0.0,
                details: format!(
                    "Lines insufficient: {} → {} (need {})",
                    before_lines, after_lines, min_lines
                ),
                metrics,
            }
        }
    }

    /// Verify sections improvement
    fn verify_sections_improvement(&self, before: &str, after: &str) -> VerificationResult {
        let before_sections = before.matches("##").count();
        let after_sections = after.matches("##").count();

        let min_sections = 2;
        let metrics = VerificationMetrics::new(
            "section_count",
            before_sections as f32,
            after_sections as f32,
            min_sections as f32,
        );

        if after_sections > before_sections && after_sections >= min_sections {
            VerificationResult {
                resolved: true,
                quality_delta: (after_sections - before_sections) as f32 * 0.05,
                details: format!("Sections: {} → {}", before_sections, after_sections),
                metrics,
            }
        } else if after_sections >= min_sections {
            VerificationResult {
                resolved: true,
                quality_delta: 0.0,
                details: format!("Sections adequate: {}", after_sections),
                metrics,
            }
        } else {
            VerificationResult {
                resolved: false,
                quality_delta: 0.0,
                details: format!(
                    "Sections insufficient: {} → {} (need {})",
                    before_sections, after_sections, min_sections
                ),
                metrics,
            }
        }
    }

    /// Verify redundancy reduction
    fn verify_redundancy_improvement(&self, before: &str, after: &str) -> VerificationResult {
        let before_redundancy = self.calculate_redundancy(before);
        let after_redundancy = self.calculate_redundancy(after);

        let improvement = before_redundancy - after_redundancy;
        let max_redundancy = 0.3;

        let metrics = VerificationMetrics::new(
            "redundancy",
            before_redundancy,
            after_redundancy,
            max_redundancy,
        );

        if after_redundancy < before_redundancy {
            VerificationResult {
                resolved: after_redundancy <= max_redundancy,
                quality_delta: improvement * 0.5,
                details: format!("Redundancy: {:.2} → {:.2}", before_redundancy, after_redundancy),
                metrics,
            }
        } else {
            VerificationResult {
                resolved: false,
                quality_delta: 0.0,
                details: format!(
                    "Redundancy increased: {:.2} → {:.2}",
                    before_redundancy, after_redundancy
                ),
                metrics,
            }
        }
    }

    /// Verify generic content removal
    fn verify_tier1_removal(&self, before: &str, after: &str) -> VerificationResult {
        let before_generic = count_generic_patterns(before);
        let after_generic = count_generic_patterns(after);

        let metrics = VerificationMetrics::new(
            "generic_patterns",
            before_generic as f32,
            after_generic as f32,
            0.0,
        );

        if after_generic < before_generic {
            VerificationResult {
                resolved: after_generic == 0,
                quality_delta: (before_generic - after_generic) as f32 * 0.1,
                details: format!("Generic patterns: {} → {}", before_generic, after_generic),
                metrics,
            }
        } else if after_generic == 0 {
            VerificationResult::success(0.0, "No generic content")
                .with_metrics(metrics)
        } else {
            VerificationResult {
                resolved: false,
                quality_delta: 0.0,
                details: format!("Generic content remains: {}", after_generic),
                metrics,
            }
        }
    }

    /// General quality improvement check for unspecific issues
    fn verify_general_quality_improvement(&self, before: &str, after: &str) -> VerificationResult {
        let before_quality = calculate_quick_quality(before);
        let after_quality = calculate_quick_quality(after);

        let improvement = after_quality - before_quality;
        let min_improvement = 0.02; // 2% minimum improvement

        let metrics = VerificationMetrics::new(
            "overall_quality",
            before_quality,
            after_quality,
            0.75, // Target quality
        );

        if improvement >= min_improvement {
            VerificationResult {
                resolved: true,
                quality_delta: improvement,
                details: format!("Quality: {:.2} → {:.2} (+{:.2})", before_quality, after_quality, improvement),
                metrics,
            }
        } else if improvement > 0.0 {
            VerificationResult {
                resolved: true, // Small improvement is still improvement
                quality_delta: improvement,
                details: format!("Quality marginally improved: {:.2} → {:.2}", before_quality, after_quality),
                metrics,
            }
        } else {
            VerificationResult {
                resolved: false,
                quality_delta: improvement,
                details: format!("Quality unchanged or degraded: {:.2} → {:.2}", before_quality, after_quality),
                metrics,
            }
        }
    }

    // Helper methods

    fn count_file_refs(&self, content: &str) -> usize {
        FILE_REF.captures_iter(content).count()
    }

    fn count_line_refs(&self, content: &str) -> usize {
        FILE_LINE_REF.captures_iter(content).count()
    }

    fn count_valid_refs(&self, content: &str, registry: &VerifiedFileRegistry) -> usize {
        FILE_REF
            .captures_iter(content)
            .filter(|cap| {
                let path = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                registry.contains(path)
            })
            .count()
    }

    fn calculate_actionability(&self, content: &str) -> f32 {
        let lines: Vec<&str> = content
            .lines()
            .filter(|l| l.trim().len() >= self.thresholds.min_line_length_actionability)
            .collect();

        if lines.is_empty() {
            return 0.0;
        }

        let actionable = lines.iter().filter(|l| ACTIONABLE_PATTERN.is_match(l)).count();
        actionable as f32 / lines.len() as f32
    }

    fn count_generic_phrases(&self, content: &str) -> usize {
        content
            .lines()
            .filter(|l| GENERIC_PATTERN.is_match(l))
            .count()
    }

    fn calculate_depth_indicators(&self, content: &str) -> usize {
        let sections = content.matches("##").count();
        let code_blocks = content.matches("```").count() / 2;
        let file_refs = self.count_file_refs(content);
        let examples = content.to_lowercase().matches("example").count();
        let rationales = content.to_lowercase().matches("why").count()
            + content.to_lowercase().matches("because").count();

        sections + code_blocks + file_refs.min(5) + examples + rationales
    }

    fn calculate_redundancy(&self, content: &str) -> f32 {
        let words: Vec<&str> = content
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .collect();

        if words.is_empty() {
            return 0.0;
        }

        let unique: std::collections::HashSet<&str> = words.iter().copied().collect();
        1.0 - (unique.len() as f32 / words.len() as f32)
    }
}

impl Default for PostStrategyVerifier {
    fn default() -> Self {
        Self::new(SemanticDimensionWeights::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_verifier() -> PostStrategyVerifier {
        PostStrategyVerifier::default()
    }

    #[test]
    fn test_evidence_improvement() {
        let verifier = create_verifier();

        let before = "Some content without references.";
        let after = "Some content with @src/main.rs:10 and @src/lib.rs:20 references.";

        let result = verifier.verify_evidence_improvement(before, after, None);
        assert!(result.resolved);
        assert!(result.quality_delta > 0.0);
    }

    #[test]
    fn test_evidence_no_improvement() {
        let verifier = create_verifier();

        let before = "Content with @src/main.rs reference.";
        let after = "Content with same @src/main.rs reference.";

        let result = verifier.verify_evidence_improvement(before, after, None);
        assert!(!result.resolved);
    }

    #[test]
    fn test_actionability_improvement() {
        let verifier = create_verifier();

        let before = "Some descriptive content about the system.";
        let after = "You must use the configuration system. You should always validate inputs.";

        let result = verifier.verify_actionability_improvement(before, after);
        assert!(result.quality_delta > 0.0 || result.resolved);
    }

    #[test]
    fn test_specificity_improvement() {
        let verifier = create_verifier();

        let before = "Generally, best practices should be followed. Typically this is how it's done.";
        let after = "Use @src/config.rs:10 for configuration. See @src/main.rs:5 for entry point.";

        let result = verifier.verify_specificity_improvement(before, after);
        assert!(result.resolved);
    }

    #[test]
    fn test_tier1_removal() {
        let verifier = create_verifier();

        let before = "Run cargo build to compile. Use cargo test for testing.";
        let after = "The build system uses custom flags. See @build.rs:10 for configuration.";

        let result = verifier.verify_tier1_removal(before, after);
        // Should show improvement as tier1 patterns are reduced
        assert!(result.metrics.after_value <= result.metrics.before_value);
    }

    #[test]
    fn test_depth_improvement() {
        let verifier = create_verifier();

        let before = "Simple content.";
        let after = r#"## Overview
Why this matters: it provides context.

## Example
```rust
fn example() {}
```

See @src/main.rs:10 for reference."#;

        let result = verifier.verify_depth_improvement(before, after);
        assert!(result.resolved);
        assert!(result.quality_delta > 0.0);
    }

    #[test]
    fn test_general_quality_improvement() {
        let verifier = create_verifier();

        let before = "x";
        let after = r#"## Module Overview
You must configure the system using @src/config.rs:10.

## Requirements
- Always validate inputs
- Never expose internal state

## Example
```rust
let config = Config::load()?;
```"#;

        let result = verifier.verify_general_quality_improvement(before, after);
        assert!(result.resolved);
        assert!(result.quality_delta > 0.0);
    }
}
