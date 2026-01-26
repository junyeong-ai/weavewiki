//! Evidence Strategy
//!
//! Enhances artifacts with file references based on source insights.
//! Operates at section level: identifies sections lacking references
//! and adds evidence from both project files and original insight evidence.
//!
//! Includes feedback loop for iterative evidence improvement.

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use regex::Regex;

use crate::ai::LlmProvider;
use crate::config::EvidenceFeedbackConfig;
use crate::pipeline::file_reference;
use crate::types::{Agent, Result, Skill};

use super::{
    IssueKind, RefinementStrategy, StrategyContext, StrategyResult, calculate_validated_quality,
};

/// Result of evidence feedback loop
#[derive(Debug, Clone)]
pub enum EvidenceResult {
    /// References meet or exceed target
    Sufficient { total_refs: usize },
    /// Some references added but below target
    Partial {
        added: usize,
        total: usize,
        target: usize,
    },
    /// No references could be added
    NoImprovement { reason: String },
    /// Feedback loop disabled
    Disabled,
}

/// Feedback loop state for evidence enhancement
struct FeedbackLoopState {
    retry: usize,
    current_refs: usize,
    target_refs: usize,
}

/// Matches common section patterns:
/// - Markdown headers (# to ####)
/// - Underlined headers (===, ---)
/// - Numbered sections (1., 1.1., etc.)
/// - All-caps headers
static SECTION_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(?:#{1,4}\s+(.+)|(\d+\.(?:\d+\.)*)\s+(.+)|([A-Z][A-Z\s]{2,}):?)$")
        .expect("section header regex")
});

/// JSON schema for enhanced sections response
static ENHANCED_SECTIONS_SCHEMA: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::json!({
        "type": "object",
        "properties": {
            "enhanced_sections": {
                "type": "array",
                "items": {"type": "string"}
            }
        },
        "required": ["enhanced_sections"]
    })
});

pub struct EvidenceStrategy {
    provider: Arc<dyn LlmProvider>,
    config: EvidenceFeedbackConfig,
}

