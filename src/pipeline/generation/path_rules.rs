//! Path-Based Rules Generator
//!
//! Generates .claude/rules/ files with path-based matching for monorepo support.
//! Implements value score enforcement to prevent low-value content generation.

use crate::config::ProjectType;
use crate::pipeline::analysis::SynthesizedAnalysis;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::enrichment::EnrichedPlan;
use crate::types::{Result, Rule};

use crate::pipeline::phases::constraint_extraction::{AntiPattern, ExtractedConstraints, Gotcha};
use crate::pipeline::phases::convention_inference::InferredConventions;
use crate::pipeline::phases::monorepo_analyzer::MonorepoAnalysis;
use crate::pipeline::phases::output_router::{OutputPlan, PlannedRuleGroup, RuleContentSource};

/// Minimum value score threshold for generating rules.
///
/// NOTE: This is a default threshold, not an authoritative gate.
/// Rules with lower scores may still have value in specific contexts.
/// LLM should determine if a rule is worth including based on project needs,
/// not solely based on this programmatic score.
///
/// The score is calculated from:
/// - Pattern counts (logarithmic scaling)
/// - Constraint severity
/// - File reference count
///
/// These metrics may not capture domain-specific value (e.g., a single
/// critical security constraint may be more valuable than many patterns).
///
/// This constant is kept as a FALLBACK default. Use `GenerationConfig::min_rule_value_score`
/// for configurable threshold. Set to 0.0 to disable filtering.
const DEFAULT_MIN_RULE_VALUE_SCORE: f32 = 0.3;

pub struct PathRulesGenerator;

impl PathRulesGenerator {
    /// Generate rules with value score enforcement using default threshold.
    ///
    /// For configurable threshold, use `generate_with_threshold` instead.
    pub fn generate(
        plan: &OutputPlan,
        monorepo: Option<&MonorepoAnalysis>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
    ) -> Result<Vec<Rule>> {
        Self::generate_with_threshold(
            plan,
            monorepo,
            conventions,
            constraints,
            None,
            None,
            DEFAULT_MIN_RULE_VALUE_SCORE,
        )
    }

    /// Generate rules with configurable value score threshold.
    ///
    /// # Arguments
    /// * `min_value_score` - Minimum value score for rules (0.0 = no filtering)
    ///
    /// Rules with scores below the threshold are skipped. Set to 0.0 to
    /// disable filtering and let LLM decide all rule values.
    pub fn generate_with_threshold(
        plan: &OutputPlan,
        monorepo: Option<&MonorepoAnalysis>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        synthesis: Option<&SynthesizedAnalysis>,
        file_registry: Option<&VerifiedFileRegistry>,
        min_value_score: f32,
    ) -> Result<Vec<Rule>> {
        let mut rules = Vec::new();
        let mut skipped_low_value = 0;

        for group in &plan.rules_plan.rule_groups {
            // Assess value before generating
            let value_score =
                Self::assess_rule_value(group, conventions, constraints, synthesis, file_registry);

            // Skip only if filtering is enabled (threshold > 0) and score is below threshold
            if min_value_score > 0.0 && value_score < min_value_score {
                tracing::debug!(
                    group = group.name,
                    score = value_score,
                    threshold = min_value_score,
                    "Skipping low-value rule group"
                );
                skipped_low_value += 1;
                continue;
            }

            let rule = Self::generate_rule_for_group(group, monorepo, conventions, constraints)?;
            if !rule.content.is_empty() {
                rules.push(rule);
            }
        }

        tracing::info!(
            rule_count = rules.len(),
            skipped = skipped_low_value,
            threshold = min_value_score,
            "Generated path-based rules with value filtering"
        );

        Ok(rules)
    }

    /// Generate rules with full value score enforcement (legacy API).
    #[deprecated(note = "Use generate_with_threshold for configurable filtering")]
    pub fn generate_with_value_filter(
        plan: &OutputPlan,
        monorepo: Option<&MonorepoAnalysis>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        synthesis: Option<&SynthesizedAnalysis>,
        file_registry: Option<&VerifiedFileRegistry>,
    ) -> Result<Vec<Rule>> {
        Self::generate_with_threshold(
            plan,
            monorepo,
            conventions,
            constraints,
            synthesis,
            file_registry,
            DEFAULT_MIN_RULE_VALUE_SCORE,
        )
    }

