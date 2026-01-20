//! Few-Shot Examples for Tier Classification
//!
//! Curated examples for LLM-based tier classification.
//! Core question: "Would AI make mistakes without this information?"
//!
//! Tier Definitions:
//! - Tier 1 (REJECT): Generic knowledge AI already knows
//! - Tier 2 (KEEP): Project conventions requiring analysis to discover
//! - Tier 3 (ESSENTIAL): Hidden constraints, gotchas, mistake prevention

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TierLevel {
    Tier1Generic,
    Tier2Convention,
    Tier3Constraint,
}

impl TierLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tier1Generic => "tier1_generic",
            Self::Tier2Convention => "tier2_convention",
            Self::Tier3Constraint => "tier3_constraint",
        }
    }

    pub fn should_reject(&self) -> bool {
        matches!(self, Self::Tier1Generic)
    }

    pub fn value_multiplier(&self) -> f32 {
        match self {
            Self::Tier1Generic => 0.0,
            Self::Tier2Convention => 0.6,
            Self::Tier3Constraint => 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierExample {
    pub content: String,
    pub tier: TierLevel,
    pub reasoning: String,
    pub language: Option<String>,
}

impl TierExample {
    pub fn new(content: impl Into<String>, tier: TierLevel, reasoning: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            tier,
            reasoning: reasoning.into(),
            language: None,
        }
    }

    pub fn with_language(mut self, lang: impl Into<String>) -> Self {
        self.language = Some(lang.into());
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct FewShotExamples {
    pub tier1_examples: Vec<TierExample>,
    pub tier2_examples: Vec<TierExample>,
    pub tier3_examples: Vec<TierExample>,
}

impl FewShotExamples {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_defaults() -> Self {
        let mut examples = Self::new();
        examples.add_default_examples();
        examples
    }

    fn add_default_examples(&mut self) {
        // Tier 1 - Generic knowledge (REJECT)
        self.tier1_examples.extend(vec![
            TierExample::new(
                "Use `cargo build` to compile the project",
                TierLevel::Tier1Generic,
                "Generic Rust knowledge - every AI knows this command",
            )
            .with_language("rust"),
            TierExample::new(
                "Run `npm install` to install dependencies",
                TierLevel::Tier1Generic,
                "Generic Node.js knowledge - standard command",
            )
            .with_language("javascript"),
            TierExample::new(
                "Use async/await for asynchronous operations",
                TierLevel::Tier1Generic,
                "Language feature documentation - not project-specific",
            ),
            TierExample::new(
                "Always write tests for your code",
                TierLevel::Tier1Generic,
                "Generic best practice - not actionable without specifics",
            ),
            TierExample::new(
                "Follow best practices for error handling",
                TierLevel::Tier1Generic,
                "Vague advice with no project-specific value",
            ),
            TierExample::new(
                "Use meaningful variable names",
                TierLevel::Tier1Generic,
                "Generic coding advice - not project-specific",
            ),
            TierExample::new(
                "Use `go build` to compile Go programs",
                TierLevel::Tier1Generic,
                "Generic Go knowledge",
            )
            .with_language("go"),
            TierExample::new(
                "Use TypeScript for type safety",
                TierLevel::Tier1Generic,
                "General language choice - not project-specific insight",
            )
            .with_language("typescript"),
        ]);

        // Tier 2 - Project conventions (KEEP)
        self.tier2_examples.extend(vec![
            TierExample::new(
                "Controllers are located in src/adapter/inbound/web/",
                TierLevel::Tier2Convention,
                "Project-specific directory structure - requires codebase analysis",
            ),
            TierExample::new(
                "This project uses hexagonal architecture with ports and adapters",
                TierLevel::Tier2Convention,
                "Architectural pattern choice - discoverable but valuable",
            ),
            TierExample::new(
                "All API endpoints follow the pattern /api/v1/{resource}",
                TierLevel::Tier2Convention,
                "Project-specific URL convention",
            ),
            TierExample::new(
                "Database migrations are in migrations/ and follow YYYYMMDD_name format",
                TierLevel::Tier2Convention,
                "Project-specific migration convention",
            ),
            TierExample::new(
                "Use the Result<T, ClaudegenError> type for fallible operations",
                TierLevel::Tier2Convention,
                "Project-specific error handling convention",
            )
            .with_language("rust"),
            TierExample::new(
                "Component tests go in __tests__ adjacent to the component",
                TierLevel::Tier2Convention,
                "Project-specific test organization",
            )
            .with_language("javascript"),
        ]);

        // Tier 3 - Hidden constraints (ESSENTIAL)
        self.tier3_examples.extend(vec![
            TierExample::new(
                "LlmProvider MUST be shared via Arc::clone() - new instances lose rate limit state",
                TierLevel::Tier3Constraint,
                "Critical constraint that would cause rate limiting issues if violated",
            )
            .with_language("rust"),
            TierExample::new(
                "Budget consumption uses CAS loop - DO NOT use fetch_add (race condition)",
                TierLevel::Tier3Constraint,
                "Hidden concurrency constraint causing bugs, not discoverable from API",
            )
            .with_language("rust"),
            TierExample::new(
                "HashMap in LearningState MUST be bounded - prune_oldest when len >= max_patterns",
                TierLevel::Tier3Constraint,
                "Memory safety constraint - unbounded growth causes OOM",
            )
            .with_language("rust"),
            TierExample::new(
                "Transaction boundary is at use case level - DO NOT start transactions in adapters",
                TierLevel::Tier3Constraint,
                "Architectural constraint - wrong placement causes partial commits",
            ),
            TierExample::new(
                "File registry uses OnceCell - expensive to build, always use get_file_registry()",
                TierLevel::Tier3Constraint,
                "Performance constraint - rebuilding on each call causes slowdown",
            )
            .with_language("rust"),
            TierExample::new(
                "WebSocket connections MUST be closed before HTTP response - order matters for browser compatibility",
                TierLevel::Tier3Constraint,
                "Browser quirk causing connection leaks if violated",
            ),
            TierExample::new(
                "Config validation MUST run before any other operation - invariants assumed elsewhere",
                TierLevel::Tier3Constraint,
                "Initialization order constraint - skipping causes undefined behavior",
            ),
            TierExample::new(
                "Never call getUserById in a loop - use getUsersByIds for batch operations (N+1 query)",
                TierLevel::Tier3Constraint,
                "Performance gotcha - causes severe database load",
            ),
        ]);
    }

    pub fn all_examples(&self) -> Vec<&TierExample> {
        self.tier1_examples
            .iter()
            .chain(self.tier2_examples.iter())
            .chain(self.tier3_examples.iter())
            .collect()
    }

    pub fn examples_for_tier(&self, tier: TierLevel) -> &[TierExample] {
        match tier {
            TierLevel::Tier1Generic => &self.tier1_examples,
            TierLevel::Tier2Convention => &self.tier2_examples,
            TierLevel::Tier3Constraint => &self.tier3_examples,
        }
    }

    pub fn sample(&self, count: usize) -> Vec<&TierExample> {
        let mut sampled = Vec::new();
        let per_tier = count / 3;
        let remainder = count % 3;

        sampled.extend(self.tier1_examples.iter().take(per_tier));
        sampled.extend(self.tier2_examples.iter().take(per_tier));
        sampled.extend(self.tier3_examples.iter().take(per_tier + remainder));

        sampled
    }

    pub fn to_prompt_format(&self, count: usize) -> String {
        let samples = self.sample(count);
        let mut lines = Vec::new();

        lines.push("TIER CLASSIFICATION EXAMPLES:\n".to_string());

        for example in samples {
            lines.push(format!(
                "Content: \"{}\"\nTier: {}\nReasoning: {}\n",
                example.content,
                example.tier.as_str(),
                example.reasoning
            ));
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueDimensions {
    pub mistake_prevention: f32,
    pub discoverability: f32,
    pub tier: TierLevel,
}

impl ValueDimensions {
    pub fn tier1() -> Self {
        Self {
            mistake_prevention: 0.0,
            discoverability: 0.0,
            tier: TierLevel::Tier1Generic,
        }
    }

    pub fn tier2(mistake_prevention: f32, discoverability: f32) -> Self {
        Self {
            mistake_prevention,
            discoverability,
            tier: TierLevel::Tier2Convention,
        }
    }

    pub fn tier3(mistake_prevention: f32, discoverability: f32) -> Self {
        Self {
            mistake_prevention,
            discoverability,
            tier: TierLevel::Tier3Constraint,
        }
    }

    pub fn overall_value(&self) -> f32 {
        let base = self.tier.value_multiplier();
        let dims = (self.mistake_prevention + self.discoverability) / 2.0;
        base * 0.6 + dims * 0.4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_examples() {
        let examples = FewShotExamples::with_defaults();

        assert!(!examples.tier1_examples.is_empty());
        assert!(!examples.tier2_examples.is_empty());
        assert!(!examples.tier3_examples.is_empty());
    }

    #[test]
    fn test_tier_should_reject() {
        assert!(TierLevel::Tier1Generic.should_reject());
        assert!(!TierLevel::Tier2Convention.should_reject());
        assert!(!TierLevel::Tier3Constraint.should_reject());
    }

    #[test]
    fn test_value_multiplier() {
        assert_eq!(TierLevel::Tier1Generic.value_multiplier(), 0.0);
        assert!(TierLevel::Tier2Convention.value_multiplier() > 0.0);
        assert_eq!(TierLevel::Tier3Constraint.value_multiplier(), 1.0);
    }

    #[test]
    fn test_sample_distribution() {
        let examples = FewShotExamples::with_defaults();
        let sampled = examples.sample(9);

        assert_eq!(sampled.len(), 9);

        let tier1_count = sampled
            .iter()
            .filter(|e| e.tier == TierLevel::Tier1Generic)
            .count();
        let tier2_count = sampled
            .iter()
            .filter(|e| e.tier == TierLevel::Tier2Convention)
            .count();
        let tier3_count = sampled
            .iter()
            .filter(|e| e.tier == TierLevel::Tier3Constraint)
            .count();

        assert!(tier1_count >= 2);
        assert!(tier2_count >= 2);
        assert!(tier3_count >= 2);
    }

    #[test]
    fn test_value_dimensions() {
        let tier1 = ValueDimensions::tier1();
        assert_eq!(tier1.overall_value(), 0.0);

        let tier3 = ValueDimensions::tier3(0.9, 0.8);
        assert!(tier3.overall_value() > 0.8);
    }

    #[test]
    fn test_prompt_format() {
        let examples = FewShotExamples::with_defaults();
        let prompt = examples.to_prompt_format(6);

        assert!(prompt.contains("TIER CLASSIFICATION EXAMPLES"));
        assert!(prompt.contains("tier1_generic") || prompt.contains("tier2_convention"));
    }
}
