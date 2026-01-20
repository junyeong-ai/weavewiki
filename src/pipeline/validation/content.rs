//! Content Completeness Validation
//!
//! Validates generated content for completeness, truncation, and quality.
//! Implements mandatory evidence requirements - content without file references
//! is rejected to ensure actionable, project-specific guidance.
//!
//! Uses quality config defaults.

use regex::Regex;
use std::sync::LazyLock;

/// Minimum required file references for different content types
/// Content without evidence is considered low-value (Tier 1)
/// These are now configurable via QualityConfig but kept as fallback defaults.
pub mod evidence_requirements {
    use crate::config::QualityConfig;

    /// Get skill minimum file refs from config
    pub fn skill_min_file_refs() -> usize {
        QualityConfig::default().skill.min_file_refs
    }

    /// Get agent minimum file refs from config
    pub fn agent_min_file_refs() -> usize {
        QualityConfig::default().agent.min_file_refs
    }

    /// Get memory minimum file refs from config
    pub fn memory_min_file_refs() -> usize {
        QualityConfig::default().memory.min_file_refs
    }

    /// Get rule minimum file refs from config
    pub fn rule_min_file_refs() -> usize {
        QualityConfig::default().rule.min_file_refs
    }

    pub const SKILL_MIN_FILE_REFS: usize = 1;
    pub const AGENT_MIN_FILE_REFS: usize = 1;
    pub const MEMORY_MIN_FILE_REFS: usize = 0;
    pub const RULE_MIN_FILE_REFS: usize = 0;
}

pub mod thresholds {
    use crate::config::QualityConfig;
    use std::sync::LazyLock;

    static CACHED: LazyLock<ThresholdValues> = LazyLock::new(|| {
        let config = QualityConfig::default();
        ThresholdValues {
            skill_min_chars: config.skill.min_chars,
            skill_min_steps: config.skill.min_steps,
            skill_target_file_refs: config.skill.target_file_refs,
            skill_target_tool_refs: 5,
            agent_min_chars: config.agent.min_chars,
            agent_min_sections: config.agent.min_sections,
            agent_target_file_refs: config.agent.target_file_refs,
            agent_target_tool_refs: 3,
            memory_min_chars: config.memory.min_chars,
            memory_min_sections: config.memory.min_sections,
            memory_target_file_refs: config.memory.target_file_refs,
            quality_threshold: config.minimum_quality,
        }
    });

    pub fn get() -> &'static ThresholdValues {
        &CACHED
    }

    pub struct ThresholdValues {
        pub skill_min_chars: usize,
        pub skill_min_steps: usize,
        pub skill_target_file_refs: usize,
        pub skill_target_tool_refs: usize,
        pub agent_min_chars: usize,
        pub agent_min_sections: usize,
        pub agent_target_file_refs: usize,
        pub agent_target_tool_refs: usize,
        pub memory_min_chars: usize,
        pub memory_min_sections: usize,
        pub memory_target_file_refs: usize,
        pub quality_threshold: f32,
    }
}