    /// Assess the value score of a rule group
    ///
    /// Value is determined by:
    /// - Presence of unique patterns specific to the paths
    /// - Presence of hidden dependencies
    /// - Presence of gotchas/anti-patterns
    /// - File references available for evidence
    fn assess_rule_value(
        group: &PlannedRuleGroup,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        synthesis: Option<&SynthesizedAnalysis>,
        file_registry: Option<&VerifiedFileRegistry>,
    ) -> f32 {
        let mut score = 0.0f32;

        // Check for conventions patterns (base value)
        if !conventions.patterns.is_empty() {
            score += 0.1;
        }

        // Check for anti-patterns relevant to this group's paths
        let relevant_anti_patterns = constraints
            .anti_patterns
            .iter()
            .filter(|ap| {
                group.paths.iter().any(|path| {
                    ap.evidence
                        .iter()
                        .any(|e| e.file.contains(path.trim_end_matches("**")))
                })
            })
            .count();

        if relevant_anti_patterns > 0 {
            // Logarithmic scaling - more patterns continue to add value with diminishing returns
            score += 0.15 * (relevant_anti_patterns as f32 + 1.0).log2().min(3.0);
        }

        // Check for hidden dependencies involving these paths
        let relevant_deps = constraints
            .hidden_dependencies
            .iter()
            .filter(|dep| {
                group.paths.iter().any(|path| {
                    let base = path.trim_end_matches("**").trim_end_matches('/');
                    dep.source.contains(base) || dep.target.contains(base)
                })
            })
            .count();

        if relevant_deps > 0 {
            // Logarithmic scaling for dependencies
            score += 0.2 * (relevant_deps as f32 + 1.0).log2().min(3.0);
        }

        // Check for gotchas
        let relevant_gotchas = constraints
            .gotchas
            .iter()
            .filter(|g| {
                group.paths.iter().any(|path| {
                    let base = path.trim_end_matches("**").trim_end_matches('/');
                    g.related_files.iter().any(|f| f.contains(base)) || g.description.contains(base)
                })
            })
            .count();

        if relevant_gotchas > 0 {
            // Logarithmic scaling for gotchas
            score += 0.15 * (relevant_gotchas as f32 + 1.0).log2().min(3.0);
        }

        // Check synthesis for module-specific insights
        if let Some(synth) = synthesis {
            let relevant_modules = synth
                .modules
                .iter()
                .filter(|m| {
                    group.paths.iter().any(|path| {
                        let base = path.trim_end_matches("**").trim_end_matches('/');
                        m.path.contains(base)
                    })
                })
                .filter(|m| !m.responsibility.is_empty() || !m.patterns.is_empty())
                .count();

            if relevant_modules > 0 {
                score += 0.2;
            }
        }

        // Check for file references (evidence potential)
        if let Some(registry) = file_registry {
            let has_files = group.paths.iter().any(|path| {
                let base = path.trim_end_matches("**").trim_end_matches('/');
                !registry.files_in_directory(base).is_empty()
            });

            if has_files {
                score += 0.1;
            }
        }

        // Content sources boost - logarithmic scaling
        if !group.content_sources.is_empty() {
            score += 0.05 * (group.content_sources.len() as f32 + 1.0).log2().min(3.0);
        }

        score.min(1.0)
    }

