//! Artifact Generation Module
//!
//! Generates Claude Code artifacts from extracted insights:
//! - CLAUDE.md: Project context and architecture
//! - Rules: Constraints and mandatory requirements
//! - Skills: Reusable workflows and procedures
//! - Agents: Domain expertise and specialized roles

mod agents;
mod claude_md;
mod rules;
mod skills;
mod validators;

pub use agents::AgentsGenerator;
pub use claude_md::ClaudeMdGenerator;
pub use rules::RulesGenerator;
pub use skills::SkillsGenerator;
pub use validators::{
    ArtifactValidator, BatchValidationResult, BatchValidator, IssueSeverity, ValidationIssue,
    ValidationResult,
};

use crate::pipeline::insight::{ArtifactClassification, ExtractedInsight, InsightExtractionResult};
use crate::types::{Agent, ProjectMemory, Rule, Skill};

/// Result of artifact generation
#[derive(Debug, Clone, Default)]
pub struct GeneratedArtifacts {
    pub claude_md: Option<ProjectMemory>,
    pub rules: Vec<Rule>,
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub stats: GenerationStats,
}

/// Statistics about generation process
#[derive(Debug, Clone, Default)]
pub struct GenerationStats {
    pub insights_used: usize,
    pub insights_filtered: usize,
    pub claude_md_sections: usize,
    pub rules_generated: usize,
    pub skills_generated: usize,
    pub agents_generated: usize,
}

/// Trait for artifact generators
pub trait ArtifactGenerator {
    type Output;

    fn generate(&self, insights: &[ExtractedInsight]) -> Self::Output;
}

/// Filter insights by artifact classification
pub fn filter_insights_for_artifact(
    insights: &[ExtractedInsight],
    artifact: ArtifactClassification,
) -> Vec<&ExtractedInsight> {
    insights
        .iter()
        .filter(|i| i.artifact == artifact)
        .collect()
}

/// Sort insights by value score descending
pub fn sort_by_value(insights: &mut [&ExtractedInsight]) {
    insights.sort_by(|a, b| {
        b.value
            .overall
            .partial_cmp(&a.value.overall)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Generate all artifacts from insight extraction result
pub fn generate_all_artifacts(
    result: &InsightExtractionResult,
    project_name: &str,
) -> GeneratedArtifacts {
    let claude_md_gen = ClaudeMdGenerator::new(project_name.to_string());
    let rules_gen = RulesGenerator::new();
    let skills_gen = SkillsGenerator::new();
    let agents_gen = AgentsGenerator::new();

    let claude_md = claude_md_gen.generate(&result.by_artifact.claude_md);
    let rules = rules_gen.generate(&result.by_artifact.rules);
    let skills = skills_gen.generate(&result.by_artifact.skills);
    let agents = agents_gen.generate(&result.by_artifact.agents);

    let stats = GenerationStats {
        insights_used: result.insights.len() - result.stats.tier0_rejected,
        insights_filtered: result.stats.tier0_rejected,
        claude_md_sections: claude_md.as_ref().map(|m| m.standards.len()).unwrap_or(0),
        rules_generated: rules.len(),
        skills_generated: skills.len(),
        agents_generated: agents.len(),
    };

    GeneratedArtifacts {
        claude_md,
        rules,
        skills,
        agents,
        stats,
    }
}
