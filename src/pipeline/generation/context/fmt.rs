use super::*;
use crate::pipeline::analysis::PatternInstance;
use crate::pipeline::evidence::{artifact_ref, artifact_ref_opt};
use crate::utils::normalize_concern_name;

impl<'a> GenerationContext<'a> {
    pub fn format_patterns(&self, patterns: &[&PatternInstance]) -> String {
        if patterns.is_empty() {
            return String::new();
        }
        patterns
            .iter()
            .map(|p| {
                let locations: Vec<_> = p
                    .locations
                    .iter()
                    .map(|l| artifact_ref(&l.file, l.line))
                    .collect();
                format!(
                    "- **{:?}**: {}\n  Evidence: {}",
                    p.category,
                    p.description,
                    locations.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn format_domain(&self, domain: &Option<DomainKnowledge>) -> String {
        match domain {
            Some(d) => {
                let mut parts = Vec::new();
                if !d.policies.is_empty() {
                    parts.push(format!("**Policies**: {}", d.policies.join("; ")));
                }
                if !d.core_logic.is_empty() {
                    parts.push(format!("**Core Logic**: {}", d.core_logic.join("; ")));
                }
                if !d.terminology.is_empty() {
                    parts.push(format!("**Terms**: {}", d.terminology.join(", ")));
                }
                if parts.is_empty() {
                    String::new()
                } else {
                    parts.join("\n")
                }
            }
            None => String::new(),
        }
    }

    pub fn format_enriched_domain(&self) -> String {
        let enriched = match self.enriched_domain_knowledge() {
            Some(d) => d,
            None => return String::new(),
        };

        let mut sections = Vec::new();

        if !enriched.policies.is_empty() {
            let policies: Vec<_> = enriched
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
                        "- **[{}/{}] {}**: {}{}{}",
                        p.policy_type,
                        p.enforcement,
                        p.name,
                        p.description,
                        modules,
                        evidence
                    )
                })
                .collect();
            sections.push(format!("### Policies\n{}", policies.join("\n")));
        }

        if !enriched.core_logic.is_empty() {
            let logic: Vec<_> = enriched
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
                        "- **[{}] {}**: {} ({}){}{}",
                        l.logic_type,
                        l.name,
                        l.description,
                        l.location,
                        deps,
                        impact
                    )
                })
                .collect();
            sections.push(format!("### Core Logic\n{}", logic.join("\n")));
        }

        if !enriched.workflows.is_empty() {
            let workflows: Vec<_> = enriched
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
                        w.name,
                        w.description,
                        w.step_count,
                        modules,
                        triggers,
                        entries
                    )
                })
                .collect();
            sections.push(format!("### Workflows\n{}", workflows.join("\n")));
        }

        if !enriched.terminology.is_empty() {
            sections.push(format!("### Domain Terms\n{}", enriched.terminology.join(", ")));
        }

        sections.join("\n\n")
    }

    pub fn format_discovered_insights(&self) -> String {
        self.format_discovered_insights_limited(usize::MAX)
    }

    pub fn format_discovered_insights_limited(&self, limit: usize) -> String {
        let insights = self.all_discovered_insights();
        if insights.is_empty() {
            return String::new();
        }

        let limited: Vec<_> = insights.into_iter().take(limit).collect();
        limited
            .iter()
            .map(|i| {
                let evidence: Vec<_> = i
                    .evidence
                    .iter()
                    .map(|e| artifact_ref(&e.file, e.start_line))
                    .collect();
                format!(
                    "### [{:?}] {}\n{}\n**Prevention**: {}\nEvidence: {}",
                    i.category,
                    i.title,
                    i.description,
                    i.prevention_guidance,
                    evidence.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn format_architecture_violations(&self) -> String {
        let violations = self.all_architecture_violations();
        if violations.is_empty() {
            return String::new();
        }

        violations
            .iter()
            .map(|v| {
                let evidence: Vec<_> = v
                    .evidence
                    .iter()
                    .map(|e| artifact_ref(&e.file, e.start_line))
                    .collect();
                format!(
                    "- **[{}] {} → {}**: {}\n  Evidence: {}\n  Fix: {}",
                    v.violation_type,
                    v.from_layer,
                    v.to_layer,
                    v.description,
                    evidence.join(", "),
                    v.suggested_fix
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn format_cross_constraints(&self) -> String {
        let constraints = self.all_cross_constraints();
        if constraints.is_empty() {
            return String::new();
        }

        constraints
            .iter()
            .map(|c| {
                let modules = c.affected_modules.join(", ");
                format!(
                    "- **[{}] {}**: {} ({})\n  Enforcement: {}",
                    c.constraint_type, c.name, c.description, modules, c.enforcement
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn format_constraints(&self) -> String {
        let mut sections = Vec::new();

        // Gotchas with full context
        if !self.constraints.gotchas.is_empty() {
            sections.push("### Gotchas".to_string());
            for g in &self.constraints.gotchas {
                let files: Vec<_> = g.related_files.iter().map(|f| format!("@{}", f)).collect();
                let mut entry = format!("- **{}**: {}", g.title, g.description);
                if !g.when.is_empty() {
                    entry.push_str(&format!("\n  When: {}", g.when));
                }
                if !g.solution.is_empty() {
                    entry.push_str(&format!("\n  Solution: {}", g.solution));
                }
                if !files.is_empty() {
                    entry.push_str(&format!("\n  Files: {}", files.join(", ")));
                }
                sections.push(entry);
            }
        }

        // Hidden dependencies with type and impact
        if !self.constraints.hidden_dependencies.is_empty() {
            sections.push("### Hidden Dependencies".to_string());
            for hd in &self.constraints.hidden_dependencies {
                let evidence: Vec<_> = hd
                    .evidence
                    .iter()
                    .map(|e| artifact_ref_opt(&e.file, e.line))
                    .collect();
                let mut entry = format!(
                    "- **{} → {}** [{}]: {}",
                    hd.source, hd.target, hd.dependency_type, hd.description
                );
                if !hd.impact.is_empty() {
                    entry.push_str(&format!("\n  Impact: {}", hd.impact));
                }
                if !evidence.is_empty() {
                    entry.push_str(&format!("\n  Evidence: {}", evidence.join(", ")));
                }
                sections.push(entry);
            }
        }

        // Anti-patterns with severity and reason
        if !self.constraints.anti_patterns.is_empty() {
            sections.push("### Anti-Patterns".to_string());
            for ap in &self.constraints.anti_patterns {
                let evidence: Vec<_> = ap
                    .evidence
                    .iter()
                    .map(|e| artifact_ref_opt(&e.file, e.line))
                    .collect();
                let mut entry = format!("- **[{}] {}**: {}", ap.severity, ap.name, ap.description);
                if !ap.why_bad.is_empty() {
                    entry.push_str(&format!("\n  Why bad: {}", ap.why_bad));
                }
                entry.push_str(&format!("\n  Instead: {}", ap.correct_approach));
                if !evidence.is_empty() {
                    entry.push_str(&format!("\n  Evidence: {}", evidence.join(", ")));
                }
                sections.push(entry);
            }
        }

        sections.join("\n\n")
    }

    pub fn format_conventions(&self) -> String {
        let conv = &self.conventions;
        let mut parts = Vec::new();

        // Architecture
        if !conv.architecture.pattern_name.is_empty() {
            parts.push(format!(
                "### Architecture\n{}: {}",
                conv.architecture.pattern_name, conv.architecture.description
            ));
        }

        // Naming conventions summary
        let naming = &conv.naming;
        let mut naming_lines = Vec::new();
        if !naming.file_naming.examples.is_empty() {
            naming_lines.push(format!(
                "- Files: {:?} (e.g., {})",
                naming.file_naming.case,
                naming.file_naming.examples.join(", ")
            ));
        }
        if !naming.function_naming.verb_prefixes.is_empty() {
            naming_lines.push(format!(
                "- Functions: verb prefixes [{}]",
                naming.function_naming.verb_prefixes.join(", ")
            ));
        }
        if !naming_lines.is_empty() {
            parts.push(format!("### Naming\n{}", naming_lines.join("\n")));
        }

        // Code patterns
        if !conv.patterns.is_empty() {
            let pattern_lines: Vec<_> = conv
                .patterns
                .iter()
                .map(|p| format!("- **{}**: {}", p.name, p.description))
                .collect();
            parts.push(format!("### Patterns\n{}", pattern_lines.join("\n")));
        }

        // Error handling
        if !conv.error_handling.error_types.is_empty() {
            parts.push(format!(
                "### Error Handling\nStyle: {}\nTypes: {}",
                conv.error_handling.style,
                conv.error_handling.error_types.join(", ")
            ));
        }

        parts.join("\n\n")
    }

    pub fn format_module_summaries(&self, limit: usize) -> String {
        let summaries = self.module_summaries();
        if summaries.is_empty() {
            return String::new();
        }

        summaries
            .iter()
            .take(limit)
            .map(|s| {
                let patterns: Vec<_> = s
                    .patterns
                    .iter()
                    .map(|p| format!("  - {}: {}", p.name, p.description))
                    .collect();
                let pattern_text = if patterns.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", patterns.join("\n"))
                };
                format!(
                    "### {} ({}){}\nFiles: {}",
                    s.module_path,
                    s.responsibility,
                    pattern_text,
                    s.file_count,
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Group-level summaries: aggregate modules by their parent group.
    /// Each group summary references its constituent modules for drill-down.
    pub fn format_group_summaries(&self) -> String {
        if self.groups.is_empty() {
            return self.format_module_summaries(usize::MAX);
        }

        let summaries = self.module_summaries();
        let mut lines = Vec::new();

        for group in self.groups {
            let member_summaries: Vec<_> = summaries
                .iter()
                .filter(|s| group.module_ids.iter().any(|id| s.module_path.contains(id)))
                .collect();

            let total_files: usize = member_summaries.iter().map(|s| s.file_count).sum();
            let module_names: Vec<_> = member_summaries
                .iter()
                .map(|s| s.module_path.as_str())
                .collect();

            let mut entry = format!(
                "### {} ({})\nModules: {} | Files: {}",
                group.name,
                group.responsibility,
                module_names.join(", "),
                total_files,
            );

            if !group.boundary_rules.is_empty() {
                entry.push_str(&format!(
                    "\nBoundary: {}",
                    group.boundary_rules.join("; ")
                ));
            }

            lines.push(entry);
        }

        // Include ungrouped modules
        let grouped_ids: Vec<_> = self
            .groups
            .iter()
            .flat_map(|g| g.module_ids.iter())
            .collect();
        let ungrouped: Vec<_> = summaries
            .iter()
            .filter(|s| {
                !grouped_ids
                    .iter()
                    .any(|id| s.module_path.contains(id.as_str()))
            })
            .collect();
        if !ungrouped.is_empty() {
            let names: Vec<_> = ungrouped.iter().map(|s| s.module_path.as_str()).collect();
            let total_files: usize = ungrouped.iter().map(|s| s.file_count).sum();
            lines.push(format!(
                "### Other\nModules: {} | Files: {}",
                names.join(", "),
                total_files,
            ));
        }

        lines.join("\n\n")
    }

    /// Project-level summary: one line per domain/group for extreme budget constraints.
    /// References groups for drill-down.
    pub fn format_project_summary(&self) -> String {
        let summaries = self.module_summaries();
        let total_modules = summaries.len();
        let total_files: usize = summaries.iter().map(|s| s.file_count).sum();

        let mut lines = vec![format!(
            "Project: {} ({} modules, {} files)",
            self.project_name, total_modules, total_files,
        )];

        if !self.groups.is_empty() {
            for group in self.groups {
                lines.push(format!(
                    "- {}: {} ({} modules)",
                    group.name,
                    group.responsibility,
                    group.module_ids.len(),
                ));
            }
        } else if !self.domains.is_empty() {
            for domain in self.domains {
                lines.push(format!(
                    "- {}: {} ({} groups)",
                    domain.name,
                    domain.responsibility,
                    domain.group_ids.len(),
                ));
            }
        }

        lines.join("\n")
    }

    pub fn build_system_prompt(&self) -> String {
        let domain = self.infer_domain();
        let tech_stack = self.format_tech_stack();
        let concerns = self.infer_critical_concerns();

        format!(
            r#"You are a domain expert in {domain} systems with deep knowledge of:
- Tech Stack: {tech_stack}
- Key Concerns: {concerns}

Your role is to generate documentation that:
1. Prevents real bugs and incidents specific to THIS codebase
2. Captures institutional knowledge that new developers need
3. Highlights non-obvious gotchas learned from experience
4. References ACTUAL code locations from verified references

Quality over quantity. Depth over breadth. Specific over generic.
Generate content that would be valuable for a senior developer joining this project."#
        )
    }

    fn infer_domain(&self) -> String {
        if let Some(enriched) = self.enriched_domain_knowledge() {
            if let Some(ref domain_type) = enriched.domain_type {
                return domain_type.clone();
            }
            if let Some(domain) = enriched.infer_domain_from_policies() {
                return domain.into();
            }
        }

        if let Some(cross) = self.cross_insights {
            let has_security_critical = cross
                .tier3_insights
                .iter()
                .any(|t| {
                    matches!(
                        t.category,
                        crate::pipeline::analysis::Tier3Category::SecurityBoundary
                    )
                });
            if has_security_critical {
                return "Security-Critical System".into();
            }
        }

        let project_type = self.detection.primary_type.as_str();
        match project_type {
            "cli" => "CLI Tool".into(),
            "library" => "Library/SDK".into(),
            "backend" | "api" => "Backend Service".into(),
            "frontend" => "Frontend Application".into(),
            "monorepo" => "Monorepo Platform".into(),
            _ => format!("{} System", self.detection.primary_type),
        }
    }

    fn format_tech_stack(&self) -> String {
        let mut parts = vec![self.tech_stack.primary_language.clone()];

        let frameworks: Vec<_> = self
            .tech_stack
            .frameworks
            .iter()
            .map(|f| f.name.clone())
            .collect();
        if !frameworks.is_empty() {
            parts.push(format!("({})", frameworks.join(", ")));
        }

        parts.join(" ")
    }

    fn infer_critical_concerns(&self) -> String {
        let mut concerns = Vec::new();

        if let Some(cross) = self.cross_insights {
            for insight in &cross.tier3_insights {
                let concern = normalize_concern_name(&format!("{:?}", insight.category));
                if !concerns.contains(&concern) {
                    concerns.push(concern);
                }
            }
        }

        if !self.constraints.gotchas.is_empty() && !concerns.contains(&"Gotchas".to_string()) {
            concerns.push("Project Gotchas".into());
        }

        if concerns.is_empty() {
            "Standard software quality".into()
        } else {
            concerns.join(", ")
        }
    }
}