    fn generate_rule_for_group(
        group: &PlannedRuleGroup,
        monorepo: Option<&MonorepoAnalysis>,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
    ) -> Result<Rule> {
        let mut content = Vec::new();

        let title = Self::generate_title(group, monorepo);
        content.push(format!("# {title}"));
        content.push(String::new());

        for source in &group.content_sources {
            match source {
                RuleContentSource::Conventions => {
                    let convention_content =
                        Self::generate_convention_content(conventions, &group.project_types);
                    if !convention_content.is_empty() {
                        content.push("## Conventions".to_string());
                        content.push(String::new());
                        content.extend(convention_content);
                        content.push(String::new());
                    }
                }
                RuleContentSource::AntiPatterns => {
                    // Include all anti-patterns - LLM determines relevance during generation
                    let anti_pattern_content =
                        Self::generate_anti_pattern_content(&constraints.anti_patterns);
                    if !anti_pattern_content.is_empty() {
                        content.push("## Anti-Patterns".to_string());
                        content.push(String::new());
                        content.extend(anti_pattern_content);
                        content.push(String::new());
                    }
                }
                RuleContentSource::HiddenDependencies => {
                    let dep_content =
                        Self::generate_dependency_content(&constraints.hidden_dependencies);
                    if !dep_content.is_empty() {
                        content.push("## Hidden Dependencies".to_string());
                        content.push(String::new());
                        content.extend(dep_content);
                        content.push(String::new());
                    }
                }
                RuleContentSource::Gotchas => {
                    let gotcha_content = Self::generate_gotcha_content(&constraints.gotchas);
                    if !gotcha_content.is_empty() {
                        content.push("## Gotchas".to_string());
                        content.push(String::new());
                        content.extend(gotcha_content);
                        content.push(String::new());
                    }
                }
            }
        }

        let content_len = content.len();
        let content_vec: Vec<String> = content
            .into_iter()
            .filter(|s| !s.is_empty() || content_len < 3)
            .collect::<Vec<_>>()
            .join("\n")
            .lines()
            .map(String::from)
            .collect();

        Ok(Rule {
            name: group.name.clone(),
            paths: if group.paths.is_empty() {
                None
            } else {
                Some(group.paths.clone())
            },
            content: content_vec,
            evidence: Vec::new(),
            tier: crate::types::ContentTier::default(),
        })
    }

    fn generate_title(group: &PlannedRuleGroup, monorepo: Option<&MonorepoAnalysis>) -> String {
        if let Some(mono) = monorepo
            && let Some(matching_group) = mono.rules_grouping.iter().find(|g| g.name == group.name)
        {
            let types: Vec<_> = matching_group
                .project_types
                .iter()
                .map(|t| t.as_str())
                .collect();
            let langs: Vec<_> = matching_group
                .languages
                .iter()
                .map(|s| s.as_str())
                .collect();

            if !types.is_empty() && !langs.is_empty() {
                return format!(
                    "{} {} Rules",
                    capitalize(&types.join("/")),
                    capitalize(&langs.join("/"))
                );
            }
        }

        capitalize(&group.name.replace('-', " "))
    }

    fn generate_convention_content(
        conventions: &InferredConventions,
        project_types: &[ProjectType],
    ) -> Vec<String> {
        let mut content = Vec::new();

        // For path-based rules in monorepo, only include patterns, not full architecture
        // (Architecture is already in CLAUDE.md - avoid duplication)
        let is_path_based_rule = !project_types.is_empty();

        if !is_path_based_rule && !conventions.architecture.pattern_name.is_empty() {
            content.push(format!(
                "- **Architecture**: {}",
                conventions.architecture.pattern_name
            ));
            if !conventions.architecture.description.is_empty() {
                content.push(format!("  - {}", conventions.architecture.description));
            }
        }

        // Note: Error handling patterns are language-specific and should NOT be included here
        // They are generated in CLAUDE.md standards with language awareness

        // Only include async pattern for non-path-based rules
        if !is_path_based_rule {
            match conventions.async_pattern.style {
                crate::pipeline::phases::convention_inference::AsyncStyle::AsyncAwait => {
                    content.push("- **Async**: Always use async/await pattern".to_string());
                    if let Some(runtime) = &conventions.async_pattern.runtime {
                        content.push(format!("  - Runtime: {runtime}"));
                    }
                }
                crate::pipeline::phases::convention_inference::AsyncStyle::Reactive => {
                    content.push("- **Async**: Should use reactive streams pattern".to_string());
                }
                _ => {}
            }
        }

        // Add patterns with actionable language to pass tier filter
        // Skip patterns for path-based rules (monorepo) to avoid duplication
        // Patterns are workspace-level and already in CLAUDE.md
        if !is_path_based_rule {
            for pattern in &conventions.patterns {
                // Ensure pattern description has actionable language
                let desc = if pattern.description.to_lowercase().contains("should")
                    || pattern.description.to_lowercase().contains("must")
                    || pattern.description.to_lowercase().contains("always")
                    || pattern.description.to_lowercase().contains("never")
                {
                    pattern.description.clone()
                } else {
                    format!("Should follow: {}", pattern.description)
                };
                content.push(format!("- **{}**: {}", pattern.name, desc));
            }
        }

        content
    }

