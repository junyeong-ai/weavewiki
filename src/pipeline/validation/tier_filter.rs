//! Tier Filter
//!
//! Structural validation filter for generated artifacts.
//! Content-based tier classification is handled by HybridClassifier.
//! This filter focuses on structural requirements: file refs, length, actionable language.

use crate::config::QualityConfig;
use crate::pipeline::patterns::{
    count_code_examples, count_file_refs, count_generic_patterns, count_tier3_indicators,
};
use crate::types::{Agent, Rule, Skill};

#[derive(Debug, Clone)]
pub struct TierFilterResult {
    pub passed: bool,
    pub tier1_violations: Vec<Tier1Violation>,
    pub filtered_content: FilteredContent,
    pub value_scores: Vec<ValueScore>,
}

#[derive(Debug, Clone)]
pub struct Tier1Violation {
    pub item_type: ItemType,
    pub item_name: String,
    pub violation: String,
    pub suggestion: String,
    pub value_score: f32,
}

#[derive(Debug, Clone)]
pub struct ValueScore {
    pub item_type: ItemType,
    pub item_name: String,
    pub score: f32,
    pub tier: ContentTier,
    pub indicators: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentTier {
    Tier1Generic,
    Tier2Convention,
    Tier3Constraint,
}

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
            Self::Skill => write!(f, "Skill"),
            Self::Agent => write!(f, "Agent"),
            Self::Rule => write!(f, "Rule"),
            Self::ClaudeMd => write!(f, "CLAUDE.md"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FilteredContent {
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub rules: Vec<Rule>,
    pub claude_md_issues: Vec<String>,
}

pub struct TierFilter {
    config: QualityConfig,
}

impl TierFilter {
    pub fn new(config: QualityConfig) -> Self {
        Self { config }
    }

    pub fn with_default_config() -> Self {
        Self::new(QualityConfig::default())
    }

    pub fn run(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md_content: &str,
    ) -> TierFilterResult {
        if !self.config.enabled {
            return TierFilterResult {
                passed: true,
                tier1_violations: Vec::new(),
                filtered_content: FilteredContent {
                    skills: skills.to_vec(),
                    agents: agents.to_vec(),
                    rules: rules.to_vec(),
                    claude_md_issues: Vec::new(),
                },
                value_scores: Vec::new(),
            };
        }

        let mut violations = Vec::new();
        let mut filtered = FilteredContent::default();
        let mut value_scores = Vec::new();

        for skill in skills {
            let score = self.calculate_skill_value(skill);
            value_scores.push(score.clone());

            if let Some(mut violation) = self.check_skill(skill) {
                violation.value_score = score.score;
                violations.push(violation);
            } else if score.tier == ContentTier::Tier1Generic {
                violations.push(Tier1Violation {
                    item_type: ItemType::Skill,
                    item_name: skill.name.clone(),
                    violation: "Content classified as Tier1 (generic)".to_string(),
                    suggestion: "Add project-specific constraints, file references, and internal knowledge".to_string(),
                    value_score: score.score,
                });
            } else {
                filtered.skills.push(skill.clone());
            }
        }

        for agent in agents {
            let score = self.calculate_agent_value(agent);
            value_scores.push(score.clone());

            if let Some(mut violation) = self.check_agent(agent) {
                violation.value_score = score.score;
                violations.push(violation);
            } else if score.tier == ContentTier::Tier1Generic {
                violations.push(Tier1Violation {
                    item_type: ItemType::Agent,
                    item_name: agent.name.clone(),
                    violation: "Content classified as Tier1 (generic)".to_string(),
                    suggestion: "Add internal knowledge section with project-specific constraints".to_string(),
                    value_score: score.score,
                });
            } else {
                filtered.agents.push(agent.clone());
            }
        }

        for rule in rules {
            let score = self.calculate_rule_value(rule);
            value_scores.push(score.clone());

            if let Some(mut violation) = self.check_rule(rule) {
                violation.value_score = score.score;
                violations.push(violation);
            } else if score.tier == ContentTier::Tier1Generic {
                violations.push(Tier1Violation {
                    item_type: ItemType::Rule,
                    item_name: rule.name.clone(),
                    violation: "Content classified as Tier1 (generic)".to_string(),
                    suggestion: "Add project-specific constraints with evidence references".to_string(),
                    value_score: score.score,
                });
            } else {
                filtered.rules.push(rule.clone());
            }
        }

        let md_issues = self.check_claude_md(claude_md_content);
        let md_score = self.calculate_claude_md_value(claude_md_content);
        value_scores.push(md_score);

        for issue in &md_issues {
            violations.push(Tier1Violation {
                item_type: ItemType::ClaudeMd,
                item_name: "CLAUDE.md".to_string(),
                violation: issue.clone(),
                suggestion: "Remove or replace with project-specific content".to_string(),
                value_score: 0.0,
            });
        }
        filtered.claude_md_issues = md_issues;

        TierFilterResult {
            passed: violations.is_empty(),
            tier1_violations: violations,
            filtered_content: filtered,
            value_scores,
        }
    }

    fn calculate_skill_value(&self, skill: &Skill) -> ValueScore {
        let content = format!("{}\n{}", skill.description, skill.body);
        let content_lower = content.to_lowercase();
        let mut indicators = Vec::new();
        let w = &self.config.scoring;
        let mut score: f32 = w.base_score;

        let file_refs = count_file_refs(&content);
        if file_refs > 0 {
            score += w.file_ref_weight * file_refs.min(w.file_ref_max_count) as f32;
            indicators.push(format!("{} file references", file_refs));
        }

        let code_examples = count_code_examples(&content);
        if code_examples > 0 {
            score += w.code_example_weight * code_examples.min(w.code_example_max_count) as f32;
            indicators.push(format!("{} code examples", code_examples));
        }

        let tier3_count = count_tier3_indicators(&content_lower);
        if tier3_count > 0 {
            score += w.tier3_indicator_weight * tier3_count.min(w.tier3_indicator_max_count) as f32;
            indicators.push(format!("{} constraint indicators", tier3_count));
        }

        let generic_count = count_generic_patterns(&content_lower);
        score -= w.tier1_penalty * generic_count as f32;

        score = score.clamp(0.0, 1.0);
        let tier = self.classify_tier(score);

        ValueScore {
            item_type: ItemType::Skill,
            item_name: skill.name.clone(),
            score,
            tier,
            indicators,
        }
    }

    fn calculate_agent_value(&self, agent: &Agent) -> ValueScore {
        let content = format!("{}\n{}", agent.description, agent.prompt);
        let content_lower = content.to_lowercase();
        let mut indicators = Vec::new();
        let w = &self.config.scoring;
        let mut score: f32 = w.base_score;

        let file_refs = count_file_refs(&content);
        if file_refs > 0 {
            score += w.file_ref_weight * file_refs.min(w.file_ref_max_count) as f32;
            indicators.push(format!("{} file references", file_refs));
        }

        let tier3_count = count_tier3_indicators(&content_lower);
        if tier3_count > 0 {
            score += w.tier3_indicator_weight * tier3_count.min(w.tier3_indicator_max_count) as f32;
            indicators.push(format!("{} constraint indicators", tier3_count));
        }

        if agent.tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false) {
            score += w.tool_presence_weight;
            indicators.push("Has specific tools".to_string());
        }

        let section_count = content.matches("##").count();
        if section_count >= 2 {
            score += w.section_weight;
            indicators.push(format!("{} sections", section_count));
        }

        let generic_phrases = ["help with", "assist with", "general purpose", "any task"];
        for phrase in generic_phrases {
            if content_lower.contains(phrase) {
                score -= w.generic_phrase_penalty;
            }
        }

        score = score.clamp(0.0, 1.0);
        let tier = self.classify_tier(score);

        ValueScore {
            item_type: ItemType::Agent,
            item_name: agent.name.clone(),
            score,
            tier,
            indicators,
        }
    }

    fn calculate_rule_value(&self, rule: &Rule) -> ValueScore {
        let content = rule.content.join("\n");
        let content_lower = content.to_lowercase();
        let mut indicators = Vec::new();
        let w = &self.config.scoring;
        let mut score: f32 = w.base_score;

        let file_refs = count_file_refs(&content);
        if file_refs > 0 {
            score += w.file_ref_weight * file_refs.min(w.file_ref_max_count) as f32;
            indicators.push(format!("{} file references", file_refs));
        }

        let code_examples = count_code_examples(&content);
        if code_examples > 0 {
            score += w.code_example_weight * code_examples.min(w.code_example_max_count) as f32;
            indicators.push(format!("{} code examples", code_examples));
        }

        if content.contains("✗") || content.contains("✓") {
            score += w.example_indicator_weight;
            indicators.push("Has good/bad examples".to_string());
        }

        if rule.paths.as_ref().map(|p| !p.is_empty()).unwrap_or(false) {
            score += w.path_scoped_weight;
            indicators.push("Path-scoped rule".to_string());
        }

        let tier3_count = count_tier3_indicators(&content_lower);
        if tier3_count > 0 {
            score += w.tier3_indicator_weight * tier3_count.min(w.tier3_indicator_max_count) as f32;
            indicators.push(format!("{} constraint indicators", tier3_count));
        }

        let generic_rules = ["always write tests", "keep code clean", "follow best practices"];
        for generic in generic_rules {
            if content_lower.contains(generic) {
                score -= w.generic_rule_penalty;
            }
        }

        score = score.clamp(0.0, 1.0);
        let tier = self.classify_tier(score);

        ValueScore {
            item_type: ItemType::Rule,
            item_name: rule.name.clone(),
            score,
            tier,
            indicators,
        }
    }

    fn calculate_claude_md_value(&self, content: &str) -> ValueScore {
        let content_lower = content.to_lowercase();
        let mut indicators = Vec::new();
        let w = &self.config.scoring;
        let mut score: f32 = w.claude_md_base_score;

        let file_refs = count_file_refs(content);
        if file_refs > 0 {
            score += w.code_example_weight * (file_refs.min(5) as f32 / 5.0);
            indicators.push(format!("{} file references", file_refs));
        }

        let code_examples = count_code_examples(content);
        if code_examples > 0 {
            score += w.code_example_weight;
            indicators.push(format!("{} code examples", code_examples));
        }

        let tier3_count = count_tier3_indicators(&content_lower);
        score += (w.tier3_indicator_weight / 2.0) * tier3_count.min(5) as f32;
        if tier3_count > 0 {
            indicators.push(format!("{} constraint indicators", tier3_count));
        }

        if content.contains("✗") || content.contains("✓") {
            score += w.section_weight;
            indicators.push("Has examples".to_string());
        }

        let generic_count = count_generic_patterns(&content_lower);
        if generic_count > 3 {
            score -= w.tier1_penalty * (generic_count - 3) as f32;
        }

        score = score.clamp(0.0, 1.0);
        let tier = self.classify_tier(score);

        ValueScore {
            item_type: ItemType::ClaudeMd,
            item_name: "CLAUDE.md".to_string(),
            score,
            tier,
            indicators,
        }
    }

    fn check_skill(&self, skill: &Skill) -> Option<Tier1Violation> {
        // Check minimum body length
        if skill.body.lines().count() < self.config.scoring.min_skill_body_lines {
            return Some(Tier1Violation {
                item_type: ItemType::Skill,
                item_name: skill.name.clone(),
                violation: "Skill body is too short to provide meaningful value".to_string(),
                suggestion: "Add detailed steps, evidence references, and gotchas".to_string(),
                value_score: 0.0,
            });
        }

        // Reject inline code blocks in reference-only mode
        if self.config.reference_only_mode && skill.body.contains("```") {
            return Some(Tier1Violation {
                item_type: ItemType::Skill,
                item_name: skill.name.clone(),
                violation: "Inline code blocks forbidden in reference-only mode".to_string(),
                suggestion: "Use @file:line references instead of inline code".to_string(),
                value_score: 0.0,
            });
        }

        // Require minimum file references
        let file_refs = count_file_refs(&skill.body);
        if file_refs < 2 {
            return Some(Tier1Violation {
                item_type: ItemType::Skill,
                item_name: skill.name.clone(),
                violation: format!("Only {} @file:line references (minimum 2 required)", file_refs),
                suggestion: "Add @file:line references to evidence the claims".to_string(),
                value_score: 0.0,
            });
        }

        None
    }

    fn check_agent(&self, agent: &Agent) -> Option<Tier1Violation> {
        let desc = agent.description.to_lowercase();
        let generic_phrases = ["help with", "assist with", "general purpose", "any task", "all code"];

        for phrase in generic_phrases {
            if desc.contains(phrase) {
                return Some(Tier1Violation {
                    item_type: ItemType::Agent,
                    item_name: agent.name.clone(),
                    violation: format!("Description is too generic: contains '{phrase}'"),
                    suggestion: "Describe specific project domain knowledge".to_string(),
                    value_score: 0.0,
                });
            }
        }

        // Reject inline code blocks in reference-only mode
        if self.config.reference_only_mode && agent.prompt.contains("```") {
            return Some(Tier1Violation {
                item_type: ItemType::Agent,
                item_name: agent.name.clone(),
                violation: "Inline code blocks forbidden in reference-only mode".to_string(),
                suggestion: "Use @file:line references instead of inline code".to_string(),
                value_score: 0.0,
            });
        }

        // Require minimum file references in prompt
        let file_refs = count_file_refs(&agent.prompt);
        if file_refs < 2 {
            return Some(Tier1Violation {
                item_type: ItemType::Agent,
                item_name: agent.name.clone(),
                violation: format!("Only {} @file:line references in prompt (minimum 2 required)", file_refs),
                suggestion: "Add @file:line references to provide project-specific context".to_string(),
                value_score: 0.0,
            });
        }

        // Require internal knowledge indicators
        let prompt_lower = agent.prompt.to_lowercase();
        let has_internal_knowledge = prompt_lower.contains("internal knowledge")
            || prompt_lower.contains("hidden")
            || prompt_lower.contains("constraint")
            || prompt_lower.contains("gotcha")
            || prompt_lower.contains("order matters")
            || prompt_lower.contains("sequence");

        if !has_internal_knowledge {
            return Some(Tier1Violation {
                item_type: ItemType::Agent,
                item_name: agent.name.clone(),
                violation: "Agent lacks internal project knowledge indicators".to_string(),
                suggestion: "Add 'Internal Knowledge' section with hidden constraints and gotchas".to_string(),
                value_score: 0.0,
            });
        }

        None
    }

    fn check_rule(&self, rule: &Rule) -> Option<Tier1Violation> {
        let content = rule.content.join("\n");
        let content_lower = content.to_lowercase();

        let generic_rules = [
            "always write tests",
            "keep code clean",
            "follow best practices",
            "use meaningful names",
            "add comments",
            "handle errors",
            "write clean code",
            "be consistent",
        ];

        for generic in generic_rules {
            if content_lower.contains(generic) {
                return Some(Tier1Violation {
                    item_type: ItemType::Rule,
                    item_name: rule.name.clone(),
                    violation: format!("Contains generic advice: '{generic}'"),
                    suggestion: "Add project-specific constraints with examples".to_string(),
                    value_score: 0.0,
                });
            }
        }

        // Require at least one file reference
        let file_refs = count_file_refs(&content);
        if file_refs < 1 {
            return Some(Tier1Violation {
                item_type: ItemType::Rule,
                item_name: rule.name.clone(),
                violation: "Rule has no @file:line references".to_string(),
                suggestion: "Add @file:line references to evidence the rule".to_string(),
                value_score: 0.0,
            });
        }

        // Require actionable language or examples
        let has_actionable = content_lower.contains("must")
            || content_lower.contains("should")
            || content_lower.contains("never")
            || content_lower.contains("always")
            || content.contains("✗")
            || content.contains("✓");

        if !has_actionable {
            return Some(Tier1Violation {
                item_type: ItemType::Rule,
                item_name: rule.name.clone(),
                violation: "Rule lacks actionable guidance".to_string(),
                suggestion: "Add must/should/never directives or ✗/✓ examples".to_string(),
                value_score: 0.0,
            });
        }

        None
    }

    fn check_claude_md(&self, content: &str) -> Vec<String> {
        let mut issues = Vec::new();

        let lines: Vec<_> = content.lines().collect();
        if lines.len() < self.config.scoring.min_claude_md_lines {
            issues.push("CLAUDE.md is too short - may lack project-specific value".to_string());
        }

        let has_constraints = content.contains("✗")
            || content.contains("never")
            || content.contains("forbidden")
            || content.contains("must not");

        if !has_constraints {
            issues.push("Missing project-specific constraints (anti-patterns)".to_string());
        }

        issues
    }

    fn classify_tier(&self, score: f32) -> ContentTier {
        let w = &self.config.scoring;
        if score >= w.tier3_threshold {
            ContentTier::Tier3Constraint
        } else if score >= w.tier2_threshold {
            ContentTier::Tier2Convention
        } else {
            ContentTier::Tier1Generic
        }
    }
}

#[cfg(test)]
fn classify_tier(score: f32) -> ContentTier {
    let w = crate::config::TierScoringWeights::default();
    if score >= w.tier3_threshold {
        ContentTier::Tier3Constraint
    } else if score >= w.tier2_threshold {
        ContentTier::Tier2Convention
    } else {
        ContentTier::Tier1Generic
    }
}

pub fn filter(
    skills: &[Skill],
    agents: &[Agent],
    rules: &[Rule],
    claude_md_content: &str,
) -> TierFilterResult {
    TierFilter::with_default_config().run(skills, agents, rules, claude_md_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_filter() -> TierFilter {
        TierFilter::with_default_config()
    }

    #[test]
    fn test_tier1_skill_detection() {
        let filter = default_filter();
        let skill = Skill::new("build-project", "Build the project", "Run cargo build");
        let result = filter.check_skill(&skill);
        assert!(result.is_some());
    }

    #[test]
    fn test_valid_skill_passes() {
        let filter = default_filter();
        let skill = Skill::new(
            "add-api-endpoint",
            "Add new API endpoint following hexagonal architecture",
            r#"
## Steps
1. Create port interface in port/inbound/ (see @src/port/inbound/mod.rs:15)
2. Implement use case following @src/usecase/example.rs:42
3. Create adapter in adapter/inbound/web/
4. Add route configuration

## Gotchas
- Transaction boundary is at use case level (@src/config/transaction.rs:88)
- Follow existing naming convention
            "#,
        );

        let result = filter.check_skill(&skill);
        assert!(result.is_none());
    }

    #[test]
    fn test_generic_agent_detection() {
        let filter = default_filter();
        let agent = Agent::new("code-reviewer", "Help with code reviews", "You are a code reviewer.");
        let result = filter.check_agent(&agent);
        assert!(result.is_some());
    }

    #[test]
    fn test_classify_tier() {
        assert_eq!(classify_tier(0.8), ContentTier::Tier3Constraint);
        assert_eq!(classify_tier(0.5), ContentTier::Tier2Convention);
        assert_eq!(classify_tier(0.2), ContentTier::Tier1Generic);
    }

    #[test]
    fn test_custom_config() {
        let config = QualityConfig {
            enabled: true,
            min_value_score: 0.5,
            ..Default::default()
        };
        let filter = TierFilter::new(config);
        let skill = Skill::new("test", "Test", "Short body");
        let result = filter.run(&[skill], &[], &[], "");
        assert!(!result.tier1_violations.is_empty());
    }

    #[test]
    fn test_disabled_filter() {
        let config = QualityConfig {
            enabled: false,
            ..Default::default()
        };
        let filter = TierFilter::new(config);
        let skill = Skill::new("build-project", "Build the project", "Run cargo build");
        let result = filter.run(&[skill], &[], &[], "");
        assert!(result.passed);
        assert!(result.tier1_violations.is_empty());
    }
}
