//! Knowledge Classifier
//!
//! Classifies insights by:
//! - Tier (0=reject, 1=low, 2=medium, 3=high value)
//! - Target artifact type (CLAUDE.md, Rules, Skills, Agents)

use std::sync::Arc;

use tracing::trace;

use crate::config::Config;

use super::types::{text_contains_any, Insight, InsightCategory};

/// Tier classification result
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TierClassification {
    /// Reject: Generic knowledge AI already knows
    Tier0,
    /// Low value: Can be found in code structure
    Tier1,
    /// Medium value: Requires analysis to discover
    Tier2,
    /// High value: Hidden knowledge, prevents mistakes
    Tier3,
}

impl TierClassification {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tier0 => "tier0",
            Self::Tier1 => "tier1",
            Self::Tier2 => "tier2",
            Self::Tier3 => "tier3",
        }
    }

    /// Numeric priority for sorting (higher is better)
    pub fn as_priority(&self) -> u8 {
        match self {
            Self::Tier0 => 0,
            Self::Tier1 => 1,
            Self::Tier2 => 2,
            Self::Tier3 => 3,
        }
    }

    pub fn value_multiplier(&self) -> f32 {
        match self {
            Self::Tier0 => 0.0,
            Self::Tier1 => 0.3,
            Self::Tier2 => 0.6,
            Self::Tier3 => 1.0,
        }
    }
}

/// Artifact classification result
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ArtifactClassification {
    ClaudeMd,
    Rules,
    Skills,
    Agents,
}

impl ArtifactClassification {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeMd => "claude_md",
            Self::Rules => "rules",
            Self::Skills => "skills",
            Self::Agents => "agents",
        }
    }
}

/// Classifies knowledge into tiers and artifact types
pub struct KnowledgeClassifier {
    config: Arc<Config>,
    tier0_patterns: Vec<String>,
    tier2_patterns: Vec<String>,
    tier3_patterns: Vec<String>,
}

impl KnowledgeClassifier {
    pub fn new(config: Arc<Config>) -> Self {
        let tier0_patterns = config
            .tiers
            .tier0
            .keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();

        let tier2_patterns = config
            .tiers
            .tier2
            .keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();

        let tier3_patterns = config
            .tiers
            .tier3
            .keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();

        Self {
            config,
            tier0_patterns,
            tier2_patterns,
            tier3_patterns,
        }
    }

    /// Get duplicate similarity threshold from config
    pub fn duplicate_threshold(&self) -> f32 {
        self.config.insight.classification.duplicate_similarity_threshold
    }

    /// Classify insight into a tier based on value
    pub fn classify_tier(&self, insight: &Insight) -> TierClassification {
        // If tier classification is disabled, return default Tier2
        if !self.config.insight.classification.enable_tier_classification {
            trace!(insight_id = %insight.id, "Tier classification disabled, defaulting to Tier2");
            return TierClassification::Tier2;
        }

        let title_lower = insight.title.to_lowercase();
        let desc_lower = insight.description.to_lowercase();
        let combined = format!("{} {}", title_lower, desc_lower);

        // Check for Tier 0 (reject) patterns
        if self.matches_tier0(&combined) {
            trace!(insight_id = %insight.id, reason = "tier0_pattern", "Classified as Tier0");
            return TierClassification::Tier0;
        }

        // Check for Tier 3 (high value) indicators
        if self.matches_tier3(&combined, insight) {
            trace!(insight_id = %insight.id, reason = "tier3_pattern", "Classified as Tier3");
            return TierClassification::Tier3;
        }

        // Check for Tier 2 (medium value) indicators
        if self.matches_tier2(&combined, insight) {
            trace!(insight_id = %insight.id, reason = "tier2_pattern", "Classified as Tier2");
            return TierClassification::Tier2;
        }

        // Default to Tier 1 (low value)
        trace!(insight_id = %insight.id, reason = "default", "Classified as Tier1");
        TierClassification::Tier1
    }