    fn generate_anti_pattern_content(anti_patterns: &[AntiPattern]) -> Vec<String> {
        let mut content = Vec::new();

        for ap in anti_patterns {
            content.push(format!("### ✗ {}", ap.name));
            content.push(ap.description.clone());
            content.push(String::new());
            content.push(format!("**Why**: {}", ap.why_bad));
            content.push(format!("**Instead**: {}", ap.correct_approach));
            content.push(String::new());
        }

        content
    }

    fn generate_dependency_content(
        deps: &[crate::pipeline::phases::constraint_extraction::HiddenDependency],
    ) -> Vec<String> {
        let mut content = Vec::new();

        for dep in deps {
            content.push(format!("### {} → {}", dep.source, dep.target));
            content.push(dep.description.clone());
            content.push(format!("**Impact**: {}", dep.impact));
            content.push(String::new());
        }

        content
    }

    fn generate_gotcha_content(gotchas: &[Gotcha]) -> Vec<String> {
        let mut content = Vec::new();

        for gotcha in gotchas {
            content.push(format!("### ⚠️ {}", gotcha.title));
            content.push(gotcha.description.clone());
            content.push(format!("**When**: {}", gotcha.when));
            content.push(format!("**Solution**: {}", gotcha.solution));
            content.push(String::new());
        }

        content
    }
}

/// CLAUDE.md Generator
///
/// Generates CLAUDE.md following Progressive Disclosure philosophy:
/// - Minimal core principles that provide unique project value
/// - Architecture with @file:line references
/// - Code standards as anti-patterns (what NOT to do)
/// - NO Tier 1 content (build commands, language basics)
pub struct ClaudeMdGenerator;

impl ClaudeMdGenerator {
    pub fn generate(
        plan: &OutputPlan,
        detection: &crate::pipeline::phases::project_detection::ProjectDetection,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        project_name: &str,
    ) -> Result<crate::types::ProjectMemory> {
        Self::generate_with_enrichment(
            plan,
            detection,
            conventions,
            constraints,
            project_name,
            None,
            None,
        )
    }

