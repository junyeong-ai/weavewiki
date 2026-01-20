//! Bidirectional Reconciliation
//!
//! Evidence-based reconciliation between bottom-up and top-down analysis.
//! Resolves conflicts using file existence and reference quality scoring.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::deep_analyzer::DeepAnalysisResult;
use super::StructuralValidationResult;
use crate::pipeline::context::VerifiedFileRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationConfig {
    pub max_iterations: usize,
    pub min_evidence_difference: f32,
    pub file_exists_weight: f32,
    pub file_line_ref_weight: f32,
    pub code_parsing_weight: f32,
    pub llm_inference_weight: f32,
    pub min_merge_confidence: f32,
    pub max_unresolved_conflicts: usize,
}

impl Default for ReconciliationConfig {
    fn default() -> Self {
        Self {
            max_iterations: 3,
            min_evidence_difference: 0.2,
            file_exists_weight: 0.9,
            file_line_ref_weight: 0.8,
            code_parsing_weight: 0.7,
            llm_inference_weight: 0.5,
            min_merge_confidence: 0.7,
            max_unresolved_conflicts: 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MergeQualityResult {
    pub passed: bool,
    pub confidence: f32,
    pub unresolved_count: usize,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReconciledAnalysis {
    pub deep: DeepAnalysisResult,
    pub structural: Option<StructuralValidationResult>,
    pub reconciliation_count: usize,
    pub unresolved_conflicts: Vec<ReconciliationConflict>,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct ReconciliationConflict {
    pub category: ConflictCategory,
    pub claim_a: AnalysisClaim,
    pub claim_b: AnalysisClaim,
    pub resolution: Option<ResolutionDecision>,
}

#[derive(Debug, Clone)]
pub struct AnalysisClaim {
    pub content: String,
    pub source: ClaimSource,
    pub file_references: Vec<FileRef>,
    pub evidence_score: f32,
}

#[derive(Debug, Clone)]
pub struct FileRef {
    pub path: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimSource {
    DeepAnalysis,
    StructuralValidation,
    CodeParsing,
    LlmInference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictCategory {
    Architecture,
    Pattern,
    Module,
    Dependency,
}

#[derive(Debug, Clone)]
pub enum ResolutionDecision {
    PreferA { reason: String, confidence: f32 },
    PreferB { reason: String, confidence: f32 },
    Merge { combined: String, confidence: f32 },
}

pub struct BidirectionalReconciler {
    config: ReconciliationConfig,
}

impl BidirectionalReconciler {
    pub fn new(config: ReconciliationConfig) -> Self {
        Self { config }
    }

    pub fn reconcile(
        &self,
        deep: DeepAnalysisResult,
        structural: Option<StructuralValidationResult>,
        file_registry: &VerifiedFileRegistry,
    ) -> ReconciledAnalysis {
        let mut result = ReconciledAnalysis {
            deep: deep.clone(),
            structural: structural.clone(),
            reconciliation_count: 0,
            unresolved_conflicts: Vec::new(),
            confidence: 1.0,
        };

        let structural = match structural {
            Some(s) => s,
            None => return result,
        };

        for _ in 0..self.config.max_iterations {
            let conflicts = self.identify_conflicts(&result.deep, &structural);
            if conflicts.is_empty() {
                break;
            }

            result.reconciliation_count += 1;
            let mut resolved = 0;

            for conflict in conflicts {
                let resolution = self.resolve_with_evidence(&conflict, file_registry);
                if let Some(ref res) = resolution {
                    self.apply_resolution(&mut result.deep, &conflict, res);
                    resolved += 1;
                } else {
                    result.unresolved_conflicts.push(conflict);
                }
            }

            if resolved == 0 {
                break;
            }
        }

        result.confidence = self.calculate_confidence(&result);
        result
    }

    fn identify_conflicts(
        &self,
        deep: &DeepAnalysisResult,
        structural: &StructuralValidationResult,
    ) -> Vec<ReconciliationConflict> {
        let mut conflicts = Vec::new();

        let deep_modules: HashSet<_> = deep
            .structure
            .core_modules
            .iter()
            .map(|m| m.name.as_str())
            .collect();

        let all_modules = structural
            .coverage_report
            .missing_modules
            .iter()
            .chain(structural.coverage_report.partially_covered.iter())
            .chain(structural.coverage_report.fully_covered.iter());

        for module_coverage in all_modules {
            let module = &module_coverage.module;
            if !deep_modules.contains(module.name.as_str()) && module.file_count > 0 {
                conflicts.push(ReconciliationConflict {
                    category: ConflictCategory::Module,
                    claim_a: AnalysisClaim {
                        content: format!("Module '{}' exists", module.name),
                        source: ClaimSource::StructuralValidation,
                        file_references: module
                            .key_files
                            .iter()
                            .map(|f| FileRef { path: f.clone(), line: None })
                            .collect(),
                        evidence_score: 0.0,
                    },
                    claim_b: AnalysisClaim {
                        content: "Module not detected".into(),
                        source: ClaimSource::DeepAnalysis,
                        file_references: Vec::new(),
                        evidence_score: 0.0,
                    },
                    resolution: None,
                });
            }
        }

        conflicts
    }

    fn resolve_with_evidence(
        &self,
        conflict: &ReconciliationConflict,
        file_registry: &VerifiedFileRegistry,
    ) -> Option<ResolutionDecision> {
        let score_a = self.calculate_evidence_score(&conflict.claim_a, file_registry);
        let score_b = self.calculate_evidence_score(&conflict.claim_b, file_registry);

        let diff = (score_a - score_b).abs();
        if diff < self.config.min_evidence_difference {
            return Some(ResolutionDecision::Merge {
                combined: format!("{} + {}", conflict.claim_a.content, conflict.claim_b.content),
                confidence: (score_a + score_b) / 2.0,
            });
        }

        if score_a > score_b {
            Some(ResolutionDecision::PreferA {
                reason: format!("Evidence: {:.2} vs {:.2}", score_a, score_b),
                confidence: score_a,
            })
        } else {
            Some(ResolutionDecision::PreferB {
                reason: format!("Evidence: {:.2} vs {:.2}", score_a, score_b),
                confidence: score_b,
            })
        }
    }

    fn calculate_evidence_score(
        &self,
        claim: &AnalysisClaim,
        file_registry: &VerifiedFileRegistry,
    ) -> f32 {
        let mut score = 0.0;
        let mut count = 0.0;

        for file_ref in &claim.file_references {
            if file_registry.contains(&file_ref.path) {
                score += self.config.file_exists_weight;
                count += 1.0;

                if file_ref.line.is_some() {
                    score += self.config.file_line_ref_weight * 0.3;
                    count += 0.3;
                }
            }
        }

        let source_weight = match claim.source {
            ClaimSource::CodeParsing => self.config.code_parsing_weight,
            ClaimSource::DeepAnalysis => self.config.llm_inference_weight + 0.1,
            ClaimSource::StructuralValidation => self.config.llm_inference_weight,
            ClaimSource::LlmInference => self.config.llm_inference_weight,
        };
        score += source_weight;
        count += 1.0;

        if count > 0.0 { score / count } else { 0.0 }
    }

    fn apply_resolution(
        &self,
        deep: &mut DeepAnalysisResult,
        conflict: &ReconciliationConflict,
        resolution: &ResolutionDecision,
    ) {
        match resolution {
            ResolutionDecision::PreferA { .. } => {
                if matches!(conflict.category, ConflictCategory::Module)
                    && let Some(module_name) = conflict.claim_a.content.strip_prefix("Module '")
                        && let Some(name) = module_name.strip_suffix("' exists")
                            && !deep.structure.core_modules.iter().any(|m| m.name == name) {
                                deep.structure.core_modules.push(
                                    crate::pipeline::analysis::deep_analyzer::CoreModule {
                                        name: name.to_string(),
                                        path: format!("src/{}", name),
                                        responsibility: "Reconciled from structural analysis".into(),
                                        public_items: conflict
                                            .claim_a
                                            .file_references
                                            .iter()
                                            .map(|r| r.path.clone())
                                            .collect(),
                                        internal_deps: Vec::new(),
                                    },
                                );
                            }
            }
            ResolutionDecision::Merge { .. } | ResolutionDecision::PreferB { .. } => {}
        }
    }

    fn calculate_confidence(&self, result: &ReconciledAnalysis) -> f32 {
        let base = 1.0;
        let unresolved_penalty = result.unresolved_conflicts.len() as f32 * 0.1;
        let iteration_penalty = result.reconciliation_count as f32 * 0.02;
        (base - unresolved_penalty - iteration_penalty).clamp(0.0, 1.0)
    }

    pub fn verify_merge_quality(&self, result: &ReconciledAnalysis) -> MergeQualityResult {
        let mut issues = Vec::new();

        if result.confidence < self.config.min_merge_confidence {
            issues.push(format!(
                "Merge confidence {:.2} below threshold {:.2}",
                result.confidence, self.config.min_merge_confidence
            ));
        }

        if result.unresolved_conflicts.len() > self.config.max_unresolved_conflicts {
            issues.push(format!(
                "{} unresolved conflicts exceed max {}",
                result.unresolved_conflicts.len(),
                self.config.max_unresolved_conflicts
            ));
        }

        for conflict in &result.unresolved_conflicts {
            if matches!(conflict.category, ConflictCategory::Architecture | ConflictCategory::Dependency) {
                issues.push(format!(
                    "Critical {:?} conflict unresolved: {}",
                    conflict.category, conflict.claim_a.content
                ));
            }
        }

        MergeQualityResult {
            passed: issues.is_empty(),
            confidence: result.confidence,
            unresolved_count: result.unresolved_conflicts.len(),
            issues,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ReconciliationConfig::default();
        assert_eq!(config.max_iterations, 3);
        assert!(config.file_exists_weight > config.llm_inference_weight);
    }
}
