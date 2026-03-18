//! Progressive Disclosure for Skills
//!
//! Value-based splitting of skills into:
//! - SKILL.md: High-value content (Tier3 insights, critical constraints, core process)
//! - patterns.md: Detailed patterns (if section is substantial)
//! - examples.md: Code examples (if section is substantial)
//!
//! IMPORTANT: Progressive disclosure is applied ONLY when it adds value:
//! - Large reference/example sections that would distract from core instructions
//! - NOT for arbitrary line limits
//! - High-quality, high-value content is NEVER removed or truncated
//!
//! Value Tiers:
//! - Tier3 (ALWAYS in main): Critical Insights, Critical Constraints, Overview, Process
//! - Tier2 (keep if space): Patterns, Constraints, Domain Context
//! - Tier1 (extract if large): Examples, Key Files, References, Implementation Details
//!
//! Based on Claude Code official documentation:
//! > "Skills can include multiple files in their directory. This keeps SKILL.md
//! > focused on the essentials while letting Claude access detailed reference
//! > material only when needed."
//! > Tip: Keep SKILL.md under 500 lines. (NOT a hard requirement)
//!
//! See: <https://code.claude.com/docs/en/skills>

use crate::config::{CrossReferenceConfig, DisclosureConfig, DynamicContextConfig};
use crate::types::module_map::TechStack;
use crate::types::skill::{Skill, SkillFile};

/// Section value tier for prioritization
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ValueTier {
    /// Tier3: Highest value - ALWAYS stays in main body
    Critical = 3,
    /// Tier2: High value - stays in main body if space permits
    High = 2,
    /// Tier1: Lower value - can be extracted to supporting files
    Normal = 1,
}

fn section_value_tier(header: &str, config: &DisclosureConfig) -> ValueTier {
    let header_lower = header.to_lowercase();

    if config
        .critical_keywords
        .iter()
        .any(|kw: &String| header_lower.contains(kw.as_str()))
    {
        return ValueTier::Critical;
    }

    if config
        .high_keywords
        .iter()
        .any(|kw: &String| header_lower.contains(kw.as_str()))
    {
        return ValueTier::High;
    }

    ValueTier::Normal
}

pub struct ProgressiveDisclosure;

impl ProgressiveDisclosure {
    /// Apply progressive disclosure to a skill ONLY when it adds value.
    ///
    /// This is NOT about enforcing line limits. It's about organizing content
    /// so that core instructions are immediately accessible while detailed
    /// reference material is available on-demand.
    ///
    /// Progressive disclosure is applied when:
    /// 1. The skill exceeds the consideration threshold (500 lines)
    /// 2. There are distinct, substantial sections (patterns, examples) that
    ///    would benefit from separation
    /// 3. Extracting these sections improves navigation without losing value
    ///
    /// High-quality, high-value content is NEVER removed or truncated.
    pub fn apply(skill: Skill) -> Skill {
        Self::apply_with_config(skill, &DisclosureConfig::default())
    }

    pub fn apply_with_config(skill: Skill, config: &DisclosureConfig) -> Skill {
        let lines: Vec<&str> = skill.body.lines().collect();

        if lines.len() <= config.consideration_threshold {
            return skill;
        }

        let (main_sections, reference_sections, examples_sections) =
            Self::categorize_sections(&lines, config);

        let reference_lines: usize = reference_sections.iter().map(|s| s.lines().count()).sum();
        let examples_lines: usize = examples_sections.iter().map(|s| s.lines().count()).sum();

        let has_substantial_reference = reference_lines >= config.min_section_size;
        let has_substantial_examples = examples_lines >= config.min_section_size;

        // If no substantial sections to extract, keep everything in main SKILL.md
        if !has_substantial_reference && !has_substantial_examples {
            return skill;
        }

        let main_body = Self::build_main_body(
            &main_sections,
            has_substantial_reference,
            has_substantial_examples,
        );

        let mut additional_files = skill.additional_files.clone();

        if has_substantial_reference {
            additional_files.push(SkillFile {
                name: "patterns.md".into(),
                content: format!(
                    "# {} Patterns\n\nDetailed patterns and constraints for this skill.\n\n{}",
                    skill.name,
                    reference_sections.join("\n\n")
                ),
            });
        }

        if has_substantial_examples {
            additional_files.push(SkillFile {
                name: "examples.md".into(),
                content: format!(
                    "# {} Examples\n\nCode examples and samples.\n\n{}",
                    skill.name,
                    examples_sections.join("\n\n")
                ),
            });
        }

        Skill {
            body: main_body,
            additional_files,
            ..skill
        }
    }

