//! Structure Agent - Turn 1
//!
//! Analyzes directory structure and organization patterns using LLM.

use super::StructureInsight;
use super::helpers::{AgentConfig, calculate_confidence, format_file_list, run_agent};
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::characterization::schemas::{AgentPrompts, AgentSchemas};
use crate::wiki::exhaustive::characterization::{
    AgentOutput, CharacterizationAgent, CharacterizationContext,
};

pub struct StructureAgent;

impl StructureAgent {
    /// Fallback analysis when LLM fails
    fn fallback_analysis(context: &CharacterizationContext) -> StructureInsight {
        let mut patterns = vec![];
        let mut modules = vec![];

        // Detect common patterns from file paths
        let has_src = context.files.iter().any(|f| f.path.starts_with("src/"));
        let has_lib = context
            .files
            .iter()
            .any(|f| f.path.contains("/lib/") || f.path.starts_with("lib/"));
        let has_tests = context
            .files
            .iter()
            .any(|f| f.path.contains("test") || f.path.contains("spec"));

        if has_src {
            patterns.push("src/ directory structure".to_string());
        }
        if has_lib {
            patterns.push("lib/ library structure".to_string());
        }
        if has_tests {
            patterns.push("dedicated test directory".to_string());
        }

        // Detect module boundaries from directory structure
        let mut seen_dirs = std::collections::HashSet::new();
        for file in &context.files {
            let parts: Vec<&str> = file.path.split('/').collect();
            if parts.len() > 1 {
                let top_dir = parts[0];
                if !seen_dirs.contains(top_dir) && ![".", ".."].contains(&top_dir) {
                    seen_dirs.insert(top_dir.to_string());
                    modules.push(super::ModuleBoundary {
                        name: top_dir.to_string(),
                        path: top_dir.to_string(),
                        purpose: None,
                    });
                }
            }
        }

        let organization_style = if modules.len() > 5 {
            "feature_based"
        } else if has_src && has_lib {
            "layered"
        } else {
            "flat"
        };

        StructureInsight {
            directory_patterns: patterns,
            module_boundaries: modules,
            organization_style: organization_style.to_string(),
            naming_conventions: vec![],
            test_organization: if has_tests {
                Some("separate".to_string())
            } else {
                None
            },
        }
    }
}

#[async_trait::async_trait]
impl CharacterizationAgent for StructureAgent {
    fn name(&self) -> &str {
        "structure"
    }

    fn turn(&self) -> u8 {
        1
    }

    async fn run(&self, context: &CharacterizationContext) -> Result<AgentOutput, WeaveError> {
        run_agent(
            context,
            AgentConfig {
                name: "structure",
                turn: 1,
                schema: AgentSchemas::structure_schema(),
                build_prompt: Box::new(|ctx| {
                    let file_list = format_file_list(&ctx.files, 100);
                    AgentPrompts::structure_prompt(&file_list)
                }),
                fallback: Box::new(Self::fallback_analysis),
                confidence: Box::new(|insight: &StructureInsight| {
                    calculate_confidence(insight.directory_patterns.is_empty())
                }),
                debug_result: Box::new(|insight: &StructureInsight| {
                    format!(
                        "Found {} patterns, {} modules",
                        insight.directory_patterns.len(),
                        insight.module_boundaries.len()
                    )
                }),
            },
        )
        .await
    }
}
