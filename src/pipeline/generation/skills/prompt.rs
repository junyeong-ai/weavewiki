//! Skill Prompt Builder
//!
//! Builds prompts for LLM skill generation with:
//! - Project identity and domain context
//! - All critical insights with full detail
//! - Prescriptive guidance with failure modes
//! - Evidence-based references
//!
//! Key principle: NEVER send empty sections. Always provide structural
//! context from EnrichedContext when analysis data is sparse.

use crate::config::ProjectType;
use crate::pipeline::analysis::cross_synthesis::{DiscoveredInsight, Tier3Insight};
use crate::pipeline::analysis::PatternInstance;
use crate::pipeline::generation::context::{BudgetedSections, EnrichedDomainKnowledge, FileContext};
use crate::pipeline::generation::context_enricher::EnrichedContext;
use crate::pipeline::evidence::artifact_ref;
use crate::pipeline::phases::few_shot::get_skill_example;
use crate::utils::normalize_concern_name;

pub struct SkillPromptBuilder {
    name: String,
    description: String,
    skill_focus: Option<String>,
    project_identity: Option<ProjectIdentity>,
    project_type: ProjectType,
    discovered_insights: Vec<DiscoveredInsightRef>,
    constraints_text: Option<String>,
    patterns: Vec<PatternRef>,
    files: Vec<FileContext>,
    enriched_domain: Option<EnrichedDomainKnowledge>,
    tools: Vec<String>,
    verified_refs: Vec<String>,
    argument_hint: Option<String>,
    enriched_context: Option<EnrichedContext>,
    budgeted: Option<BudgetedSections>,
    evidence_instructions: Option<String>,
    /// Monorepo context: workspace information
    monorepo_context: Option<MonorepoContext>,
    /// Workspace scope: for workspace-specific skills
    workspace_scope: Option<WorkspaceScope>,
}

/// Monorepo context for cross-workspace skills
#[derive(Debug, Clone)]
pub struct MonorepoContext {
    pub workspace_info: String,
    pub cross_dependencies: String,
}

/// Workspace scope for workspace-specific skills
#[derive(Debug, Clone)]
pub struct WorkspaceScope {
    pub scope_info: String,
    pub workspace_path: String,
}

#[derive(Debug, Clone)]
pub struct ProjectIdentity {
    pub domain_type: String,
    pub purpose: String,
    pub critical_concerns: Vec<String>,
}

