//! llms.txt Generator
//!
//! Generates llms.txt - a standardized format for AI-readable documentation.
//! Based on the llms.txt convention (similar to robots.txt for web crawlers).

use std::path::Path;
use tracing::info;

use super::bottom_up::{FileInsight, Importance};
use super::types::DocSession;
use crate::types::Result;

/// Generator for llms.txt output
pub struct LlmsTxtGenerator {
    project_name: String,
    project_description: Option<String>,
}

impl LlmsTxtGenerator {
    pub fn new(project_name: &str) -> Self {
        Self {
            project_name: project_name.to_string(),
            project_description: None,
        }
    }

    pub fn with_description(mut self, description: &str) -> Self {
        self.project_description = Some(description.to_string());
        self
    }

    /// Generate llms.txt content from session data and file insights
    pub fn generate(&self, session: &DocSession, insights: &[FileInsight]) -> String {
        let mut output = String::new();

        // Header
        output.push_str(&format!("# {}\n\n", self.project_name));
        if let Some(ref desc) = self.project_description {
            output.push_str(&format!("> {}\n\n", desc));
        }

        // Quick Facts
        output.push_str("## Quick Facts\n\n");
        output.push_str(&format!("- Files: {}\n", session.files_analyzed));
        output.push_str(&format!(
            "- Quality: {:.0}%\n",
            session.quality_score * 100.0
        ));
        output.push_str(&format!(
            "- Generated: {}\n\n",
            chrono::Utc::now().format("%Y-%m-%d")
        ));

        // Critical Files (most important to understand first)
        output.push_str("## Critical Files\n\n");
        output.push_str("Files that are essential to understand the system:\n\n");
        for insight in insights
            .iter()
            .filter(|i| matches!(i.importance, Importance::Critical))
        {
            let purpose = if insight.purpose.is_empty() {
                "No description"
            } else {
                &insight.purpose
            };
            output.push_str(&format!("- `{}`: {}\n", insight.file_path, purpose));
        }
        if insights
            .iter()
            .all(|i| !matches!(i.importance, Importance::Critical))
        {
            output.push_str("- No critical files identified\n");
        }
        output.push('\n');

        // High Importance Files
        output.push_str("## Key Files\n\n");
        output.push_str("Important files to understand:\n\n");
        for insight in insights
            .iter()
            .filter(|i| matches!(i.importance, Importance::High))
            .take(15)
        {
            let purpose = if insight.purpose.is_empty() {
                "No description"
            } else {
                &insight.purpose
            };
            output.push_str(&format!("- `{}`: {}\n", insight.file_path, purpose));
        }
        output.push('\n');

        // Documentation summaries from content
        output.push_str("## Documentation Highlights\n\n");
        let documented_files: Vec<_> = insights
            .iter()
            .filter(|i| i.has_content())
            .take(10)
            .collect();

        if documented_files.is_empty() {
            output.push_str("- No documentation content generated yet\n");
        } else {
            for insight in documented_files {
                // Extract first 100 chars of content as summary
                let summary = insight.content.chars().take(100).collect::<String>();
                let summary = summary.split('\n').next().unwrap_or(&summary);
                output.push_str(&format!("- `{}`: {}\n", insight.file_path, summary));
            }
        }
        output.push('\n');

        // Module Overview (grouped by directory)
        output.push_str("## Modules\n\n");
        let mut modules: std::collections::HashMap<String, Vec<&FileInsight>> =
            std::collections::HashMap::new();
        for insight in insights {
            let module = insight.file_path.rsplit('/').nth(1).unwrap_or("root");
            modules.entry(module.to_string()).or_default().push(insight);
        }
        let mut module_names: Vec<_> = modules.keys().cloned().collect();
        module_names.sort();
        for module in module_names.iter().take(20) {
            let Some(files) = modules.get(module) else {
                continue;
            };
            let critical_count = files
                .iter()
                .filter(|f| matches!(f.importance, Importance::Critical | Importance::High))
                .count();
            output.push_str(&format!(
                "- `{}`: {} files ({} important)\n",
                module,
                files.len(),
                critical_count
            ));
        }
        output.push('\n');

        // Key Dependencies (from related_files)
        output.push_str("## Key Dependencies\n\n");
        let mut deps: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for insight in insights {
            for rel in &insight.related_files {
                *deps.entry(rel.path.clone()).or_insert(0) += 1;
            }
        }
        let mut dep_list: Vec<_> = deps.into_iter().collect();
        dep_list.sort_by(|a, b| b.1.cmp(&a.1));
        for (dep, count) in dep_list.iter().take(10) {
            output.push_str(&format!("- `{}`: referenced {} times\n", dep, count));
        }
        if dep_list.is_empty() {
            output.push_str("- No dependencies documented\n");
        }
        output.push('\n');

        // Diagrams
        output.push_str("## Architecture Diagrams\n\n");
        let files_with_diagrams: Vec<_> = insights
            .iter()
            .filter(|i| i.has_diagram())
            .take(5)
            .collect();
        if files_with_diagrams.is_empty() {
            output.push_str("- No architecture diagrams generated\n");
        } else {
            output.push_str(&format!(
                "{} files have architecture diagrams:\n",
                files_with_diagrams.len()
            ));
            for insight in files_with_diagrams {
                output.push_str(&format!("- `{}`\n", insight.file_path));
            }
        }
        output.push('\n');

        // Footer
        output.push_str("---\n");
        output.push_str(&format!(
            "Generated by WeaveWiki v{}\n",
            env!("CARGO_PKG_VERSION")
        ));

        output
    }

