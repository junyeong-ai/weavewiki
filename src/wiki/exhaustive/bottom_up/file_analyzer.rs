//! File Analyzer
//!
//! Analyzes individual files with:
//! - Tier-aware analysis depth
//! - Hierarchical context from child documents
//! - Deep Research workflow for Important/Core files
//! - Diagram validation with auto-fix
//! - Token budget management
//!
//! Note: Child context lookup is now handled by `InsightRegistry` in the parent module.
//! This analyzer is immutable and can be shared across concurrent tasks without locking.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;

use crate::ai::provider::SharedProvider;
use crate::ai::validation::{DiagramValidation, validate_mermaid};
use crate::analyzer::parser::language::detect_language;
use crate::config::ModeConfig;
use crate::storage::FileAnalysisCheckpoint;
use crate::types::error::WeaveError;
use crate::types::node::{EvidenceLocation, InformationTier, NodeMetadata, NodeStatus, NodeType};
use crate::types::{Node, estimate_tokens, truncate_to_token_limit};
use crate::wiki::exhaustive::characterization::profile::ProjectProfile;
use crate::wiki::exhaustive::checkpoint::CheckpointContext;
use crate::wiki::exhaustive::research::{ResearchContext, ResearchPhase, build_research_prompt};

use super::graph_context::{FileStructuralContext, GraphContextProvider};
use super::parsers::parse_file_insight;
use super::prompts::{build_analysis_prompt, diagram_fix_schema, file_insight_schema};
use super::types::*;

/// Maximum diagram fix attempts
const MAX_DIAGRAM_FIX_ATTEMPTS: usize = 2;

/// File analyzer with full context support
///
/// This struct is immutable and can be shared via `Arc` without locking.
/// Child context lookup is handled externally by `InsightRegistry`.
pub struct FileAnalyzer {
    pub project_root: PathBuf,
    pub profile: Arc<ProjectProfile>,
    pub config: ModeConfig,
    pub provider: SharedProvider,
    checkpoint: Option<CheckpointContext>,
}

impl FileAnalyzer {
    pub fn new(
        project_root: PathBuf,
        profile: Arc<ProjectProfile>,
        config: ModeConfig,
        provider: SharedProvider,
        checkpoint: Option<CheckpointContext>,
    ) -> Self {
        Self {
            project_root,
            profile,
            config,
            provider,
            checkpoint,
        }
    }

    /// Analyze a file with full tier-aware processing
    ///
    /// Uses Deep Research workflow for Important/Core tiers,
    /// single-pass analysis for Leaf/Standard tiers.
    pub async fn analyze(&self, request: AnalysisRequest) -> Result<FileInsight, WeaveError> {
        let full_path = self.project_root.join(&request.file_path);
        let content = tokio::fs::read_to_string(&full_path).await?;
        let line_count = content.lines().count();
        let language = detect_language(&full_path).map(|s| s.to_string());

        // Mark file as analyzing
        self.mark_file_status(&request.file_path, "analyzing");

        // Choose analysis strategy based on tier
        let insight = if request.tier.uses_deep_research() {
            // Deep Research for Important/Core tiers
            self.analyze_with_deep_research(&request, &content, language.clone(), line_count)
                .await?
        } else {
            // Single-pass for Leaf/Standard tiers
            self.analyze_single_pass(&request, &content, language.clone(), line_count)
                .await?
        };

        // Mark file as analyzed
        self.mark_file_status(&request.file_path, "analyzed");

        Ok(insight)
    }

