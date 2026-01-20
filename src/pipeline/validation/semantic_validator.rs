//! Semantic Quality Validator
//!
//! Validates generated content for actual value rather than surface metrics.
//! Measures: actionability, specificity, evidence quality, redundancy, depth.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::SemanticValidationConfig;
use crate::pipeline::patterns::{
    ACTIONABLE_PATTERN, FILE_LINE_REF, FILE_REF, GENERIC_PATTERN,
};
use crate::types::{Agent, ProjectMemory, Result, Rule, Skill};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticQualityResult {
    pub passed: bool,
    pub overall_score: f32,
    pub actionability: SemanticScore,
    pub specificity: SemanticScore,
    pub evidence_quality: SemanticScore,
    pub redundancy: SemanticScore,
    pub depth: SemanticScore,
    pub issues: Vec<SemanticIssue>,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticScore {
    pub score: f32,
    pub passed: bool,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticIssue {
    pub category: IssueCategory,
    pub target: String,
    pub description: String,
    pub severity: IssueSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IssueCategory {
    LowActionability,
    TooGeneric,
    WeakEvidence,
    Redundant,
    Shallow,
    MissingReference,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Critical,
    High,
    Medium,
    Low,
}

pub struct SemanticValidator {
    config: SemanticValidationConfig,
    project_root: std::path::PathBuf,
}

impl SemanticValidator {
    pub fn new(config: SemanticValidationConfig, project_root: impl AsRef<Path>) -> Self {
        Self {
            config,
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    pub async fn validate(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> Result<SemanticQualityResult> {
        if !self.config.enabled {
            return Ok(SemanticQualityResult {
                passed: true,
                overall_score: 1.0,
                actionability: SemanticScore { score: 1.0, passed: true, details: "Disabled".into() },
                specificity: SemanticScore { score: 1.0, passed: true, details: "Disabled".into() },
                evidence_quality: SemanticScore { score: 1.0, passed: true, details: "Disabled".into() },
                redundancy: SemanticScore { score: 1.0, passed: true, details: "Disabled".into() },
                depth: SemanticScore { score: 1.0, passed: true, details: "Disabled".into() },
                issues: Vec::new(),
                suggestions: Vec::new(),
            });
        }

        let mut issues = Vec::new();
        let all_content = self.collect_all_content(skills, agents, rules, claude_md);

        let actionability = self.assess_actionability(&all_content, &mut issues);
        let specificity = self.assess_specificity(&all_content, &mut issues);
        let evidence_quality = self.assess_evidence_quality(&all_content, &mut issues);
        let redundancy = self.assess_redundancy(skills, agents, rules, claude_md, &mut issues);
        let depth = self.assess_depth(&all_content, &mut issues);

        let w = &self.config.weights;
        let overall_score = (actionability.score * w.actionability
            + specificity.score * w.specificity
            + evidence_quality.score * w.evidence
            + (1.0 - redundancy.score) * w.redundancy
            + depth.score * w.depth)
            .clamp(0.0, 1.0);

        let passed = actionability.passed
            && specificity.passed
            && evidence_quality.passed
            && redundancy.passed
            && depth.passed;

        let suggestions = self.generate_suggestions(&issues);

        Ok(SemanticQualityResult {
            passed,
            overall_score,
            actionability,
            specificity,
            evidence_quality,
            redundancy,
            depth,
            issues,
            suggestions,
        })
    }

    fn collect_all_content(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> Vec<ContentItem> {
        let mut items = Vec::new();

        items.push(ContentItem {
            source: "CLAUDE.md".to_string(),
            content: claude_md.to_markdown(),
        });

        for skill in skills {
            items.push(ContentItem {
                source: format!("Skill:{}", skill.name),
                content: skill.to_markdown(),
            });
        }

        for agent in agents {
            items.push(ContentItem {
                source: format!("Agent:{}", agent.name),
                content: agent.to_markdown(),
            });
        }

        for rule in rules {
            items.push(ContentItem {
                source: format!("Rule:{}", rule.name),
                content: rule.to_markdown(),
            });
        }

        items
    }

    fn assess_actionability(
        &self,
        content: &[ContentItem],
        issues: &mut Vec<SemanticIssue>,
    ) -> SemanticScore {
        let mut total_statements = 0;
        let mut actionable_statements = 0;
        let t = &self.config.thresholds;

        for item in content {
            for line in item.content.lines() {
                let trimmed = line.trim();
                if trimmed.len() < t.min_line_length_actionability
                    || trimmed.starts_with('#')
                    || trimmed.starts_with("```")
                {
                    continue;
                }

                total_statements += 1;
                if ACTIONABLE_PATTERN.is_match(trimmed) {
                    actionable_statements += 1;
                }
            }

            let item_actionable_ratio = if total_statements > 0 {
                actionable_statements as f32 / total_statements as f32
            } else {
                0.0
            };

            if item_actionable_ratio < self.config.min_actionability * t.low_actionability_multiplier
            {
                issues.push(SemanticIssue {
                    category: IssueCategory::LowActionability,
                    target: item.source.clone(),
                    description: format!(
                        "Only {:.0}% of content is actionable (needs {}%+)",
                        item_actionable_ratio * 100.0,
                        self.config.min_actionability * 100.0
                    ),
                    severity: IssueSeverity::High,
                });
            }
        }

        let score = if total_statements > 0 {
            (actionable_statements as f32 / total_statements as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };

        SemanticScore {
            score,
            passed: score >= self.config.min_actionability,
            details: format!(
                "{}/{} statements actionable",
                actionable_statements, total_statements
            ),
        }
    }

    fn assess_specificity(
        &self,
        content: &[ContentItem],
        issues: &mut Vec<SemanticIssue>,
    ) -> SemanticScore {
        let mut total_segments = 0;
        let mut specific_segments = 0;
        let mut generic_count = 0;
        let t = &self.config.thresholds;

        for item in content {
            for line in item.content.lines() {
                let trimmed = line.trim();
                if trimmed.len() < t.min_line_length_specificity {
                    continue;
                }

                total_segments += 1;

                let has_file_ref = FILE_REF.is_match(trimmed);
                let has_code_example = trimmed.contains('`') || trimmed.starts_with("```");
                let has_specific_name =
                    trimmed.contains("::") || trimmed.contains("()") || trimmed.contains("fn ");

                if has_file_ref || has_code_example || has_specific_name {
                    specific_segments += 1;
                }

                if GENERIC_PATTERN.is_match(trimmed) {
                    generic_count += 1;
                    if self.config.reject_generic_content {
                        issues.push(SemanticIssue {
                            category: IssueCategory::TooGeneric,
                            target: item.source.clone(),
                            description: format!(
                                "Generic language detected: \"{}\"",
                                truncate(trimmed, 60)
                            ),
                            severity: IssueSeverity::Medium,
                        });
                    }
                }
            }
        }

        let specificity_ratio = if total_segments > 0 {
            specific_segments as f32 / total_segments as f32
        } else {
            0.0
        };

        let generic_penalty = if total_segments > 0 {
            (generic_count as f32 / total_segments as f32).min(t.max_generic_penalty)
        } else {
            0.0
        };

        let score = (specificity_ratio - generic_penalty).clamp(0.0, 1.0);

        SemanticScore {
            score,
            passed: score >= self.config.min_specificity,
            details: format!(
                "{} specific segments, {} generic phrases",
                specific_segments, generic_count
            ),
        }
    }

    fn assess_evidence_quality(
        &self,
        content: &[ContentItem],
        issues: &mut Vec<SemanticIssue>,
    ) -> SemanticScore {
        let mut total_refs = 0;
        let mut valid_refs = 0;
        let mut file_line_refs = 0;
        let t = &self.config.thresholds;

        for item in content {
            for cap in FILE_LINE_REF.captures_iter(&item.content) {
                total_refs += 1;
                file_line_refs += 1;
                let path = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if self.project_root.join(path).exists() {
                    valid_refs += 1;
                }
            }

            for cap in FILE_REF.captures_iter(&item.content) {
                if !FILE_LINE_REF.is_match(cap.get(0).map(|m| m.as_str()).unwrap_or("")) {
                    total_refs += 1;
                    let path = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    if self.project_root.join(path).exists() {
                        valid_refs += 1;
                    }
                }
            }
        }

        if self.config.require_file_line_refs && file_line_refs < self.config.min_actionable_items {
            issues.push(SemanticIssue {
                category: IssueCategory::MissingReference,
                target: "All content".to_string(),
                description: format!(
                    "Only {} file:line references found (need {}+)",
                    file_line_refs, self.config.min_actionable_items
                ),
                severity: IssueSeverity::High,
            });
        }

        let validity_score = if total_refs > 0 {
            valid_refs as f32 / total_refs as f32
        } else {
            0.0
        };

        let quantity_score = (total_refs as f32
            / (self.config.min_actionable_items as f32 * t.quantity_score_multiplier))
            .min(1.0);
        let file_line_bonus = if file_line_refs >= self.config.min_actionable_items {
            t.file_line_bonus
        } else {
            0.0
        };
        let score = (validity_score * t.validity_score_weight
            + quantity_score * t.quantity_score_weight
            + file_line_bonus)
            .clamp(0.0, 1.0);

        SemanticScore {
            score,
            passed: score >= self.config.min_evidence_quality,
            details: format!(
                "{}/{} refs valid, {} with line numbers",
                valid_refs, total_refs, file_line_refs
            ),
        }
    }

    fn assess_redundancy(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
        issues: &mut Vec<SemanticIssue>,
    ) -> SemanticScore {
        let claude_md_content = claude_md.to_markdown();
        let mut redundant_items = 0;
        let total_items = skills.len() + agents.len() + rules.len();
        let t = &self.config.thresholds;

        if total_items == 0 {
            return SemanticScore {
                score: 0.0,
                passed: true,
                details: "No items to check".into(),
            };
        }

        let claude_md_phrases: HashSet<_> = self.extract_key_phrases(&claude_md_content);

        for rule in rules {
            let rule_phrases = self.extract_key_phrases(&rule.to_markdown());
            let overlap = claude_md_phrases.intersection(&rule_phrases).count();
            let rule_phrase_count = rule_phrases.len().max(1);

            if overlap as f32 / rule_phrase_count as f32 > t.overlap_threshold {
                redundant_items += 1;
                issues.push(SemanticIssue {
                    category: IssueCategory::Redundant,
                    target: format!("Rule:{}", rule.name),
                    description: "Significant overlap with CLAUDE.md content".to_string(),
                    severity: IssueSeverity::Medium,
                });
            }
        }

        let redundancy_ratio = redundant_items as f32 / total_items as f32;

        SemanticScore {
            score: redundancy_ratio,
            passed: redundancy_ratio <= self.config.max_redundancy,
            details: format!("{}/{} items have redundancy", redundant_items, total_items),
        }
    }

    fn extract_key_phrases(&self, content: &str) -> HashSet<String> {
        let t = &self.config.thresholds;
        content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.len() > t.min_phrase_line_length
                    && !trimmed.starts_with('#')
                    && !trimmed.starts_with("```")
            })
            .map(|line| {
                line.trim()
                    .to_lowercase()
                    .chars()
                    .filter(|c| c.is_alphanumeric() || c.is_whitespace())
                    .collect::<String>()
                    .split_whitespace()
                    .take(t.max_phrase_words)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn assess_depth(
        &self,
        content: &[ContentItem],
        issues: &mut Vec<SemanticIssue>,
    ) -> SemanticScore {
        let mut deep_items = 0;
        let mut shallow_items = 0;
        let thresholds = &self.config.thresholds;

        for item in content {
            let lines: Vec<_> = item.content.lines().collect();
            let total_lines = lines.len();
            let substantive_lines = lines
                .iter()
                .filter(|l| {
                    let trimmed = l.trim();
                    trimmed.len() > thresholds.min_substantive_line_length
                        && !trimmed.starts_with('#')
                        && !trimmed.starts_with('-')
                        && !trimmed.starts_with("```")
                })
                .count();

            let has_code_refs = FILE_LINE_REF.is_match(&item.content);
            let has_examples = item.content.contains("```") || item.content.contains("Example");
            let has_rationale = item.content.to_lowercase().contains("why")
                || item.content.to_lowercase().contains("because")
                || item.content.to_lowercase().contains("rationale");

            let depth_indicators = [has_code_refs, has_examples, has_rationale]
                .iter()
                .filter(|&&b| b)
                .count();

            if substantive_lines > thresholds.min_substantive_lines_deep
                && depth_indicators >= thresholds.min_depth_indicators
            {
                deep_items += 1;
            } else if total_lines > thresholds.min_total_lines_shallow_check && depth_indicators == 0
            {
                shallow_items += 1;
                issues.push(SemanticIssue {
                    category: IssueCategory::Shallow,
                    target: item.source.clone(),
                    description: "Lacks depth indicators (examples, rationale, code refs)"
                        .to_string(),
                    severity: IssueSeverity::Medium,
                });
            }
        }

        let total = deep_items + shallow_items;
        let score = if total > 0 {
            deep_items as f32 / total as f32
        } else {
            thresholds.default_depth_score
        };

        SemanticScore {
            score,
            passed: score >= self.config.min_depth,
            details: format!("{} deep, {} shallow items", deep_items, shallow_items),
        }
    }

    fn generate_suggestions(&self, issues: &[SemanticIssue]) -> Vec<String> {
        let mut suggestions = Vec::new();
        let t = &self.config.thresholds;
        let category_counts: std::collections::HashMap<IssueCategory, usize> =
            issues.iter().fold(std::collections::HashMap::new(), |mut acc, issue| {
                *acc.entry(issue.category).or_insert(0) += 1;
                acc
            });

        if category_counts.get(&IssueCategory::LowActionability).unwrap_or(&0)
            > &t.low_actionability_suggestion_threshold
        {
            suggestions.push(
                "Add more directive language (must, should, avoid, use) with specific guidance"
                    .to_string(),
            );
        }

        if category_counts.get(&IssueCategory::TooGeneric).unwrap_or(&0)
            > &t.too_generic_suggestion_threshold
        {
            suggestions.push(
                "Replace generic phrases with project-specific details and file references"
                    .to_string(),
            );
        }

        if category_counts.get(&IssueCategory::MissingReference).unwrap_or(&0) > &0 {
            suggestions.push(
                "Add @file:line references to provide concrete evidence for each guideline"
                    .to_string(),
            );
        }

        if category_counts.get(&IssueCategory::Redundant).unwrap_or(&0)
            > &t.redundant_suggestion_threshold
        {
            suggestions.push(
                "Move shared content to CLAUDE.md and keep rules/skills focused on specific contexts"
                    .to_string(),
            );
        }

        if category_counts.get(&IssueCategory::Shallow).unwrap_or(&0)
            > &t.shallow_suggestion_threshold
        {
            suggestions.push(
                "Add rationale (why), examples, and code references to increase depth".to_string(),
            );
        }

        suggestions
    }
}

fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len { s } else { &s[..s.floor_char_boundary(max_len)] }
}

struct ContentItem {
    source: String,
    content: String,
}

pub fn validate(
    config: &SemanticValidationConfig,
    project_root: impl AsRef<Path>,
    skills: &[Skill],
    agents: &[Agent],
    rules: &[Rule],
    claude_md: &ProjectMemory,
) -> SemanticQualityResult {
    let validator = SemanticValidator::new(config.clone(), project_root);
    tokio::runtime::Handle::current()
        .block_on(validator.validate(skills, agents, rules, claude_md))
        .unwrap_or_else(|_| SemanticQualityResult {
            passed: false,
            overall_score: 0.0,
            actionability: SemanticScore { score: 0.0, passed: false, details: "Error".into() },
            specificity: SemanticScore { score: 0.0, passed: false, details: "Error".into() },
            evidence_quality: SemanticScore { score: 0.0, passed: false, details: "Error".into() },
            redundancy: SemanticScore { score: 0.0, passed: false, details: "Error".into() },
            depth: SemanticScore { score: 0.0, passed: false, details: "Error".into() },
            issues: Vec::new(),
            suggestions: Vec::new(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::patterns::{ACTIONABLE_PATTERN, GENERIC_PATTERN, FILE_LINE_REF, FILE_REF};

    #[test]
    fn test_actionable_pattern() {
        assert!(ACTIONABLE_PATTERN.is_match("You must use Result"));
        assert!(ACTIONABLE_PATTERN.is_match("Avoid using println!"));
        assert!(ACTIONABLE_PATTERN.is_match("Always prefer Arc"));
        assert!(!ACTIONABLE_PATTERN.is_match("This is a file"));
    }

    #[test]
    fn test_generic_pattern() {
        assert!(GENERIC_PATTERN.is_match("Following best practices"));
        assert!(GENERIC_PATTERN.is_match("This is typically done"));
        assert!(!GENERIC_PATTERN.is_match("Use Arc::clone() for sharing"));
    }

    #[test]
    fn test_file_ref_patterns() {
        assert!(FILE_LINE_REF.is_match("See @src/main.rs:42"));
        assert!(FILE_REF.is_match("Check @src/lib.rs"));
        assert!(!FILE_LINE_REF.is_match("email@example.com"));
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5).len(), 5);
    }
}