    /// Classify insight into target artifact type
    pub fn classify_artifact(&self, insight: &Insight) -> ArtifactClassification {
        // If artifact routing is disabled, return default ClaudeMd
        if !self.config.insight.classification.enable_artifact_routing {
            trace!(insight_id = %insight.id, "Artifact routing disabled, defaulting to ClaudeMd");
            return ArtifactClassification::ClaudeMd;
        }

        // Rules: Constraints, mandatory requirements, forbidden actions
        if self.is_rule_material(insight) {
            trace!(insight_id = %insight.id, artifact = "rules", "Classified as Rules");
            return ArtifactClassification::Rules;
        }

        // Skills: Reusable workflows, checklists, procedures
        if self.is_skill_material(insight) {
            trace!(insight_id = %insight.id, artifact = "skills", "Classified as Skills");
            return ArtifactClassification::Skills;
        }

        // Agents: Domain expertise, specialized roles
        if self.is_agent_material(insight) {
            trace!(insight_id = %insight.id, artifact = "agents", "Classified as Agents");
            return ArtifactClassification::Agents;
        }

        // Default: CLAUDE.md for context and architecture
        trace!(insight_id = %insight.id, artifact = "claude_md", "Classified as ClaudeMd (default)");
        ArtifactClassification::ClaudeMd
    }

    fn matches_tier0(&self, text: &str) -> bool {
        // Check configured patterns
        if self.tier0_patterns.iter().any(|p| text.contains(p)) {
            return true;
        }

        // Built-in Tier 0 patterns (generic advice)
        const BUILTIN_TIER0: &[&str] = &[
            "use best practices", "follow conventions", "write clean code",
            "write tests", "handle errors", "npm install", "cargo build",
            "git commit", "docker run", "pip install", "use typescript",
            "prefer async/await", "use descriptive names", "add comments",
            "follow dry principle", "avoid magic numbers",
        ];

        text_contains_any(text, BUILTIN_TIER0)
    }

    fn matches_tier3(&self, text: &str, insight: &Insight) -> bool {
        // Check configured patterns
        if self.tier3_patterns.iter().any(|p| text.contains(p)) {
            return true;
        }

        // Tier 3 indicators: High-value categories
        match insight.category {
            InsightCategory::SecurityConstraint | InsightCategory::Compliance => return true,
            InsightCategory::TechnicalConstraint if insight.severity == Some("critical".to_string()) => {
                return true;
            }
            InsightCategory::BusinessRule if insight.prevention_info.is_some() => return true,
            _ => {}
        }

        // Built-in Tier 3 patterns (project-specific constraints)
        const BUILTIN_TIER3: &[&str] = &[
            "must", "never", "always", "critical", "required", "forbidden",
            "mandatory", "breaks if", "fails when", "race condition",
            "security", "compliance", "regulation", "sla", "production",
            "data loss", "injection", "authentication", "authorization",
            "deadlock", "memory leak",
        ];

        if text_contains_any(text, BUILTIN_TIER3) {
            return true;
        }

        // Has evidence and prevention info = likely high value
        !insight.evidence.is_empty() && insight.prevention_info.is_some()
    }

    fn matches_tier2(&self, text: &str, insight: &Insight) -> bool {
        // Medium value indicators
        match insight.category {
            InsightCategory::ArchitectureIntent | InsightCategory::DomainKnowledge => return true,
            InsightCategory::TechnicalConstraint if !insight.evidence.is_empty() => return true,
            _ => {}
        }

        // Check config patterns first
        if self.tier2_patterns.iter().any(|p| text.contains(p)) {
            return true;
        }

        // Built-in Tier 2 patterns (project-specific but not critical)
        const BUILTIN_TIER2: &[&str] = &[
            "should", "prefer", "recommend", "architecture", "design", "workflow",
        ];

        if text_contains_any(text, BUILTIN_TIER2) {
            return true;
        }

        // Has evidence = likely at least Tier 2
        !insight.evidence.is_empty()
    }

    fn is_rule_material(&self, insight: &Insight) -> bool {
        const RULE_KEYWORDS: &[&str] = &[
            "must", "never", "always", "forbidden", "required",
            "constraint", "rule", "policy", "enforce", "mandate", "prohibit",
        ];

        // Rules are for constraints and mandatory requirements
        match insight.category {
            InsightCategory::TechnicalConstraint
            | InsightCategory::SecurityConstraint
            | InsightCategory::PerformanceConstraint
            | InsightCategory::Compliance => return true,
            _ => {}
        }

        let text = format!("{} {}", insight.title, insight.description).to_lowercase();
        text_contains_any(&text, RULE_KEYWORDS)
    }