static STEP_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(\d+)\.\s+\*?\*?").unwrap());
static SECTION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#{1,3}\s+").unwrap());
// Pattern for ALL CAPS sections like "KEY CONTEXT:", "CORE RESPONSIBILITIES:"
static CAPS_SECTION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][A-Z\s]{4,}:$").unwrap());
static FILE_REF_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`[a-zA-Z0-9_/\-\.]+\.(rs|ts|py|go|js|md)`").unwrap());
static LINE_REF_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\d+`?").unwrap());
static TOOL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(Read|Write|Edit|Grep|Glob|Bash|Task|WebFetch|WebSearch)\b").unwrap()
});

#[derive(Debug, Clone, Default)]
pub struct ContentAssessment {
    pub char_count: usize,
    pub step_count: usize,
    pub section_count: usize,
    pub file_ref_count: usize,
    pub line_ref_count: usize,
    pub tool_ref_count: usize,
    pub is_truncated: bool,
    pub quality_score: f32,
}

impl ContentAssessment {
    /// Check if skill has required evidence (file references)
    /// Skills without evidence are considered Tier 1 (low-value)
    pub fn has_required_skill_evidence(&self) -> bool {
        self.file_ref_count >= evidence_requirements::SKILL_MIN_FILE_REFS
    }

    /// Check if agent has required evidence
    pub fn has_required_agent_evidence(&self) -> bool {
        self.file_ref_count >= evidence_requirements::AGENT_MIN_FILE_REFS
    }

    /// Check if memory content has recommended evidence
    /// Note: Memory has a soft requirement (0 by default), so this checks
    /// if evidence exists beyond the minimum requirement.
    pub fn has_recommended_memory_evidence(&self) -> bool {
        let min_refs = evidence_requirements::MEMORY_MIN_FILE_REFS;
        // For soft requirements (0), return true. For non-zero, check against threshold.
        min_refs == 0 || self.file_ref_count >= min_refs
    }

    /// Returns a list of missing evidence requirements
    pub fn get_missing_evidence(&self, content_type: ContentType) -> Vec<EvidenceIssue> {
        let mut issues = Vec::new();

        let required_refs = match content_type {
            ContentType::Skill => evidence_requirements::SKILL_MIN_FILE_REFS,
            ContentType::Agent => evidence_requirements::AGENT_MIN_FILE_REFS,
            ContentType::Memory => evidence_requirements::MEMORY_MIN_FILE_REFS,
            ContentType::Rule => evidence_requirements::RULE_MIN_FILE_REFS,
        };

        if self.file_ref_count < required_refs {
            issues.push(EvidenceIssue::InsufficientFileRefs {
                found: self.file_ref_count,
                required: required_refs,
            });
        }

        // Skills should have line references for specific evidence
        if content_type == ContentType::Skill && self.line_ref_count == 0 && self.file_ref_count > 0 {
            issues.push(EvidenceIssue::MissingLineReferences);
        }

        issues
    }

    pub fn is_complete_skill(&self) -> bool {
        let t = thresholds::get();
        let has_structure = self.step_count >= t.skill_min_steps
            || self.section_count >= 3;
        self.char_count >= t.skill_min_chars
            && has_structure
            && !self.is_truncated
            && self.has_required_skill_evidence()
    }

    pub fn is_complete_agent(&self) -> bool {
        let t = thresholds::get();
        self.char_count >= t.agent_min_chars
            && self.section_count >= t.agent_min_sections
            && !self.is_truncated
            && self.has_required_agent_evidence()
    }

    pub fn is_complete_memory(&self) -> bool {
        let t = thresholds::get();
        self.char_count >= t.memory_min_chars
            && self.section_count >= t.memory_min_sections
            && !self.is_truncated
            // Memory has soft requirement for evidence
    }

    pub fn is_acceptable(&self) -> bool {
        self.quality_score >= thresholds::get().quality_threshold && !self.is_truncated
    }

    /// Check if content meets all evidence requirements for its type
    pub fn meets_evidence_requirements(&self, content_type: ContentType) -> bool {
        self.get_missing_evidence(content_type).is_empty()
    }
}

/// Types of content for evidence validation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Skill,
    Agent,
    Memory,
    Rule,
}

/// Issues found during evidence validation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceIssue {
    /// Not enough file references
    InsufficientFileRefs { found: usize, required: usize },
    /// Has file refs but no line numbers (less specific evidence)
    MissingLineReferences,
    /// File references don't exist in the project
    InvalidFileReferences(Vec<String>),
}

/// Assess skill body content for completeness and quality
pub fn assess_skill_content(body: &str) -> ContentAssessment {
    let mut assessment = ContentAssessment {
        char_count: body.len(),
        step_count: count_steps(body),
        section_count: count_sections(body),
        file_ref_count: count_file_refs(body),
        line_ref_count: count_line_refs(body),
        tool_ref_count: count_tool_refs(body),
        is_truncated: is_truncated(body),
        quality_score: 0.0,
    };

    assessment.quality_score = calculate_skill_quality(&assessment);
    assessment
}

/// Assess agent prompt content for completeness and quality
pub fn assess_agent_content(prompt: &str) -> ContentAssessment {
    let mut assessment = ContentAssessment {
        char_count: prompt.len(),
        step_count: count_steps(prompt),
        section_count: count_sections(prompt),
        file_ref_count: count_file_refs(prompt),
        line_ref_count: count_line_refs(prompt),
        tool_ref_count: count_tool_refs(prompt),
        is_truncated: is_truncated(prompt),
        quality_score: 0.0,
    };

    assessment.quality_score = calculate_agent_quality(&assessment);
    assessment
}

/// Assess project memory (CLAUDE.md) content for completeness and quality
pub fn assess_memory_content(content: &str) -> ContentAssessment {
    let mut assessment = ContentAssessment {
        char_count: content.len(),
        step_count: count_steps(content),
        section_count: count_sections(content),
        file_ref_count: count_file_refs(content),
        line_ref_count: count_line_refs(content),
        tool_ref_count: count_tool_refs(content),
        is_truncated: is_truncated(content),
        quality_score: 0.0,
    };

    assessment.quality_score = calculate_memory_quality(&assessment);
    assessment
}

/// Detect if memory content contains raw JSON (extraction bug)
pub fn contains_raw_json(text: &str) -> bool {
    text.contains(r#""type":"#) || text.contains(r#""content":"#) || text.contains(r#""value":"#)
}

/// Detect if content contains absolute paths
pub fn contains_absolute_paths(text: &str) -> bool {
    text.contains("/Users/") || text.contains("/home/") || text.contains("C:\\Users\\")
}

/// Detect if content was truncated mid-generation
pub fn is_truncated(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }

    // Check for unclosed code blocks
    if !trimmed.matches("```").count().is_multiple_of(2) {
        return true;
    }

    // Check last line for incomplete patterns
    if let Some(last_line) = trimmed.lines().last() {
        let last = last_line.trim();

        // Incomplete numbered step (just number with period, e.g., "2." or "2. ")
        if last.len() <= 5 {
            let without_space = last.trim_end();
            if without_space
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .count()
                > 0
                && without_space.ends_with('.')
            {
                return true;
            }
        }

        // Incomplete numbered step with partial content
        if STEP_PATTERN.is_match(last) && last.len() < 10 {
            return true;
        }

        // Ends with opening markers
        if last.ends_with(':') && last.len() < 20 {
            return true;
        }

        // Ends mid-word (incomplete word, not ending with alphanumeric)
        // Valid endings: punctuation, complete words, code references
        let last_char = last.chars().last().unwrap_or(' ');
        if last.len() > 50
            && !last_char.is_alphanumeric()
            && !last_char.is_ascii_punctuation()
            && last_char != '`'
            && last_char != ')'
        {
            return true;
        }
    }

    false
}

