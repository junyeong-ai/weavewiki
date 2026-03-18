//! Verified Reference Pool
//!
//! Collects and validates @file:line references from analysis results.
//! Only references verified against actual file contents are stored.
//!
//! # Line Number Semantics
//! - `line=0`: File-level evidence (marked with `is_file_level: true`)
//! - `line>0`: Line-specific evidence (validated against file line count)
//!
//! # LLM-Trust Architecture
//! - No fixed skill names - uses pattern matching for skill categories
//! - References filtered by semantic relevance, not enum matching

use std::collections::HashMap;

use crate::pipeline::context::{FileRegistryExt, VerifiedFileRegistry};
use super::cross_synthesis::SynthesizedInsights;
use super::deep_analyzer::DeepAnalysisResult;
use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;

#[derive(Debug, Clone)]
pub struct VerifiedLine {
    pub line: usize,
    pub context: String,
    pub source: ReferenceSource,
    pub is_file_level: bool,
}

#[derive(Debug, Clone)]
pub enum ReferenceSource {
    Pattern { name: String },
    Constraint { id: String },
    Violation { category: String },
    Abstraction { name: String },
}

/// Skill category for reference filtering (determined by skill name patterns)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCategory {
    Review,      // code-review, audit, check
    Implement,   // implement, build, create, add
    Debug,       // debug, fix, troubleshoot
    Plan,        // plan, design, architect
    Refactor,    // refactor, restructure, reorganize
    Other,       // unknown category - include all references
}

impl SkillCategory {
    pub fn from_skill_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        let parts: Vec<&str> = lower.split(['-', '_', ' ']).collect();
        let has = |word: &str| parts.contains(&word);

        if has("review") || has("audit") || has("check") {
            Self::Review
        } else if has("implement") || has("build") || has("create") || has("add") {
            Self::Implement
        } else if has("debug") || has("fix") || has("troubleshoot") {
            Self::Debug
        } else if has("plan") || has("design") || has("architect") {
            Self::Plan
        } else if has("refactor") || has("restructure") || has("reorganize") {
            Self::Refactor
        } else {
            Self::Other
        }
    }
}

