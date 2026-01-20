//! Agents Generator
//!
//! Generates .claude/agents/ files from extracted insights.
//! Agents contain:
//! - Domain expertise and specialized roles
//! - Business knowledge
//! - Industry-specific rules

use crate::pipeline::insight::{ExtractedInsight, InsightCategory, TierClassification};
use crate::types::Agent;

use super::ArtifactGenerator;

/// Minimum value threshold for generating an agent
const MIN_AGENT_VALUE: f32 = 0.6;

/// Generates agents from insights
pub struct AgentsGenerator;

impl AgentsGenerator {
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
        if insight.value.overall < MIN_AGENT_VALUE {
            return false;
        }

        // Agents are appropriate for domain expertise
        matches!(
            insight.insight.category,
            InsightCategory::DomainKnowledge
                | InsightCategory::BusinessRule
                | InsightCategory::Compliance
        )
    }

    fn group_insights_by_domain<'a>(&self, insights: &'a [ExtractedInsight]) -> Vec<DomainGroup<'a>> {
        let mut groups: std::collections::HashMap<String, Vec<&ExtractedInsight>> =
            std::collections::HashMap::new();

        for insight in insights {
            let domain = self.infer_domain(insight);
            groups.entry(domain).or_default().push(insight);
        }

        groups
            .into_iter()
            .map(|(domain, insights)| DomainGroup { domain, insights })
            .collect()
    }

    fn infer_domain(&self, insight: &ExtractedInsight) -> String {
        let text = format!(
            "{} {}",
            insight.insight.title.to_lowercase(),
            insight.insight.description.to_lowercase()
        );

        // Domain-specific keywords
        let domains = [
            ("payment", vec!["payment", "transaction", "billing", "invoice", "refund"]),
            ("authentication", vec!["auth", "login", "credential", "token", "session"]),
            ("compliance", vec!["gdpr", "pci", "hipaa", "compliance", "regulation", "audit"]),
            ("finance", vec!["account", "balance", "ledger", "financial", "currency"]),
            ("user-management", vec!["user", "profile", "permission", "role", "access"]),
            ("data", vec!["database", "storage", "cache", "persistence", "query"]),
            ("integration", vec!["api", "webhook", "external", "third-party", "integration"]),
        ];

        for (domain, keywords) in domains {
            if keywords.iter().any(|k| text.contains(k)) {
                return domain.to_string();
            }
        }

        // Default domain based on category
        match insight.insight.category {
            InsightCategory::BusinessRule => "business-rules".to_string(),
            InsightCategory::DomainKnowledge => "domain-expert".to_string(),
            InsightCategory::Compliance => "compliance".to_string(),
            _ => "general".to_string(),
        }
    }

    fn domain_group_to_agent(&self, group: DomainGroup) -> Agent {
        let name = format!("{}-expert", group.domain);
        let description = format!(
            "Expert in {} domain knowledge for this project",
            group.domain.replace('-', " ")
        );

        let mut instructions = Vec::new();

        // Add role description
        instructions.push(format!(
            "You are a {} domain expert for this project.",
            group.domain.replace('-', " ")
        ));
        instructions.push(String::new());

        // Group insights by category
        let business_rules: Vec<_> = group
            .insights
            .iter()
            .filter(|i| matches!(i.insight.category, InsightCategory::BusinessRule))
            .collect();

        let domain_knowledge: Vec<_> = group
            .insights
            .iter()
            .filter(|i| matches!(i.insight.category, InsightCategory::DomainKnowledge))
            .collect();

        let compliance: Vec<_> = group
            .insights
            .iter()
            .filter(|i| matches!(i.insight.category, InsightCategory::Compliance))
            .collect();

        // Add business rules section
        if !business_rules.is_empty() {
            instructions.push("## Business Rules".to_string());
            instructions.push(String::new());
            for insight in business_rules {
                instructions.push(format!("**{}**: {}", insight.insight.title, insight.insight.description));
                if let Some(prevention) = &insight.insight.prevention_info {
                    instructions.push(format!("- Consequence: {}", prevention));
                }
                instructions.push(String::new());
            }
        }

        // Add domain knowledge section
        if !domain_knowledge.is_empty() {
            instructions.push("## Domain Knowledge".to_string());
            instructions.push(String::new());
            for insight in domain_knowledge {
                instructions.push(format!("**{}**: {}", insight.insight.title, insight.insight.description));
                instructions.push(String::new());
            }
        }

        // Add compliance section
        if !compliance.is_empty() {
            instructions.push("## Compliance Requirements".to_string());
            instructions.push(String::new());
            for insight in compliance {
                instructions.push(format!("⚠️ **{}**: {}", insight.insight.title, insight.insight.description));
                if let Some(prevention) = &insight.insight.prevention_info {
                    instructions.push(format!("- Action: {}", prevention));
                }
                instructions.push(String::new());
            }
        }

        // Add references
        let all_evidence: Vec<_> = group
            .insights
            .iter()
            .flat_map(|i| i.insight.evidence.iter())
            .collect();

        if !all_evidence.is_empty() {
            instructions.push("## References".to_string());
            instructions.push(String::new());
            for evidence in all_evidence.iter().take(10) {
                instructions.push(format!("- @{}", evidence));
            }
        }

        Agent::new(&name, &description, instructions.join("\n"))
    }
}

