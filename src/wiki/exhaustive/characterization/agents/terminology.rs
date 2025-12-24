//! Terminology Agent - Turn 2
//!
//! Extracts domain-specific patterns and terminology using LLM.
//! Receives Turn 1 context (structure).
//!
//! Note: Renamed from DomainAgent to avoid name collision with top_down/agents/domain.rs

use super::DomainTraitsInsight;
use super::helpers::{
    AgentConfig, calculate_confidence, extract_prior_insight_string, format_file_list, run_agent,
};
use crate::types::DomainTerm;
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::characterization::schemas::{AgentPrompts, AgentSchemas};
use crate::wiki::exhaustive::characterization::{
    AgentOutput, CharacterizationAgent, CharacterizationContext,
};

/// Terminology Agent - extracts domain terminology and patterns
///
/// This agent analyzes project files to identify:
/// - Domain-specific traits and characteristics
/// - Technical terminology used in the codebase
/// - Domain patterns and conventions
pub struct TerminologyAgent;

impl TerminologyAgent {
    /// Fallback analysis when LLM fails
    fn fallback_analysis(context: &CharacterizationContext) -> DomainTraitsInsight {
        let mut domain_traits = vec![];
        let mut terminology = vec![];
        let domain_patterns = vec![];

        // Infer from file paths and names
        let paths: Vec<&str> = context.files.iter().map(|f| f.path.as_str()).collect();

        // Check for common domain patterns
        if paths.iter().any(|p| p.contains("api")) {
            domain_traits.push("API/Web service domain".to_string());
        }
        if paths.iter().any(|p| p.contains("parser")) {
            domain_traits.push("Parsing/text processing domain".to_string());
        }
        if paths.iter().any(|p| p.contains("graph")) {
            domain_traits.push("Graph/data structure domain".to_string());
        }
        if paths
            .iter()
            .any(|p| p.contains("storage") || p.contains("database"))
        {
            domain_traits.push("Data storage domain".to_string());
        }
        if paths
            .iter()
            .any(|p| p.contains("wiki") || p.contains("doc"))
        {
            domain_traits.push("Documentation/knowledge domain".to_string());
        }

        // Extract potential terms from directory names
        let mut seen = std::collections::HashSet::new();
        for file in &context.files {
            for part in file.path.split('/') {
                if part.len() > 3
                    && !["src", "lib", "mod", "test", "tests"].contains(&part)
                    && !seen.contains(part)
                {
                    seen.insert(part.to_string());
                    if seen.len() <= 10 {
                        terminology.push(
                            DomainTerm::new(part, format!("Component or module named '{}'", part))
                                .with_context(file.path.clone()),
                        );
                    }
                }
            }
        }

        if domain_traits.is_empty() {
            domain_traits.push("General software application".to_string());
        }

        DomainTraitsInsight {
            domain_traits,
            terminology,
            domain_patterns,
        }
    }
}

#[async_trait::async_trait]
impl CharacterizationAgent for TerminologyAgent {
    fn name(&self) -> &str {
        "terminology"
    }

    fn turn(&self) -> u8 {
        2
    }

    async fn run(&self, context: &CharacterizationContext) -> Result<AgentOutput, WeaveError> {
        run_agent(
            context,
            AgentConfig {
                name: "terminology",
                turn: 2,
                schema: AgentSchemas::domain_schema(),
                build_prompt: Box::new(|ctx| {
                    let structure_insight =
                        extract_prior_insight_string(&ctx.prior_insights, "structure");
                    let code_samples = format!(
                        "Project files and their characteristics:\n{}",
                        format_file_list(&ctx.files, 30)
                    );
                    AgentPrompts::domain_prompt(&structure_insight, &code_samples)
                }),
                fallback: Box::new(Self::fallback_analysis),
                confidence: Box::new(|insight: &DomainTraitsInsight| {
                    calculate_confidence(insight.domain_traits.is_empty())
                }),
                debug_result: Box::new(|insight: &DomainTraitsInsight| {
                    format!(
                        "Found {} traits, {} terms",
                        insight.domain_traits.len(),
                        insight.terminology.len()
                    )
                }),
            },
        )
        .await
    }
}
