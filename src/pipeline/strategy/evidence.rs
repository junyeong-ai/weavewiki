//! Evidence Strategy
//!
//! Operates at section level: identifies sections lacking references
//! and adds evidence only to sections that need it.

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use regex::Regex;

use crate::ai::LlmProvider;
use crate::types::{Agent, Result, Skill};

use super::{calculate_validated_quality, IssueKind, RefinementStrategy, StrategyContext, StrategyResult};

static FILE_REF_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@([a-zA-Z0-9_\-./]+\.(?:rs|ts|tsx|js|jsx|py|go|kt|java))(?::(\d+))?").unwrap()
});

static SECTION_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#{1,4}\s+(.+)$").unwrap());

pub struct EvidenceStrategy {
    provider: Arc<dyn LlmProvider>,
    min_refs_per_section: usize,
}

impl EvidenceStrategy {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            min_refs_per_section: 1,
        }
    }

    fn count_valid_references(&self, content: &str, context: &StrategyContext<'_>) -> usize {
        FILE_REF_PATTERN
            .captures_iter(content)
            .filter(|cap| {
                let path = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                context.file_registry.contains(path)
            })
            .count()
    }

    /// Parse content into sections with their reference counts
    fn analyze_sections(
        &self,
        content: &str,
        context: &StrategyContext<'_>,
    ) -> Vec<SectionAnalysis> {
        let mut sections = Vec::new();
        let mut current_section = SectionAnalysis {
            header: "Introduction".to_string(),
            content: String::new(),
            ref_count: 0,
        };

        for line in content.lines() {
            if let Some(cap) = SECTION_HEADER.captures(line) {
                // Save previous section if it has content
                if !current_section.content.trim().is_empty() {
                    current_section.ref_count =
                        self.count_valid_references(&current_section.content, context);
                    sections.push(current_section);
                }
                // Start new section
                current_section = SectionAnalysis {
                    header: cap.get(1).map(|m| m.as_str()).unwrap_or(line).to_string(),
                    content: line.to_string() + "\n",
                    ref_count: 0,
                };
            } else {
                current_section.content.push_str(line);
                current_section.content.push('\n');
            }
        }

        // Don't forget the last section
        if !current_section.content.trim().is_empty() {
            current_section.ref_count =
                self.count_valid_references(&current_section.content, context);
            sections.push(current_section);
        }

        sections
    }

    /// Build prompt targeting only sections that need more evidence
    fn build_section_evidence_prompt(
        &self,
        content_type: &str,
        name: &str,
        sections_needing_evidence: &[&SectionAnalysis],
        context: &StrategyContext<'_>,
    ) -> String {
        let file_context = context.file_registry.to_prompt_context(50);
        let code_samples = context.file_registry.get_code_samples(3);

        let sections_text = sections_needing_evidence
            .iter()
            .enumerate()
            .map(|(i, s)| format!("SECTION {}: {}\n{}", i + 1, s.header, s.content))
            .collect::<Vec<_>>()
            .join("\n---\n");

        format!(
            r##"Add @file:line references to ONLY these sections of {content_type} "{name}".

AVAILABLE FILES:
{file_context}

CODE SAMPLES:
{code_samples}

SECTIONS NEEDING EVIDENCE:
{sections_text}

For each section above, return the enhanced version with @file:line references.
Format: @path/to/file.ext:LINE (e.g., @src/main.rs:42)

Return JSON with enhanced_sections array in the same order."##,
            content_type = content_type,
            name = name,
            file_context = file_context,
            code_samples = code_samples,
            sections_text = sections_text,
        )
    }

}

struct SectionAnalysis {
    header: String,
    content: String,
    ref_count: usize,
}

#[async_trait]
impl RefinementStrategy for EvidenceStrategy {
    fn name(&self) -> &str {
        "evidence"
    }

    fn applicable_to(&self, issue: &IssueKind) -> bool {
        matches!(
            issue,
            IssueKind::WeakEvidence | IssueKind::MissingReferences
        )
    }

    fn priority(&self) -> u8 {
        70
    }

    async fn refine_skill(
        &self,
        skill: &mut Skill,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult> {
        // Analyze at section level
        let sections = self.analyze_sections(&skill.body, context);
        let sections_needing_evidence: Vec<&SectionAnalysis> = sections
            .iter()
            .filter(|s| s.ref_count < self.min_refs_per_section && !s.content.trim().is_empty())
            .collect();

        if sections_needing_evidence.is_empty() {
            return Ok(StrategyResult {
                success: true,
                quality_delta: 0.0,
                changes_made: vec!["All sections have sufficient evidence".to_string()],
            });
        }

        tracing::debug!(
            skill = skill.name,
            sections_needing_evidence = sections_needing_evidence.len(),
            total_sections = sections.len(),
            "Section-level evidence analysis"
        );

        // Build targeted prompt for sections needing evidence
        let prompt =
            self.build_section_evidence_prompt("skill", &skill.name, &sections_needing_evidence, context);

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "enhanced_sections": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["enhanced_sections"]
        });