    fn is_skill_material(&self, insight: &Insight) -> bool {
        const SKILL_KEYWORDS: &[&str] = &[
            "how to", "step", "procedure", "checklist", "workflow",
            "guide", "tutorial", "when to", "add new", "create",
            "implement", "extend", "modify",
        ];

        // Domain knowledge and business rules belong to Agents, not Skills
        if matches!(
            insight.category,
            InsightCategory::DomainKnowledge | InsightCategory::BusinessRule
        ) {
            return false;
        }

        let text = format!("{} {}", insight.title, insight.description).to_lowercase();
        text_contains_any(&text, SKILL_KEYWORDS)
    }

    fn is_agent_material(&self, insight: &Insight) -> bool {
        const AGENT_KEYWORDS: &[&str] = &[
            "expert", "specialist", "domain", "business", "industry",
            "regulatory", "compliance", "finance", "medical", "legal",
        ];

        // Domain knowledge and expertise
        if matches!(
            insight.category,
            InsightCategory::DomainKnowledge | InsightCategory::BusinessRule
        ) {
            return true;
        }

        let text = format!("{} {}", insight.title, insight.description).to_lowercase();
        text_contains_any(&text, AGENT_KEYWORDS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TierConfig, TierPatterns};

    fn create_test_config() -> Arc<Config> {
        Arc::new(Config {
            tiers: TierConfig {
                tier0: TierPatterns {
                    keywords: vec!["best practices".to_string()],
                    patterns: Vec::new(),
                },
                tier2: TierPatterns {
                    keywords: vec!["convention".to_string()],
                    patterns: Vec::new(),
                },
                tier3: TierPatterns {
                    keywords: vec!["critical constraint".to_string()],
                    patterns: Vec::new(),
                },
            },
            ..Config::default()
        })
    }

    fn create_insight(category: InsightCategory, title: &str, description: &str) -> Insight {
        Insight {
            id: "test".to_string(),
            category,
            title: title.to_string(),
            description: description.to_string(),
            prevention_info: None,
            evidence: Vec::new(),
            source: super::super::types::InsightSource::MistakeAnalysis,
            severity: None,
        }
    }

    #[test]
    fn test_tier0_classification() {
        let config = create_test_config();
        let classifier = KnowledgeClassifier::new(config);

        let insight = create_insight(
            InsightCategory::TechnicalConstraint,
            "Follow best practices",
            "Use best practices when writing code",
        );

        assert_eq!(classifier.classify_tier(&insight), TierClassification::Tier0);
    }

    #[test]
    fn test_tier3_classification() {
        let config = create_test_config();
        let classifier = KnowledgeClassifier::new(config);

        let mut insight = create_insight(
            InsightCategory::SecurityConstraint,
            "Authentication Required",
            "Must validate JWT tokens on all protected endpoints",
        );
        insight.prevention_info = Some("Use auth middleware".to_string());
        insight.evidence = vec!["src/auth.rs".to_string()];

        assert_eq!(classifier.classify_tier(&insight), TierClassification::Tier3);
    }

    #[test]
    fn test_rule_classification() {
        let config = create_test_config();
        let classifier = KnowledgeClassifier::new(config);

        let insight = create_insight(
            InsightCategory::TechnicalConstraint,
            "Database Connection",
            "Must use connection pool, never create direct connections",
        );

        assert_eq!(
            classifier.classify_artifact(&insight),
            ArtifactClassification::Rules
        );
    }

    #[test]
    fn test_skill_classification() {
        let config = create_test_config();
        let classifier = KnowledgeClassifier::new(config);

        let insight = create_insight(
            InsightCategory::ArchitectureIntent,
            "Adding New Endpoint",
            "How to add a new API endpoint: step 1, step 2...",
        );

        assert_eq!(
            classifier.classify_artifact(&insight),
            ArtifactClassification::Skills
        );
    }

    #[test]
    fn test_agent_classification() {
        let config = create_test_config();
        let classifier = KnowledgeClassifier::new(config);

        let insight = create_insight(
            InsightCategory::DomainKnowledge,
            "Payment Processing Expert",
            "Domain expert knowledge about payment processing and compliance",
        );

        assert_eq!(
            classifier.classify_artifact(&insight),
            ArtifactClassification::Agents
        );
    }
}