impl Default for AgentsGenerator {
    fn default() -> Self {
        Self::new()
    }
}

struct DomainGroup<'a> {
    domain: String,
    insights: Vec<&'a ExtractedInsight>,
}

impl ArtifactGenerator for AgentsGenerator {
    type Output = Vec<Agent>;

    fn generate(&self, insights: &[ExtractedInsight]) -> Self::Output {
        let filtered: Vec<_> = insights
            .iter()
            .filter(|i| self.should_include(i))
            .cloned()
            .collect();

        if filtered.is_empty() {
            return Vec::new();
        }

        let groups = self.group_insights_by_domain(&filtered);

        // Only create agents for groups with sufficient insights
        groups
            .into_iter()
            .filter(|g| g.insights.len() >= 2 || g.insights.iter().any(|i| i.tier == TierClassification::Tier3))
            .map(|g| self.domain_group_to_agent(g))
            .collect()
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
        value: f32,
    ) -> ExtractedInsight {
        ExtractedInsight {
            insight: Insight {
                id: "test".to_string(),
                category,
                title: title.to_string(),
                description: description.to_string(),
                prevention_info: Some("Do this correctly".to_string()),
                evidence: vec!["src/domain/payment.rs:25".to_string()],
                source: InsightSource::DomainAnalysis,
                severity: None,
            },
            tier,
            artifact: ArtifactClassification::Agents,
            value: ValueScore {
                mistake_prevention: value,
                discoverability: value,
                artifact_fitness: value,
                overall: value,
            },
        }
    }

    #[test]
    fn test_generate_agents() {
        let generator = AgentsGenerator::new();

        let insights = vec![
            create_test_insight(
                InsightCategory::BusinessRule,
                "Payment Validation",
                "All payments must be validated before processing",
                TierClassification::Tier3,
                0.8,
            ),
            create_test_insight(
                InsightCategory::Compliance,
                "PCI Compliance",
                "Payment data must follow PCI-DSS requirements",
                TierClassification::Tier3,
                0.9,
            ),
        ];

        let agents = generator.generate(&insights);

        assert!(!agents.is_empty());
        // Should be grouped into payment domain
        assert!(agents.iter().any(|a| a.name.contains("payment") || a.name.contains("compliance")));
    }

    #[test]
    fn test_domain_inference() {
        let generator = AgentsGenerator::new();

        let insight = create_test_insight(
            InsightCategory::BusinessRule,
            "User Authentication",
            "Users must be authenticated before accessing protected resources",
            TierClassification::Tier3,
            0.8,
        );

        let domain = generator.infer_domain(&insight);
        assert_eq!(domain, "authentication");
    }

    #[test]
    fn test_filter_low_value() {
        let generator = AgentsGenerator::new();

        let insights = vec![create_test_insight(
            InsightCategory::DomainKnowledge,
            "Low Value",
            "Some domain knowledge",
            TierClassification::Tier2,
            0.3, // Below threshold
        )];

        let agents = generator.generate(&insights);

        assert!(agents.is_empty());
    }

    #[test]
    fn test_require_multiple_insights() {
        let generator = AgentsGenerator::new();

        // Single Tier2 insight should not create an agent
        let insights = vec![create_test_insight(
            InsightCategory::DomainKnowledge,
            "Single Insight",
            "Some domain knowledge",
            TierClassification::Tier2,
            0.7,
        )];

        let agents = generator.generate(&insights);

        assert!(agents.is_empty());
    }
}