impl EvidenceStrategy {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            config: EvidenceFeedbackConfig::default(),
        }
    }

    pub fn with_feedback_config(mut self, config: EvidenceFeedbackConfig) -> Self {
        self.config = config;
        self
    }

    fn min_refs_per_section(&self) -> usize {
        self.config.min_refs_per_section
    }

    /// Evidence feedback loop: iteratively improve references until target met or max retries
    pub async fn evidence_feedback_loop(
        &self,
        skill: &mut Skill,
        context: &StrategyContext<'_>,
    ) -> Result<EvidenceResult> {
        if !self.config.enabled {
            return Ok(EvidenceResult::Disabled);
        }

        let initial_refs = self.count_valid_references(&skill.body, context);
        let target = self.config.target_refs;

        if initial_refs >= target {
            return Ok(EvidenceResult::Sufficient {
                total_refs: initial_refs,
            });
        }

        let mut current_refs = initial_refs;
        let mut retry = 0;

        while retry < self.config.max_retries && current_refs < target {
            retry += 1;

            // Analyze which sections still need evidence
            let sections = self.analyze_sections(&skill.body, context);
            let sections_needing_evidence: Vec<&SectionAnalysis> = sections
                .iter()
                .filter(|s| {
                    s.ref_count < self.min_refs_per_section() && !s.content.trim().is_empty()
                })
                .collect();

            if sections_needing_evidence.is_empty() {
                break;
            }

            // Build feedback-enhanced prompt
            let state = FeedbackLoopState {
                retry,
                current_refs,
                target_refs: target,
            };
            let feedback_prompt = self.build_feedback_prompt(
                "skill",
                &skill.name,
                &sections_needing_evidence,
                context,
                &state,
            );

            let schema = &*ENHANCED_SECTIONS_SCHEMA;

            match self.provider.generate(&feedback_prompt, schema).await {
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
                            let mut new_body = skill.body.clone();
                            for (section, enhanced_content) in sections_needing_evidence
                                .iter()
                                .zip(enhanced_strings.iter())
                            {
                                new_body = new_body.replace(&section.content, enhanced_content);
                            }

                            let new_refs = self.count_valid_references(&new_body, context);
                            let old_quality =
                                calculate_validated_quality(&skill.body, context.file_registry);
                            let new_quality =
                                calculate_validated_quality(&new_body, context.file_registry);

                            // Only accept if refs improved without quality degradation
                            if new_refs > current_refs && new_quality >= old_quality * 0.95 {
                                skill.body = new_body;
                                current_refs = new_refs;

                                tracing::debug!(
                                    skill = %skill.name,
                                    retry,
                                    refs = current_refs,
                                    target,
                                    "Evidence feedback loop: refs improved"
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        skill = %skill.name,
                        retry,
                        error = %e,
                        "Evidence feedback loop: LLM call failed"
                    );
                }
            }
        }

        let added = current_refs - initial_refs;
        if current_refs >= target {
            Ok(EvidenceResult::Sufficient {
                total_refs: current_refs,
            })
        } else if added > 0 {
            Ok(EvidenceResult::Partial {
                added,
                total: current_refs,
                target,
            })
        } else {
            Ok(EvidenceResult::NoImprovement {
                reason: format!("Could not add references after {} retries", retry),
            })
        }
    }

    /// Build prompt with feedback context for retry attempts
    fn build_feedback_prompt(
        &self,
        content_type: &str,
        name: &str,
        sections_needing_evidence: &[&SectionAnalysis],
        context: &StrategyContext<'_>,
        state: &FeedbackLoopState,
    ) -> String {
        let file_context = context.file_registry.to_prompt_context(50);
        let code_samples = context.file_registry.get_code_samples(3);
        let retry = state.retry;
        let current_refs = state.current_refs;
        let target_refs = state.target_refs;

        let sections_text = sections_needing_evidence
            .iter()
            .enumerate()
            .map(|(i, s)| format!("SECTION {}: {}\n{}", i + 1, s.header, s.content))
            .collect::<Vec<_>>()
            .join("\n---\n");

        format!(
            r##"[RETRY {retry}/{max_retries}] Add @file:line references to {content_type} "{name}".

CURRENT STATUS:
- References found: {current_refs}
- Target references: {target_refs}
- Gap: {gap} more needed

FEEDBACK FROM PREVIOUS ATTEMPT:
- Some sections still lack file references
- Ensure references point to actual code in AVAILABLE FILES
- Use specific line numbers where key code exists

AVAILABLE FILES:
{file_context}

CODE SAMPLES:
{code_samples}

SECTIONS STILL NEEDING EVIDENCE:
{sections_text}

REQUIREMENTS:
1. Add @file:line references (e.g., @src/main.rs:42)
2. Reference real files from AVAILABLE FILES list
3. Each section needs at least {min_refs} valid reference(s)
4. Preserve original content structure

Return JSON with enhanced_sections array."##,
            retry = retry,
            max_retries = self.config.max_retries,
            content_type = content_type,
            name = name,
            current_refs = current_refs,
            target_refs = target_refs,
            gap = target_refs.saturating_sub(current_refs),
            file_context = file_context,
            code_samples = code_samples,
            sections_text = sections_text,
            min_refs = self.min_refs_per_section(),
        )
    }

    fn count_valid_references(&self, content: &str, context: &StrategyContext<'_>) -> usize {
        file_reference::extract_references(content)
            .into_iter()
            .filter(|r| context.file_registry.contains(&r.path))
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

        const DEFAULT_SUGGESTIONS: &str =
            "- Add specific @file:line references from the files listed above";

        format!(
            r##"Add @file:line references to these sections of {content_type} "{name}".

QUALITY ISSUE: {issue}

{feedback_section}

AVAILABLE FILES:
{file_context}

CODE SAMPLES:
{code_samples}

SECTIONS NEEDING EVIDENCE:
{sections_text}

SUGGESTIONS:
{suggestions}

REQUIREMENTS:
1. Add @file:line references from AVAILABLE FILES (e.g., @src/main.rs:42)
2. Each section needs at least 1 valid file reference
3. Reference actual line numbers where relevant code exists
4. Preserve the original content while adding references

Return JSON with enhanced_sections array in the same order."##,
            content_type = content_type,
            name = name,
            issue = context.format_issues(),
            feedback_section = context.feedback_section(),
            file_context = file_context,
            code_samples = code_samples,
            sections_text = sections_text,
            suggestions = context.suggestions_section(DEFAULT_SUGGESTIONS),
        )
    }

    /// Single-pass evidence enhancement (used when feedback loop disabled)
    async fn refine_skill_single_pass(
        &self,
        skill: &mut Skill,
        context: &StrategyContext<'_>,
        old_total_refs: usize,
        old_quality: f32,
    ) -> Result<StrategyResult> {
        let sections = self.analyze_sections(&skill.body, context);
        let sections_needing_evidence: Vec<&SectionAnalysis> = sections
            .iter()
            .filter(|s| s.ref_count < self.min_refs_per_section() && !s.content.trim().is_empty())
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

        let prompt = self.build_section_evidence_prompt(
            "skill",
            &skill.name,
            &sections_needing_evidence,
            context,
        );

        let schema = &*ENHANCED_SECTIONS_SCHEMA;

        match self.provider.generate(&prompt, schema).await {
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
                        let mut new_body = skill.body.clone();
                        for (section, enhanced_content) in sections_needing_evidence
                            .iter()
                            .zip(enhanced_strings.iter())
                        {
                            new_body = new_body.replace(&section.content, enhanced_content);
                        }

                        let new_refs = self.count_valid_references(&new_body, context);
                        let new_quality =
                            calculate_validated_quality(&new_body, context.file_registry);

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
        let old_total_refs = self.count_valid_references(&skill.body, context);
        let old_quality = calculate_validated_quality(&skill.body, context.file_registry);

        // Use feedback loop if enabled (provides retry capability)
        if self.config.enabled {
            let result = self.evidence_feedback_loop(skill, context).await?;
            let new_quality = calculate_validated_quality(&skill.body, context.file_registry);

            return match result {
                EvidenceResult::Sufficient { total_refs } => Ok(StrategyResult {
                    success: true,
                    quality_delta: new_quality - old_quality,
                    changes_made: vec![format!(
                        "Evidence sufficient: {} refs in skill '{}' (quality: {:.0}%)",
                        total_refs,
                        skill.name,
                        new_quality * 100.0
                    )],
                }),
                EvidenceResult::Partial {
                    added,
                    total,
                    target,
                } => Ok(StrategyResult {
                    success: added > 0,
                    quality_delta: new_quality - old_quality,
                    changes_made: vec![format!(
                        "Added {} refs to skill '{}' (total: {}, target: {}, quality: {:.0}% -> {:.0}%)",
                        added,
                        skill.name,
                        total,
                        target,
                        old_quality * 100.0,
                        new_quality * 100.0
                    )],
                }),
                EvidenceResult::NoImprovement { reason } => {
                    tracing::debug!(skill = %skill.name, reason = %reason, "Evidence feedback loop: no improvement");
                    Ok(StrategyResult::default())
                }
                EvidenceResult::Disabled => {
                    // Fallback to single-pass approach
                    self.refine_skill_single_pass(skill, context, old_total_refs, old_quality)
                        .await
                }
            };
        }

        // Single-pass approach (feedback loop disabled)
        self.refine_skill_single_pass(skill, context, old_total_refs, old_quality)
            .await
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
            .filter(|s| s.ref_count < self.min_refs_per_section() && !s.content.trim().is_empty())
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

        let prompt = self.build_section_evidence_prompt(
            "agent",
            &agent.name,
            &sections_needing_evidence,
            context,
        );

        let schema = &*ENHANCED_SECTIONS_SCHEMA;

        let old_total_refs = self.count_valid_references(&agent.prompt, context);

        match self.provider.generate(&prompt, schema).await {
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
                        for (section, enhanced_content) in sections_needing_evidence
                            .iter()
                            .zip(enhanced_strings.iter())
                        {
                            new_prompt = new_prompt.replace(&section.content, enhanced_content);
                        }

                        let new_refs = self.count_valid_references(&new_prompt, context);

                        // CRITICAL: Check overall quality, not just reference count
                        let old_quality =
                            calculate_validated_quality(&agent.prompt, context.file_registry);
                        let new_quality =
                            calculate_validated_quality(&new_prompt, context.file_registry);

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
