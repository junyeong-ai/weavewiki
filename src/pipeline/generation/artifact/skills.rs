//! Skills Generator
//!
//! Generates .claude/skills/ files from extracted insights.
//! Skills contain:
//! - Reusable workflows and procedures
//! - Checklists for common tasks
//! - Project-specific guides

use crate::pipeline::insight::{ExtractedInsight, InsightCategory, TierClassification};
use crate::types::Skill;

use super::ArtifactGenerator;

/// Minimum value threshold for generating a skill
const MIN_SKILL_VALUE: f32 = 0.5;

/// Generates skills from insights
pub struct SkillsGenerator;

impl SkillsGenerator {
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
        if insight.value.overall < MIN_SKILL_VALUE {
            return false;
        }

        // Skills are appropriate for procedural content
        // But we accept broader categories since the classifier already determined this is skill material
        true
    }

    fn insight_to_skill(&self, insight: &ExtractedInsight) -> Skill {
        let name = self.generate_skill_name(&insight.insight.title);
        let description = self.generate_description(insight);
        let body = self.generate_body(insight);

        Skill::new(&name, &description, body).with_user_invocable(true)
    }

    fn generate_skill_name(&self, title: &str) -> String {
        // Convert title to kebab-case skill name
        title
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect()
    }

    fn generate_description(&self, insight: &ExtractedInsight) -> String {
        // Short one-line description
        let desc = &insight.insight.description;
        if desc.len() <= 80 {
            desc.clone()
        } else {
            format!("{}...", &desc[..77])
        }
    }

    fn generate_body(&self, insight: &ExtractedInsight) -> String {
        let mut body = String::new();

        // Title
        body.push_str(&format!("## {}\n\n", insight.insight.title));

        // Main description
        body.push_str(&insight.insight.description);
        body.push_str("\n\n");

        // Category-specific section
        match insight.insight.category {
            InsightCategory::Gotcha => {
                body.push_str("### ⚠️ Warning\n\n");
                if let Some(prevention) = &insight.insight.prevention_info {
                    body.push_str(prevention);
                    body.push_str("\n\n");
                }
            }
            InsightCategory::TechnicalConstraint | InsightCategory::SecurityConstraint => {
                body.push_str("### Requirements\n\n");
                if let Some(prevention) = &insight.insight.prevention_info {
                    body.push_str(&format!("- {}\n", prevention));
                }
                body.push('\n');
            }
            _ => {
                if let Some(prevention) = &insight.insight.prevention_info {
                    body.push_str("### Guidelines\n\n");
                    body.push_str(prevention);
                    body.push_str("\n\n");
                }
            }
        }

        // Evidence references
        if !insight.insight.evidence.is_empty() {
            body.push_str("### References\n\n");
            for evidence in &insight.insight.evidence {
                body.push_str(&format!("- @{}\n", evidence));
            }
        }

        body
    }

    fn deduplicate_skills(&self, skills: Vec<Skill>) -> Vec<Skill> {
        let mut seen_names = std::collections::HashSet::new();
        let mut result = Vec::new();

        for skill in skills {
            let key = skill.name.to_lowercase();
            if !seen_names.contains(&key) {
                seen_names.insert(key);
                result.push(skill);
            }
        }

        result
    }
}

impl Default for SkillsGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl ArtifactGenerator for SkillsGenerator {
    type Output = Vec<Skill>;

    fn generate(&self, insights: &[ExtractedInsight]) -> Self::Output {
        let skills: Vec<Skill> = insights
            .iter()
            .filter(|i| self.should_include(i))
            .map(|i| self.insight_to_skill(i))
            .collect();

        // Sort by value and deduplicate
        let mut sorted = skills;
        sorted.sort_by(|a, b| {
            // Skills with longer body likely have more content
            b.body.len().cmp(&a.body.len())
        });

        self.deduplicate_skills(sorted)
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
                description: format!("How to handle {} in this project", title.to_lowercase()),
                prevention_info: Some("Follow these steps to do it correctly".to_string()),
                evidence: vec!["src/handler.rs:50".to_string()],
                source: InsightSource::PatternMining,
                severity: None,
            },
            tier,
            artifact: ArtifactClassification::Skills,
            value: ValueScore {
                mistake_prevention: value,
                discoverability: value,
                artifact_fitness: value,
                overall: value,
            },
        }
    }

    #[test]
    fn test_generate_skills() {
        let generator = SkillsGenerator::new();

        let insights = vec![
            create_test_insight(
                InsightCategory::TechnicalConstraint,
                "Add New API Endpoint",
                TierClassification::Tier3,
                0.8,
            ),
            create_test_insight(
                InsightCategory::Gotcha,
                "Handle Rate Limiting",
                TierClassification::Tier3,
                0.7,
            ),
        ];

        let skills = generator.generate(&insights);

        assert_eq!(skills.len(), 2);
        assert!(skills.iter().any(|s| s.name.contains("api")));
    }

    #[test]
    fn test_skill_body_structure() {
        let generator = SkillsGenerator::new();

        let insights = vec![create_test_insight(
            InsightCategory::Gotcha,
            "Test Gotcha",
            TierClassification::Tier3,
            0.8,
        )];

        let skills = generator.generate(&insights);

        assert_eq!(skills.len(), 1);
        let body = skills[0].body.clone();
        assert!(body.contains("## Test Gotcha"));
        assert!(body.contains("⚠️ Warning"));
        assert!(body.contains("References"));
    }

    #[test]
    fn test_filter_low_tier() {
        let generator = SkillsGenerator::new();

        let insights = vec![create_test_insight(
            InsightCategory::TechnicalConstraint,
            "Generic Skill",
            TierClassification::Tier1,
            0.8,
        )];

        let skills = generator.generate(&insights);

        assert!(skills.is_empty());
    }

    #[test]
    fn test_deduplicate() {
        let generator = SkillsGenerator::new();

        let insights = vec![
            create_test_insight(
                InsightCategory::TechnicalConstraint,
                "Same Name",
                TierClassification::Tier3,
                0.8,
            ),
            create_test_insight(
                InsightCategory::TechnicalConstraint,
                "Same Name", // Duplicate
                TierClassification::Tier3,
                0.7,
            ),
        ];

        let skills = generator.generate(&insights);

        assert_eq!(skills.len(), 1);
    }
}
