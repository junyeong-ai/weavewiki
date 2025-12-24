//! Technical Agent - Turn 2
//!
//! Extracts technical patterns and architecture traits using LLM.
//! Receives Turn 1 context (structure, dependencies).

use super::TechnicalInsight;
use super::helpers::{AgentConfig, calculate_confidence, extract_prior_insight_string, run_agent};
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::characterization::schemas::{AgentPrompts, AgentSchemas};
use crate::wiki::exhaustive::characterization::{
    AgentOutput, CharacterizationAgent, CharacterizationContext,
};

pub struct TechnicalAgent;

impl TechnicalAgent {
    /// Fallback analysis when LLM fails
    fn fallback_analysis(context: &CharacterizationContext) -> TechnicalInsight {
        let mut technical_traits = vec![];
        let mut architecture_patterns = vec![];
        let mut async_patterns = vec![];

        // Infer from file patterns and languages
        let has_rust = context
            .files
            .iter()
            .any(|f| f.language.as_deref() == Some("rust"));
        let has_typescript = context
            .files
            .iter()
            .any(|f| f.language.as_deref() == Some("typescript"));
        let has_async = context
            .files
            .iter()
            .any(|f| f.path.contains("async") || f.path.contains("tokio"));

        if has_rust {
            technical_traits.push("Strongly typed (Rust)".to_string());
            technical_traits.push("Memory safe".to_string());
        }
        if has_typescript {
            technical_traits.push("Strongly typed (TypeScript)".to_string());
        }
        if has_async {
            async_patterns.push("Async/await patterns".to_string());
        }

        // Detect architecture from structure
        let dirs: std::collections::HashSet<String> = context
            .files
            .iter()
            .filter_map(|f| {
                f.path
                    .split('/')
                    .find(|p| !p.is_empty() && *p != "src")
                    .map(String::from)
            })
            .collect();

        if dirs.contains("domain") || dirs.contains("entities") {
            architecture_patterns.push("Domain-driven design".to_string());
        }
        if dirs.contains("handlers") || dirs.contains("controllers") {
            architecture_patterns.push("Layered architecture".to_string());
        }
        if dirs.contains("services") {
            architecture_patterns.push("Service-oriented".to_string());
        }

        if technical_traits.is_empty() {
            technical_traits.push("General purpose application".to_string());
        }

        TechnicalInsight {
            technical_traits,
            architecture_patterns,
            quality_focus: vec![],
            async_patterns,
        }
    }
}

#[async_trait::async_trait]
impl CharacterizationAgent for TechnicalAgent {
    fn name(&self) -> &str {
        "technical"
    }

    fn turn(&self) -> u8 {
        2
    }

    async fn run(&self, context: &CharacterizationContext) -> Result<AgentOutput, WeaveError> {
        run_agent(
            context,
            AgentConfig {
                name: "technical",
                turn: 2,
                schema: AgentSchemas::technical_schema(),
                build_prompt: Box::new(|ctx| {
                    let structure_insight =
                        extract_prior_insight_string(&ctx.prior_insights, "structure");
                    let dependencies =
                        extract_prior_insight_string(&ctx.prior_insights, "dependency");
                    AgentPrompts::technical_prompt(&structure_insight, &dependencies)
                }),
                fallback: Box::new(Self::fallback_analysis),
                confidence: Box::new(|insight: &TechnicalInsight| {
                    calculate_confidence(insight.technical_traits.is_empty())
                }),
                debug_result: Box::new(|insight: &TechnicalInsight| {
                    format!(
                        "Found {} traits, {} patterns",
                        insight.technical_traits.len(),
                        insight.architecture_patterns.len()
                    )
                }),
            },
        )
        .await
    }
}
