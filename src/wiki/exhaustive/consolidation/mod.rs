//! Consolidation
//!
//! Groups file insights by semantic domain and synthesizes cohesive
//! domain-level documentation using AI.
//!
//! ## Key Principle
//!
//! Instead of simple concatenation, AI synthesizes file documentation
//! into a unified domain narrative, linking to individual files rather
//! than duplicating their content.

pub mod gap_detector;
pub mod grouping;

use crate::ai::provider::SharedProvider;
use crate::storage::SharedDatabase;
use crate::types::error::WeaveError;
use crate::wiki::exhaustive::bottom_up::{FileInsight, Importance, RelatedFile};
use crate::wiki::exhaustive::characterization::profile::ProjectProfile;
use crate::wiki::exhaustive::checkpoint::CheckpointContext;
use crate::wiki::exhaustive::top_down::insights::ProjectInsight;
use futures::stream::StreamExt;
use grouping::SemanticDomainGrouper;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;

/// Maximum files to include in full detail in synthesis prompt
const MAX_FULL_DETAIL_FILES: usize = 5;
/// Maximum summary files to include
const MAX_SUMMARY_FILES: usize = 10;

/// Orchestrator for consolidation with AI synthesis
#[derive(Clone)]
pub struct ConsolidationAnalyzer {
    profile: Arc<ProjectProfile>,
    provider: SharedProvider,
    checkpoint: Option<CheckpointContext>,
}

impl ConsolidationAnalyzer {
    pub fn new(profile: Arc<ProjectProfile>, provider: SharedProvider) -> Self {
        Self {
            profile,
            provider,
            checkpoint: None,
        }
    }

    pub fn with_checkpoint(mut self, db: SharedDatabase, session_id: String) -> Self {
        self.checkpoint = Some(CheckpointContext::new(db, session_id));
        self
    }

    /// Run consolidation with AI synthesis
    pub async fn run(
        &self,
        file_insights: Vec<FileInsight>,
        project_insights: Vec<ProjectInsight>,
    ) -> Result<Vec<DomainInsight>, WeaveError> {
        tracing::info!(
            "Consolidation: Starting ({} files, {} project insights)",
            file_insights.len(),
            project_insights.len()
        );

        // Try to load existing domain summaries (for resume)
        if let Some(summaries) = self.load_domain_summaries()? {
            tracing::info!(
                "Consolidation: Resuming with {} cached domain summaries",
                summaries.len()
            );
            return Ok(summaries);
        }

        // 1. Group file insights by semantic domain
        let grouper = SemanticDomainGrouper::new(&self.profile, self.provider.clone());
        let grouped = grouper.group(&file_insights).await?;

        tracing::info!(
            "Consolidation: Grouped {} files into {} domains",
            file_insights.len(),
            grouped.len()
        );

        // 2. Synthesize domain documentation using AI (parallel with concurrency control)
        const MAX_DOMAIN_CONCURRENCY: usize = 3;

        let mut summaries = Vec::new();

        // Use Arc to share project_insights across async tasks without cloning the entire Vec
        let shared_insights = Arc::new(project_insights);

        let mut stream = futures::stream::iter(grouped)
            .map(|(domain_name, files)| {
                let analyzer = self.clone();
                let insights = Arc::clone(&shared_insights);
                async move {
                    analyzer
                        .synthesize_domain(&domain_name, files, &insights)
                        .await
                }
            })
            .buffer_unordered(MAX_DOMAIN_CONCURRENCY);

        while let Some(result) = stream.next().await {
            match result {
                Ok(summary) => summaries.push(summary),
                Err(e) => tracing::warn!("Domain synthesis failed: {}", e),
            }
        }

        // 3. Detect gaps
        for summary in &mut summaries {
            summary.gaps = gap_detector::detect_gaps(summary);
        }

        // 4. Store results
        self.store_domain_summaries(&summaries)?;

        tracing::info!(
            "Consolidation: Complete ({} domain summaries)",
            summaries.len()
        );
        Ok(summaries)
    }

