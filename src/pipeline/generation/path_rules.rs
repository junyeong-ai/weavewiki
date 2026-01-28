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

const DEFAULT_MIN_RULE_VALUE_SCORE: f32 = 0.3;

/// Context for enriched generation containing optional analysis results
#[derive(Default)]
pub struct EnrichmentContext<'a> {
    pub enriched_plan: Option<&'a EnrichedPlan>,
    pub synthesis: Option<&'a SynthesizedAnalysis>,
    pub domain_analysis: Option<&'a crate::types::domain::DomainAnalysisResult>,
    pub cross_insights: Option<&'a crate::pipeline::analysis::SynthesizedInsights>,
}

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

    /// Assess the value score of a rule group
    fn assess_rule_value(
        group: &PlannedRuleGroup,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        synthesis: Option<&SynthesizedAnalysis>,
        file_registry: Option<&VerifiedFileRegistry>,
    ) -> f32 {
        let mut signals = 0usize;

        if !conventions.patterns.is_empty() {
            signals += 1;
        }

        let has_anti_patterns = constraints.anti_patterns.iter().any(|ap| {
            group.paths.iter().any(|path| {
                ap.evidence
                    .iter()
                    .any(|e| e.file.contains(path.trim_end_matches("**")))
            })
        });
        if has_anti_patterns {
            signals += 1;
        }

        let has_deps = constraints.hidden_dependencies.iter().any(|dep| {
            group.paths.iter().any(|path| {
                let base = path.trim_end_matches("**").trim_end_matches('/');
                dep.source.contains(base) || dep.target.contains(base)
            })
        });
        if has_deps {
            signals += 1;
        }

        let has_gotchas = constraints.gotchas.iter().any(|g| {
            group.paths.iter().any(|path| {
                let base = path.trim_end_matches("**").trim_end_matches('/');
                g.related_files.iter().any(|f| f.contains(base)) || g.description.contains(base)
            })
        });
        if has_gotchas {
            signals += 1;
        }

        if let Some(synth) = synthesis {
            let has_modules = synth.modules.iter().any(|m| {
                group.paths.iter().any(|path| {
                    let base = path.trim_end_matches("**").trim_end_matches('/');
                    m.path.contains(base)
                }) && (!m.responsibility.is_empty() || !m.patterns.is_empty())
            });
            if has_modules {
                signals += 1;
            }
        }

        if let Some(registry) = file_registry {
            let has_files = group.paths.iter().any(|path| {
                let base = path.trim_end_matches("**").trim_end_matches('/');
                !registry.files_in_directory(base).is_empty()
            });
            if has_files {
                signals += 1;
            }
        }

        if !group.content_sources.is_empty() {
            signals += 1;
        }

        match signals {
            0 => 0.2,
            1..=2 => 0.4,
            3..=4 => 0.6,
            _ => 0.8,
        }
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

        // Add patterns for non-path-based rules
        // Patterns are workspace-level and already in CLAUDE.md for monorepo paths
        // Pattern descriptions are used as-is - LLM determines actionability during generation
        if !is_path_based_rule {
            for pattern in &conventions.patterns {
                content.push(format!("- **{}**: {}", pattern.name, pattern.description));
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
            &EnrichmentContext::default(),
        )
    }

    pub fn generate_with_enrichment(
        plan: &OutputPlan,
        detection: &crate::pipeline::phases::project_detection::ProjectDetection,
        conventions: &InferredConventions,
        constraints: &ExtractedConstraints,
        project_name: &str,
        ctx: &EnrichmentContext<'_>,
    ) -> Result<crate::types::ProjectMemory> {
        use crate::types::ProjectMemory;

        let overview = Self::generate_overview(detection, project_name);

        let architecture = if plan.claude_md_plan.include_architecture {
            Self::generate_architecture_with_enrichment(
                conventions,
                detection,
                ctx.enriched_plan,
                ctx.synthesis,
            )
        } else {
            None
        };

        let commands = Vec::new();

        let standards = if plan.claude_md_plan.include_conventions {
            Self::generate_standards_with_evidence(conventions, constraints, ctx.synthesis, ctx.cross_insights)
        } else {
            Vec::new()
        };

        let domain_knowledge = Self::generate_domain_knowledge(ctx.domain_analysis);
        let gotchas = Self::generate_gotchas(constraints, ctx.cross_insights);

        Ok(ProjectMemory {
            overview,
            architecture,
            commands,
            standards,
            imports: Vec::new(),
            domain_knowledge,
            gotchas,
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
            // Include all key directories - LLM token budget is natural limit
            if !conventions.file_organization.key_directories.is_empty() {
                for dir in &conventions.file_organization.key_directories {
                    arch.push_str(&format!("- `{}` - {}\n", dir.path, dir.role));
                }
            }
        }

        // Key Abstractions from EnrichedPlan - include all (high-value content)
        if let Some(plan) = enriched_plan
            && !plan.key_abstractions.is_empty()
        {
            arch.push_str("\n\n## Key Abstractions\n\n");
            for abst in &plan.key_abstractions {
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

        // File insights with gotchas from EnrichedPlan - include all with gotchas
        if let Some(plan) = enriched_plan
            && !plan.file_insights.is_empty()
        {
            let insights_with_gotchas: Vec<_> = plan
                .file_insights
                .iter()
                .filter(|i| !i.gotchas.is_empty())
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
        cross_insights: Option<&crate::pipeline::analysis::SynthesizedInsights>,
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

        // Hidden dependencies (critical for correctness) - include all, LLM token budget is natural limit
        for dep in &constraints.hidden_dependencies {
            standards.push(format!(
                "⚠️ {} → {}: {}",
                dep.source, dep.target, dep.description
            ));
        }

        // Gotchas with solutions - include all (high-value Tier 3 content)
        for gotcha in &constraints.gotchas {
            if let Some(first_file) = gotcha.related_files.first() {
                standards.push(format!(
                    "⚠️ {}: {} (affects {})",
                    gotcha.title, gotcha.solution, first_file
                ));
            } else {
                standards.push(format!("⚠️ {}: {}", gotcha.title, gotcha.solution));
            }
        }

        // Add patterns from synthesis if valuable - include all with evidence
        if let Some(synth) = synthesis {
            for pattern in &synth.deep.patterns {
                if !pattern.locations.is_empty() {
                    let loc = &pattern.locations[0];
                    standards.push(format!(
                        "- {}: {} (see @{}:{})",
                        pattern.name, pattern.description, loc.file, loc.line
                    ));
                }
            }

            // File insights with gotchas (from deep analysis) - include all gotchas
            for insight in synth.deep.insights.iter().filter(|i| !i.gotchas.is_empty()) {
                for gotcha in &insight.gotchas {
                    standards.push(format!("⚠️ {} ({})", gotcha, insight.file));
                }
            }
        }

        // Tier 2 insights from cross-synthesis (project conventions)
        if let Some(insights) = cross_insights {
            for insight in &insights.tier2_insights {
                if insight.scope.is_empty() || insight.scope == "Project-wide" {
                    standards.push(format!(
                        "- {}: {} — {}",
                        insight.category, insight.title, insight.description
                    ));
                } else {
                    standards.push(format!(
                        "- {}: {} — {} (scope: {})",
                        insight.category, insight.title, insight.description, insight.scope
                    ));
                }
            }
        }

        standards
    }

    fn generate_domain_knowledge(
        domain: Option<&crate::types::domain::DomainAnalysisResult>,
    ) -> Option<String> {
        let domain = domain?;
        if domain.policies.is_empty()
            && domain.glossary.terms.is_empty()
            && domain.workflows.is_empty()
        {
            return None;
        }

        let mut content = String::new();

        // Core Policies - include all (high-value domain knowledge)
        if !domain.policies.is_empty() {
            content.push_str("### Core Policies\n\n");
            for policy in &domain.policies {
                content.push_str(&format!(
                    "- **{}** ({}): {}\n",
                    policy.name,
                    format!("{:?}", policy.policy_type).to_lowercase(),
                    policy.description
                ));
                if !policy.evidence.is_empty() {
                    let ev = &policy.evidence[0];
                    content.push_str(&format!("  - Evidence: @{}:{}\n", ev.file, ev.start_line));
                }
            }
            content.push('\n');
        }

        // Core Domain Logic - include all (critical for understanding business rules)
        if !domain.core_logic.is_empty() {
            content.push_str("### Core Domain Logic\n\n");
            for logic in &domain.core_logic {
                content.push_str(&format!("- **{}**: {}\n", logic.name, logic.description));
                if !logic.business_impact.is_empty() {
                    content.push_str(&format!("  - Impact: {}\n", logic.business_impact));
                }
            }
            content.push('\n');
        }

        // Glossary - include all (domain terminology is essential context)
        if !domain.glossary.terms.is_empty() {
            content.push_str("### Glossary\n\n");
            for term in &domain.glossary.terms {
                content.push_str(&format!("- **{}**: {}\n", term.term, term.definition));
            }
            content.push('\n');
        }

        // Workflows - include all (business process understanding)
        if !domain.workflows.is_empty() {
            content.push_str("### Business Workflows\n\n");
            for workflow in &domain.workflows {
                content.push_str(&format!("**{}**\n", workflow.name));
                content.push_str(&format!("{}\n", workflow.description));
                for step in &workflow.steps {
                    content.push_str(&format!("{}. {}: {}\n", step.order, step.name, step.action));
                }
                content.push('\n');
            }
        }

        if content.is_empty() {
            None
        } else {
            Some(content.trim().to_string())
        }
    }

    fn generate_gotchas(
        constraints: &ExtractedConstraints,
        cross_insights: Option<&crate::pipeline::analysis::SynthesizedInsights>,
    ) -> Vec<String> {
        let mut gotchas = Vec::new();

        // Tier 3 insights from cross-synthesis (highest value) - include all
        if let Some(insights) = cross_insights {
            for insight in &insights.tier3_insights {
                gotchas.push(format!(
                    "**{}**: {} → {}",
                    insight.title, insight.description, insight.prevention_guidance
                ));
            }

            // Hidden dependencies - include all (critical for correctness)
            for dep in &insights.hidden_dependencies {
                gotchas.push(format!(
                    "**Hidden Dep**: {} → {} ({:?}): {}",
                    dep.from_module, dep.to_module, dep.dependency_type, dep.description
                ));
            }

            // Architecture violations - include all
            for violation in &insights.architecture_violations {
                gotchas.push(format!(
                    "**Violation**: {} ({} → {}): {}",
                    violation.description,
                    violation.from_layer,
                    violation.to_layer,
                    violation.suggested_fix
                ));
            }
        }

        // Gotchas from constraint extraction - include all (Tier 3 content)
        for gotcha in &constraints.gotchas {
            let entry = format!("**{}**: {} → {}", gotcha.title, gotcha.description, gotcha.solution);
            if !gotchas.contains(&entry) {
                gotchas.push(entry);
            }
        }

        gotchas
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