impl ReferenceSource {
    /// Check if this reference is relevant for a skill category
    pub fn is_relevant_for_category(&self, category: SkillCategory) -> bool {
        match category {
            SkillCategory::Review => {
                matches!(self, Self::Pattern { .. } | Self::Violation { .. })
            }
            SkillCategory::Implement => {
                matches!(self, Self::Pattern { .. } | Self::Abstraction { .. })
            }
            SkillCategory::Debug => {
                matches!(self, Self::Constraint { .. } | Self::Violation { .. })
            }
            SkillCategory::Plan | SkillCategory::Refactor => {
                matches!(self, Self::Pattern { .. } | Self::Abstraction { .. })
            }
            SkillCategory::Other => true, // Include all for unknown skills
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VerifiedReferencePool {
    pool: HashMap<String, Vec<VerifiedLine>>,
}

impl VerifiedReferencePool {
    pub fn build(
        deep: Option<&DeepAnalysisResult>,
        cross: Option<&SynthesizedInsights>,
        constraints: &ExtractedConstraints,
        registry: &VerifiedFileRegistry,
    ) -> Self {
        let mut builder = PoolBuilder::new(registry);

        if let Some(d) = deep {
            builder.add_patterns(&d.patterns);
            builder.add_abstractions(&d.key_abstractions);
        }

        if let Some(c) = cross {
            builder.add_violations(&c.architecture_violations);
            builder.add_hidden_deps(&c.hidden_dependencies);
        }

        builder.add_anti_patterns(&constraints.anti_patterns);
        builder.add_constraint_deps(&constraints.hidden_dependencies);

        builder.finish()
    }

    pub fn references_for_skill(&self, skill_name: &str) -> Vec<String> {
        let category = SkillCategory::from_skill_name(skill_name);
        let mut refs = Vec::new();

        for (file, lines) in &self.pool {
            for l in lines {
                if l.source.is_relevant_for_category(category) {
                    refs.push(Self::format_reference(file, l));
                }
            }
        }
        refs
    }



    fn format_reference(file: &str, line: &VerifiedLine) -> String {
        if line.is_file_level {
            format!("@{} - {}", file, line.context)
        } else {
            format!("@{}:{} - {}", file, line.line, line.context)
        }
    }

    pub fn contains(&self, file: &str, line: usize) -> bool {
        self.pool
            .get(file)
            .is_some_and(|lines| lines.iter().any(|l| l.line == line))
    }

    pub fn total_count(&self) -> usize {
        self.pool.values().map(|v| v.len()).sum()
    }

    pub fn file_count(&self) -> usize {
        self.pool.len()
    }
}

struct PoolBuilder<'a> {
    registry: &'a VerifiedFileRegistry,
    pool: HashMap<String, Vec<VerifiedLine>>,
}

impl<'a> PoolBuilder<'a> {
    fn new(registry: &'a VerifiedFileRegistry) -> Self {
        Self {
            registry,
            pool: HashMap::new(),
        }
    }

    fn add(&mut self, file: &str, line: usize, context: String, source: ReferenceSource) {
        if !self.registry.file_exists(file) {
            return;
        }

        let is_file_level = line == 0;

        if !is_file_level {
            match self.registry.line_count(file) {
                Some(max) if line > max => return,
                Some(_) => {}
                None => {}
            }
        }

        self.pool
            .entry(file.to_string())
            .or_default()
            .push(VerifiedLine {
                line,
                context,
                source,
                is_file_level,
            });
    }

    fn add_patterns(&mut self, patterns: &[super::deep_analyzer::PatternInstance]) {
        for pattern in patterns {
            for loc in &pattern.locations {
                self.add(
                    &loc.file,
                    loc.line as usize,
                    pattern.description.clone(),
                    ReferenceSource::Pattern {
                        name: pattern.name.clone(),
                    },
                );
            }
        }
    }

    fn add_abstractions(&mut self, abstractions: &[super::deep_analyzer::KeyAbstraction]) {
        for abs in abstractions {
            self.add(
                &abs.file,
                abs.line as usize,
                abs.name.clone(),
                ReferenceSource::Abstraction {
                    name: abs.name.clone(),
                },
            );
        }
    }

    fn add_violations(&mut self, violations: &[super::cross_synthesis::ArchitectureViolation]) {
        for violation in violations {
            for ev in &violation.evidence {
                self.add(
                    &ev.file,
                    ev.start_line as usize,
                    violation.description.clone(),
                    ReferenceSource::Violation {
                        category: format!("{:?}", violation.violation_type),
                    },
                );
            }
        }
    }

    fn add_hidden_deps(&mut self, deps: &[super::cross_synthesis::HiddenDependency]) {
        for dep in deps {
            for ev in &dep.evidence {
                self.add(
                    &ev.file,
                    ev.start_line as usize,
                    dep.description.clone(),
                    ReferenceSource::Constraint {
                        id: format!("hidden-dep:{}->{}", dep.from_module, dep.to_module),
                    },
                );
            }
        }
    }

    fn add_anti_patterns(
        &mut self,
        anti_patterns: &[crate::pipeline::phases::constraint_extraction::AntiPattern],
    ) {
        for ap in anti_patterns {
            for ev in &ap.evidence {
                if let Some(line) = ev.line {
                    self.add(
                        &ev.file,
                        line as usize,
                        ap.description.clone(),
                        ReferenceSource::Constraint {
                            id: ap.name.clone(),
                        },
                    );
                }
            }
        }
    }

    fn add_constraint_deps(
        &mut self,
        deps: &[crate::pipeline::phases::constraint_extraction::HiddenDependency],
    ) {
        for dep in deps {
            for ev in &dep.evidence {
                if let Some(line) = ev.line {
                    self.add(
                        &ev.file,
                        line as usize,
                        dep.description.clone(),
                        ReferenceSource::Constraint {
                            id: format!("dep:{}->{}", dep.source, dep.target),
                        },
                    );
                }
            }
        }
    }

    fn finish(mut self) -> VerifiedReferencePool {
        for lines in self.pool.values_mut() {
            lines.sort_by_key(|l| l.line);
        }
        VerifiedReferencePool { pool: self.pool }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_registry() -> VerifiedFileRegistry {
        VerifiedFileRegistry::empty()
    }

    #[test]
    fn test_empty_pool() {
        let constraints = ExtractedConstraints::default();
        let pool = VerifiedReferencePool::build(None, None, &constraints, &empty_registry());
        assert_eq!(pool.total_count(), 0);
    }

    #[test]
    fn test_references_for_skill() {
        let pool = VerifiedReferencePool::default();
        let refs = pool.references_for_skill("code-review");
        assert!(refs.is_empty());
    }

    #[test]
    fn test_skill_category_inference() {
        assert_eq!(SkillCategory::from_skill_name("code-review"), SkillCategory::Review);
        assert_eq!(SkillCategory::from_skill_name("security-audit"), SkillCategory::Review);
        assert_eq!(SkillCategory::from_skill_name("implement"), SkillCategory::Implement);
        assert_eq!(SkillCategory::from_skill_name("build-feature"), SkillCategory::Implement);
        assert_eq!(SkillCategory::from_skill_name("debug"), SkillCategory::Debug);
        assert_eq!(SkillCategory::from_skill_name("fix-bug"), SkillCategory::Debug);
        assert_eq!(SkillCategory::from_skill_name("plan"), SkillCategory::Plan);
        assert_eq!(SkillCategory::from_skill_name("design-api"), SkillCategory::Plan);
        assert_eq!(SkillCategory::from_skill_name("refactor"), SkillCategory::Refactor);
        assert_eq!(SkillCategory::from_skill_name("unknown-skill"), SkillCategory::Other);
    }

    #[test]
    fn test_skill_category_word_boundary() {
        assert_eq!(
            SkillCategory::from_skill_name("rebuild-service"),
            SkillCategory::Other
        );
        assert_eq!(
            SkillCategory::from_skill_name("checksum-validator"),
            SkillCategory::Other
        );
        assert_eq!(
            SkillCategory::from_skill_name("code-review-lint"),
            SkillCategory::Review
        );
        assert_eq!(
            SkillCategory::from_skill_name("add-feature"),
            SkillCategory::Implement
        );
    }

    #[test]
    fn test_source_relevance_by_category() {
        let pattern = ReferenceSource::Pattern {
            name: "test".into(),
        };
        let constraint = ReferenceSource::Constraint { id: "test".into() };
        let violation = ReferenceSource::Violation {
            category: "test".into(),
        };
        let abstraction = ReferenceSource::Abstraction {
            name: "test".into(),
        };

        // Review: Pattern, Violation
        assert!(pattern.is_relevant_for_category(SkillCategory::Review));
        assert!(violation.is_relevant_for_category(SkillCategory::Review));
        assert!(!constraint.is_relevant_for_category(SkillCategory::Review));
        assert!(!abstraction.is_relevant_for_category(SkillCategory::Review));

        // Debug: Constraint, Violation
        assert!(constraint.is_relevant_for_category(SkillCategory::Debug));
        assert!(violation.is_relevant_for_category(SkillCategory::Debug));
        assert!(!pattern.is_relevant_for_category(SkillCategory::Debug));
        assert!(!abstraction.is_relevant_for_category(SkillCategory::Debug));

        // Implement: Pattern, Abstraction
        assert!(pattern.is_relevant_for_category(SkillCategory::Implement));
        assert!(abstraction.is_relevant_for_category(SkillCategory::Implement));
        assert!(!constraint.is_relevant_for_category(SkillCategory::Implement));
        assert!(!violation.is_relevant_for_category(SkillCategory::Implement));

        // Other: all relevant
        assert!(pattern.is_relevant_for_category(SkillCategory::Other));
        assert!(constraint.is_relevant_for_category(SkillCategory::Other));
        assert!(violation.is_relevant_for_category(SkillCategory::Other));
        assert!(abstraction.is_relevant_for_category(SkillCategory::Other));
    }
}