    /// Single-pass analysis for Leaf/Standard tiers
    async fn analyze_single_pass(
        &self,
        request: &AnalysisRequest,
        content: &str,
        language: Option<String>,
        line_count: usize,
    ) -> Result<FileInsight, WeaveError> {
        // Get structural context from Knowledge Graph
        let structural_context = self.get_structural_context(&request.file_path);

        tracing::debug!(
            "Single-pass analysis: {} (tier={:?})",
            request.file_path,
            request.tier
        );

        // Build prompt
        let prompt = build_analysis_prompt(
            request,
            content,
            &self.profile,
            structural_context.as_ref(),
            self.config.bottom_up_max_file_chars,
            None, // TODO: Pass SessionContext to save ~300-400 tokens per file
        );

        let schema = file_insight_schema();
        let response = self.provider.generate(&prompt, &schema).await?.content;

        // Parse result
        let mut parsed = parse_file_insight(&request.file_path, language, line_count, response);
        parsed.tier = request.tier;

        // Validate and process
        self.post_process_insight(&mut parsed, request.tier).await;

        Ok(parsed)
    }

    /// Deep Research analysis for Important/Core tiers
    async fn analyze_with_deep_research(
        &self,
        request: &AnalysisRequest,
        content: &str,
        language: Option<String>,
        line_count: usize,
    ) -> Result<FileInsight, WeaveError> {
        let max_iterations = request.tier.research_iterations();

        tracing::info!(
            "Deep Research: {} ({} iterations, tier={:?})",
            request.file_path,
            max_iterations,
            request.tier
        );

        let mut research_context = ResearchContext::new(request.file_path.clone());

        // Execute research iterations
        for iter in 1..=max_iterations {
            let phase = ResearchPhase::from_iteration(iter, max_iterations);

            tracing::info!(
                "Deep Research: {} - {:?} (iteration {}/{})",
                request.file_path,
                phase,
                iter,
                max_iterations
            );

            // Build phase-specific prompt
            let prompt = build_research_prompt(
                phase,
                &request.file_path,
                &research_context,
                content,
                &self.profile,
                self.config.bottom_up_max_file_chars,
            );

            // Get schema for this phase
            let schema = crate::wiki::exhaustive::research::prompts::research_output_schema(phase);

            // Execute LLM call
            let response = self.provider.generate(&prompt, &schema).await?;

            // Parse and accumulate findings
            let iteration_result =
                crate::wiki::exhaustive::research::prompts::parse_research_output(
                    phase,
                    &response.content,
                )?;

            // Update context with new findings
            research_context.add_iteration(iteration_result);
        }

        // Build final FileInsight from research context
        self.build_insight_from_research(request, language, line_count, &research_context)
            .await
    }

    /// Build FileInsight from completed research
    async fn build_insight_from_research(
        &self,
        request: &AnalysisRequest,
        language: Option<String>,
        line_count: usize,
        context: &ResearchContext,
    ) -> Result<FileInsight, WeaveError> {
        let synthesis = context
            .get_synthesis()
            .ok_or_else(|| WeaveError::WikiGeneration {
                item: request.file_path.clone(),
                reason: "Deep Research did not produce synthesis".to_string(),
            })?;

        // Serialize research context for checkpoint/resume
        let research_iterations_json = serde_json::to_string(&context.iterations).ok();
        let research_aspects_json = serde_json::to_string(&context.covered_aspects).ok();

        let mut insight = FileInsight {
            file_path: request.file_path.clone(),
            language,
            line_count,
            purpose: synthesis
                .purpose
                .clone()
                .unwrap_or_else(|| "Purpose not specified".to_string()),
            importance: crate::wiki::exhaustive::types::Importance::High,
            tier: request.tier,
            content: synthesis.content.clone().unwrap_or_default(),
            diagram: synthesis.diagram.clone(),
            related_files: synthesis.related_files.clone(),
            token_count: 0,
            research_iterations_json,
            research_aspects_json,
        };

        // Post-process (validate diagram, enforce token budget)
        self.post_process_insight(&mut insight, request.tier).await;

        Ok(insight)
    }