    fn categorize_sections(
        lines: &[&str],
        config: &DisclosureConfig,
    ) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut main_sections = Vec::new();
        let mut reference_sections = Vec::new();
        let mut examples_sections = Vec::new();

        let mut current_section = String::new();
        let mut current_header = String::new();

        for line in lines {
            if line.starts_with("## ") {
                if !current_section.is_empty() {
                    Self::place_section(
                        &current_header,
                        current_section.clone(),
                        &mut main_sections,
                        &mut reference_sections,
                        &mut examples_sections,
                        config,
                    );
                }
                current_header = line.to_string();
                current_section = String::new();
            }
            current_section.push_str(line);
            current_section.push('\n');
        }

        if !current_section.is_empty() {
            Self::place_section(
                &current_header,
                current_section,
                &mut main_sections,
                &mut reference_sections,
                &mut examples_sections,
                config,
            );
        }

        (main_sections, reference_sections, examples_sections)
    }

    fn place_section(
        header: &str,
        content: String,
        main: &mut Vec<String>,
        reference: &mut Vec<String>,
        examples: &mut Vec<String>,
        config: &DisclosureConfig,
    ) {
        let header_lower = header.to_lowercase();
        let tier = section_value_tier(header, config);

        if tier >= ValueTier::High {
            main.push(content);
            return;
        }

        // Primary: keyword-based classification
        if config.example_keywords.iter().any(|kw| header_lower.contains(kw.as_str())) {
            examples.push(content);
            return;
        }
        if config.reference_keywords.iter().any(|kw| header_lower.contains(kw.as_str())) {
            reference.push(content);
            return;
        }

        // Secondary: language-neutral content signals
        let lines: Vec<&str> = content.lines().collect();
        let line_count = lines.len();
        let code_block_count = lines.iter().filter(|l| l.starts_with("```")).count() / 2;
        let file_ref_count = lines.iter().filter(|l| l.contains('@') && l.contains(':')).count();

        // Dense code blocks in long sections → reference material
        if line_count >= 100 && code_block_count >= 3 {
            reference.push(content);
            return;
        }

        // Dense @file:line references → reference material
        if file_ref_count >= 3 {
            reference.push(content);
            return;
        }

        main.push(content);
    }

    fn build_main_body(
        main_sections: &[String],
        has_patterns: bool,
        has_examples: bool,
    ) -> String {
        let mut body = main_sections.join("\n\n");

        if has_patterns || has_examples {
            body.push_str("\n\n## Resources\n\n");
            if has_patterns {
                body.push_str("For detailed patterns, see [patterns.md](patterns.md)\n");
            }
            if has_examples {
                body.push_str("For examples, see [examples.md](examples.md)\n");
            }
        }

        body
    }
}

/// Appends rule cross-references to a skill body.
///
/// Adds `@.claude/rules/` references so skills link to relevant rules.
/// This enables Claude Code to load related rules when a skill is invoked.
///
/// Uses path-based matching: extracts skill evidence files, then matches against rule path globs.
pub struct RuleCrossReferencer;

impl RuleCrossReferencer {
    /// Match skills to rules using evidence file paths and rule globs.
    ///
    /// For each skill, extracts `@file:line` evidence references from the body,
    /// then checks which rules' `paths` globs match those evidence files.
    pub fn apply_with_rules(
        skills: Vec<Skill>,
        rules: &[crate::types::Rule],
        config: &CrossReferenceConfig,
    ) -> Vec<Skill> {
        // Pre-compile rule globs for efficiency
        let compiled_rules: Vec<_> = rules
            .iter()
            .filter_map(|rule| {
                let paths = rule.paths.as_ref()?;
                let patterns: Vec<glob::Pattern> = paths
                    .iter()
                    .filter_map(|p| glob::Pattern::new(p).ok())
                    .collect();
                if patterns.is_empty() {
                    return None;
                }
                Some((rule, patterns))
            })
            .collect();

        skills
            .into_iter()
            .map(|skill| Self::match_skill_to_rules(skill, &compiled_rules, config))
            .collect()
    }

