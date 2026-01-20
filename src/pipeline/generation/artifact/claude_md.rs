//! CLAUDE.md Generator
//!
//! Generates CLAUDE.md from extracted insights with focus on:
//! - Project overview and context
//! - Architecture with file references
//! - Critical constraints and gotchas
//! - NO Tier 1 content (build commands, language basics)

use crate::pipeline::insight::{ExtractedInsight, InsightCategory, TierClassification};
use crate::types::ProjectMemory;

use super::ArtifactGenerator;

/// Generates CLAUDE.md project memory
pub struct ClaudeMdGenerator {
    project_name: String,
}

impl ClaudeMdGenerator {
    pub fn new(project_name: String) -> Self {
        Self { project_name }
    }

    fn build_overview(&self, insights: &[ExtractedInsight]) -> String {
        let mut overview = self.project_name.clone();

        // Find architecture insights
        let arch_insights: Vec<_> = insights
            .iter()
            .filter(|i| matches!(i.insight.category, InsightCategory::ArchitectureIntent))
            .collect();

        if let Some(arch) = arch_insights.first() {
            overview.push_str(&format!(" - {}", arch.insight.description));
        }

        overview
    }

    fn build_architecture(&self, insights: &[ExtractedInsight]) -> Option<String> {
        let arch_insights: Vec<_> = insights
            .iter()
            .filter(|i| matches!(i.insight.category, InsightCategory::ArchitectureIntent))
            .collect();

        if arch_insights.is_empty() {
            return None;
        }

        let mut arch = String::new();

        for insight in arch_insights.iter().take(5) {
            arch.push_str(&format!("**{}**\n", insight.insight.title));
            arch.push_str(&insight.insight.description);
            arch.push('\n');

            // Add file references
            if !insight.insight.evidence.is_empty() {
                arch.push('\n');
                for evidence in insight.insight.evidence.iter().take(3) {
                    arch.push_str(&format!("- @{}\n", evidence));
                }
            }
            arch.push('\n');
        }

        Some(arch.trim().to_string())
    }

    fn build_standards(&self, insights: &[ExtractedInsight]) -> Vec<String> {
        let mut standards = Vec::new();

        // Sort by tier (as numeric) and value
        let mut sorted: Vec<_> = insights.iter().collect();
        sorted.sort_by(|a, b| {
            let tier_a = a.tier.as_priority();
            let tier_b = b.tier.as_priority();
            let tier_cmp = tier_b.cmp(&tier_a);
            if tier_cmp != std::cmp::Ordering::Equal {
                return tier_cmp;
            }
            b.value
                .overall
                .partial_cmp(&a.value.overall)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for insight in sorted.into_iter().take(20) {
            // Skip Tier 0 and Tier 1
            if matches!(
                insight.tier,
                TierClassification::Tier0 | TierClassification::Tier1
            ) {
                continue;
            }

            let standard = self.format_standard(insight);
            if !standard.is_empty() {
                standards.push(standard);
            }
        }

        standards
    }

    fn format_standard(&self, insight: &ExtractedInsight) -> String {
        let mut standard = String::new();

        // Add emoji based on category
        let emoji = match insight.insight.category {
            InsightCategory::SecurityConstraint => "🔒",
            InsightCategory::TechnicalConstraint => "⚙️",
            InsightCategory::PerformanceConstraint => "⚡",
            InsightCategory::Compliance => "📋",
            InsightCategory::Gotcha => "⚠️",
            InsightCategory::BusinessRule => "📊",
            InsightCategory::DomainKnowledge => "📖",
            InsightCategory::ArchitectureIntent => "🏗️",
        };

        standard.push_str(emoji);
        standard.push(' ');

        // Title and description
        if insight.insight.title != insight.insight.description {
            standard.push_str(&format!("**{}**: ", insight.insight.title));
        }
        standard.push_str(&insight.insight.description);

        // Add prevention info if available
        if let Some(prevention) = &insight.insight.prevention_info {
            standard.push_str(&format!(" → {}", prevention));
        }

        // Add file reference if available (first one only for brevity)
        if let Some(evidence) = insight.insight.evidence.first() {
            standard.push_str(&format!(" (see @{})", evidence));
        }

        standard
    }
}

impl ArtifactGenerator for ClaudeMdGenerator {
    type Output = Option<ProjectMemory>;

    fn generate(&self, insights: &[ExtractedInsight]) -> Self::Output {
        if insights.is_empty() {
            return None;
        }

        let overview = self.build_overview(insights);
        let architecture = self.build_architecture(insights);
        let standards = self.build_standards(insights);

        // Only generate if we have meaningful content
        if architecture.is_none() && standards.is_empty() {
            return None;
        }

        Some(ProjectMemory {
            overview,
            architecture,
            commands: Vec::new(), // No Tier 1 commands
            standards,
            imports: Vec::new(),
        })
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
        description: &str,
        tier: TierClassification,
    ) -> ExtractedInsight {
        ExtractedInsight {
            insight: Insight {
                id: "test".to_string(),
                category,
                title: title.to_string(),
                description: description.to_string(),
                prevention_info: Some("Prevention info".to_string()),
                evidence: vec!["src/main.rs".to_string()],
                source: InsightSource::MistakeAnalysis,
                severity: Some("high".to_string()),
            },
            tier,
            artifact: ArtifactClassification::ClaudeMd,
            value: ValueScore {
                mistake_prevention: 0.8,
                discoverability: 0.7,
                artifact_fitness: 0.9,
                overall: 0.8,
            },
        }
    }

    #[test]
    fn test_generate_with_architecture() {
        let generator = ClaudeMdGenerator::new("TestProject".to_string());

        let insights = vec![
            create_test_insight(
                InsightCategory::ArchitectureIntent,
                "Hexagonal Architecture",
                "Uses port and adapter pattern for clean separation",
                TierClassification::Tier3,
            ),
            create_test_insight(
                InsightCategory::SecurityConstraint,
                "Input Validation",
                "All user input must be validated before processing",
                TierClassification::Tier3,
            ),
        ];

        let result = generator.generate(&insights);

        assert!(result.is_some());
        let memory = result.unwrap();
        assert!(memory.architecture.is_some());
        assert!(!memory.standards.is_empty());
    }

    #[test]
    fn test_skip_tier1_insights() {
        let generator = ClaudeMdGenerator::new("TestProject".to_string());

        let insights = vec![create_test_insight(
            InsightCategory::TechnicalConstraint,
            "Use cargo build",
            "Build the project with cargo",
            TierClassification::Tier1,
        )];

        let result = generator.generate(&insights);

        // Should have overview but no standards (Tier 1 filtered)
        assert!(result.is_none() || result.as_ref().unwrap().standards.is_empty());
    }

    #[test]
    fn test_empty_insights() {
        let generator = ClaudeMdGenerator::new("TestProject".to_string());
        let result = generator.generate(&[]);
        assert!(result.is_none());
    }
}
