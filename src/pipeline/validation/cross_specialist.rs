//! Cross-Specialist Validation
//!
//! Validates consistency between specialist analysis results.
//! Detects conflicts, missing corroboration, and contradictions.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::config::AnalysisSpecialty;
use crate::pipeline::analysis::multi_agent::MultiAgentResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossSpecialistConfig {
    pub max_conflicts_allowed: usize,
    pub min_agreement_ratio: f32,
    pub require_corroboration: bool,
    pub min_corroboration_specialists: usize,
}

impl Default for CrossSpecialistConfig {
    fn default() -> Self {
        Self {
            max_conflicts_allowed: 0,
            min_agreement_ratio: 0.8,
            require_corroboration: true,
            min_corroboration_specialists: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CrossSpecialistResult {
    pub passed: bool,
    pub conflicts: Vec<SpecialistConflict>,
    pub agreements: Vec<SpecialistAgreement>,
    pub agreement_ratio: f32,
    pub specialists_to_rerun: Vec<AnalysisSpecialty>,
}

#[derive(Debug, Clone)]
pub struct SpecialistConflict {
    pub specialist_a: AnalysisSpecialty,
    pub specialist_b: AnalysisSpecialty,
    pub claim_a: String,
    pub claim_b: String,
    pub conflict_type: ConflictType,
}

#[derive(Debug, Clone)]
pub struct SpecialistAgreement {
    pub specialists: Vec<AnalysisSpecialty>,
    pub claim: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictType {
    Contradictory,
    PartialDisagreement,
    MissingCorroboration,
}

pub struct CrossSpecialistValidator {
    config: CrossSpecialistConfig,
}

impl CrossSpecialistValidator {
    pub fn new(config: CrossSpecialistConfig) -> Self {
        Self { config }
    }

    pub fn validate(&self, result: &MultiAgentResult) -> CrossSpecialistResult {
        let mut conflicts = Vec::new();
        let mut agreements = Vec::new();

        self.check_structure_pattern_consistency(result, &mut conflicts, &mut agreements);
        self.check_constraint_pattern_consistency(result, &mut conflicts);
        self.check_module_coverage_consistency(result, &mut conflicts);

        if self.config.require_corroboration {
            self.check_corroboration(result, &mut conflicts);
        }

        let total = conflicts.len() + agreements.len();
        let agreement_ratio = if total > 0 {
            agreements.len() as f32 / total as f32
        } else {
            1.0
        };

        let passed = conflicts.len() <= self.config.max_conflicts_allowed
            && agreement_ratio >= self.config.min_agreement_ratio;

        let specialists_to_rerun = if !passed {
            self.determine_specialists_to_rerun(&conflicts)
        } else {
            Vec::new()
        };

        CrossSpecialistResult {
            passed,
            conflicts,
            agreements,
            agreement_ratio,
            specialists_to_rerun,
        }
    }

    fn check_structure_pattern_consistency(
        &self,
        result: &MultiAgentResult,
        conflicts: &mut Vec<SpecialistConflict>,
        agreements: &mut Vec<SpecialistAgreement>,
    ) {
        for module in &result.structure.core_modules {
            let patterns_in_module: Vec<_> = result
                .patterns
                .iter()
                .filter(|p| p.locations.iter().any(|l| l.file.starts_with(&module.path)))
                .collect();

            let module_size = module.public_items.len() + module.internal_deps.len();
            if patterns_in_module.is_empty() && module_size > 3 {
                conflicts.push(SpecialistConflict {
                    specialist_a: AnalysisSpecialty::Structure,
                    specialist_b: AnalysisSpecialty::Pattern,
                    claim_a: format!("Module {} ({} items)", module.name, module_size),
                    claim_b: "No patterns detected".into(),
                    conflict_type: ConflictType::MissingCorroboration,
                });
            } else if !patterns_in_module.is_empty() {
                agreements.push(SpecialistAgreement {
                    specialists: vec![AnalysisSpecialty::Structure, AnalysisSpecialty::Pattern],
                    claim: format!(
                        "Module {} has {} patterns",
                        module.name,
                        patterns_in_module.len()
                    ),
                    confidence: 0.9,
                });
            }
        }
    }

    fn check_constraint_pattern_consistency(
        &self,
        result: &MultiAgentResult,
        conflicts: &mut Vec<SpecialistConflict>,
    ) {
        for constraint in &result.constraints {
            let title_lower = constraint.title.to_lowercase();
            let is_prohibition = title_lower.contains("never")
                || title_lower.contains("forbidden")
                || title_lower.contains("禁止")
                || title_lower.contains("금지");

            if is_prohibition {
                let forbidden_term = title_lower
                    .replace("never ", "")
                    .replace("forbidden: ", "")
                    .replace("禁止: ", "")
                    .replace("금지: ", "");

                let violating_pattern = result.patterns.iter().find(|p| {
                    p.name.to_lowercase().contains(&forbidden_term)
                        || p.description.to_lowercase().contains(&forbidden_term)
                });

                if let Some(pattern) = violating_pattern {
                    conflicts.push(SpecialistConflict {
                        specialist_a: AnalysisSpecialty::Constraint,
                        specialist_b: AnalysisSpecialty::Pattern,
                        claim_a: constraint.title.clone(),
                        claim_b: format!("Pattern '{}' exists", pattern.name),
                        conflict_type: ConflictType::Contradictory,
                    });
                }
            }
        }
    }

    fn check_module_coverage_consistency(
        &self,
        _result: &MultiAgentResult,
        _conflicts: &mut Vec<SpecialistConflict>,
    ) {
        // Module coverage consistency check was simplified to 3 specialists (Structure, Pattern, Constraint)
        // Abstraction specialist has been removed, so this check is no longer applicable
    }

    fn check_corroboration(
        &self,
        result: &MultiAgentResult,
        conflicts: &mut Vec<SpecialistConflict>,
    ) {
        let active_specialists: Vec<_> = result
            .specialist_confidences
            .iter()
            .filter(|&(_, conf)| *conf > 0.0)
            .map(|(spec, _)| *spec)
            .collect();

        if active_specialists.len() < self.config.min_corroboration_specialists {
            conflicts.push(SpecialistConflict {
                specialist_a: *active_specialists.first().unwrap_or(&AnalysisSpecialty::Structure),
                specialist_b: AnalysisSpecialty::Structure,
                claim_a: format!("Only {} specialists active", active_specialists.len()),
                claim_b: format!("Minimum {} required", self.config.min_corroboration_specialists),
                conflict_type: ConflictType::MissingCorroboration,
            });
        }
    }

    fn determine_specialists_to_rerun(&self, conflicts: &[SpecialistConflict]) -> Vec<AnalysisSpecialty> {
        let mut specialists = HashSet::new();

        for conflict in conflicts {
            if matches!(conflict.conflict_type, ConflictType::Contradictory) {
                specialists.insert(conflict.specialist_a);
                specialists.insert(conflict.specialist_b);
            }
        }

        if specialists.is_empty() {
            for conflict in conflicts {
                specialists.insert(conflict.specialist_a);
            }
        }

        specialists.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CrossSpecialistConfig::default();
        assert_eq!(config.max_conflicts_allowed, 0);
        assert_eq!(config.min_agreement_ratio, 0.8);
    }

    #[test]
    fn test_empty_result_passes() {
        let validator = CrossSpecialistValidator::new(CrossSpecialistConfig {
            require_corroboration: false,
            ..Default::default()
        });
        let result = validator.validate(&MultiAgentResult::default());
        assert!(result.passed);
    }
}