    fn match_skill_to_rules(
        skill: Skill,
        compiled_rules: &[(&crate::types::Rule, Vec<glob::Pattern>)],
        config: &CrossReferenceConfig,
    ) -> Skill {
        // Strip any existing "## Related Rules" section (idempotent)
        let body = Self::strip_related_rules(&skill.body);
        let skill = Skill { body, ..skill };

        let evidence_files = Self::extract_evidence_files(&skill.body);

        // Start with default refs
        let mut refs: Vec<String> = config.default_refs.clone();
        let mut seen: std::collections::HashSet<String> = refs.iter().cloned().collect();

        // Config-based matching (name patterns → rule paths)
        let name_lower = skill.name.to_lowercase();
        for mapping in &config.skill_rule_mappings {
            if name_lower.contains(&mapping.skill_pattern) {
                for path in &mapping.rule_paths {
                    if seen.insert(path.clone()) {
                        refs.push(path.clone());
                    }
                }
            }
        }

        // Path-based matching (evidence files → rule globs)
        for (rule, patterns) in compiled_rules {
            let rule_ref = format!(".claude/rules/{}", rule.output_path());
            if seen.contains(&rule_ref) {
                continue;
            }
            let matches = evidence_files
                .iter()
                .any(|file| patterns.iter().any(|p| p.matches(file)));
            if matches {
                seen.insert(rule_ref.clone());
                refs.push(rule_ref);
            }
        }

        if refs.is_empty() {
            return skill;
        }

        let mut body = skill.body.clone();
        body.push_str("\n\n## Related Rules\n\n");
        for rule_ref in &refs {
            body.push_str(&format!("- @{}\n", rule_ref));
        }

        Skill { body, ..skill }
    }

    /// Extract file paths from `@file:line` evidence references in skill body.
    fn extract_evidence_files(body: &str) -> Vec<String> {
        let mut files = Vec::new();
        for word in body.split_whitespace() {
            let candidate = word.trim_start_matches('@').trim_end_matches([',', '.', ')']);
            if let Some(colon_pos) = candidate.rfind(':') {
                let after_colon = &candidate[colon_pos + 1..];
                // Validate the part after last colon is a line number (digits only)
                if after_colon.is_empty() || !after_colon.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let file = &candidate[..colon_pos];
                if file.contains('/') && !file.is_empty() {
                    files.push(file.to_string());
                }
            }
        }
        files
    }

    /// Remove existing "## Related Rules" section from body (for idempotent re-application).
    fn strip_related_rules(body: &str) -> String {
        if let Some(pos) = body.find("\n\n## Related Rules\n") {
            body[..pos].to_string()
        } else {
            body.to_string()
        }
    }

}

/// Annotates skills with cross-references to related skills and recommended agents.
///
/// When a skill shares tools or keyword patterns with other skills, a "Related Skills"
/// section is appended. A "Recommended Agent" annotation is added based on skill scope.
pub struct SkillCrossReferencer;

impl SkillCrossReferencer {
    /// Annotate all skills with cross-references to each other and recommended agents.
    pub fn annotate_all(skills: Vec<Skill>) -> Vec<Skill> {
        let names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
        let tools: Vec<Option<Vec<String>>> = skills
            .iter()
            .map(|s| s.allowed_tools.clone())
            .collect();

        skills
            .into_iter()
            .enumerate()
            .map(|(i, skill)| {
                let related = Self::find_related(&skill, i, &names, &tools);
                let agent = Self::recommend_agent(&skill.name);
                Self::annotate(skill, &related, agent)
            })
            .collect()
    }