fn count_steps(text: &str) -> usize {
    text.lines().filter(|l| STEP_PATTERN.is_match(l)).count()
}

fn count_sections(text: &str) -> usize {
    text.lines()
        .filter(|l| SECTION_PATTERN.is_match(l) || CAPS_SECTION_PATTERN.is_match(l))
        .count()
}

fn count_file_refs(text: &str) -> usize {
    FILE_REF_PATTERN.find_iter(text).count()
}

fn count_line_refs(text: &str) -> usize {
    LINE_REF_PATTERN.find_iter(text).count()
}

fn count_tool_refs(text: &str) -> usize {
    TOOL_PATTERN.find_iter(text).count()
}

fn calculate_skill_quality(assessment: &ContentAssessment) -> f32 {
    let t = thresholds::get();
    let is_command_skill = assessment.step_count == 0 && assessment.section_count >= 3;

    let mut score = 0.0;
    let mut factors = 0.0;

    let length_score = (assessment.char_count as f32 / t.skill_min_chars as f32).min(1.0);
    score += length_score;
    factors += 1.0;

    let structure_score = if is_command_skill {
        (assessment.section_count as f32 / 3.0).min(1.0)
    } else {
        (assessment.step_count as f32 / t.skill_min_steps as f32).min(1.0)
    };
    score += structure_score;
    factors += 1.0;

    if is_command_skill {
        let tool_score = if assessment.tool_ref_count > 0 { 1.0 } else { 0.5 };
        score += tool_score;
        factors += 1.0;
    } else {
        let file_score =
            (assessment.file_ref_count as f32 / t.skill_target_file_refs as f32).min(1.0);
        score += file_score;
        factors += 1.0;

        let tool_score =
            (assessment.tool_ref_count as f32 / t.skill_target_tool_refs as f32).min(1.0);
        score += tool_score;
        factors += 1.0;
    }

    if assessment.is_truncated {
        score *= 0.3;
    }

    score / factors
}

fn calculate_agent_quality(assessment: &ContentAssessment) -> f32 {
    let t = thresholds::get();
    let mut score = 0.0;
    let mut factors = 0.0;

    let length_score = (assessment.char_count as f32 / t.agent_min_chars as f32).min(1.0);
    score += length_score;
    factors += 1.0;

    let sections_score = (assessment.section_count as f32 / t.agent_min_sections as f32).min(1.0);
    score += sections_score;
    factors += 1.0;

    let file_score = (assessment.file_ref_count as f32 / t.agent_target_file_refs as f32).min(1.0);
    score += file_score;
    factors += 1.0;

    let tool_score = (assessment.tool_ref_count as f32 / t.agent_target_tool_refs as f32).min(1.0);
    score += tool_score;
    factors += 1.0;

    if assessment.is_truncated {
        score *= 0.3;
    }

    score / factors
}