    /// Synthesize a domain using AI instead of simple concatenation
    async fn synthesize_domain(
        &self,
        domain_name: &str,
        file_insights: Vec<FileInsight>,
        project_insights: &Arc<Vec<ProjectInsight>>,
    ) -> Result<DomainInsight, WeaveError> {
        let mut summary = DomainInsight::new(domain_name.to_string());

        // Collect file paths
        summary.files = file_insights
            .iter()
            .map(|fi| fi.file_path.clone())
            .collect();

        // Determine domain importance
        summary.importance = file_insights
            .iter()
            .map(|fi| fi.importance)
            .max()
            .unwrap_or(Importance::Medium);

        // For small domains, skip LLM synthesis
        if file_insights.len() <= 2 {
            return self.simple_merge(summary, &file_insights);
        }

        // Build synthesis prompt
        let prompt = self.build_synthesis_prompt(domain_name, &file_insights, project_insights);
        let schema = domain_synthesis_schema();

        // Call LLM for synthesis
        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                // response.content is already a serde_json::Value
                self.parse_synthesis_response(&mut summary, &response.content)?;
            }
            Err(e) => {
                tracing::warn!(
                    "Synthesis failed for domain {}, falling back to merge: {}",
                    domain_name,
                    e
                );
                return self.simple_merge(summary, &file_insights);
            }
        }

        // Collect all related files (O(1) lookup with HashSet)
        // Build set of existing paths first (using owned Strings to avoid borrow issues)
        let existing_paths: HashSet<String> = summary
            .related_files
            .iter()
            .map(|r| r.path.clone())
            .chain(summary.files.iter().cloned())
            .collect();

        for fi in &file_insights {
            for rel in &fi.related_files {
                if !existing_paths.contains(&rel.path) {
                    summary.related_files.push(rel.clone());
                }
            }
        }

        Ok(summary)
    }

    /// Build AI synthesis prompt with Role + Objectives pattern
    fn build_synthesis_prompt(
        &self,
        domain_name: &str,
        file_insights: &[FileInsight],
        project_insights: &Arc<Vec<ProjectInsight>>,
    ) -> String {
        let mut prompt = String::new();

        // Role and Objectives (CodeWiki pattern)
        prompt.push_str(&format!(
            r#"<ROLE>
You are a documentation architect synthesizing the `{domain}` domain documentation.
Your task is to create a unified, cohesive narrative that helps developers understand this domain as a whole.
</ROLE>

<OBJECTIVES>
Create domain documentation that:
1. Explains the domain's PURPOSE and its role in the broader system
2. Shows HOW the files collaborate to achieve this purpose
3. Provides an ENTRY POINT for developers new to this domain
4. Links to individual file documentation instead of duplicating content
</OBJECTIVES>

"#,
            domain = domain_name
        ));

        // Project context
        prompt.push_str(&format!("# Domain: {}\n\n", domain_name));
        prompt.push_str(&format!(
            "**Project**: {} - {}\n\n",
            self.profile.name,
            self.profile.purposes.join(", ")
        ));

        // Include project-level insights
        if !project_insights.is_empty() {
            prompt.push_str("## Architectural Context\n\n");
            for pi in project_insights.iter().take(3) {
                if let Some(ref pattern) = pi.architecture_pattern {
                    prompt.push_str(&format!("- **{}**: {}\n", pi.agent, pattern));
                } else if !pi.layers.is_empty() {
                    let layers: Vec<_> = pi.layers.iter().map(|l| l.name.as_str()).collect();
                    prompt.push_str(&format!(
                        "- **{}**: Layers: {}\n",
                        pi.agent,
                        layers.join(", ")
                    ));
                }
            }
            prompt.push('\n');
        }

        // Sort files by importance (Critical/High first for full detail)
        let mut sorted_files: Vec<&FileInsight> = file_insights.iter().collect();
        sorted_files.sort_by(|a, b| b.importance.cmp(&a.importance));

        prompt.push_str("## Files in This Domain\n\n");

        // Full detail for most important files
        for (i, fi) in sorted_files.iter().enumerate() {
            if i < MAX_FULL_DETAIL_FILES {
                prompt.push_str(&format!(
                    "### `{}` ({:?})\n\n**Purpose**: {}\n\n{}\n\n",
                    fi.file_path, fi.importance, fi.purpose, fi.content
                ));
            } else if i < MAX_FULL_DETAIL_FILES + MAX_SUMMARY_FILES {
                prompt.push_str(&format!(
                    "- `{}` ({:?}): {}\n",
                    fi.file_path, fi.importance, fi.purpose
                ));
            } else {
                // Just count remaining
                if i == MAX_FULL_DETAIL_FILES + MAX_SUMMARY_FILES {
                    let remaining = sorted_files.len() - i;
                    prompt.push_str(&format!("\n*... and {} more files*\n", remaining));
                }
                break;
            }
        }

        // Synthesis instructions with richness guidance
        prompt.push_str(&format!(
            r#"
## Synthesis Task

Create a **unified domain overview** for `{domain}` that tells a coherent story.

### Required Sections

1. **Overview** (2-3 sentences)
   - What this domain does
   - Why it exists
   - Its role in the project

2. **Architecture** (with Mermaid diagram if >3 files)
   - How the files collaborate
   - Data flow between components
   - Key interfaces/abstractions

3. **Key Concepts**
   - Important patterns used
   - Critical invariants to maintain
   - Design decisions and rationale

4. **Getting Started**
   - Entry points for developers new to this domain
   - Common modification scenarios
   - Links to key file documentation

### Richness Guidelines

Make the domain documentation genuinely useful:
- **Tell a story**: "When X needs to happen, first Y triggers, then Z processes..."
- **Explain collaboration**: "The orchestrator delegates to workers, which report back via..."
- **Show the big picture**: Use a Mermaid diagram to visualize relationships
- **Provide navigation**: "To add a new feature, start with [file.md] then..."

### Requirements

- LINK to files using `[filename](filename.md)` - don't duplicate their content
- Create ONE cohesive narrative, not a list of file descriptions
- Focus on PURPOSE and COLLABORATION, not implementation details
- Include Mermaid diagram if domain has >3 files with non-trivial interactions

### ANTI-PATTERNS (DO NOT)

<what_not_to_do>
❌ "This domain contains file1.rs which does X, file2.rs which does Y..."
❌ "Here is the synthesized documentation for this domain..."
❌ "The files in this domain are well-organized and follow best practices"
</what_not_to_do>

<what_to_do>
✓ "The storage layer provides persistent state management through a SQLite-backed graph store. When the pipeline needs to checkpoint, [database.rs](database.rs.md) serializes the session state, while [graph_store.rs](graph_store.rs.md) handles relationship persistence..."
</what_to_do>
"#,
            domain = domain_name
        ));

        prompt
    }

    /// Parse AI synthesis response (Value is already parsed JSON)
    fn parse_synthesis_response(
        &self,
        summary: &mut DomainInsight,
        json: &serde_json::Value,
    ) -> Result<(), WeaveError> {
        if let Some(overview) = json.get("overview").and_then(|v| v.as_str()) {
            summary.description = overview.to_string();
        }
        if let Some(content) = json.get("content").and_then(|v| v.as_str()) {
            summary.content = content.to_string();
        }
        if let Some(diagram) = json
            .get("diagram")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            summary.diagram = Some(diagram.to_string());
        }
        Ok(())
    }

    /// Simple merge for small domains
    fn simple_merge(
        &self,
        mut summary: DomainInsight,
        file_insights: &[FileInsight],
    ) -> Result<DomainInsight, WeaveError> {
        // Use most important file's content as primary
        let primary = file_insights.iter().max_by_key(|fi| fi.importance);

        if let Some(fi) = primary {
            summary.description = fi.purpose.clone();
            summary.content = fi.content.clone();
            summary.diagram = fi.diagram.clone();
        }

        // Add links to other files
        if file_insights.len() > 1 {
            summary.content.push_str("\n\n## Related Files\n\n");
            for fi in file_insights {
                if primary.map(|p| p.file_path != fi.file_path).unwrap_or(true) {
                    summary.content.push_str(&format!(
                        "- [{}]({}.md): {}\n",
                        fi.file_path,
                        fi.file_path.replace('/', "_"),
                        fi.purpose
                    ));
                }
            }
        }

        // Collect related files
        for fi in file_insights {
            for rel in &fi.related_files {
                if !summary.related_files.iter().any(|r| r.path == rel.path) {
                    summary.related_files.push(rel.clone());
                }
            }
        }

        Ok(summary)
    }

    /// Load domain summaries from checkpoint
    fn load_domain_summaries(&self) -> Result<Option<Vec<DomainInsight>>, WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(None);
        };

        let conn = ctx.db.connection()?;
        let mut stmt = conn.prepare(
            "SELECT checkpoint_data FROM doc_sessions WHERE id = ?1 AND current_phase >= 5",
        )?;

        let result: Option<String> = stmt
            .query_row([&ctx.session_id], |row| row.get(0))
            .map_err(|e| {
                tracing::warn!("Failed to load domain summaries checkpoint: {}", e);
                e
            })
            .ok();

        let summaries = result
            .and_then(|data| {
                serde_json::from_str::<crate::wiki::exhaustive::types::PipelineCheckpoint>(&data)
                    .map_err(|e| {
                        tracing::warn!("Failed to parse checkpoint data: {}", e);
                        e
                    })
                    .ok()
            })
            .and_then(|checkpoint| checkpoint.domain_insights_json)
            .and_then(|domain_json| {
                serde_json::from_str(&domain_json)
                    .map_err(|e| {
                        tracing::warn!("Failed to parse domain insights JSON: {}", e);
                        e
                    })
                    .ok()
            });

        Ok(summaries)
    }

    /// Store domain summaries to checkpoint
    fn store_domain_summaries(&self, summaries: &[DomainInsight]) -> Result<(), WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(());
        };

        let summaries_json = serde_json::to_string(summaries)?;
        let now = chrono::Utc::now().to_rfc3339();

        ctx.db.execute(
            "UPDATE doc_sessions SET current_phase = 5, last_checkpoint_at = ?1 WHERE id = ?2",
            &[&now, &ctx.session_id],
        )?;

        let conn = ctx.db.connection()?;
        let mut stmt = conn.prepare("SELECT checkpoint_data FROM doc_sessions WHERE id = ?1")?;
        let existing: Option<String> = stmt
            .query_row([&ctx.session_id], |row| row.get(0))
            .map_err(|e| {
                tracing::warn!("Failed to load existing checkpoint: {}", e);
                e
            })
            .ok();

        let mut checkpoint: crate::wiki::exhaustive::types::PipelineCheckpoint = existing
            .and_then(|s| {
                serde_json::from_str(&s)
                    .map_err(|e| {
                        tracing::warn!("Failed to parse existing checkpoint, using default: {}", e);
                        e
                    })
                    .ok()
            })
            .unwrap_or_default();

        checkpoint.domain_insights_json = Some(summaries_json);
        checkpoint.last_completed_phase = 5;
        checkpoint.touch();

        let checkpoint_json = serde_json::to_string(&checkpoint)?;
        ctx.db.execute(
            "UPDATE doc_sessions SET checkpoint_data = ?1 WHERE id = ?2",
            &[&checkpoint_json, &ctx.session_id],
        )?;

        Ok(())
    }
}