    /// Generate with enriched plan for complete data flow (no information loss)
    pub fn generate_with_enrichment(
        plan: &OutputPlan,
        detection: &crate::pipeline::phases::project_detection::ProjectDetection,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        project_name: &str,
        enriched_plan: Option<&EnrichedPlan>,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Result<crate::types::ProjectMemory> {
        use crate::types::ProjectMemory;

        let overview = Self::generate_overview(detection, project_name);

        let architecture = if plan.claude_md_plan.include_architecture {
            Self::generate_architecture_with_enrichment(
                conventions,
                detection,
                enriched_plan,
                synthesis,
            )
        } else {
            None
        };

        // Commands are Tier 1 content (generic knowledge) - not included
        let commands = Vec::new();

        let standards = if plan.claude_md_plan.include_conventions {
            Self::generate_standards_with_evidence(conventions, constraints, synthesis)
        } else {
            Vec::new()
        };

        Ok(ProjectMemory {
            overview,
            architecture,
            commands,
            standards,
            imports: Vec::new(),
        })
    }

    fn generate_overview(
        detection: &crate::pipeline::phases::project_detection::ProjectDetection,
        project_name: &str,
    ) -> String {
        let project_type = detection.primary_type.as_str();
        let languages: Vec<_> = detection
            .languages
            .iter()
            .map(|l| l.language.as_str())
            .take(3)
            .collect();

        let mut overview = format!("{} is a {} project", project_name, project_type);

        if !languages.is_empty() {
            overview.push_str(&format!(" written in {}", languages.join(", ")));
        }

        overview.push('.');

        if detection.is_monorepo
            && let Some(ws) = &detection.workspace_config
        {
            overview.push_str(&format!(
                "\n\nThis is a {:?} monorepo with {} members.",
                ws.workspace_type,
                ws.members.len()
            ));
        }

        overview
    }

    fn generate_architecture_with_enrichment(
        conventions: &InferredConventions,
        _detection: &crate::pipeline::phases::project_detection::ProjectDetection,
        enriched_plan: Option<&EnrichedPlan>,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Option<String> {
        let mut arch = String::new();

        if !conventions.architecture.pattern_name.is_empty() {
            arch.push_str(&format!(
                "**Pattern**: {}\n\n",
                conventions.architecture.pattern_name
            ));
        }

        if !conventions.architecture.description.is_empty() {
            arch.push_str(&conventions.architecture.description);
            arch.push_str("\n\n");
        }

        // Use synthesis modules if available for richer architecture description with file refs
        if let Some(synth) = synthesis
            && !synth.modules.is_empty()
        {
            for module in &synth.modules {
                if !module.responsibility.is_empty() {
                    arch.push_str(&format!(
                        "- `{}` - {}\n",
                        module.path, module.responsibility
                    ));
                }
            }
        }

        // Fall back to convention layers if no synthesis
        if synthesis.is_none_or(|s| s.modules.is_empty()) {
            for layer in &conventions.architecture.layers {
                arch.push_str(&format!(
                    "- `{}` - {}\n",
                    layer.path_pattern, layer.responsibility
                ));
            }
        }

        if arch.is_empty() {
            let key_dirs: Vec<_> = conventions
                .file_organization
                .key_directories
                .iter()
                .take(5)
                .collect();

            if !key_dirs.is_empty() {
                for dir in key_dirs {
                    arch.push_str(&format!("- `{}` - {}\n", dir.path, dir.role));
                }
            }
        }

        // Key Abstractions from EnrichedPlan (properly consumed from enrichment layer)
        if let Some(plan) = enriched_plan
            && !plan.key_abstractions.is_empty()
        {
            arch.push_str("\n\n## Key Abstractions\n\n");
            for abst in plan.key_abstractions.iter().take(10) {
                arch.push_str(&format!(
                    "### {} ({}) {}\n",
                    abst.name, abst.kind, abst.file_ref
                ));
                arch.push_str(&format!("{}\n", abst.description));
                for note in &abst.usage_notes {
                    arch.push_str(&format!("- {}\n", note));
                }
                arch.push('\n');
            }
        }

        // File insights with gotchas from EnrichedPlan
        if let Some(plan) = enriched_plan
            && !plan.file_insights.is_empty()
        {
            let insights_with_gotchas: Vec<_> = plan
                .file_insights
                .iter()
                .filter(|i| !i.gotchas.is_empty())
                .take(5)
                .collect();

            if !insights_with_gotchas.is_empty() {
                arch.push_str("\n\n## Critical File Gotchas\n\n");
                for insight in insights_with_gotchas {
                    arch.push_str(&format!("### @{}\n", insight.file));
                    arch.push_str(&format!("{}\n", insight.purpose));
                    for gotcha in &insight.gotchas {
                        arch.push_str(&format!("- ⚠️ {}\n", gotcha));
                    }
                    arch.push('\n');
                }
            }
        }

        if arch.is_empty() {
            None
        } else {
            Some(arch.trim().to_string())
        }
    }

    /// Generate standards with evidence from synthesis
    ///
    /// Focus on:
    /// - Anti-patterns (what NOT to do) with evidence
    /// - Hidden dependencies that break if violated
    /// - Gotchas specific to this project
    /// - NO generic advice (Tier 1)
    fn generate_standards_with_evidence(
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        synthesis: Option<&SynthesizedAnalysis>,
    ) -> Vec<String> {
        let mut standards = Vec::new();

        // Architecture pattern - only if specific and evidenced
        if !conventions.architecture.pattern_name.is_empty() {
            standards.push(format!(
                "Follow {} architecture pattern",
                conventions.architecture.pattern_name
            ));
        }

        // Anti-patterns with evidence (highest value)
        for ap in &constraints.anti_patterns {
            // Include evidence file reference if available
            if let Some(evidence) = ap.evidence.first() {
                standards.push(format!(
                    "✗ {}: {} (see @{}:{})",
                    ap.name,
                    ap.correct_approach,
                    evidence.file,
                    evidence.line.unwrap_or(1)
                ));
            } else {
                standards.push(format!("✗ {}: {}", ap.name, ap.correct_approach));
            }
        }

        // Hidden dependencies (critical for correctness)
        for dep in constraints.hidden_dependencies.iter().take(5) {
            standards.push(format!(
                "⚠️ {} → {}: {}",
                dep.source, dep.target, dep.description
            ));
        }

        // Gotchas with solutions
        for gotcha in constraints.gotchas.iter().take(5) {
            if let Some(first_file) = gotcha.related_files.first() {
                standards.push(format!(
                    "⚠️ {}: {} (affects {})",
                    gotcha.title, gotcha.solution, first_file
                ));
            } else {
                standards.push(format!("⚠️ {}: {}", gotcha.title, gotcha.solution));
            }
        }

        // Add patterns from synthesis if valuable
        if let Some(synth) = synthesis {
            for pattern in synth.deep.patterns.iter().take(3) {
                if !pattern.locations.is_empty() {
                    let loc = &pattern.locations[0];
                    standards.push(format!(
                        "- {}: {} (see @{}:{})",
                        pattern.name, pattern.description, loc.file, loc.line
                    ));
                }
            }

            // File insights with gotchas (previously lost in pipeline)
            for insight in synth.deep.insights.iter().filter(|i| !i.gotchas.is_empty()) {
                for gotcha in insight.gotchas.iter().take(2) {
                    standards.push(format!("⚠️ {} ({})", gotcha, insight.file));
                }
            }
        }

        standards
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::phases::constraint_extraction::HiddenDependency;
    use crate::pipeline::phases::convention_inference::{
        ArchitectureConvention, AsyncPattern, ErrorHandlingPattern, FileOrganization,
        NamingConventions, TestingConvention,
    };

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("hello"), "Hello");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("WORLD"), "WORLD");
    }

