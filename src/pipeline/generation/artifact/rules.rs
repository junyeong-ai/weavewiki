//! Rules Generator
//!
//! Generates .claude/rules/ files from extracted insights.
//! Rules contain:
//! - Constraints and mandatory requirements
//! - Security policies
//! - Performance requirements
//! - Compliance rules

use crate::pipeline::insight::{ExtractedInsight, InsightCategory, TierClassification};
use crate::types::{EvidenceLocation, Rule};

use super::ArtifactGenerator;

/// Minimum value threshold for generating a rule
const MIN_RULE_VALUE: f32 = 0.4;

/// Generates rules from insights
pub struct RulesGenerator;

impl RulesGenerator {
    pub fn new() -> Self {
        Self
    }

    fn should_include(&self, insight: &ExtractedInsight) -> bool {
        // Must be Tier 2 or 3
        if matches!(
            insight.tier,
            TierClassification::Tier0 | TierClassification::Tier1
        ) {
            return false;
        }

        // Must meet value threshold
        if insight.value.overall < MIN_RULE_VALUE {
            return false;
        }

        // Must be a rule-appropriate category
        matches!(
            insight.insight.category,
            InsightCategory::TechnicalConstraint
                | InsightCategory::SecurityConstraint
                | InsightCategory::PerformanceConstraint
                | InsightCategory::Compliance
                | InsightCategory::Gotcha
        )
    }

    fn insight_to_rule(&self, insight: &ExtractedInsight) -> Rule {
        let mut content = Vec::new();

        // Title
        content.push(format!("# {}", insight.insight.title));
        content.push(String::new());

        // Description
        content.push(insight.insight.description.clone());
        content.push(String::new());

        // Severity if available
        if let Some(severity) = &insight.insight.severity {
            content.push(format!("**Severity**: {}", severity));
            content.push(String::new());
        }

        // Prevention info
        if let Some(prevention) = &insight.insight.prevention_info {
            content.push("## Prevention".to_string());
            content.push(String::new());
            content.push(prevention.clone());
            content.push(String::new());
        }

        // Evidence
        if !insight.insight.evidence.is_empty() {
            content.push("## References".to_string());
            content.push(String::new());
            for evidence in &insight.insight.evidence {
                content.push(format!("- @{}", evidence));
            }
        }

        // Generate rule name from title
        let name = insight
            .insight
            .title
            .to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();

        // Determine paths based on evidence
        let paths = if !insight.insight.evidence.is_empty() {
            // Extract directory paths from evidence
            let dirs: Vec<String> = insight
                .insight
                .evidence
                .iter()
                .filter_map(|e| {
                    let path = e.split(':').next()?;
                    let dir = std::path::Path::new(path).parent()?;
                    Some(format!("{}/**", dir.display()))
                })
                .collect();

            if dirs.is_empty() {
                None
            } else {
                Some(dirs)
            }
        } else {
            None
        };

        Rule {
            name,
            paths,
            content,
            evidence: insight
                .insight
                .evidence
                .iter()
                .map(|e| Self::parse_evidence_string(e))
                .collect(),
        }
    }

    fn parse_evidence_string(s: &str) -> EvidenceLocation {
        // Parse "file:line" or just "file" format
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        let file = parts[0].to_string();
        let line = parts.get(1).and_then(|l| l.parse::<u32>().ok()).unwrap_or(1);
        EvidenceLocation {
            file,
            start_line: line,
            end_line: line,
            start_column: None,
            end_column: None,
        }
    }

    fn group_related_rules(&self, rules: Vec<Rule>) -> Vec<Rule> {
        // Group rules by common path prefixes
        let mut grouped: std::collections::HashMap<String, Vec<Rule>> = std::collections::HashMap::new();

        for rule in rules {
            let key = rule
                .paths
                .as_ref()
                .and_then(|p| p.first())
                .and_then(|p| p.split('/').next())
                .unwrap_or("general")
                .to_string();

            grouped.entry(key).or_default().push(rule);
        }

        // Merge rules with same group if they're related
        let mut result = Vec::new();
        for (group_name, group_rules) in grouped {
            if group_rules.len() == 1 {
                result.extend(group_rules);
            } else if group_rules.len() <= 3 {
                // Keep separate if small group
                result.extend(group_rules);
            } else {
                // Merge into a combined rule
                let combined = self.merge_rules(&group_name, group_rules);
                result.push(combined);
            }
        }

        result
    }