fn calculate_memory_quality(assessment: &ContentAssessment) -> f32 {
    let t = thresholds::get();
    let mut score = 0.0;
    let mut factors = 0.0;

    let length_score = (assessment.char_count as f32 / t.memory_min_chars as f32).min(1.0);
    score += length_score;
    factors += 1.0;

    let sections_score = (assessment.section_count as f32 / t.memory_min_sections as f32).min(1.0);
    score += sections_score;
    factors += 1.0;

    let file_score = (assessment.file_ref_count as f32 / t.memory_target_file_refs as f32).min(1.0);
    score += file_score;
    factors += 1.0;

    if assessment.is_truncated {
        score *= 0.3;
    }

    score / factors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_truncated_complete() {
        let complete = "## Overview\n\n1. First step with details.\n2. Second step here.";
        assert!(!is_truncated(complete));
    }

    #[test]
    fn test_is_truncated_unclosed_code_block() {
        let truncated = "## Code\n\n```rust\nfn main() {";
        assert!(is_truncated(truncated));
    }

    #[test]
    fn test_is_truncated_incomplete_step() {
        let truncated = "1. First step.\n2.";
        assert!(is_truncated(truncated));
    }

    #[test]
    fn test_count_steps() {
        let body = "1. First\n2. Second\n3. Third\nSome text\n4. Fourth";
        assert_eq!(count_steps(body), 4);
    }

    #[test]
    fn test_count_file_refs() {
        let body = "See `src/main.rs` and `src/lib.rs` for details.";
        assert_eq!(count_file_refs(body), 2);
    }

    #[test]
    fn test_assess_skill_complete() {
        let body = r#"## Overview

This skill helps with testing.

1. **First step** - Read `src/main.rs` to understand.
2. **Second step** - Use Grep to search patterns.
3. **Third step** - Edit the configuration.
4. **Fourth step** - Write new tests.
5. **Fifth step** - Run Bash commands to verify.
6. **Sixth step** - Complete the task."#;

        let assessment = assess_skill_content(body);
        assert!(assessment.step_count >= 5);
        assert!(!assessment.is_truncated);
    }

    #[test]
    fn test_assess_skill_truncated() {
        let body = "1. First step";
        let assessment = assess_skill_content(body);
        assert!(assessment.step_count < thresholds::get().skill_min_steps);
        assert!(!assessment.is_complete_skill());
    }

    #[test]
    fn test_quality_score_range() {
        let body = "Short";
        let assessment = assess_skill_content(body);
        assert!(assessment.quality_score >= 0.0 && assessment.quality_score <= 1.0);
    }

    #[test]
    fn test_is_truncated_valid_word_ending() {
        // Content ending with a complete word (no punctuation) should NOT be truncated
        let complete = "   - Verify the feature integrates properly with the pipeline flow";
        assert!(!is_truncated(complete));
    }

    #[test]
    fn test_is_truncated_code_ref_ending() {
        // Content ending with a code reference should NOT be truncated
        let complete = "Review the implementation in `src/main.rs`";
        assert!(!is_truncated(complete));
    }

    #[test]
    fn test_is_truncated_parenthesis_ending() {
        // Content ending with closing parenthesis should NOT be truncated
        let complete = "This is a long line with a reference to a function call (see docs)";
        assert!(!is_truncated(complete));
    }

    #[test]
    fn test_evidence_requirements_skill_without_refs() {
        let body = "## Overview\n\n1. Do this thing.\n2. Do that thing.";
        let assessment = assess_skill_content(body);

        // Skill without file references should fail evidence requirements
        assert!(!assessment.has_required_skill_evidence());
        let issues = assessment.get_missing_evidence(ContentType::Skill);
        assert!(!issues.is_empty());
        assert!(matches!(
            issues[0],
            EvidenceIssue::InsufficientFileRefs { found: 0, .. }
        ));
    }

    #[test]
    fn test_evidence_requirements_skill_with_refs() {
        let body = "## Overview\n\n1. Check `src/main.rs:10` for the entry point.\n2. Review `src/lib.rs`.";
        let assessment = assess_skill_content(body);

        // Skill with file references should pass
        assert!(assessment.has_required_skill_evidence());
        assert!(assessment.file_ref_count >= 1);
    }

    #[test]
    fn test_evidence_requirements_missing_line_refs() {
        let body = "## Steps\n\n1. Look at `src/main.rs` for details.\n2. Check the config.";
        let assessment = assess_skill_content(body);

        // Has file refs but no line numbers - should generate warning
        assert!(assessment.file_ref_count > 0);
        let issues = assessment.get_missing_evidence(ContentType::Skill);

        // Should warn about missing line references
        assert!(issues.iter().any(|i| matches!(i, EvidenceIssue::MissingLineReferences)));
    }

    #[test]
    fn test_meets_evidence_requirements() {
        // Content with proper evidence
        let good = "See `src/main.rs:42` for the implementation.";
        let good_assessment = assess_skill_content(good);
        // Note: meets_evidence_requirements checks file refs, not line refs for minimum
        assert!(good_assessment.meets_evidence_requirements(ContentType::Memory)); // Memory has 0 requirement
    }
}