/// Schema for domain synthesis
fn domain_synthesis_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "description": "Domain synthesis output. Create unified overview, not file list.",
        "required": ["overview", "content"],
        "additionalProperties": false,
        "properties": {
            "overview": {
                "type": "string",
                "description": "2-3 sentences: what this domain does and its role"
            },
            "content": {
                "type": "string",
                "description": "Rich markdown with architecture, key concepts, usage. LINK to files, don't duplicate."
            },
            "diagram": {
                "type": "string",
                "description": "Mermaid diagram showing how files in this domain interact. NO wrapper."
            }
        }
    })
}

/// Domain-level insight with synthesized content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainInsight {
    /// Domain name
    pub name: String,

    /// Synthesized domain description
    pub description: String,

    /// Domain importance
    pub importance: Importance,

    /// Files in this domain
    pub files: Vec<String>,

    /// Synthesized markdown content (NOT concatenated)
    pub content: String,

    /// Domain-level diagram
    pub diagram: Option<String>,

    /// Cross-domain relationships
    pub related_files: Vec<RelatedFile>,

    /// Documentation gaps detected
    pub gaps: Vec<String>,

    /// Token count for budget tracking
    #[serde(default)]
    pub token_count: usize,
}

impl Default for DomainInsight {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            importance: Importance::Medium,
            files: Vec::new(),
            content: String::new(),
            diagram: None,
            related_files: Vec::new(),
            gaps: Vec::new(),
            token_count: 0,
        }
    }
}