    fn merge_rules(&self, group_name: &str, rules: Vec<Rule>) -> Rule {
        let mut content = Vec::new();
        let mut all_evidence = Vec::new();
        let mut all_paths = Vec::new();

        content.push(format!("# {} Rules", capitalize(group_name)));
        content.push(String::new());

        for rule in rules {
            content.push(format!("## {}", rule.name.replace('-', " ")));
            content.push(String::new());
            content.extend(
                rule.content
                    .into_iter()
                    .skip(2) // Skip the original title
                    .filter(|s| !s.starts_with("# ")),
            );
            content.push(String::new());

            all_evidence.extend(rule.evidence);
            if let Some(paths) = rule.paths {
                all_paths.extend(paths);
            }
        }

        // Deduplicate paths
        all_paths.sort();
        all_paths.dedup();

        // Deduplicate evidence by file path
        let mut seen_files = std::collections::HashSet::new();
        all_evidence.retain(|e| seen_files.insert(e.file.clone()));

        Rule {
            name: format!("{}-rules", group_name),
            paths: if all_paths.is_empty() {
                None
            } else {
                Some(all_paths)
            },
            content,
            evidence: all_evidence,
        }
    }
}

impl Default for RulesGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl ArtifactGenerator for RulesGenerator {
    type Output = Vec<Rule>;

    fn generate(&self, insights: &[ExtractedInsight]) -> Self::Output {
        let mut rules: Vec<Rule> = insights
            .iter()
            .filter(|i| self.should_include(i))
            .map(|i| self.insight_to_rule(i))
            .collect();

        // Sort by value score
        rules.sort_by(|a, b| {
            let a_evidence = a.evidence.len();
            let b_evidence = b.evidence.len();
            b_evidence.cmp(&a_evidence)
        });

        // Group related rules
        self.group_related_rules(rules)
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::insight::{
        ArtifactClassification, Insight, InsightSource, ValueScore,
    };

    fn create_test_insight(
        category: InsightCategory,
        title: &str,
        tier: TierClassification,
        value: f32,
    ) -> ExtractedInsight {
        ExtractedInsight {
            insight: Insight {
                id: "test".to_string(),
                category,
                title: title.to_string(),
                description: format!("Description for {}", title),
                prevention_info: Some("Do this instead".to_string()),
                evidence: vec!["src/main.rs:10".to_string()],
                source: InsightSource::ConstraintDetection,
                severity: Some("high".to_string()),
            },
            tier,
            artifact: ArtifactClassification::Rules,
            value: ValueScore {
                mistake_prevention: value,
                discoverability: value,
                artifact_fitness: value,
                overall: value,
            },
        }
    }

    #[test]
    fn test_generate_rules() {
        let generator = RulesGenerator::new();

        let insights = vec![
            create_test_insight(
                InsightCategory::SecurityConstraint,
                "Input Validation Required",
                TierClassification::Tier3,
                0.8,
            ),
            create_test_insight(
                InsightCategory::TechnicalConstraint,
                "Mutex Lock Order",
                TierClassification::Tier3,
                0.7,
            ),
        ];

        let rules = generator.generate(&insights);

        assert_eq!(rules.len(), 2);
        assert!(rules.iter().any(|r| r.name.contains("validation")));
    }

    #[test]
    fn test_filter_low_value() {
        let generator = RulesGenerator::new();

        let insights = vec![create_test_insight(
            InsightCategory::SecurityConstraint,
            "Low Value Rule",
            TierClassification::Tier2,
            0.2, // Below threshold
        )];

        let rules = generator.generate(&insights);

        assert!(rules.is_empty());
    }

    #[test]
    fn test_filter_tier1() {
        let generator = RulesGenerator::new();

        let insights = vec![create_test_insight(
            InsightCategory::TechnicalConstraint,
            "Generic Rule",
            TierClassification::Tier1,
            0.8,
        )];

        let rules = generator.generate(&insights);

        assert!(rules.is_empty());
    }
}