    /// Post-process insight (quality validation, diagram fix, token budget)
    async fn post_process_insight(&self, insight: &mut FileInsight, tier: ProcessingTier) {
        // Validate output quality
        let quality_issues = validate_output_quality(insight, tier);
        if !quality_issues.is_empty() {
            for issue in &quality_issues {
                tracing::warn!("Quality issue in {}: {}", insight.file_path, issue);
            }
        }

        // Validate and fix diagram if present
        if let Some(ref diagram) = insight.diagram {
            insight.diagram = self.validate_and_fix_diagram(diagram).await;
        }

        // Calculate token count
        insight.token_count = estimate_tokens(&insight.content);

        // Enforce token budget
        if insight.token_count > tier.max_content_tokens() {
            insight.content = truncate_to_token_limit(&insight.content, tier.max_content_tokens());
            insight.token_count = tier.max_content_tokens();
        }
    }

    /// Validate Mermaid diagram and attempt to fix if invalid
    async fn validate_and_fix_diagram(&self, diagram: &str) -> Option<String> {
        if diagram.trim().is_empty() {
            return None;
        }

        let validation = validate_mermaid(diagram);
        if validation.is_valid {
            return Some(diagram.to_string());
        }

        // Attempt to fix
        for attempt in 0..MAX_DIAGRAM_FIX_ATTEMPTS {
            tracing::debug!(
                "Diagram validation failed, fix attempt {}/{}",
                attempt + 1,
                MAX_DIAGRAM_FIX_ATTEMPTS
            );

            let fix_prompt = self.build_diagram_fix_prompt(diagram, &validation);
            let schema = diagram_fix_schema();

            match self.provider.generate(&fix_prompt, &schema).await {
                Ok(response) => {
                    // response.content is already a serde_json::Value
                    if let Some(fixed) = response.content.get("diagram").and_then(|d| d.as_str()) {
                        let revalidation = validate_mermaid(fixed);
                        if revalidation.is_valid {
                            return Some(fixed.to_string());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Diagram fix attempt failed: {}", e);
                }
            }
        }

        // Return original if all fixes failed
        tracing::warn!(
            "Could not fix diagram after {} attempts",
            MAX_DIAGRAM_FIX_ATTEMPTS
        );
        Some(diagram.to_string())
    }

    /// Build prompt for diagram fix
    fn build_diagram_fix_prompt(&self, diagram: &str, validation: &DiagramValidation) -> String {
        let errors: Vec<String> = validation
            .errors
            .iter()
            .map(|e| {
                if let Some(ref suggestion) = e.suggestion {
                    format!(
                        "Line {}: {} (suggestion: {})",
                        e.line, e.message, suggestion
                    )
                } else {
                    format!("Line {}: {}", e.line, e.message)
                }
            })
            .collect();

        format!(
            r#"Fix this Mermaid diagram. The diagram type is: {}

Original diagram:
```
{}
```

Errors found:
{}

Return ONLY the corrected diagram code, no markdown fences."#,
            validation.diagram_type,
            diagram,
            errors.join("\n")
        )
    }

    // Note: get_child_contexts() has been moved to InsightRegistry in mod.rs
    // for lock-free concurrent access via DashMap.

    /// Get structural context from Knowledge Graph
    fn get_structural_context(&self, file_path: &str) -> Option<FileStructuralContext> {
        let ctx = self.checkpoint.as_ref()?;
        let provider = GraphContextProvider::new(&ctx.db);
        let structural_ctx = provider.get_file_context(file_path);

        if structural_ctx.is_empty() {
            None
        } else {
            tracing::debug!(
                "Using {} structural facts for {}",
                structural_ctx.functions.len()
                    + structural_ctx.structs.len()
                    + structural_ctx.enums.len()
                    + structural_ctx.traits.len(),
                file_path
            );
            Some(structural_ctx)
        }
    }

    /// Mark file status in tracking table
    fn mark_file_status(&self, file_path: &str, status: &str) {
        let Some(ctx) = &self.checkpoint else {
            return;
        };

        if let Err(e) = ctx.db.execute(
            "UPDATE file_tracking SET status = ?1 WHERE session_id = ?2 AND file_path = ?3",
            &[&status.to_string(), &ctx.session_id, &file_path.to_string()],
        ) {
            tracing::warn!(
                "Failed to update file status for '{}' to '{}': {}",
                file_path,
                status,
                e
            );
        }
    }

    /// Store file insight with atomic checkpoint
    pub fn store_insight(&self, insight: &FileInsight) -> Result<(), WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(());
        };

        let content_json = serde_json::to_string(&insight.content)?;
        let diagram_json = serde_json::to_string(&insight.diagram)?;

        let checkpoint = FileAnalysisCheckpoint {
            file_path: insight.file_path.clone(),
            language: insight.language.clone(),
            line_count: insight.line_count,
            complexity: insight.importance.as_str().to_string(),
            purpose_summary: insight.purpose.clone(),
            sections_json: content_json,
            key_insights_json: diagram_json,
            // Deep Research context for Important/Core tiers
            research_iterations_json: insight.research_iterations_json.clone(),
            research_aspects_json: insight.research_aspects_json.clone(),
        };

        let graph_nodes = self.build_graph_nodes(insight);
        ctx.db
            .checkpoint_file_analysis(&ctx.session_id, &checkpoint, &graph_nodes, &[])?;

        tracing::debug!(
            "Checkpointed {} (tier={:?}, tokens={}, deep_research={})",
            insight.file_path,
            insight.tier,
            insight.token_count,
            insight.research_iterations_json.is_some()
        );

        Ok(())
    }