    fn find_related(
        skill: &Skill,
        idx: usize,
        names: &[String],
        tools: &[Option<Vec<String>>],
    ) -> Vec<String> {
        let my_tools = skill.allowed_tools.as_deref().unwrap_or(&[]);
        let my_keywords = Self::extract_keywords(&skill.name);

        let mut related = Vec::new();
        for (j, name) in names.iter().enumerate() {
            if j == idx {
                continue;
            }
            let other_tools = tools[j].as_deref().unwrap_or(&[]);
            let other_keywords = Self::extract_keywords(name);

            let shared_tools = my_tools.iter().filter(|t| other_tools.contains(t)).count();
            let shared_keywords = my_keywords.iter().any(|k| other_keywords.contains(k));

            if shared_tools >= 2 || shared_keywords {
                related.push(name.clone());
            }
        }
        related
    }

    fn extract_keywords(name: &str) -> Vec<String> {
        name.split('-').map(|s| s.to_lowercase()).collect()
    }

    fn recommend_agent(skill_name: &str) -> &'static str {
        let name_lower = skill_name.to_lowercase();
        if name_lower.contains("review") || name_lower.contains("audit") || name_lower.contains("lint") {
            "reviewer"
        } else if name_lower.contains("plan") || name_lower.contains("design") || name_lower.contains("architect") {
            "architect"
        } else {
            "coder"
        }
    }

    fn annotate(skill: Skill, related: &[String], agent: &str) -> Skill {
        if related.is_empty() && agent.is_empty() {
            return skill;
        }

        let mut body = skill.body.clone();

        body.push_str("\n\n## Recommended Agent\n\n");
        body.push_str(&format!("Best suited for: **{}**\n", agent));

        if !related.is_empty() {
            body.push_str("\n## Related Skills\n\n");
            for name in related {
                body.push_str(&format!("- /{}\n", name));
            }
        }

        Skill { body, ..skill }
    }
}

/// Injects dynamic context commands into skill bodies based on project evidence.
///
/// Claude Code's skill preprocessor executes `!command` lines when a skill is loaded,
/// replacing them with the command output. This enables runtime-aware skills.
///
/// Example: A git-based skill gets `!git status --short` injected so it always
/// has current working tree state when invoked.
pub struct DynamicContextInjector;

impl DynamicContextInjector {
    /// Inject `!command` directives into a skill's body based on skill name patterns
    /// and the project's tech stack.
    pub fn inject(skill: Skill, tech_stack: &TechStack) -> Skill {
        Self::inject_with_config(skill, tech_stack, &DynamicContextConfig::default())
    }

    pub fn inject_with_config(skill: Skill, tech_stack: &TechStack, config: &DynamicContextConfig) -> Skill {
        let commands = Self::commands_for(&skill.name, tech_stack, config);
        if commands.is_empty() {
            return skill;
        }

        let section = Self::build_section(&commands);
        let body = format!("{}\n{}", skill.body, section);

        Skill { body, ..skill }
    }

    /// Determine which `!command` directives to inject based on config-driven patterns.
    fn commands_for(name: &str, tech_stack: &TechStack, config: &DynamicContextConfig) -> Vec<String> {
        let name_lower = name.to_lowercase();
        let lang_lower = tech_stack.primary_language.to_lowercase();
        let mut commands = Vec::new();

        for pattern in &config.command_patterns {
            let keyword_match = pattern.skill_keywords.iter().any(|kw| name_lower.contains(kw.as_str()));
            if !keyword_match {
                continue;
            }

            if let Some(ref lang_filter) = pattern.language_filter
                && lang_filter.to_lowercase() != lang_lower
            {
                continue;
            }

            for cmd in &pattern.commands {
                commands.push(cmd.clone());
            }
        }

        commands
    }