impl DomainInsight {
    pub fn new(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    /// Check if this domain has meaningful content
    pub fn has_content(&self) -> bool {
        !self.content.is_empty() && self.content.len() > 100
    }

    /// Get content word count
    pub fn content_word_count(&self) -> usize {
        self.content.split_whitespace().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file_insight(path: &str, importance: Importance) -> FileInsight {
        FileInsight {
            file_path: path.to_string(),
            language: Some("rust".to_string()),
            line_count: 100,
            purpose: format!("Purpose of {}", path),
            importance,
            tier: crate::wiki::exhaustive::bottom_up::ProcessingTier::Standard,
            content: format!("## Overview\n\nThis is the content for {}.", path),
            diagram: None,
            related_files: vec![],
            token_count: 50,
            research_iterations_json: None,
            research_aspects_json: None,
        }
    }

    #[test]
    fn test_domain_insight_creation() {
        let insight = DomainInsight::new("test-domain".to_string());
        assert_eq!(insight.name, "test-domain");
        assert!(insight.files.is_empty());
        assert!(!insight.has_content());
    }

    #[test]
    fn test_domain_insight_has_content() {
        let mut insight = DomainInsight::new("test".to_string());
        assert!(!insight.has_content());

        insight.content =
            "This is meaningful content explaining the domain in detail with multiple sentences."
                .repeat(2);
        assert!(insight.has_content());
    }

    #[test]
    fn test_importance_ordering() {
        let files = [
            make_file_insight("a.rs", Importance::Low),
            make_file_insight("b.rs", Importance::Critical),
            make_file_insight("c.rs", Importance::Medium),
        ];

        let max_importance = files
            .iter()
            .map(|f| f.importance)
            .max()
            .expect("files array is non-empty");
        assert_eq!(max_importance, Importance::Critical);
    }
}
