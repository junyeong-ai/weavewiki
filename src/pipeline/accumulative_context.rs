//! Accumulative Context Module
//!
//! Zero-information-loss context preservation across pipeline phases.
//! All analysis results are accumulated and available for generation,
//! with Tier-based compression for context window optimization.
//!
//! **DEPRECATED**: This module is being replaced by `ClaudegenContext` in `context.rs`.
//! New code should use `ClaudegenContext` for session management and tier classification.
//! This module remains for backward compatibility during the migration period.

use serde::{Deserialize, Serialize};

use super::analysis::{DeepAnalysisResult, MergedModule, SynthesizedAnalysis};
use crate::types::Severity;
use super::phases::{
    constraint_extraction::ExtractedConstraints, convention_inference::InferredConventions,
    project_detection::ProjectDetection,
};
use crate::pipeline::analysis::deep_analyzer::ConstraintKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Tier {
    Tier1,
    Tier2,
    Tier3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tier3Item {
    pub content: String,
    pub source_file: Option<String>,
    pub line: Option<u32>,
    pub category: Tier3Category,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Tier3Category {
    HiddenDependency,
    RaceCondition,
    AntiPattern,
    SecurityConstraint,
    PerformanceGotcha,
    OwnershipRule,
    CriticalInvariant,
    WorkflowRequirement,
    NamingConvention,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionSummary {
    pub category: String,
    pub pattern: String,
    pub file_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ContextSummary {
    pub tier3_items: Vec<Tier3Item>,
    pub tier2_conventions: Vec<ConventionSummary>,
    pub key_abstractions: Vec<AbstractionSummary>,
    pub file_gotchas: Vec<FileGotcha>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbstractionSummary {
    pub name: String,
    pub kind: String,
    pub file_ref: String,
    pub description: String,
    pub usage_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileGotcha {
    pub file: String,
    pub gotcha: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Default)]
pub struct AccumulativeContext {
    pub detection: Option<ProjectDetection>,
    pub conventions: Option<InferredConventions>,
    pub constraints: Option<ExtractedConstraints>,
    pub deep_analysis: Option<DeepAnalysisResult>,
    pub synthesis: Option<SynthesizedAnalysis>,
    pub summary: ContextSummary,
    iteration_count: usize,
}

impl AccumulativeContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_detection(mut self, detection: ProjectDetection) -> Self {
        self.detection = Some(detection);
        self
    }

    pub fn with_conventions(mut self, conventions: InferredConventions) -> Self {
        self.conventions = Some(conventions);
        self.rebuild_summary();
        self
    }

    pub fn with_constraints(mut self, constraints: ExtractedConstraints) -> Self {
        self.constraints = Some(constraints);
        self.rebuild_summary();
        self
    }

    pub fn with_deep_analysis(mut self, deep: DeepAnalysisResult) -> Self {
        self.deep_analysis = Some(deep);
        self.rebuild_summary();
        self
    }

    pub fn with_synthesis(mut self, synthesis: SynthesizedAnalysis) -> Self {
        self.synthesis = Some(synthesis);
        self.rebuild_summary();
        self
    }

    pub fn set_detection(&mut self, detection: ProjectDetection) {
        self.detection = Some(detection);
    }

    pub fn set_conventions(&mut self, conventions: InferredConventions) {
        self.conventions = Some(conventions);
        self.rebuild_summary();
    }

    pub fn set_constraints(&mut self, constraints: ExtractedConstraints) {
        self.constraints = Some(constraints);
        self.rebuild_summary();
    }

    pub fn set_deep_analysis(&mut self, deep: DeepAnalysisResult) {
        self.deep_analysis = Some(deep);
        self.rebuild_summary();
    }

    pub fn set_synthesis(&mut self, synthesis: SynthesizedAnalysis) {
        self.synthesis = Some(synthesis);
        self.rebuild_summary();
    }

    pub fn increment_iteration(&mut self) {
        self.iteration_count += 1;
    }

    pub fn iteration_count(&self) -> usize {
        self.iteration_count
    }

    fn rebuild_summary(&mut self) {
        self.summary = ContextSummary::default();
        self.extract_tier3_items();
        self.extract_conventions();
        self.extract_abstractions();
        self.extract_gotchas();
    }

    fn extract_tier3_items(&mut self) {
        if let Some(ref constraints) = self.constraints {
            for gotcha in &constraints.gotchas {
                self.summary.tier3_items.push(Tier3Item {
                    content: gotcha.description.clone(),
                    source_file: gotcha.related_files.first().cloned(),
                    line: None,
                    category: Self::categorize_gotcha(&gotcha.description),
                });
            }

            for anti in &constraints.anti_patterns {
                self.summary.tier3_items.push(Tier3Item {
                    content: anti.description.clone(),
                    source_file: anti.evidence.first().map(|e| e.file.clone()),
                    line: anti.evidence.first().and_then(|e| e.line),
                    category: Tier3Category::AntiPattern,
                });
            }

            for dep in &constraints.hidden_dependencies {
                self.summary.tier3_items.push(Tier3Item {
                    content: format!("{} depends on {} - {}", dep.source, dep.target, dep.description),
                    source_file: Some(dep.source.clone()),
                    line: None,
                    category: Tier3Category::HiddenDependency,
                });
            }
        }

        if let Some(ref deep) = self.deep_analysis {
            for constraint in &deep.constraints {
                let category = match constraint.kind {
                    ConstraintKind::HiddenDependency => Tier3Category::HiddenDependency,
                    ConstraintKind::AntiPattern => Tier3Category::AntiPattern,
                    ConstraintKind::Invariant => Tier3Category::CriticalInvariant,
                    ConstraintKind::WorkflowRequirement => Tier3Category::WorkflowRequirement,
                    ConstraintKind::NamingConvention => Tier3Category::NamingConvention,
                };

                let evidence = constraint.evidence.first();
                self.summary.tier3_items.push(Tier3Item {
                    content: constraint.description.clone(),
                    source_file: evidence.map(|e| e.file.clone()),
                    line: evidence.and_then(|e| e.line),
                    category,
                });
            }
        }
    }

    fn extract_conventions(&mut self) {
        if let Some(ref conv) = self.conventions {
            // Extract naming conventions
            let naming = &conv.naming;
            self.summary.tier2_conventions.push(ConventionSummary {
                category: "File Naming".into(),
                pattern: format!("{:?}", naming.file_naming.case),
                file_count: naming.file_naming.examples.len(),
            });

            // Extract architecture layers
            for layer in &conv.architecture.layers {
                self.summary.tier2_conventions.push(ConventionSummary {
                    category: format!("Layer: {}", layer.name),
                    pattern: layer.path_pattern.clone(),
                    file_count: 1,
                });
            }

            // Extract file organization
            for dir in &conv.file_organization.key_directories {
                self.summary.tier2_conventions.push(ConventionSummary {
                    category: format!("Directory: {}", dir.path),
                    pattern: dir.role.clone(),
                    file_count: 1,
                });
            }

            // Extract code patterns
            for pattern in &conv.patterns {
                self.summary.tier2_conventions.push(ConventionSummary {
                    category: format!("{:?}", pattern.category),
                    pattern: pattern.description.clone(),
                    file_count: pattern.evidence.len(),
                });
            }
        }
    }

    fn extract_abstractions(&mut self) {
        if let Some(ref deep) = self.deep_analysis {
            for abst in &deep.key_abstractions {
                self.summary.key_abstractions.push(AbstractionSummary {
                    name: abst.name.clone(),
                    kind: format!("{:?}", abst.kind),
                    file_ref: format!("@{}:{}", abst.file, abst.line),
                    description: abst.description.clone(),
                    usage_notes: abst.usage_notes.clone(),
                });
            }
        }

        if let Some(ref synth) = self.synthesis {
            for abst in &synth.deep.key_abstractions {
                let exists = self.summary.key_abstractions.iter().any(|a| a.name == abst.name);
                if !exists {
                    self.summary.key_abstractions.push(AbstractionSummary {
                        name: abst.name.clone(),
                        kind: format!("{:?}", abst.kind),
                        file_ref: format!("@{}:{}", abst.file, abst.line),
                        description: abst.description.clone(),
                        usage_notes: abst.usage_notes.clone(),
                    });
                }
            }
        }
    }

    fn extract_gotchas(&mut self) {
        if let Some(ref deep) = self.deep_analysis {
            for insight in &deep.insights {
                for gotcha in &insight.gotchas {
                    self.summary.file_gotchas.push(FileGotcha {
                        file: insight.file.clone(),
                        gotcha: gotcha.clone(),
                        severity: Self::assess_gotcha_severity(gotcha),
                    });
                }
            }
        }

        if let Some(ref synth) = self.synthesis {
            for insight in &synth.deep.insights {
                for gotcha in &insight.gotchas {
                    let exists = self.summary.file_gotchas.iter()
                        .any(|g| g.file == insight.file && g.gotcha == *gotcha);
                    if !exists {
                        self.summary.file_gotchas.push(FileGotcha {
                            file: insight.file.clone(),
                            gotcha: gotcha.clone(),
                            severity: Self::assess_gotcha_severity(gotcha),
                        });
                    }
                }
            }
        }
    }

    fn categorize_gotcha(description: &str) -> Tier3Category {
        let lower = description.to_lowercase();
        if lower.contains("race") || lower.contains("concurrent") || lower.contains("thread") {
            Tier3Category::RaceCondition
        } else if lower.contains("security") || lower.contains("injection") || lower.contains("auth") {
            Tier3Category::SecurityConstraint
        } else if lower.contains("performance") || lower.contains("slow") || lower.contains("memory") {
            Tier3Category::PerformanceGotcha
        } else if lower.contains("ownership") || lower.contains("arc") || lower.contains("borrow") {
            Tier3Category::OwnershipRule
        } else if lower.contains("depend") {
            Tier3Category::HiddenDependency
        } else {
            Tier3Category::CriticalInvariant
        }
    }

    fn assess_gotcha_severity(gotcha: &str) -> Severity {
        let lower = gotcha.to_lowercase();
        if lower.contains("critical") || lower.contains("never") || lower.contains("must not") {
            Severity::Critical
        } else if lower.contains("always") || lower.contains("required") || lower.contains("security") {
            Severity::High
        } else if lower.contains("should") || lower.contains("prefer") {
            Severity::Medium
        } else {
            Severity::Low
        }
    }

    pub fn tier3_items(&self) -> &[Tier3Item] {
        &self.summary.tier3_items
    }

    pub fn key_abstractions(&self) -> &[AbstractionSummary] {
        &self.summary.key_abstractions
    }

    pub fn file_gotchas(&self) -> &[FileGotcha] {
        &self.summary.file_gotchas
    }

    pub fn modules(&self) -> Vec<&MergedModule> {
        self.synthesis.as_ref().map(|s| s.modules.iter().collect()).unwrap_or_default()
    }

    pub fn to_generation_prompt(&self, max_tokens: usize) -> String {
        let mut prompt = String::new();
        let mut tokens_used = 0;
        let token_estimate = |s: &str| s.len() / 4;

        prompt.push_str("# PROJECT CONTEXT\n\n");

        if let Some(ref detection) = self.detection {
            let type_info = format!(
                "Project Type: {:?} (confidence: {:.0}%)\n",
                detection.primary_type,
                detection.confidence * 100.0
            );
            tokens_used += token_estimate(&type_info);
            if tokens_used < max_tokens {
                prompt.push_str(&type_info);
            }

            if !detection.languages.is_empty() {
                let langs: Vec<_> = detection.languages.iter()
                    .map(|l| format!("{} ({:.0}%)", l.language, l.percentage * 100.0))
                    .collect();
                let lang_info = format!("Languages: {}\n\n", langs.join(", "));
                tokens_used += token_estimate(&lang_info);
                if tokens_used < max_tokens {
                    prompt.push_str(&lang_info);
                }
            }
        }

        prompt.push_str("## CRITICAL CONSTRAINTS (Tier 3)\n\n");
        let mut tier3_sorted = self.summary.tier3_items.clone();
        tier3_sorted.sort_by(|a, b| {
            let priority = |cat: &Tier3Category| match cat {
                Tier3Category::SecurityConstraint => 0,
                Tier3Category::RaceCondition => 1,
                Tier3Category::HiddenDependency => 2,
                Tier3Category::AntiPattern => 3,
                Tier3Category::OwnershipRule => 4,
                Tier3Category::CriticalInvariant => 5,
                Tier3Category::WorkflowRequirement => 6,
                Tier3Category::PerformanceGotcha => 7,
                Tier3Category::NamingConvention => 8,
                Tier3Category::Other => 9,
            };
            priority(&a.category).cmp(&priority(&b.category))
        });

        for item in &tier3_sorted {
            let item_str = match (&item.source_file, item.line) {
                (Some(file), Some(line)) => format!("- @{}:{} - {}\n", file, line, item.content),
                (Some(file), None) => format!("- @{} - {}\n", file, item.content),
                _ => format!("- {}\n", item.content),
            };
            let item_tokens = token_estimate(&item_str);
            if tokens_used + item_tokens <= max_tokens {
                prompt.push_str(&item_str);
                tokens_used += item_tokens;
            } else {
                break;
            }
        }

        if tokens_used < max_tokens {
            prompt.push_str("\n## KEY ABSTRACTIONS\n\n");
            for abst in &self.summary.key_abstractions {
                let abst_str = format!(
                    "### {} ({})\n{}\nUsage: {}\n\n",
                    abst.name,
                    abst.file_ref,
                    abst.description,
                    abst.usage_notes.join("; ")
                );
                let abst_tokens = token_estimate(&abst_str);
                if tokens_used + abst_tokens <= max_tokens {
                    prompt.push_str(&abst_str);
                    tokens_used += abst_tokens;
                } else {
                    break;
                }
            }
        }

        if tokens_used < max_tokens {
            prompt.push_str("## FILE-SPECIFIC GOTCHAS\n\n");
            let mut sorted_gotchas = self.summary.file_gotchas.clone();
            sorted_gotchas.sort_by(|a, b| {
                let priority = |s: &Severity| match s {
                    Severity::Critical => 0,
                    Severity::High => 1,
                    Severity::Medium => 2,
                    Severity::Low => 3,
                };
                priority(&a.severity).cmp(&priority(&b.severity))
            });

            for gotcha in &sorted_gotchas {
                let gotcha_str = format!("- @{}: {}\n", gotcha.file, gotcha.gotcha);
                let gotcha_tokens = token_estimate(&gotcha_str);
                if tokens_used + gotcha_tokens <= max_tokens {
                    prompt.push_str(&gotcha_str);
                    tokens_used += gotcha_tokens;
                } else {
                    break;
                }
            }
        }

        if tokens_used < max_tokens && !self.summary.tier2_conventions.is_empty() {
            prompt.push_str("\n## CONVENTIONS (Tier 2)\n\n");
            for conv in &self.summary.tier2_conventions {
                let conv_str = format!("- {}: {} ({} files)\n", conv.category, conv.pattern, conv.file_count);
                let conv_tokens = token_estimate(&conv_str);
                if tokens_used + conv_tokens <= max_tokens {
                    prompt.push_str(&conv_str);
                    tokens_used += conv_tokens;
                } else {
                    break;
                }
            }
        }

        prompt
    }

    pub fn merge_from(&mut self, other: &AccumulativeContext) {
        if other.detection.is_some() && self.detection.is_none() {
            self.detection = other.detection.clone();
        }
        if other.conventions.is_some() && self.conventions.is_none() {
            self.conventions = other.conventions.clone();
        }
        if other.constraints.is_some() && self.constraints.is_none() {
            self.constraints = other.constraints.clone();
        }
        if other.deep_analysis.is_some() && self.deep_analysis.is_none() {
            self.deep_analysis = other.deep_analysis.clone();
        }
        if other.synthesis.is_some() && self.synthesis.is_none() {
            self.synthesis = other.synthesis.clone();
        }

        for item in &other.summary.tier3_items {
            if !self.summary.tier3_items.iter().any(|i| i.content == item.content) {
                self.summary.tier3_items.push(item.clone());
            }
        }
        for abst in &other.summary.key_abstractions {
            if !self.summary.key_abstractions.iter().any(|a| a.name == abst.name) {
                self.summary.key_abstractions.push(abst.clone());
            }
        }
        for gotcha in &other.summary.file_gotchas {
            if !self.summary.file_gotchas.iter().any(|g| g.file == gotcha.file && g.gotcha == gotcha.gotcha) {
                self.summary.file_gotchas.push(gotcha.clone());
            }
        }
    }

    pub fn stats(&self) -> ContextStats {
        ContextStats {
            tier3_count: self.summary.tier3_items.len(),
            abstraction_count: self.summary.key_abstractions.len(),
            gotcha_count: self.summary.file_gotchas.len(),
            convention_count: self.summary.tier2_conventions.len(),
            has_detection: self.detection.is_some(),
            has_synthesis: self.synthesis.is_some(),
            iteration_count: self.iteration_count,
        }
    }

    /// Convert to ClaudegenContext (migration helper)
    ///
    /// Use this method to gradually migrate from AccumulativeContext to ClaudegenContext.
    /// The returned ClaudegenContext will have analysis results populated from this context.
    #[deprecated(
        since = "0.2.0",
        note = "Use ClaudegenContext directly. AccumulativeContext is being phased out."
    )]
    pub fn to_claudegen_context(&self, project_root: impl AsRef<std::path::Path>) -> super::context::ClaudegenContext {
        use super::context::{AnalysisResults, ClaudegenContext, SynthesizedAnalysis as ClaudegenSynthesis};

        let mut ctx = ClaudegenContext::new(project_root);

        let analysis = AnalysisResults {
            detection: self.detection.clone(),
            conventions: self.conventions.clone(),
            constraints: self.constraints.clone(),
            deep_analysis: self.deep_analysis.clone(),
            synthesis: self.synthesis.as_ref().map(|s| ClaudegenSynthesis {
                summary: format!("Confidence: {:.2}", s.confidence.overall),
                key_insights: s.modules.iter().map(|m| m.name.clone()).collect(),
                critical_paths: s.deep.constraints.iter().map(|c| c.description.clone()).collect(),
            }),
        };

        ctx.set_analysis(analysis);

        // Convert tier3 items to constraints
        for item in &self.summary.tier3_items {
            ctx.record_classification(
                "tier3",
                &item.content.chars().take(50).collect::<String>(),
                super::context::ContentTier::Tier3Constraint,
                &item.content,
            );
        }

        ctx
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContextStats {
    pub tier3_count: usize,
    pub abstraction_count: usize,
    pub gotcha_count: usize,
    pub convention_count: usize,
    pub has_detection: bool,
    pub has_synthesis: bool,
    pub iteration_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_context() {
        let ctx = AccumulativeContext::new();
        assert!(ctx.tier3_items().is_empty());
        assert!(ctx.key_abstractions().is_empty());
        assert_eq!(ctx.iteration_count(), 0);
    }

    #[test]
    fn test_to_generation_prompt() {
        let mut ctx = AccumulativeContext::new();
        ctx.summary.tier3_items.push(Tier3Item {
            content: "Provider must be Arc-shared".into(),
            source_file: Some("src/ai/provider/mod.rs".into()),
            line: Some(42),
            category: Tier3Category::OwnershipRule,
        });
        ctx.summary.key_abstractions.push(AbstractionSummary {
            name: "LlmProvider".into(),
            kind: "Trait".into(),
            file_ref: "@src/ai/provider/mod.rs:227".into(),
            description: "All LLM interactions go through this trait".into(),
            usage_notes: vec!["Share via Arc::clone".into()],
        });

        let prompt = ctx.to_generation_prompt(10000);
        assert!(prompt.contains("Provider must be Arc-shared"));
        assert!(prompt.contains("LlmProvider"));
        assert!(prompt.contains("@src/ai/provider/mod.rs:42"));
    }

    #[test]
    fn test_merge_from() {
        let mut ctx1 = AccumulativeContext::new();
        ctx1.summary.tier3_items.push(Tier3Item {
            content: "Item 1".into(),
            source_file: None,
            line: None,
            category: Tier3Category::Other,
        });

        let mut ctx2 = AccumulativeContext::new();
        ctx2.summary.tier3_items.push(Tier3Item {
            content: "Item 2".into(),
            source_file: None,
            line: None,
            category: Tier3Category::Other,
        });

        ctx1.merge_from(&ctx2);
        assert_eq!(ctx1.summary.tier3_items.len(), 2);
    }

    #[test]
    fn test_gotcha_severity() {
        assert_eq!(
            AccumulativeContext::assess_gotcha_severity("NEVER do this"),
            Severity::Critical
        );
        assert_eq!(
            AccumulativeContext::assess_gotcha_severity("Always use this pattern"),
            Severity::High
        );
        assert_eq!(
            AccumulativeContext::assess_gotcha_severity("You should prefer X"),
            Severity::Medium
        );
    }
}