        let old_total_refs = self.count_valid_references(&skill.body, context);

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                if let Some(enhanced) = response
                    .content
                    .get("enhanced_sections")
                    .and_then(|v| v.as_array())
                {
                    // Rebuild content with enhanced sections
                    let enhanced_strings: Vec<String> = enhanced
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();

                    if !enhanced_strings.is_empty() {
                        // Merge enhanced sections back into original
                        let mut new_body = skill.body.clone();
                        for (section, enhanced_content) in
                            sections_needing_evidence.iter().zip(enhanced_strings.iter())
                        {
                            new_body = new_body.replace(&section.content, enhanced_content);
                        }

                        let new_refs = self.count_valid_references(&new_body, context);

                        // CRITICAL: Check overall quality, not just reference count
                        // Adding references shouldn't degrade actionability
                        let old_quality = calculate_validated_quality(&skill.body, context.file_registry);
                        let new_quality = calculate_validated_quality(&new_body, context.file_registry);

                        if new_refs > old_total_refs && new_quality >= old_quality {
                            skill.body = new_body;
                            return Ok(StrategyResult {
                                success: true,
                                quality_delta: new_quality - old_quality,
                                changes_made: vec![format!(
                                    "Added {} references to {} sections in skill '{}' (total: {}, quality: {:.0}% -> {:.0}%)",
                                    new_refs - old_total_refs,
                                    sections_needing_evidence.len(),
                                    skill.name,
                                    new_refs,
                                    old_quality * 100.0,
                                    new_quality * 100.0
                                )],
                            });
                        } else if new_refs > old_total_refs {
                            tracing::warn!(
                                skill = %skill.name,
                                old_quality = %old_quality,
                                new_quality = %new_quality,
                                "Rejecting evidence enhancement that would decrease quality"
                            );
                        }
                    }
                }
                Ok(StrategyResult::default())
            }
            Err(e) => {
                tracing::warn!(skill = skill.name, error = %e, "Section evidence enhancement failed");
                Ok(StrategyResult::default())
            }
        }
    }

    async fn refine_agent(
        &self,
        agent: &mut Agent,
        context: &StrategyContext<'_>,
    ) -> Result<StrategyResult> {
        // Analyze at section level
        let sections = self.analyze_sections(&agent.prompt, context);
        let sections_needing_evidence: Vec<&SectionAnalysis> = sections
            .iter()
            .filter(|s| s.ref_count < self.min_refs_per_section && !s.content.trim().is_empty())
            .collect();

        if sections_needing_evidence.is_empty() {
            return Ok(StrategyResult {
                success: true,
                quality_delta: 0.0,
                changes_made: vec!["All sections have sufficient evidence".to_string()],
            });
        }

        tracing::debug!(
            agent = agent.name,
            sections_needing_evidence = sections_needing_evidence.len(),
            total_sections = sections.len(),
            "Section-level evidence analysis"
        );

        let prompt =
            self.build_section_evidence_prompt("agent", &agent.name, &sections_needing_evidence, context);

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "enhanced_sections": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["enhanced_sections"]
        });

        let old_total_refs = self.count_valid_references(&agent.prompt, context);

        match self.provider.generate(&prompt, &schema).await {
            Ok(response) => {
                if let Some(enhanced) = response
                    .content
                    .get("enhanced_sections")
                    .and_then(|v| v.as_array())
                {
                    let enhanced_strings: Vec<String> = enhanced
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();

                    if !enhanced_strings.is_empty() {
                        let mut new_prompt = agent.prompt.clone();
                        for (section, enhanced_content) in
                            sections_needing_evidence.iter().zip(enhanced_strings.iter())
                        {
                            new_prompt = new_prompt.replace(&section.content, enhanced_content);
                        }

                        let new_refs = self.count_valid_references(&new_prompt, context);

                        // CRITICAL: Check overall quality, not just reference count
                        let old_quality = calculate_validated_quality(&agent.prompt, context.file_registry);
                        let new_quality = calculate_validated_quality(&new_prompt, context.file_registry);

                        if new_refs > old_total_refs && new_quality >= old_quality {
                            agent.prompt = new_prompt;
                            return Ok(StrategyResult {
                                success: true,
                                quality_delta: new_quality - old_quality,
                                changes_made: vec![format!(
                                    "Added {} references to {} sections in agent '{}' (total: {}, quality: {:.0}% -> {:.0}%)",
                                    new_refs - old_total_refs,
                                    sections_needing_evidence.len(),
                                    agent.name,
                                    new_refs,
                                    old_quality * 100.0,
                                    new_quality * 100.0
                                )],
                            });
                        } else if new_refs > old_total_refs {
                            tracing::warn!(
                                agent = %agent.name,
                                old_quality = %old_quality,
                                new_quality = %new_quality,
                                "Rejecting evidence enhancement that would decrease quality"
                            );
                        }
                    }
                }
                Ok(StrategyResult::default())
            }
            Err(e) => {
                tracing::warn!(agent = agent.name, error = %e, "Section evidence enhancement failed");
                Ok(StrategyResult::default())
            }
        }
    }
}