    /// Convert insight to Knowledge Graph nodes
    fn build_graph_nodes(&self, insight: &FileInsight) -> Vec<Node> {
        let mut nodes = Vec::new();
        let now = Utc::now();

        if insight.has_content() {
            let mut extra = HashMap::new();
            extra.insert(
                "importance".to_string(),
                serde_json::Value::String(insight.importance.as_str().to_string()),
            );
            extra.insert(
                "tier".to_string(),
                serde_json::Value::String(format!("{:?}", insight.tier)),
            );
            if insight.has_diagram() {
                extra.insert("has_diagram".to_string(), serde_json::Value::Bool(true));
            }
            extra.insert(
                "token_count".to_string(),
                serde_json::Value::Number(insight.token_count.into()),
            );

            let node = Node {
                id: format!("doc:{}", insight.file_path),
                node_type: NodeType::Entity,
                path: insight.file_path.clone(),
                name: insight.purpose.clone(),
                metadata: NodeMetadata {
                    description: Some(insight.purpose.clone()),
                    extra,
                    ..Default::default()
                },
                evidence: EvidenceLocation {
                    file: insight.file_path.clone(),
                    start_line: 1,
                    end_line: insight.line_count as u32,
                    start_column: None,
                    end_column: None,
                },
                tier: InformationTier::Inference,
                confidence: 0.85,
                last_verified: now,
                status: NodeStatus::Verified,
            };
            nodes.push(node);
        }

        // Create relationship nodes
        for rel in &insight.related_files {
            let mut extra = HashMap::new();
            extra.insert(
                "relationship_type".to_string(),
                serde_json::Value::String(rel.relationship.clone()),
            );

            let node = Node {
                id: format!("rel:{}:{}", insight.file_path, rel.path),
                node_type: NodeType::Module,
                path: insight.file_path.clone(),
                name: format!("{} -> {}", insight.file_path, rel.path),
                metadata: NodeMetadata {
                    extra,
                    ..Default::default()
                },
                evidence: EvidenceLocation {
                    file: insight.file_path.clone(),
                    start_line: 0,
                    end_line: 0,
                    start_column: None,
                    end_column: None,
                },
                tier: InformationTier::Inference,
                confidence: 0.80,
                last_verified: now,
                status: NodeStatus::Verified,
            };
            nodes.push(node);
        }

        nodes
    }

