//! Purpose Agent - Turn 2
//!
//! Discovers project purposes and target users using LLM.
//! Receives Turn 1 context (structure, entry points).

use super::PurposeInsight;
use super::helpers::{AgentConfig, calculate_confidence, extract_prior_insight_string, run_agent};
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::characterization::schemas::{AgentPrompts, AgentSchemas};
use crate::wiki::exhaustive::characterization::{
    AgentOutput, CharacterizationAgent, CharacterizationContext,
};

pub struct PurposeAgent;

impl PurposeAgent {
    /// Fallback analysis when LLM fails
    fn fallback_analysis(context: &CharacterizationContext) -> PurposeInsight {
        let mut purposes = vec![];
        let mut target_users = vec![];

        // Infer from file patterns
        let has_cli = context
            .files
            .iter()
            .any(|f| f.path.contains("cli") || f.path.contains("/bin/"));
        let has_lib = context.files.iter().any(|f| f.path.contains("lib.rs"));
        let has_api = context
            .files
            .iter()
            .any(|f| f.path.contains("api") || f.path.contains("handler"));
        let has_web = context
            .files
            .iter()
            .any(|f| f.path.contains("web") || f.path.contains("server"));

        if has_cli {
            purposes.push("Command-line tool".to_string());
            target_users.push("Developers".to_string());
        }
        if has_lib {
            purposes.push("Library/SDK".to_string());
            target_users.push("Library consumers".to_string());
        }
        if has_api {
            purposes.push("API service".to_string());
            target_users.push("API consumers".to_string());
        }
        if has_web {
            purposes.push("Web application".to_string());
            target_users.push("End users".to_string());
        }

        if purposes.is_empty() {
            purposes.push("Software application".to_string());
            target_users.push("Developers".to_string());
        }

        PurposeInsight {
            purposes,
            target_users,
            problems_solved: vec![],
        }
    }
}

#[async_trait::async_trait]
impl CharacterizationAgent for PurposeAgent {
    fn name(&self) -> &str {
        "purpose"
    }

    fn turn(&self) -> u8 {
        2
    }

    async fn run(&self, context: &CharacterizationContext) -> Result<AgentOutput, WeaveError> {
        run_agent(
            context,
            AgentConfig {
                name: "purpose",
                turn: 2,
                schema: AgentSchemas::purpose_schema(),
                build_prompt: Box::new(|ctx| {
                    let structure_insight =
                        extract_prior_insight_string(&ctx.prior_insights, "structure");
                    let entry_points =
                        extract_prior_insight_string(&ctx.prior_insights, "entry_point");
                    AgentPrompts::purpose_prompt(&structure_insight, &entry_points)
                }),
                fallback: Box::new(Self::fallback_analysis),
                confidence: Box::new(|insight: &PurposeInsight| {
                    calculate_confidence(insight.purposes.is_empty())
                }),
                debug_result: Box::new(|insight: &PurposeInsight| {
                    format!(
                        "Discovered {} purposes, {} target users",
                        insight.purposes.len(),
                        insight.target_users.len()
                    )
                }),
            },
        )
        .await
    }
}
