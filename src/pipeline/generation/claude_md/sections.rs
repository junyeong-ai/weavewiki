//! CLAUDE.md Section Generators
//!
//! Contains functions for generating individual CLAUDE.md sections:
//! - Overview
//! - Architecture
//! - Standards
//! - Domain Knowledge
//! - Gotchas

use crate::pipeline::analysis::SynthesizedInsights;
use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
use crate::pipeline::phases::convention_inference::InferredConventions;
use crate::pipeline::phases::project_detection::ProjectDetection;
use crate::types::domain::DomainAnalysisResult;

use super::SynthesizedAnalysis;

/// Generate the project overview section
pub fn generate_overview(detection: &ProjectDetection, project_name: &str) -> String {
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
            "\n\nThis is a {} monorepo with {} members.",
            ws.workspace_type,
            ws.members.len()
        ));
    }

    overview
}

/// Generate the architecture section
pub fn generate_architecture(
    conventions: &InferredConventions,
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

    if synthesis.is_none_or(|s| s.modules.is_empty()) {
        for layer in &conventions.architecture.layers {
            arch.push_str(&format!(
                "- `{}` - {}\n",
                layer.path_pattern, layer.responsibility
            ));
        }
    }

    if arch.is_empty() && !conventions.file_organization.key_directories.is_empty() {
        for dir in &conventions.file_organization.key_directories {
            arch.push_str(&format!("- `{}` - {}\n", dir.path, dir.role));
        }
    }

    if arch.is_empty() {
        None
    } else {
        Some(arch.trim().to_string())
    }
}

/// Generate the standards section
pub fn generate_standards(
    conventions: &InferredConventions,
    constraints: &ExtractedConstraints,
    synthesis: Option<&SynthesizedAnalysis>,
    cross_insights: Option<&SynthesizedInsights>,
) -> Vec<String> {
    let mut standards = Vec::new();

    if !conventions.architecture.pattern_name.is_empty() {
        standards.push(format!(
            "Follow {} architecture pattern",
            conventions.architecture.pattern_name
        ));
    }

    for ap in &constraints.anti_patterns {
        if let Some(evidence) = ap.evidence.first() {
            standards.push(format!(
                "X {}: {} (see @{}:{})",
                ap.name,
                ap.correct_approach,
                evidence.file,
                evidence.line.unwrap_or(1)
            ));
        } else {
            standards.push(format!("X {}: {}", ap.name, ap.correct_approach));
        }
    }

    for dep in &constraints.hidden_dependencies {
        standards.push(format!(
            "! {} -> {}: {}",
            dep.source, dep.target, dep.description
        ));
    }

    for gotcha in &constraints.gotchas {
        if let Some(first_file) = gotcha.related_files.first() {
            standards.push(format!(
                "! {}: {} (affects {})",
                gotcha.title, gotcha.solution, first_file
            ));
        } else {
            standards.push(format!("! {}: {}", gotcha.title, gotcha.solution));
        }
    }

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

        for insight in synth.deep.insights.iter().filter(|i| !i.gotchas.is_empty()) {
            for gotcha in &insight.gotchas {
                standards.push(format!("! {} ({})", gotcha, insight.file));
            }
        }
    }

    if let Some(insights) = cross_insights {
        for insight in &insights.tier2_insights {
            if insight.scope.is_empty() || insight.scope == "Project-wide" {
                standards.push(format!(
                    "- {}: {} - {}",
                    insight.category, insight.title, insight.description
                ));
            } else {
                standards.push(format!(
                    "- {}: {} - {} (scope: {})",
                    insight.category, insight.title, insight.description, insight.scope
                ));
            }
        }
    }

    standards
}

/// Generate the domain knowledge section
pub fn generate_domain_knowledge(domain: Option<&DomainAnalysisResult>) -> Option<String> {
    let domain = domain?;
    if domain.policies.is_empty()
        && domain.glossary.terms.is_empty()
        && domain.workflows.is_empty()
    {
        return None;
    }

    let mut content = String::new();

    if !domain.policies.is_empty() {
        content.push_str("### Core Policies\n\n");
        for policy in &domain.policies {
            content.push_str(&format!(
                "- **{}** ({}): {}\n",
                policy.name,
                policy.policy_type.to_string().to_lowercase(),
                policy.description
            ));
            if !policy.evidence.is_empty() {
                let ev = &policy.evidence[0];
                content.push_str(&format!("  - Evidence: @{}:{}\n", ev.file, ev.start_line));
            }
        }
        content.push('\n');
    }

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

    if !domain.glossary.terms.is_empty() {
        content.push_str("### Glossary\n\n");
        for term in &domain.glossary.terms {
            content.push_str(&format!("- **{}**: {}\n", term.term, term.definition));
        }
        content.push('\n');
    }

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

/// Generate the gotchas section
pub fn generate_gotchas(
    constraints: &ExtractedConstraints,
    cross_insights: Option<&SynthesizedInsights>,
) -> Vec<String> {
    let mut gotchas = Vec::new();

    if let Some(insights) = cross_insights {
        for insight in &insights.tier3_insights {
            gotchas.push(format!(
                "**{}**: {} -> {}",
                insight.title, insight.description, insight.prevention_guidance
            ));
        }

        for dep in &insights.hidden_dependencies {
            gotchas.push(format!(
                "**Hidden Dep**: {} -> {} ({}): {}",
                dep.from_module, dep.to_module, dep.dependency_type, dep.description
            ));
        }

        for violation in &insights.architecture_violations {
            gotchas.push(format!(
                "**Violation**: {} ({} -> {}): {}",
                violation.description,
                violation.from_layer,
                violation.to_layer,
                violation.suggested_fix
            ));
        }
    }

    for gotcha in &constraints.gotchas {
        let entry = format!(
            "**{}**: {} -> {}",
            gotcha.title, gotcha.description, gotcha.solution
        );
        if !gotchas.contains(&entry) {
            gotchas.push(entry);
        }
    }

    gotchas
}