    /// Mark a file as unanalyzed with reason
    pub fn mark_unanalyzed(&self, file_path: &str, reason: &str) -> Result<(), WeaveError> {
        let Some(ctx) = &self.checkpoint else {
            return Ok(());
        };

        ctx.db
            .mark_file_failed(&ctx.session_id, file_path, reason)?;
        Ok(())
    }
}

/// Validate output quality against anti-patterns
///
/// Returns a list of quality issues found. Empty list means good quality.
fn validate_output_quality(insight: &FileInsight, tier: ProcessingTier) -> Vec<String> {
    let mut issues = Vec::new();
    let content_lower = insight.content.to_lowercase();
    let purpose_lower = insight.purpose.to_lowercase();

    // 1. Check for preambles
    let preamble_patterns = [
        "here is the documentation",
        "here's the documentation",
        "this file contains the implementation",
        "let me explain",
        "i'll document",
        "the following documentation",
    ];
    for pattern in preamble_patterns {
        if content_lower.starts_with(pattern) || purpose_lower.starts_with(pattern) {
            issues.push(format!("Starts with preamble: '{}'", pattern));
            break;
        }
    }

    // 2. Check for generic praise statements
    let generic_patterns = [
        "well-structured",
        "well structured",
        "clean code",
        "follows best practices",
        "this is a well",
        "nicely organized",
        "good organization",
    ];
    for pattern in generic_patterns {
        if content_lower.contains(pattern) {
            issues.push(format!("Contains generic praise: '{}'", pattern));
            break;
        }
    }

    // 3. Check minimum content length by tier
    let min_words = match tier {
        ProcessingTier::Leaf => 20,
        ProcessingTier::Standard => 50,
        ProcessingTier::Important => 100,
        ProcessingTier::Core => 150,
    };
    let word_count = insight.content.split_whitespace().count();
    if word_count < min_words {
        issues.push(format!(
            "Content too short for {:?} tier: {} words (min: {})",
            tier, word_count, min_words
        ));
    }

    // 4. Check for empty purpose
    if insight.purpose.trim().is_empty() {
        issues.push("Empty purpose field".to_string());
    }

    // 5. Check for obvious repetition of file path in purpose
    let file_name = insight
        .file_path
        .rsplit('/')
        .next()
        .unwrap_or(&insight.file_path);
    let file_base = file_name.split('.').next().unwrap_or(file_name);
    if purpose_lower.starts_with(&format!("the {} file", file_base.to_lowercase()))
        || purpose_lower.starts_with(&format!("{} is", file_base.to_lowercase()))
    {
        issues.push("Purpose repeats obvious file name information".to_string());
    }

    // 6. Check for placeholder content
    let placeholder_patterns = ["todo:", "[todo]", "[placeholder]", "needs more detail"];
    for pattern in placeholder_patterns {
        if content_lower.contains(pattern) {
            issues.push(format!("Contains placeholder: '{}'", pattern));
            break;
        }
    }

    // 7. Check for markdown code fence wrapping (the whole content)
    if insight.content.trim().starts_with("```markdown")
        || insight.content.trim().starts_with("```md")
    {
        issues.push("Content wrapped in markdown code fence".to_string());
    }

    // 8. Check Important/Core tiers have diagram
    if matches!(tier, ProcessingTier::Important | ProcessingTier::Core) && insight.diagram.is_none()
    {
        issues.push(format!(
            "{:?} tier should have a diagram but none provided",
            tier
        ));
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_basic() {
        // Empty content returns 0
        assert_eq!(estimate_tokens(""), 0);

        // Non-empty content returns > 0
        assert!(estimate_tokens("hello world") > 0);
    }

    #[test]
    fn test_truncate_to_token_limit() {
        let content = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";

        // Should not truncate if within limit
        let result = truncate_to_token_limit(content, 1000);
        assert_eq!(result, content);

        // Should truncate and add marker when limit is small
        let result = truncate_to_token_limit(content, 5);
        assert!(result.contains("truncated"));
    }
}