    /// Write llms.txt to the output directory
    pub fn write(
        &self,
        session: &DocSession,
        insights: &[FileInsight],
        output_dir: &Path,
    ) -> Result<()> {
        let content = self.generate(session, insights);
        let path = output_dir.join("llms.txt");

        std::fs::write(&path, &content)?;
        info!("Generated llms.txt at {}", path.display());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session() -> DocSession {
        DocSession {
            id: "test".to_string(),
            project_path: ".".to_string(),
            status: super::super::types::SessionStatus::Completed,
            current_phase: 6,
            total_files: 10,
            files_analyzed: 8,
            quality_score: 0.85,
            started_at: Some("2024-01-01".to_string()),
            last_checkpoint_at: None,
            completed_at: None,
            last_error: None,
            analysis_mode: "standard".to_string(),
            detected_scale: "medium".to_string(),
            project_profile: None,
            quality_scores_history: None,
            refinement_turn: 0,
            checkpoint_data: None,
        }
    }

    fn make_insight(path: &str, importance: Importance) -> FileInsight {
        FileInsight {
            file_path: path.to_string(),
            language: Some("rust".to_string()),
            line_count: 100,
            importance,
            tier: super::super::bottom_up::ProcessingTier::Standard,
            purpose: format!("{} purpose", path),
            content: "## Overview\n\nThis is the documentation content.".to_string(),
            diagram: Some("graph TD; A-->B".to_string()),
            related_files: vec![super::super::bottom_up::RelatedFile {
                path: "other.rs".to_string(),
                relationship: "imports".to_string(),
            }],
            token_count: 50,
            research_iterations_json: None,
            research_aspects_json: None,
        }
    }

    #[test]
    fn test_generate_basic() {
        let session = make_session();
        let insights = vec![
            make_insight("src/main.rs", Importance::Critical),
            make_insight("src/lib.rs", Importance::High),
            make_insight("src/utils.rs", Importance::Low),
        ];

        let generator = LlmsTxtGenerator::new("TestProject").with_description("A test project");

        let output = generator.generate(&session, &insights);

        assert!(output.contains("# TestProject"));
        assert!(output.contains("> A test project"));
        assert!(output.contains("## Critical Files"));
        assert!(output.contains("src/main.rs"));
    }

    #[test]
    fn test_empty_insights() {
        let session = make_session();
        let insights: Vec<FileInsight> = vec![];

        let generator = LlmsTxtGenerator::new("Empty");
        let output = generator.generate(&session, &insights);

        assert!(output.contains("# Empty"));
    }
}
