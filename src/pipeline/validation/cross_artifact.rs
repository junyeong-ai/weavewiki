//! Cross-Artifact Validator
//!
//! Validates coherence and consistency between different artifact types.
//! Ensures skills, agents, rules, and CLAUDE.md work together effectively.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::types::{Agent, ProjectMemory, Rule, Skill};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossArtifactResult {
    pub passed: bool,
    pub score: f32,
    pub reference_coherence: ReferenceCoherence,
    pub coverage_balance: CoverageBalance,
    pub role_clarity: RoleClarity,
    pub issues: Vec<CrossArtifactIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceCoherence {
    pub score: f32,
    pub shared_references: usize,
    pub orphan_references: usize,
    pub circular_dependencies: Vec<CircularDep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircularDep {
    pub chain: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageBalance {
    pub score: f32,
    pub module_coverage: HashMap<String, ModuleCoverageDetail>,
    pub uncovered_modules: Vec<String>,
    pub over_documented_modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleCoverageDetail {
    pub skill_mentions: usize,
    pub agent_mentions: usize,
    pub rule_mentions: usize,
    pub claude_md_mentions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleClarity {
    pub score: f32,
    pub overlapping_responsibilities: Vec<OverlapDetail>,
    pub gaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapDetail {
    pub artifacts: Vec<String>,
    pub shared_topic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossArtifactIssue {
    pub category: IssueCategory,
    pub severity: Severity,
    pub description: String,
    pub affected_artifacts: Vec<String>,
    pub suggestion: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IssueCategory {
    OrphanReference,
    DuplicateCoverage,
    MissingCoverage,
    RoleOverlap,
    InconsistentNaming,
    CircularDependency,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

pub struct CrossArtifactValidator {
    min_coherence_score: f32,
    max_overlap_ratio: f32,
    reference_weight: f32,
    coverage_weight: f32,
    role_clarity_weight: f32,
}

impl Default for CrossArtifactValidator {
    fn default() -> Self {
        Self {
            min_coherence_score: 0.7,
            max_overlap_ratio: 0.3,
            reference_weight: 0.4,
            coverage_weight: 0.3,
            role_clarity_weight: 0.3,
        }
    }
}

impl CrossArtifactValidator {
    pub fn new(min_coherence_score: f32, max_overlap_ratio: f32) -> Self {
        Self {
            min_coherence_score,
            max_overlap_ratio,
            reference_weight: 0.4,
            coverage_weight: 0.3,
            role_clarity_weight: 0.3,
        }
    }

    pub fn from_config(config: &crate::config::CrossArtifactConfig) -> Self {
        Self {
            min_coherence_score: config.min_coherence_score,
            max_overlap_ratio: config.max_overlap_ratio,
            reference_weight: config.reference_weight,
            coverage_weight: config.coverage_weight,
            role_clarity_weight: config.role_clarity_weight,
        }
    }

    pub fn validate(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> CrossArtifactResult {
        let mut issues = Vec::new();

        let reference_coherence = self.check_reference_coherence(skills, agents, rules, claude_md, &mut issues);
        let coverage_balance = self.check_coverage_balance(skills, agents, rules, claude_md, &mut issues);
        let role_clarity = self.check_role_clarity(skills, agents, &mut issues);

        let score = (reference_coherence.score * self.reference_weight
            + coverage_balance.score * self.coverage_weight
            + role_clarity.score * self.role_clarity_weight)
            .clamp(0.0, 1.0);

        let passed = score >= self.min_coherence_score && !issues.iter().any(|i| i.severity == Severity::Critical);

        CrossArtifactResult {
            passed,
            score,
            reference_coherence,
            coverage_balance,
            role_clarity,
            issues,
        }
    }

    fn check_reference_coherence(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
        issues: &mut Vec<CrossArtifactIssue>,
    ) -> ReferenceCoherence {
        let mut all_references: HashMap<String, Vec<String>> = HashMap::new();
        let claude_md_content = claude_md.to_markdown();

        self.extract_references(&claude_md_content, "CLAUDE.md", &mut all_references);

        for skill in skills {
            self.extract_references(&skill.body, &format!("skill:{}", skill.name), &mut all_references);
        }

        for agent in agents {
            self.extract_references(&agent.prompt, &format!("agent:{}", agent.name), &mut all_references);
        }

        for rule in rules {
            self.extract_references(&rule.to_markdown(), &format!("rule:{}", rule.name), &mut all_references);
        }

        let mut shared_refs = 0;
        let mut orphan_refs = 0;

        for (reference, sources) in &all_references {
            if sources.len() > 1 {
                shared_refs += 1;
            } else if !reference.starts_with("src/") && !reference.contains(".rs")
                && sources.iter().any(|s| s.starts_with("skill:") || s.starts_with("agent:")) {
                    orphan_refs += 1;
                    issues.push(CrossArtifactIssue {
                        category: IssueCategory::OrphanReference,
                        severity: Severity::Low,
                        description: format!("Reference '{}' only appears in {}", reference, sources[0]),
                        affected_artifacts: sources.clone(),
                        suggestion: "Consider if this reference should be shared or removed".into(),
                    });
                }
        }

        let total_refs = all_references.len().max(1);
        let score = if orphan_refs == 0 {
            1.0
        } else {
            (1.0 - (orphan_refs as f32 / total_refs as f32)).clamp(0.0, 1.0)
        };

        ReferenceCoherence {
            score,
            shared_references: shared_refs,
            orphan_references: orphan_refs,
            circular_dependencies: Vec::new(),
        }
    }

    fn extract_references(&self, content: &str, source: &str, refs: &mut HashMap<String, Vec<String>>) {
        let patterns = [
            r"@([a-zA-Z0-9_\-/\.]+(?::\d+)?)",
            r"see\s+([a-zA-Z0-9_\-/\.]+)",
            r"`([a-zA-Z0-9_\-/\.]+\.rs)`",
        ];

        for pattern in patterns {
            match regex::Regex::new(pattern) {
                Ok(re) => {
                    for cap in re.captures_iter(content) {
                        if let Some(m) = cap.get(1) {
                            let reference = m.as_str().to_string();
                            if !reference.is_empty() && reference.len() > 3 {
                                refs.entry(reference).or_default().push(source.to_string());
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(pattern, error = %e, "Failed to compile reference extraction regex");
                }
            }
        }
    }

    fn check_coverage_balance(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
        issues: &mut Vec<CrossArtifactIssue>,
    ) -> CoverageBalance {
        let modules = self.extract_modules(skills, agents, rules, claude_md);
        let mut module_coverage: HashMap<String, ModuleCoverageDetail> = HashMap::new();
        let claude_md_content = claude_md.to_markdown();

        for module in &modules {
            let detail = ModuleCoverageDetail {
                skill_mentions: skills.iter().filter(|s| s.body.contains(module)).count(),
                agent_mentions: agents.iter().filter(|a| a.prompt.contains(module)).count(),
                rule_mentions: rules.iter().filter(|r| r.to_markdown().contains(module)).count(),
                claude_md_mentions: if claude_md_content.contains(module) { 1 } else { 0 },
            };
            module_coverage.insert(module.clone(), detail);
        }

        let uncovered: Vec<_> = module_coverage
            .iter()
            .filter(|(_, d)| d.skill_mentions + d.agent_mentions + d.rule_mentions + d.claude_md_mentions == 0)
            .map(|(m, _)| m.clone())
            .collect();

        let over_documented: Vec<_> = module_coverage
            .iter()
            .filter(|(_, d)| {
                d.skill_mentions + d.agent_mentions + d.rule_mentions > 5
            })
            .map(|(m, _)| m.clone())
            .collect();

        for module in &uncovered {
            issues.push(CrossArtifactIssue {
                category: IssueCategory::MissingCoverage,
                severity: Severity::Medium,
                description: format!("Module '{}' has no documentation coverage", module),
                affected_artifacts: vec![module.clone()],
                suggestion: "Add relevant documentation for this module".into(),
            });
        }

        for module in &over_documented {
            issues.push(CrossArtifactIssue {
                category: IssueCategory::DuplicateCoverage,
                severity: Severity::Low,
                description: format!("Module '{}' is documented in many places", module),
                affected_artifacts: vec![module.clone()],
                suggestion: "Consider consolidating documentation".into(),
            });
        }

        let total_modules = modules.len().max(1);
        let covered = total_modules - uncovered.len();
        let score = (covered as f32 / total_modules as f32).clamp(0.0, 1.0);

        CoverageBalance {
            score,
            module_coverage,
            uncovered_modules: uncovered,
            over_documented_modules: over_documented,
        }
    }

    fn extract_modules(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> HashSet<String> {
        let mut modules = HashSet::new();
        let re = match regex::Regex::new(r"src/([a-zA-Z_]+)/") {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to compile module extraction regex");
                return modules;
            }
        };

        let contents = [
            claude_md.to_markdown(),
            skills.iter().map(|s| s.body.clone()).collect::<Vec<_>>().join("\n"),
            agents.iter().map(|a| a.prompt.clone()).collect::<Vec<_>>().join("\n"),
            rules.iter().map(|r| r.to_markdown()).collect::<Vec<_>>().join("\n"),
        ];

        for content in &contents {
            for cap in re.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    modules.insert(m.as_str().to_string());
                }
            }
        }

        modules
    }

    fn check_role_clarity(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        issues: &mut Vec<CrossArtifactIssue>,
    ) -> RoleClarity {
        let mut overlaps = Vec::new();
        let mut topic_coverage: HashMap<String, Vec<String>> = HashMap::new();

        let topics = ["debug", "test", "build", "deploy", "config", "pipeline", "validation"];

        for topic in topics {
            let mut covering = Vec::new();

            for skill in skills {
                if skill.body.to_lowercase().contains(topic) {
                    covering.push(format!("skill:{}", skill.name));
                }
            }

            for agent in agents {
                if agent.prompt.to_lowercase().contains(topic) {
                    covering.push(format!("agent:{}", agent.name));
                }
            }

            if covering.len() > 2 {
                overlaps.push(OverlapDetail {
                    artifacts: covering.clone(),
                    shared_topic: topic.to_string(),
                });

                if covering.len() > 3 {
                    issues.push(CrossArtifactIssue {
                        category: IssueCategory::RoleOverlap,
                        severity: Severity::Medium,
                        description: format!("Topic '{}' is covered by {} artifacts", topic, covering.len()),
                        affected_artifacts: covering.clone(),
                        suggestion: "Consider consolidating or clarifying distinct responsibilities".into(),
                    });
                }
            }

            if !covering.is_empty() {
                topic_coverage.insert(topic.to_string(), covering);
            }
        }

        let overlap_ratio = if skills.len() + agents.len() > 0 {
            overlaps.len() as f32 / (skills.len() + agents.len()) as f32
        } else {
            0.0
        };

        let score = (1.0 - overlap_ratio.min(self.max_overlap_ratio) / self.max_overlap_ratio).clamp(0.0, 1.0);

        RoleClarity {
            score,
            overlapping_responsibilities: overlaps,
            gaps: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_validator() {
        let validator = CrossArtifactValidator::default();
        assert_eq!(validator.min_coherence_score, 0.7);
    }

    #[test]
    fn test_empty_validation() {
        let validator = CrossArtifactValidator::default();
        let claude_md = ProjectMemory::default();
        let result = validator.validate(&[], &[], &[], &claude_md);
        assert!(result.passed);
    }
}
