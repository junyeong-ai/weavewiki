//! Shared formatting functions for agent and skill discovery prompts.

use crate::pipeline::analysis::deep_analyzer::PatternInstance;
use crate::pipeline::evidence::artifact_ref_opt;
use crate::pipeline::generation::context::GenerationContext;
use crate::pipeline::generation::context_enricher::EnrichedContext;

pub(crate) struct DiscoveryFormat {
    pub(crate) include_domain_count: bool,
    pub(crate) include_value_score: bool,
    pub(crate) sort_patterns_by_locations: bool,
}

impl DiscoveryFormat {
    pub(crate) fn for_agents() -> Self {
        Self {
            include_domain_count: true,
            include_value_score: true,
            sort_patterns_by_locations: false,
        }
    }

    pub(crate) fn for_skills() -> Self {
        Self {
            include_domain_count: false,
            include_value_score: false,
            sort_patterns_by_locations: true,
        }
    }

}

pub(crate) fn format_project_summary(ctx: &GenerationContext<'_>, fmt: &DiscoveryFormat) -> String {
    let frameworks: Vec<_> = ctx
        .tech_stack
        .frameworks
        .iter()
        .map(|f| f.name.clone())
        .collect();

    let mut lines = format!(
        r#"## PROJECT SUMMARY
- Name: {name}
- Type: {project_type}
- Language: {language}
- Frameworks: {frameworks}
- File Count: {file_count}
- Module Count: {module_count}"#,
        name = ctx.project_name,
        project_type = ctx.detection.primary_type.as_str(),
        language = ctx.tech_stack.primary_language,
        frameworks = if frameworks.is_empty() {
            "None detected".into()
        } else {
            frameworks.join(", ")
        },
        file_count = ctx.file_registry.file_count(),
        module_count = ctx.modules.len(),
    );

    if fmt.include_domain_count {
        lines.push_str(&format!("\n- Domain Count: {}", ctx.domains.len()));
    }

    lines
}

pub(crate) fn format_modules(ctx: &GenerationContext<'_>, fmt: &DiscoveryFormat) -> String {
    if ctx.modules.is_empty() {
        return "## MODULES\nNo modules detected - single-module project.".into();
    }

    let modules: Vec<_> = ctx
        .modules
        .iter()
        .map(|m| {
            let files_count = m.paths.len();
            let deps = if m.dependencies.is_empty() {
                String::new()
            } else {
                format!(" → depends on: {}", m.dependencies.join(", "))
            };

            if fmt.include_value_score {
                let value = format!("(value: {:.2})", m.value_score);
                format!(
                    "- **{}** {} ({} files): {}{}\n  Key: {}",
                    m.module_id,
                    value,
                    files_count,
                    m.responsibility,
                    deps,
                    if m.key_files.is_empty() {
                        "none specified".into()
                    } else {
                        m.key_files.join(", ")
                    }
                )
            } else {
                format!(
                    "- **{}** ({} files): {}{}\n  Key: {}",
                    m.module_id,
                    files_count,
                    m.responsibility,
                    deps,
                    m.key_files.join(", ")
                )
            }
        })
        .collect();

    let header = if fmt.include_value_score {
        "## MODULES (Candidates for Specialists)"
    } else {
        "## MODULES"
    };
    format!("{}\n{}", header, modules.join("\n"))
}