    fn create_test_conventions() -> InferredConventions {
        InferredConventions {
            architecture: ArchitectureConvention::default(),
            naming: NamingConventions::default(),
            file_organization: FileOrganization::default(),
            error_handling: ErrorHandlingPattern::default(),
            async_pattern: AsyncPattern::default(),
            patterns: Vec::new(),
            testing: TestingConvention::default(),
        }
    }

    fn create_test_constraints() -> ExtractedConstraints {
        ExtractedConstraints::default()
    }

    #[test]
    fn test_assess_rule_value_empty() {
        let conventions = create_test_conventions();
        let constraints = create_test_constraints();
        let group = PlannedRuleGroup {
            name: "test-group".to_string(),
            paths: vec!["src/**".to_string()],
            languages: vec!["rust".to_string()],
            project_types: vec![],
            content_sources: vec![],
        };

        let score =
            PathRulesGenerator::assess_rule_value(&group, &conventions, &constraints, None, None);

        // Empty context should have low value
        assert!(score < DEFAULT_MIN_RULE_VALUE_SCORE);
    }

    #[test]
    fn test_assess_rule_value_with_dependencies() {
        use crate::pipeline::phases::constraint_extraction::HiddenDepType;

        let conventions = create_test_conventions();
        let mut constraints = create_test_constraints();

        // Add hidden dependencies related to src/api
        constraints.hidden_dependencies.push(HiddenDependency {
            source: "src/api/handler.rs".to_string(),
            target: "src/domain/model.rs".to_string(),
            dependency_type: HiddenDepType::ImplicitOrdering,
            description: "API must validate before domain".to_string(),
            impact: "Validation failures".to_string(),
            evidence: Vec::new(),
        });

        let group = PlannedRuleGroup {
            name: "api-rules".to_string(),
            paths: vec!["src/api/**".to_string()],
            languages: vec!["rust".to_string()],
            project_types: vec![],
            content_sources: vec![RuleContentSource::HiddenDependencies],
        };

        let score =
            PathRulesGenerator::assess_rule_value(&group, &conventions, &constraints, None, None);

        // With hidden dependency, score should be higher
        assert!(score >= 0.2); // At least 0.2 from the dependency
    }

    #[test]
    fn test_value_threshold_enforcement() {
        let conventions = create_test_conventions();
        let constraints = create_test_constraints();

        // Group with no relevant content should not be generated
        let score = PathRulesGenerator::assess_rule_value(
            &PlannedRuleGroup {
                name: "empty-group".to_string(),
                paths: vec!["nonexistent/**".to_string()],
                languages: vec![],
                project_types: vec![],
                content_sources: vec![],
            },
            &conventions,
            &constraints,
            None,
            None,
        );

        assert!(
            score < DEFAULT_MIN_RULE_VALUE_SCORE,
            "Empty group should be below threshold"
        );
    }
}