    /// Build the `## Dynamic Context` markdown section from a list of commands.
    fn build_section(commands: &[String]) -> String {
        let mut section = String::from("\n## Dynamic Context\n\n");
        section.push_str("<!-- Runtime state injected when skill is invoked -->\n");
        for cmd in commands {
            section.push_str(cmd);
            section.push('\n');
        }
        section
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_with_lines(count: usize) -> Skill {
        let lines: Vec<String> = (0..count).map(|i| format!("Line {}", i)).collect();
        Skill::new("test-skill", "Test description", lines.join("\n"))
    }

    #[test]
    fn test_small_skill_unchanged() {
        let skill = skill_with_lines(100);
        let result = ProgressiveDisclosure::apply(skill.clone());

        assert_eq!(result.body, skill.body);
        assert!(result.additional_files.is_empty());
    }

    #[test]
    fn test_large_skill_with_substantial_sections_split() {
        // Create substantial sections (>50 lines each) to trigger extraction
        let examples_content = (0..60).map(|i| format!("Example line {}", i)).collect::<Vec<_>>().join("\n");
        let details_content = (0..60).map(|i| format!("Detail line {}", i)).collect::<Vec<_>>().join("\n");

        let body = format!(
            r#"# Large Skill

## Overview

Essential overview content.

## Critical Constraints

Must follow these rules.

## Examples

{}

## Implementation Details

{}

## Extra Content

{}"#,
            examples_content,
            details_content,
            "Extra line\n".repeat(400)
        );

        let skill = Skill::new("large-skill", "Large skill", &body);
        let result = ProgressiveDisclosure::apply(skill);

        // Should have additional files because sections are substantial
        assert!(!result.additional_files.is_empty());

        // Check for examples.md (substantial examples section)
        let examples_file = result
            .additional_files
            .iter()
            .find(|f| f.name == "examples.md");
        assert!(examples_file.is_some(), "Should extract substantial examples section");

        // Check for patterns.md (substantial implementation details section)
        let patterns_file = result
            .additional_files
            .iter()
            .find(|f| f.name == "patterns.md");
        assert!(patterns_file.is_some(), "Should extract substantial patterns section");
    }

    #[test]
    fn test_large_skill_without_substantial_sections_unchanged() {
        // Large skill but without substantial extractable sections
        // Note: Each section must be followed by another ## header to limit its size
        let body = format!(
            "# Skill\n\n## Overview\n\nCore content.\n\n## Examples\n\nSmall example.\n\n## Main Content\n\n{}",
            "Core line\n".repeat(500)  // Just core content in main section
        );

        let skill = Skill::new("skill", "Skill", &body);
        let result = ProgressiveDisclosure::apply(skill);

        // Small examples section (<50 lines) should NOT be extracted
        // Keep everything in main SKILL.md for high-value content preservation
        assert!(
            result.additional_files.is_empty(),
            "Should not extract small sections - preserve high-value content"
        );
    }

    #[test]
    fn test_main_body_has_resource_links_when_extracted() {
        // Create substantial sections to trigger extraction
        let examples_content = (0..60).map(|i| format!("Example line {}", i)).collect::<Vec<_>>().join("\n");

        let body = format!(
            "# Skill\n\n## Overview\n\nCore content.\n\n## Examples\n\n{}\n\n{}",
            examples_content,
            "Extra line\n".repeat(450)
        );

        let skill = Skill::new("skill", "Skill", &body);
        let result = ProgressiveDisclosure::apply(skill);

        // Should have resource links when sections are extracted
        if !result.additional_files.is_empty() {
            assert!(result.body.contains("## Resources"));
            assert!(result.body.contains("[examples.md]"));
        }
    }

    #[test]
    fn test_tier3_critical_content_always_in_main_body() {
        // Create a large skill with Critical Insights (Tier3)
        let examples_content = (0..60).map(|i| format!("Example line {}", i)).collect::<Vec<_>>().join("\n");
        let critical_content = "CRITICAL: Must always wrap providers with Arc::new()\nEvidence: @src/provider.rs:42\n";

        let body = format!(
            r#"# IMPLEMENT

## Critical Insights

{}

## Overview

Short overview.

## Examples

{}

## Implementation Details

{}"#,
            critical_content,
            examples_content,
            "Detail line\n".repeat(100)
        );

        let skill = Skill::new("implement", "Implementation skill", &body);
        let result = ProgressiveDisclosure::apply(skill);

        // Critical Insights MUST remain in main body (Tier3)
        assert!(
            result.body.contains("Critical Insights"),
            "Tier3 Critical Insights must stay in main body"
        );
        assert!(
            result.body.contains("CRITICAL: Must always wrap"),
            "Tier3 content must stay in main body"
        );

        // Overview MUST remain in main body (Tier3)
        assert!(
            result.body.contains("Overview"),
            "Tier3 Overview must stay in main body"
        );
    }

    #[test]
    fn test_value_tier_classification() {
        use super::{section_value_tier, ValueTier};
        let config = DisclosureConfig::default();

        // Tier3 (Critical)
        assert_eq!(section_value_tier("## Critical Insights", &config), ValueTier::Critical);
        assert_eq!(section_value_tier("## Overview", &config), ValueTier::Critical);
        assert_eq!(section_value_tier("## Process", &config), ValueTier::Critical);
        assert_eq!(section_value_tier("## Input/Output Spec", &config), ValueTier::Critical);
        assert_eq!(section_value_tier("## Expected Output", &config), ValueTier::Critical);

        // Tier2 (High)
        assert_eq!(section_value_tier("## Constraints", &config), ValueTier::High);
        assert_eq!(section_value_tier("## Patterns", &config), ValueTier::High);
        assert_eq!(section_value_tier("## Domain Context", &config), ValueTier::High);
        assert_eq!(section_value_tier("## Warning", &config), ValueTier::High);
        assert_eq!(section_value_tier("## Gotcha List", &config), ValueTier::High);

        // Tier1 (Normal)
        assert_eq!(section_value_tier("## Examples", &config), ValueTier::Normal);
        assert_eq!(section_value_tier("## Key Files", &config), ValueTier::Normal);
        assert_eq!(section_value_tier("## Implementation Details", &config), ValueTier::Normal);
    }

    // =========================================================================
    // DynamicContextInjector tests
    // =========================================================================

    #[test]
    fn test_git_skill_gets_dynamic_context() {
        let skill = Skill::new(
            "commit-workflow",
            "Guide for committing changes",
            "# Commit Workflow\n\nSteps to commit.",
        );
        let tech_stack = TechStack::new("rust");

        let result = DynamicContextInjector::inject(skill, &tech_stack);

        assert!(
            result.body.contains("!git status --short"),
            "Git skill should have git status command"
        );
        assert!(
            result.body.contains("!git log --oneline -5"),
            "Git skill should have git log command"
        );
    }

    #[test]
    fn test_build_skill_gets_check_command() {
        let skill = Skill::new(
            "build-verify",
            "Verify the build",
            "# Build Verify\n\nCheck compilation.",
        );
        let tech_stack = TechStack::new("rust");

        let result = DynamicContextInjector::inject(skill, &tech_stack);

        assert!(
            result.body.contains("!cargo check --message-format=short"),
            "Rust build skill should have cargo check command"
        );
    }

    #[test]
    fn test_unrelated_skill_no_injection() {
        let skill = Skill::new(
            "api-design",
            "API design guidelines",
            "# API Design\n\nDesign patterns for APIs.",
        );
        let tech_stack = TechStack::new("rust");

        let result = DynamicContextInjector::inject(skill, &tech_stack);

        assert!(
            !result.body.contains("## Dynamic Context"),
            "Unrelated skill should not get dynamic context"
        );
        assert!(
            !result.body.contains("!git"),
            "Unrelated skill should not get git commands"
        );
    }

    #[test]
    fn test_dynamic_context_section_format() {
        let skill = Skill::new(
            "git-review",
            "Code review process",
            "# Git Review\n\nReview steps.",
        );
        let tech_stack = TechStack::new("typescript");

        let result = DynamicContextInjector::inject(skill, &tech_stack);

        assert!(
            result.body.contains("## Dynamic Context"),
            "Should have Dynamic Context section header"
        );
        assert!(
            result.body.contains("<!-- Runtime state injected when skill is invoked -->"),
            "Should have explanatory comment"
        );
        // Verify commands are on their own lines starting with !
        for line in result.body.lines() {
            if line.starts_with('!') {
                assert!(
                    line.starts_with("!git") || line.starts_with("!cargo") || line.starts_with("!node"),
                    "Commands should start with known prefixes, got: {}",
                    line
                );
            }
        }
    }

    // =========================================================================
    // SkillCrossReferencer tests
    // =========================================================================

    #[test]
    fn test_cross_ref_single_skill_gets_agent() {
        let skill = Skill::new("test", "Test skill", "# Test");
        let result = SkillCrossReferencer::annotate_all(vec![skill]);

        assert_eq!(result.len(), 1);
        assert!(result[0].body.contains("## Recommended Agent"));
        assert!(result[0].body.contains("coder"));
    }

    #[test]
    fn test_cross_ref_reviewer_skills() {
        let skill = Skill::new("code-review", "Review code", "# Review");
        let result = SkillCrossReferencer::annotate_all(vec![skill]);

        assert!(result[0].body.contains("reviewer"));
    }

    #[test]
    fn test_cross_ref_architect_skills() {
        let skill = Skill::new("plan", "Design plan", "# Plan");
        let result = SkillCrossReferencer::annotate_all(vec![skill]);

        assert!(result[0].body.contains("architect"));
    }

    #[test]
    fn test_cross_ref_related_skills_by_shared_tools() {
        let s1 = Skill::new("test", "Test", "# Test")
            .tools(vec!["Read".into(), "Grep".into(), "Edit".into()]);
        let s2 = Skill::new("document", "Doc", "# Doc")
            .tools(vec!["Read".into(), "Grep".into(), "Edit".into()]);
        let s3 = Skill::new("security-audit", "Audit", "# Audit")
            .tools(vec!["Read".into(), "Grep".into()]);

        let result = SkillCrossReferencer::annotate_all(vec![s1, s2, s3]);

        // test and document share 3 tools, should be related
        assert!(
            result[0].body.contains("## Related Skills"),
            "test should have related skills"
        );
        assert!(
            result[0].body.contains("/document"),
            "test should list document as related"
        );
    }

    #[test]
    fn test_cross_ref_no_related_for_unrelated_skills() {
        let s1 = Skill::new("test", "Test", "# Test")
            .tools(vec!["Read".into()]);
        let s2 = Skill::new("deploy", "Deploy", "# Deploy")
            .tools(vec!["Bash".into()]);

        let result = SkillCrossReferencer::annotate_all(vec![s1, s2]);

        // Only 1 shared tool ("Read" vs "Bash" = 0 shared), no keyword overlap
        assert!(
            !result[0].body.contains("## Related Skills"),
            "Unrelated skills should not be cross-referenced"
        );
    }

    #[test]
    fn test_cross_ref_empty_input() {
        let result = SkillCrossReferencer::annotate_all(vec![]);
        assert!(result.is_empty());
    }

    // =========================================================================
    // Language-neutral content signal tests
    // =========================================================================

    #[test]
    fn test_language_neutral_code_dense_section_extracted() {
        // Section with 100+ lines and 3+ code blocks → reference
        let mut code_section = String::from("## 코드 패턴\n\n"); // Korean header
        for i in 0..35 {
            code_section.push_str(&format!("설명 라인 {}\n", i));
            if i % 10 == 0 {
                code_section.push_str("```rust\nfn example() {}\n```\n");
            }
        }
        // Pad to 100+ lines
        for i in 35..100 {
            code_section.push_str(&format!("추가 라인 {}\n", i));
        }

        let config = DisclosureConfig::default();
        let mut main = Vec::new();
        let mut reference = Vec::new();
        let mut examples = Vec::new();

        ProgressiveDisclosure::place_section(
            "## 코드 패턴",
            code_section,
            &mut main,
            &mut reference,
            &mut examples,
            &config,
        );

        assert!(!reference.is_empty(), "Code-dense section should be extracted as reference");
        assert!(main.is_empty(), "Should not be in main");
    }

    #[test]
    fn test_language_neutral_file_ref_dense_section_extracted() {
        let section = "## 참조\n\n@src/auth.rs:42 인증 처리\n@src/db.rs:100 DB 연결\n@src/api.rs:55 API 엔드포인트\n추가 설명\n";

        let config = DisclosureConfig::default();
        let mut main = Vec::new();
        let mut reference = Vec::new();
        let mut examples = Vec::new();

        ProgressiveDisclosure::place_section(
            "## 참조",
            section.to_string(),
            &mut main,
            &mut reference,
            &mut examples,
            &config,
        );

        assert!(!reference.is_empty(), "File-ref dense section should be reference");
    }

    #[test]
    fn test_small_section_stays_in_main() {
        let section = "## 간단한 섹션\n\n짧은 내용\n";

        let config = DisclosureConfig::default();
        let mut main = Vec::new();
        let mut reference = Vec::new();
        let mut examples = Vec::new();

        ProgressiveDisclosure::place_section(
            "## 간단한 섹션",
            section.to_string(),
            &mut main,
            &mut reference,
            &mut examples,
            &config,
        );

        assert!(!main.is_empty(), "Small section should stay in main");
        assert!(reference.is_empty());
    }

    // =========================================================================
    // RuleCrossReferencer path-based matching tests
    // =========================================================================

    #[test]
    fn test_rule_cross_ref_matches_evidence_to_rule_paths() {
        use crate::types::Rule;

        let skill = Skill::new(
            "auth-flow",
            "Authentication flow",
            "# Auth Flow\n\nSee @src/auth/handler.rs:42 and @src/auth/middleware.rs:10",
        );
        let rules = vec![
            Rule::new("auth-rules", vec!["# Auth".into()])
                .paths(vec!["src/auth/**".into()])
                .priority(80),
            Rule::new("api-rules", vec!["# API".into()])
                .paths(vec!["src/api/**".into()])
                .priority(75),
        ];
        let config = CrossReferenceConfig {
            default_refs: vec![],
            skill_rule_mappings: vec![],
        };

        let result = RuleCrossReferencer::apply_with_rules(vec![skill], &rules, &config);

        assert!(result[0].body.contains("## Related Rules"));
        assert!(
            result[0].body.contains("auth-rules"),
            "Should match auth rule via evidence file path"
        );
        assert!(
            !result[0].body.contains("api-rules"),
            "Should not match api rule (no evidence in src/api/)"
        );
    }

    #[test]
    fn test_rule_cross_ref_no_match_without_evidence() {
        use crate::types::Rule;

        let skill = Skill::new(
            "general-skill",
            "General purpose",
            "# General Skill\n\nNo file references here.",
        );
        let rules = vec![Rule::new("auth-rules", vec!["# Auth".into()])
            .paths(vec!["src/auth/**".into()])
            .priority(80)];
        let config = CrossReferenceConfig {
            default_refs: vec![],
            skill_rule_mappings: vec![],
        };

        let result = RuleCrossReferencer::apply_with_rules(vec![skill], &rules, &config);

        assert!(!result[0].body.contains("## Related Rules"));
    }

    #[test]
    fn test_rule_cross_ref_idempotent() {
        use crate::types::Rule;

        let skill = Skill::new(
            "auth-flow",
            "Auth flow",
            "# Auth Flow\n\nSee @src/auth/handler.rs:42\n\n## Related Rules\n\n- @old-rule.md\n",
        );
        let rules = vec![Rule::new("auth-rules", vec!["# Auth".into()])
            .paths(vec!["src/auth/**".into()])
            .priority(80)];
        let config = CrossReferenceConfig {
            default_refs: vec![],
            skill_rule_mappings: vec![],
        };

        let result = RuleCrossReferencer::apply_with_rules(vec![skill], &rules, &config);

        // Should have exactly one "## Related Rules" section
        let count = result[0].body.matches("## Related Rules").count();
        assert_eq!(count, 1, "Should have exactly one Related Rules section");
        assert!(
            !result[0].body.contains("old-rule"),
            "Old refs should be stripped"
        );
        assert!(result[0].body.contains("auth-rules"));
    }

    #[test]
    fn test_extract_evidence_files() {
        let body = "See @src/auth/handler.rs:42 and @src/api/routes.rs:10, plus @src/db.rs:5.";
        let files = RuleCrossReferencer::extract_evidence_files(body);

        assert_eq!(files.len(), 3);
        assert!(files.contains(&"src/auth/handler.rs".to_string()));
        assert!(files.contains(&"src/api/routes.rs".to_string()));
        assert!(files.contains(&"src/db.rs".to_string()));
    }

    #[test]
    fn test_strip_related_rules() {
        let body = "# Skill\n\nContent\n\n## Related Rules\n\n- @old.md\n";
        let stripped = RuleCrossReferencer::strip_related_rules(body);
        assert_eq!(stripped, "# Skill\n\nContent");
    }
}