impl Default for ProjectIdentity {
    fn default() -> Self {
        Self {
            domain_type: "Software".into(),
            purpose: "General purpose application".into(),
            critical_concerns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveredInsightRef {
    pub title: String,
    pub description: String,
    pub category: String,
    pub evidence: Vec<String>,
    pub prevention: String,
}

impl From<&DiscoveredInsight> for DiscoveredInsightRef {
    fn from(insight: &DiscoveredInsight) -> Self {
        Self {
            title: insight.title.clone(),
            description: insight.description.clone(),
            category: insight.category.clone(),
            evidence: insight
                .evidence
                .iter()
                .map(|e| artifact_ref(&e.file, e.start_line))
                .collect(),
            prevention: insight.prevention_guidance.clone(),
        }
    }
}

impl From<&Tier3Insight> for DiscoveredInsightRef {
    fn from(insight: &Tier3Insight) -> Self {
        Self {
            title: insight.title.clone(),
            description: insight.description.clone(),
            category: insight.category.to_string(),
            evidence: insight
                .evidence
                .iter()
                .map(|e| artifact_ref(&e.file, e.start_line))
                .collect(),
            prevention: insight.prevention_guidance.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PatternRef {
    pub category: String,
    pub description: String,
    pub locations: Vec<String>,
}

impl From<&PatternInstance> for PatternRef {
    fn from(p: &PatternInstance) -> Self {
        Self {
            category: p.category.to_string(),
            description: p.description.clone(),
            locations: p
                .locations
                .iter()
                .map(|l| artifact_ref(&l.file, l.line))
                .collect(),
        }
    }
}

impl SkillPromptBuilder {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            skill_focus: None,
            project_identity: None,
            project_type: ProjectType::Auto,
            discovered_insights: Vec::new(),
            constraints_text: None,
            patterns: Vec::new(),
            files: Vec::new(),
            enriched_domain: None,
            tools: Vec::new(),
            verified_refs: Vec::new(),
            argument_hint: None,
            enriched_context: None,
            budgeted: None,
            evidence_instructions: None,
            monorepo_context: None,
            workspace_scope: None,
        }
    }

    /// Add monorepo context for cross-workspace skills
    pub fn monorepo_context(
        mut self,
        workspace_info: String,
        cross_dependencies: String,
    ) -> Self {
        self.monorepo_context = Some(MonorepoContext {
            workspace_info,
            cross_dependencies,
        });
        self
    }

    /// Add workspace scope for workspace-specific skills
    pub fn workspace_scope(mut self, scope_info: String, workspace_path: String) -> Self {
        self.workspace_scope = Some(WorkspaceScope {
            scope_info,
            workspace_path,
        });
        self
    }

    pub fn enriched_context(mut self, ctx: EnrichedContext) -> Self {
        self.enriched_context = Some(ctx);
        self
    }

    pub fn skill_focus(mut self, focus: impl Into<String>) -> Self {
        self.skill_focus = Some(focus.into());
        self
    }

    pub fn project_type(mut self, project_type: ProjectType) -> Self {
        self.project_type = project_type;
        self
    }

    pub fn project_identity(mut self, identity: ProjectIdentity) -> Self {
        self.project_identity = Some(identity);
        self
    }

    pub fn verified_refs(mut self, refs: Vec<String>) -> Self {
        self.verified_refs = refs;
        self
    }

    pub fn argument_hint(mut self, hint: Option<&str>) -> Self {
        self.argument_hint = hint.map(String::from);
        self
    }

    pub fn discovered_insights(mut self, insights: Vec<&Tier3Insight>) -> Self {
        self.discovered_insights = insights.into_iter().map(DiscoveredInsightRef::from).collect();
        self
    }

    pub fn constraints_text(mut self, text: String) -> Self {
        self.constraints_text = Some(text);
        self
    }

    pub fn patterns(mut self, patterns: Vec<&PatternInstance>) -> Self {
        self.patterns = patterns.into_iter().map(PatternRef::from).collect();
        self
    }

    pub fn files(mut self, files: Vec<FileContext>) -> Self {
        self.files = files;
        self
    }

    pub fn enriched_domain(mut self, domain: Option<EnrichedDomainKnowledge>) -> Self {
        self.enriched_domain = domain;
        self
    }

    pub fn tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }

    pub fn evidence_instructions(mut self, instructions: String) -> Self {
        self.evidence_instructions = Some(instructions);
        self
    }

    pub fn build(&self) -> String {
        let few_shot_example = get_skill_example(self.project_type);
        let confidence_section = self.format_confidence();
        let structural_section = self.format_structural();
        let ast_section = self.format_ast();
        let monorepo_section = self.format_monorepo_context();
        let workspace_section = self.format_workspace_scope();

        format!(
            r#"Generate a project-specific skill for Claude Code.

## EXAMPLE OF A HIGH-QUALITY SKILL
{few_shot_example}

---

## PROJECT IDENTITY
{project_identity}

## ANALYSIS CONFIDENCE
{confidence_section}

## SKILL SPECIFICATION
Name: {name}
Purpose: {description}
Focus: {focus}
Tools: {tools}
{workspace_section}

## PROJECT STRUCTURE (verified - always available)
{structural_section}

## CODE FACTS (from AST - ground truth)
{ast_section}
{monorepo_section}

## VERIFIED REFERENCES (use these for @file:line citations)
{verified_refs}

## CRITICAL INSIGHTS TO ADDRESS
{priority_insights}

## PROJECT CONSTRAINTS
{constraints_section}

## DETECTED PATTERNS
{patterns_section}

## KEY FILES
{files_section}

## DOMAIN CONTEXT
{domain_section}

---

## SKILL GENERATION REQUIREMENTS

### Structure
```markdown
# {name}

## Overview
[What this skill does, when to use it]

## Prerequisites
[Required setup, dependencies to check]

## Process
1. [Step with @file:line reference]
2. [Step with specific instruction]
...

## Gotchas
- [Gotcha from insights with prevention]
- [Pattern to follow with evidence]

## Failure Modes
- WITHOUT [action]: [consequence]
- IF [condition]: [what happens]
```

### Content Rules

1. **PRESCRIPTIVE**: Use "DO X" not "Consider X"
   - "Consider checking error handling" -> wrong
   - "CHECK error handling at @src/api/error.rs:42. WITHOUT this, errors propagate silently." -> correct

2. **EVIDENCE-BASED**: Every claim needs a reference
   - "Follow the project patterns" -> wrong
   - "Follow async pattern at @src/handlers/mod.rs:15" -> correct

3. **FAILURE MODES**: Explain consequences
   - "Use Arc for providers" -> wrong
   - "WRAP providers with Arc::new(). FAILURE: Thread-unsafe sharing causes runtime panics." -> correct

4. **PROJECT-SPECIFIC**: Reference THIS project
   - "Use standard error handling" -> wrong
   - "Use AppError from @src/types/error.rs:28 for all error returns" -> correct

{evidence_guidelines}

{argument_instruction}

Return ONLY the skill body starting with # {name}."#,
            few_shot_example = few_shot_example,
            project_identity = self.format_project_identity(),
            confidence_section = confidence_section,
            name = self.name,
            description = self.description,
            focus = self.skill_focus.as_deref().unwrap_or("General"),
            tools = self.format_tools(),
            workspace_section = workspace_section,
            structural_section = structural_section,
            ast_section = ast_section,
            monorepo_section = monorepo_section,
            verified_refs = self.format_verified_refs(),
            priority_insights = self.format_priority_insights(),
            constraints_section = self.format_constraints(),
            patterns_section = self.format_patterns(),
            files_section = self.format_files(),
            domain_section = self.format_domain(),
            evidence_guidelines = self.format_evidence_guidelines(),
            argument_instruction = self.format_argument_instruction(),
        )
    }

    fn format_monorepo_context(&self) -> String {
        match &self.monorepo_context {
            Some(ctx) => {
                let mut sections = Vec::new();
                if !ctx.workspace_info.is_empty() {
                    sections.push(format!("\n## MONOREPO WORKSPACES\n{}", ctx.workspace_info));
                }
                if !ctx.cross_dependencies.is_empty() {
                    sections.push(ctx.cross_dependencies.clone());
                }
                sections.join("\n\n")
            }
            None => String::new(),
        }
    }

    fn format_workspace_scope(&self) -> String {
        match &self.workspace_scope {
            Some(scope) => {
                format!(
                    "\n## WORKSPACE SCOPE\nThis skill is scoped to workspace: `{}`\n\n{}\n\nAll file references should be relative to this workspace path.",
                    scope.workspace_path,
                    scope.scope_info
                )
            }
            None => String::new(),
        }
    }

    fn format_confidence(&self) -> String {
        match &self.enriched_context {
            Some(ctx) => ctx.format_confidence_section(),
            None => "[MEDIUM] No explicit confidence computed.\nUse available evidence when possible.".into(),
        }
    }

    fn format_structural(&self) -> String {
        match &self.enriched_context {
            Some(ctx) => ctx.format_structural_section(),
            None => "Structural context not available.".into(),
        }
    }

    fn format_ast(&self) -> String {
        match &self.enriched_context {
            Some(ctx) => ctx.format_ast_section(),
            None => "AST analysis not available.".into(),
        }
    }

    fn format_project_identity(&self) -> String {
        match &self.project_identity {
            Some(id) => {
                let concerns = if id.critical_concerns.is_empty() {
                    "None specified".to_string()
                } else {
                    id.critical_concerns.join(", ")
                };
                format!(
                    "Domain: {}\nPurpose: {}\nCritical Concerns: {}",
                    id.domain_type, id.purpose, concerns
                )
            }
            None => self.infer_project_identity(),
        }
    }

    fn infer_project_identity(&self) -> String {
        let domain = self.infer_domain_type();
        let concerns = self.infer_critical_concerns();
        format!(
            "Domain: {}\nPurpose: (Inferred from analysis)\nCritical Concerns: {}",
            domain,
            if concerns.is_empty() {
                "Standard software quality".to_string()
            } else {
                concerns.join(", ")
            }
        )
    }

    fn infer_domain_type(&self) -> String {
        if let Some(d) = &self.enriched_domain {
            if let Some(ref domain_type) = d.domain_type {
                return domain_type.clone();
            }
            if let Some(domain) = d.infer_domain_from_policies() {
                return domain.into();
            }
        }
        for insight in &self.discovered_insights {
            if insight.category.contains("security") {
                return "Security-Critical System".into();
            }
        }
        "Software System".into()
    }

    fn infer_critical_concerns(&self) -> Vec<String> {
        let mut concerns = Vec::new();
        for insight in &self.discovered_insights {
            let concern = Self::category_to_concern(&insight.category);
            if !concern.is_empty() && !concerns.contains(&concern) {
                concerns.push(concern);
            }
        }
        concerns
    }

    fn category_to_concern(category: &str) -> String {
        let cat_lower = category.to_lowercase();
        if cat_lower.contains("security") {
            "Security".into()
        } else if cat_lower.contains("concurrency") || cat_lower.contains("thread") {
            "Concurrency Safety".into()
        } else if cat_lower.contains("performance") {
            "Performance".into()
        } else if cat_lower.contains("resource") || cat_lower.contains("leak") {
            "Resource Management".into()
        } else if cat_lower.contains("order") || cat_lower.contains("initialization") {
            "Initialization Order".into()
        } else if cat_lower.contains("dependency") {
            "Hidden Dependencies".into()
        } else if cat_lower.contains("state") || cat_lower.contains("invariant") {
            "State Invariants".into()
        } else {
            normalize_concern_name(category)
        }
    }

    fn format_tools(&self) -> String {
        if self.tools.is_empty() {
            "Read, Grep, Glob".to_string()
        } else {
            self.tools.join(", ")
        }
    }

    fn format_verified_refs(&self) -> String {
        if self.verified_refs.is_empty() {
            return "No pre-verified references. When adding references:\n\
                    - Use @file:line format for specific code locations\n\
                    - Reference actual files from PROJECT STRUCTURE\n\
                    - Include line numbers when citing specific patterns".into();
        }
        self.verified_refs
            .iter()
            .map(|r| format!("- {}", r))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_priority_insights(&self) -> String {
        // Use budgeted tier2 insights if available
        if let Some(budgeted) = &self.budgeted
            && !budgeted.tier2.discovered_insights.is_empty()
        {
            return budgeted.tier2.discovered_insights.clone();
        }
        if self.discovered_insights.is_empty() {
            return self.build_insights_from_structure();
        }

        self.discovered_insights
            .iter()
            .enumerate()
            .map(|(i, insight)| {
                let category = normalize_concern_name(&insight.category).to_uppercase();
                let evidence = if insight.evidence.is_empty() {
                    String::new()
                } else {
                    format!("\n   Evidence: {}", insight.evidence.join(", "))
                };
                format!(
                    "{}. [{category}] {title}\n   {description}{evidence}\n   → Action: {prevention}",
                    i + 1,
                    category = category,
                    title = insight.title,
                    description = insight.description,
                    evidence = evidence,
                    prevention = insight.prevention
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn format_constraints(&self) -> String {
        // Use budgeted tier1 constraints if available
        if let Some(budgeted) = &self.budgeted
            && !budgeted.tier1.constraints.is_empty()
        {
            return budgeted.tier1.constraints.clone();
        }
        if let Some(text) = self.constraints_text.as_ref().filter(|t| !t.is_empty()) {
            return text.clone();
        }
        self.build_constraints_from_structure()
    }

    fn build_constraints_from_structure(&self) -> String {
        let mut constraints = Vec::new();

        if let Some(ctx) = &self.enriched_context {
            if let Some(ast) = &ctx.ast {
                // Error handling patterns
                if ast.dominant_patterns.iter().any(|p| p.contains("Result")) {
                    constraints.push("Uses Result<T, E> error handling - propagate errors, don't unwrap");
                }
                if ast.dominant_patterns.iter().any(|p| p.contains("async")) {
                    constraints.push("Async codebase - use async/await patterns consistently");
                }
            }

            // Module structure constraints
            let module_count = ctx.structural.modules.total;
            if module_count > 5 {
                constraints.push("Multi-module architecture - respect module boundaries");
            }
        }

        if constraints.is_empty() {
            "Use PROJECT STRUCTURE and CODE FACTS to identify project-specific constraints.".into()
        } else {
            format!("Inferred constraints:\n- {}", constraints.join("\n- "))
        }
    }

    fn format_patterns(&self) -> String {
        // Use budgeted tier2 patterns if available
        if let Some(budgeted) = &self.budgeted
            && !budgeted.tier2.patterns.is_empty()
        {
            return budgeted.tier2.patterns.clone();
        }
        if self.patterns.is_empty() {
            if let Some(ctx) = &self.enriched_context
                && let Some(ast) = &ctx.ast
                && !ast.dominant_patterns.is_empty()
            {
                return format!(
                    "Detected from code structure:\n{}",
                    ast.dominant_patterns
                        .iter()
                        .map(|p| format!("- {}", p))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
            return "Limited pattern detection. Identify patterns by examining:\n\
                    - Repeated structural motifs in CODE FACTS\n\
                    - Naming conventions (Builder, Factory, Handler suffixes)\n\
                    - Import/dependency patterns across modules".into();
        }
        self.patterns
            .iter()
            .map(|p| {
                let evidence = if p.locations.is_empty() {
                    String::new()
                } else {
                    format!("\n  See: {}", p.locations.join(", "))
                };
                format!("- **{}**: {}{}", p.category, p.description, evidence)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_files(&self) -> String {
        if self.files.is_empty() {
            // Use enriched context entry points
            if let Some(ctx) = &self.enriched_context
                && !ctx.structural.entry_points.is_empty()
            {
                return format!(
                    "From PROJECT STRUCTURE entry points:\n{}",
                    ctx.structural
                        .entry_points
                        .iter()
                        .map(|ep| format!("- @{} ({})", ep.path, ep.kind))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
            return "Use files from PROJECT STRUCTURE section.".into();
        }
        self.files
            .iter()
            .map(|f| {
                if f.abstractions.is_empty() {
                    format!("- @{}", f.path)
                } else {
                    format!("- @{}: {}", f.path, f.abstractions.join(", "))
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_domain(&self) -> String {
        // Use budgeted tier3 domain if available
        if let Some(budgeted) = &self.budgeted
            && !budgeted.tier3.domain_knowledge.is_empty()
        {
            return budgeted.tier3.domain_knowledge.clone();
        }
        match &self.enriched_domain {
            Some(d) => {
                let mut sections = Vec::new();

                if !d.policies.is_empty() {
                    let policies: Vec<_> = d
                        .policies
                        .iter()
                        .map(|p| {
                            let modules = if p.affected_modules.is_empty() {
                                String::new()
                            } else {
                                format!(" ({})", p.affected_modules.join(", "))
                            };
                            let evidence = if p.evidence.is_empty() {
                                String::new()
                            } else {
                                format!("\n    Evidence: {}", p.evidence.join(", "))
                            };
                            format!(
                                "- [{}/{}] **{}**: {}{}{}",
                                p.policy_type, p.enforcement, p.name, p.description, modules, evidence
                            )
                        })
                        .collect();
                    sections.push(format!("**Policies**\n{}", policies.join("\n")));
                }

                if !d.core_logic.is_empty() {
                    let logic: Vec<_> = d
                        .core_logic
                        .iter()
                        .map(|l| {
                            let deps = if l.dependencies.is_empty() {
                                String::new()
                            } else {
                                format!("\n    Deps: {}", l.dependencies.join(", "))
                            };
                            let impact = if l.business_impact.is_empty() {
                                String::new()
                            } else {
                                format!("\n    Impact: {}", l.business_impact)
                            };
                            format!(
                                "- [{}] **{}**: {} ({}){}{}",
                                l.logic_type, l.name, l.description, l.location, deps, impact
                            )
                        })
                        .collect();
                    sections.push(format!("**Core Logic**\n{}", logic.join("\n")));
                }

                if !d.workflows.is_empty() {
                    let workflows: Vec<_> = d
                        .workflows
                        .iter()
                        .map(|w| {
                            let modules = if w.involved_modules.is_empty() {
                                String::new()
                            } else {
                                format!("\n    Modules: {}", w.involved_modules.join(", "))
                            };
                            let triggers = if w.triggers.is_empty() {
                                String::new()
                            } else {
                                format!("\n    Triggers: {}", w.triggers.join(", "))
                            };
                            let entries = if w.entry_points.is_empty() {
                                String::new()
                            } else {
                                format!("\n    Entry: {}", w.entry_points.join(", "))
                            };
                            format!(
                                "- **{}**: {} ({} steps){}{}{}",
                                w.name, w.description, w.step_count, modules, triggers, entries
                            )
                        })
                        .collect();
                    sections.push(format!("**Workflows**\n{}", workflows.join("\n")));
                }

                if !d.terminology.is_empty() {
                    sections.push(format!("**Domain Terms**: {}", d.terminology.join(", ")));
                }

                if sections.is_empty() {
                    self.infer_domain_from_structure()
                } else {
                    sections.join("\n\n")
                }
            }
            None => self.infer_domain_from_structure(),
        }
    }

    fn build_insights_from_structure(&self) -> String {
        let mut insights = Vec::new();

        if let Some(ctx) = &self.enriched_context {
            // Entry points - include all
            if !ctx.structural.entry_points.is_empty() {
                let entries: Vec<_> = ctx
                    .structural
                    .entry_points
                    .iter()
                    .map(|e| format!("@{} [{}]", e.path, e.kind))
                    .collect();
                insights.push(format!("Entry points ({}): {}", entries.len(), entries.join(", ")));
            }

            // Core modules - include all core
            let core_modules: Vec<_> = ctx
                .structural
                .modules
                .iter()
                .filter(|m| m.is_core)
                .map(|m| m.name.clone())
                .collect();
            if !core_modules.is_empty() {
                insights.push(format!("Core modules ({}): {}", core_modules.len(), core_modules.join(", ")));
            }

            // Key types from AST - include all
            if let Some(ast) = &ctx.ast
                && !ast.key_types.is_empty()
            {
                let types: Vec<_> = ast
                    .key_types
                    .iter()
                    .map(|t| format!("{}@{}:{}", t.name, t.file, t.line))
                    .collect();
                insights.push(format!("Key types ({}): {}", types.len(), types.join(", ")));
            }
        }

        if insights.is_empty() {
            "Reference CODE FACTS and PROJECT STRUCTURE above for project-specific guidance.".into()
        } else {
            format!(
                "Structural context (use for project-specific guidance):\n- {}",
                insights.join("\n- ")
            )
        }
    }

    fn infer_domain_from_structure(&self) -> String {
        if let Some(ctx) = &self.enriched_context {
            let mut inferred = Vec::new();

            // Infer from module names
            let domain_modules: Vec<_> = ctx
                .structural
                .modules
                .iter()
                .filter(|m| {
                    let name = m.name.to_lowercase();
                    name.contains("auth")
                        || name.contains("payment")
                        || name.contains("order")
                        || name.contains("user")
                        || name.contains("api")
                        || name.contains("domain")
                })
                .map(|m| format!("{} ({} files)", m.name, m.file_count))
                .collect();

            if !domain_modules.is_empty() {
                inferred.push(format!("**Inferred Domain Modules**: {}", domain_modules.join(", ")));
            }

            // Infer from language
            inferred.push(format!(
                "**Primary Language**: {}",
                ctx.structural.primary_language
            ));

            if !inferred.is_empty() {
                return format!(
                    "Domain analysis unavailable. Inferred from structure:\n{}",
                    inferred.join("\n")
                );
            }
        }

        "Domain context unavailable. Generate based on file structure and CODE FACTS.".into()
    }

    fn format_evidence_guidelines(&self) -> &str {
        const DEFAULT_GUIDELINES: &str = "\
### Reference Guidelines
- Use @file:line from VERIFIED REFERENCES section when available
- Use @file only when line number not verified
- DO NOT invent line numbers";

        match &self.evidence_instructions {
            Some(instructions) => instructions.as_str(),
            None => DEFAULT_GUIDELINES,
        }
    }

    fn format_argument_instruction(&self) -> String {
        match &self.argument_hint {
            Some(hint) => format!(
                "\nInclude \"## Input\" section with $ARGUMENTS placeholder (accepts: {})",
                hint
            ),
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EvidenceLocation;

    #[test]
    fn test_builder_basic() {
        let prompt = SkillPromptBuilder::new("code-review", "Systematic code review")
            .constraints_text("Always check error handling".into())
            .build();

        assert!(prompt.contains("code-review"));
        assert!(prompt.contains("Always check error handling"));
    }

    #[test]
    fn test_builder_with_discovered_insight() {
        use crate::pipeline::analysis::cross_synthesis::Tier3Category;
        let insight = Tier3Insight {
            title: "Provider must be Arc-wrapped".into(),
            description: "LlmProvider requires Arc for thread-safe sharing".into(),
            category: Tier3Category::ConcurrencyTrap,
            evidence: vec![EvidenceLocation {
                file: "src/ai/provider.rs".into(),
                start_line: 42,
                end_line: 42,
                start_column: None,
                end_column: None,
            }],
            prevention_guidance: "Always wrap with Arc::new()".into(),
        };

        let prompt = SkillPromptBuilder::new("implement", "Feature implementation")
            .discovered_insights(vec![&insight])
            .build();

        assert!(prompt.contains("CONCURRENCYTRAP"));
        assert!(prompt.contains("Provider must be Arc-wrapped"));
        assert!(prompt.contains("@src/ai/provider.rs:42"));
    }

    #[test]
    fn test_builder_with_project_identity() {
        let identity = ProjectIdentity {
            domain_type: "FinTech".into(),
            purpose: "Payment processing system".into(),
            critical_concerns: vec!["Security".into(), "Compliance".into()],
        };

        let prompt = SkillPromptBuilder::new("code-review", "Code review")
            .project_identity(identity)
            .build();

        assert!(prompt.contains("FinTech"));
        assert!(prompt.contains("Payment processing"));
        assert!(prompt.contains("Security"));
    }

    #[test]
    fn test_all_insights_shown() {
        use crate::pipeline::analysis::cross_synthesis::Tier3Category;
        let insights: Vec<Tier3Insight> = (0..5)
            .map(|i| Tier3Insight {
                title: format!("Insight {}", i),
                description: format!("Description {}", i),
                category: Tier3Category::HiddenDependency,
                evidence: vec![],
                prevention_guidance: format!("Prevention {}", i),
            })
            .collect();

        let refs: Vec<&Tier3Insight> = insights.iter().collect();
        let prompt = SkillPromptBuilder::new("test", "test")
            .discovered_insights(refs)
            .build();

        for i in 0..5 {
            assert!(prompt.contains(&format!("Insight {}", i)));
            assert!(prompt.contains(&format!("Prevention {}", i)));
        }
    }

    #[test]
    fn test_category_normalization() {
        // Tests normalize_concern_name utility used for category display
        assert_eq!(normalize_concern_name("hidden-dependency"), "Hidden Dependency");
        assert_eq!(normalize_concern_name("concurrency_trap"), "Concurrency Trap");
    }
}