pub(crate) fn format_patterns(
    ctx: &GenerationContext<'_>,
    enriched: &EnrichedContext,
    fmt: &DiscoveryFormat,
) -> String {
    let patterns = ctx.all_patterns();
    if patterns.is_empty() {
        if let Some(ast) = &enriched.ast
            && !ast.dominant_patterns.is_empty()
        {
            let label = if fmt.sort_patterns_by_locations {
                "## PATTERNS (inferred from AST)"
            } else {
                "## PATTERNS (from AST)"
            };
            return format!(
                "{}\n{}",
                label,
                ast.dominant_patterns
                    .iter()
                    .map(|p| format!("- {}", p))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        let fallback = if fmt.sort_patterns_by_locations {
            "## PATTERNS\nNo explicit patterns detected. Use CODE FACTS and module structure above."
        } else {
            "## PATTERNS\nNo patterns detected. Use module structure for agent discovery."
        };
        return fallback.into();
    }

    let formatted: Vec<_> = if fmt.sort_patterns_by_locations {
        let mut sorted = patterns.clone();
        sorted.sort_by_key(|p| std::cmp::Reverse(p.locations.len()));
        sorted
            .iter()
            .map(|p| format_single_pattern(p))
            .collect()
    } else {
        patterns.iter().map(|p| format_single_pattern(p)).collect()
    };

    let header = if fmt.sort_patterns_by_locations {
        "## PATTERNS DETECTED"
    } else {
        "## DETECTED PATTERNS"
    };
    format!("{}\n{}", header, formatted.join("\n"))
}

fn format_single_pattern(p: &PatternInstance) -> String {
    let locations: Vec<_> = p
        .locations
        .iter()
        .map(|l| artifact_ref_opt(&l.file, Some(l.line)))
        .collect();
    format!(
        "- **{}**: {} ({})",
        p.category,
        p.description,
        locations.join(", ")
    )
}

pub(crate) fn format_insights(
    ctx: &GenerationContext<'_>,
    enriched: &EnrichedContext,
    header_suffix: &str,
    fallback_fn: impl FnOnce(&GenerationContext<'_>, &EnrichedContext) -> String,
) -> String {
    let insights = ctx.all_discovered_insights();
    if insights.is_empty() {
        return fallback_fn(ctx, enriched);
    }

    let formatted: Vec<_> = insights
        .iter()
        .map(|i| {
            let evidence: Vec<_> = i
                .evidence
                .iter()
                .map(|e| artifact_ref_opt(&e.file, Some(e.start_line)))
                .collect();
            format!(
                "- **[{}] {}**: {}\n  Prevention: {}\n  Evidence: {}",
                i.category,
                i.title,
                i.description,
                i.prevention_guidance,
                evidence.join(", ")
            )
        })
        .collect();

    format!(
        "## CRITICAL INSIGHTS ({})\n{}",
        header_suffix,
        formatted.join("\n\n")
    )
}

pub(crate) fn format_domain_knowledge(ctx: &GenerationContext<'_>) -> String {
    let domain = match ctx.domain_knowledge() {
        Some(d) => d,
        None => return String::new(),
    };

    let mut parts = Vec::new();
    if !domain.policies.is_empty() {
        parts.push(format!("**Policies**: {}", domain.policies.join("; ")));
    }
    if !domain.core_logic.is_empty() {
        parts.push(format!("**Core Logic**: {}", domain.core_logic.join("; ")));
    }
    if !domain.terminology.is_empty() {
        parts.push(format!("**Domain Terms**: {}", domain.terminology.join(", ")));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!("## DOMAIN KNOWLEDGE\n{}", parts.join("\n"))
    }
}

/// Build common structural insights (entry points, core modules).
fn build_common_structural_insights(enriched: &EnrichedContext) -> Vec<String> {
    let mut insights = Vec::new();

    let entry_points: Vec<_> = enriched
        .structural
        .entry_points
        .iter()
        .map(|e| format!("@{} [{}]", e.path, e.kind))
        .collect();

    if !entry_points.is_empty() {
        insights.push(format!("**Entry Points**: {}", entry_points.join(", ")));
    }

    let core_modules: Vec<_> = enriched
        .structural
        .modules
        .iter()
        .filter(|m| m.is_core)
        .map(|m| format!("{} ({} files)", m.name, m.file_count))
        .collect();

    if !core_modules.is_empty() {
        insights.push(format!("**Core Modules**: {}", core_modules.join(", ")));
    }

    insights
}

/// Format module dependency summary. If `include_count` is true, prefix with count.
fn format_dep_summary(ctx: &GenerationContext<'_>, include_count: bool) -> Option<String> {
    let dep_summary: Vec<_> = ctx
        .modules
        .iter()
        .filter(|m| !m.dependencies.is_empty())
        .map(|m| format!("{} → {}", m.module_id, m.dependencies.join(", ")))
        .collect();

    if dep_summary.is_empty() {
        None
    } else if include_count {
        Some(format!(
            "**Module Dependencies ({})**: {}",
            dep_summary.len(),
            dep_summary.join("; ")
        ))
    } else {
        Some(format!("**Module Dependencies**: {}", dep_summary.join("; ")))
    }
}

pub(crate) fn format_structural_insights_fallback(
    ctx: &GenerationContext<'_>,
    enriched: &EnrichedContext,
) -> String {
    let mut insights = build_common_structural_insights(enriched);

    if let Some(deps) = format_dep_summary(ctx, false) {
        insights.push(deps);
    }

    if insights.is_empty() {
        format!(
            "## STRUCTURAL INSIGHTS\n\
             Project type: {} | Language: {} | Files: {}\n\
             No detailed insights available.",
            ctx.detection.primary_type.as_str(),
            enriched.structural.primary_language,
            enriched.structural.file_count
        )
    } else {
        format!(
            "## STRUCTURAL INSIGHTS\n\
             Project: {} ({}) | {} files\n\n{}",
            ctx.detection.primary_type.as_str(),
            enriched.structural.primary_language,
            enriched.structural.file_count,
            insights.join("\n")
        )
    }
}

pub(crate) fn format_structural_insights_fallback_with_ast(
    ctx: &GenerationContext<'_>,
    enriched: &EnrichedContext,
) -> String {
    let mut insights = build_common_structural_insights(enriched);

    if let Some(ast) = &enriched.ast {
        let key_types: Vec<_> = ast
            .key_types
            .iter()
            .map(|t| format!("{}@{}:{}", t.name, t.file, t.line))
            .collect();
        if !key_types.is_empty() {
            insights.push(format!(
                "**Key Types ({})**: {}",
                key_types.len(),
                key_types.join(", ")
            ));
        }

        let key_funcs: Vec<_> = ast
            .key_functions
            .iter()
            .map(|f| {
                let async_marker = if f.is_async { "async " } else { "" };
                format!("{}{}()@{}:{}", async_marker, f.name, f.file, f.line)
            })
            .collect();
        if !key_funcs.is_empty() {
            insights.push(format!(
                "**Key Functions ({})**: {}",
                key_funcs.len(),
                key_funcs.join(", ")
            ));
        }
    }

    if ctx.constraint_count() > 0 {
        insights.push(format!(
            "**Constraints Detected**: {} (see PROJECT CONSTRAINTS section)",
            ctx.constraint_count()
        ));
    }

    if !ctx.modules.is_empty()
        && let Some(deps) = format_dep_summary(ctx, true)
    {
        insights.push(deps);
    }

    if insights.is_empty() {
        format!(
            "## STRUCTURAL INSIGHTS\n\
             Project type: {}\n\
             Primary language: {}\n\
             File count: {}\n\
             Generate skills based on the actual project structure above.",
            ctx.detection.primary_type.as_str(),
            enriched.structural.primary_language,
            enriched.structural.file_count
        )
    } else {
        format!(
            "## STRUCTURAL INSIGHTS\n\
             Project type: {} | Language: {} | Files: {}\n\n\
             {}",
            ctx.detection.primary_type.as_str(),
            enriched.structural.primary_language,
            enriched.structural.file_count,
            insights.join("\n")
        )
    }
}
