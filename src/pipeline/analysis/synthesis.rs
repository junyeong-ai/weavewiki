//! Analysis Synthesis Layer
//!
//! Merges results from multiple analysis sources:
//! - Bottom-up: File-level patterns, dependencies, constraints from code reading
//! - Top-down: Architectural analysis, module coverage, structural validation
//!
//! Cross-validates findings and resolves conflicts to produce unified analysis.

use std::collections::{HashMap, HashSet};

use super::{DeepAnalysisResult, StructuralValidationResult};
use crate::config::AnalysisConfig;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::phases::ProjectDetection;

/// Unified analysis result after synthesis
#[derive(Debug, Clone, Default)]
pub struct SynthesizedAnalysis {
    /// Deep analysis findings (bottom-up)
    pub deep: DeepAnalysisResult,
    /// Structural coverage (top-down)
    pub structural: Option<StructuralValidationResult>,
    /// Cross-validation results
    pub validation: CrossValidation,
    /// Merged module understanding
    pub modules: Vec<MergedModule>,
    /// Confidence scores per dimension
    pub confidence: ConfidenceScores,
    /// File reference validation results
    pub reference_validation: ReferenceValidationResult,
}

/// Results of validating file references across all analysis outputs
#[derive(Debug, Clone, Default)]
pub struct ReferenceValidationResult {
    /// Total file references found
    pub total_references: usize,
    /// References that point to valid files
    pub valid_references: usize,
    /// References that were filtered as hallucinations
    pub filtered_hallucinations: usize,
    /// Validation ratio (valid / total)
    pub validation_ratio: f32,
    /// Specific invalid references found (for debugging)
    pub invalid_refs: Vec<InvalidReference>,
}

/// An invalid file reference that was filtered
#[derive(Debug, Clone)]
pub struct InvalidReference {
    /// The reference that was found
    pub reference: String,
    /// Where it was found (e.g., "pattern: ErrorHandling")
    pub source: String,
    /// Reason it's invalid
    pub reason: String,
}

impl SynthesizedAnalysis {
    /// Resolve all conflicts using category-specific strategies
    /// Returns the number of conflicts resolved
    pub fn resolve_conflicts(&mut self) -> usize {
        let mut resolved_count = 0;

        for conflict in &mut self.validation.conflicts {
            if matches!(conflict.resolution, ConflictResolution::Unresolved) {
                conflict.resolution = Self::resolve_conflict(conflict);
                if !matches!(conflict.resolution, ConflictResolution::Unresolved) {
                    resolved_count += 1;
                }
            }
        }

        tracing::debug!(
            resolved = resolved_count,
            remaining = self
                .validation
                .conflicts
                .iter()
                .filter(|c| matches!(c.resolution, ConflictResolution::Unresolved))
                .count(),
            "Conflict resolution complete"
        );

        resolved_count
    }

    /// Apply resolution strategy based on conflict category
    fn resolve_conflict(conflict: &AnalysisConflict) -> ConflictResolution {
        match conflict.category {
            FindingCategory::Architecture => {
                // Architecture: prefer source with more structural evidence
                // Deep analysis reads actual code, so typically more reliable
                if conflict.source_a == "deep_analysis" {
                    ConflictResolution::PreferSourceA(
                        "Deep analysis reads actual code structure".into(),
                    )
                } else if conflict.source_b == "deep_analysis" {
                    ConflictResolution::PreferSourceB(
                        "Deep analysis reads actual code structure".into(),
                    )
                } else {
                    // Both from same source type - merge with lower confidence
                    ConflictResolution::Merge(format!(
                        "Combined: {} AND {}",
                        conflict.claim_a, conflict.claim_b
                    ))
                }
            }
            FindingCategory::Pattern => {
                // Patterns: merge if compatible, otherwise prefer with more locations
                if Self::claims_are_compatible(&conflict.claim_a, &conflict.claim_b) {
                    ConflictResolution::Merge(format!(
                        "Pattern variants: {} / {}",
                        conflict.claim_a, conflict.claim_b
                    ))
                } else {
                    // Prefer the claim that mentions file references
                    if conflict.claim_a.contains('@') || conflict.claim_a.contains("src/") {
                        ConflictResolution::PreferSourceA("Has file references".into())
                    } else if conflict.claim_b.contains('@') || conflict.claim_b.contains("src/") {
                        ConflictResolution::PreferSourceB("Has file references".into())
                    } else {
                        ConflictResolution::Unresolved
                    }
                }
            }
            FindingCategory::Constraint => {
                // Constraints: combine with confidence weighting
                // More restrictive constraint takes precedence (safety first)
                ConflictResolution::Merge(format!(
                    "Combined constraint: {}; Also note: {}",
                    conflict.claim_a, conflict.claim_b
                ))
            }
            FindingCategory::Dependency => {
                // Dependencies: merge both claims as both might be valid
                // Source name matching was removed as too fragile
                ConflictResolution::Merge(format!("{} + {}", conflict.claim_a, conflict.claim_b))
            }
            FindingCategory::ModuleStructure => {
                // Module structure: deep analysis reads actual files
                if conflict.source_a == "deep_analysis" {
                    ConflictResolution::PreferSourceA("Deep analysis reads actual files".into())
                } else if conflict.source_b == "deep_analysis" {
                    ConflictResolution::PreferSourceB("Deep analysis reads actual files".into())
                } else {
                    // Prefer structural analysis for coverage information
                    if conflict.source_a == "structural_analysis" {
                        ConflictResolution::PreferSourceA("Structural analysis for coverage".into())
                    } else {
                        ConflictResolution::PreferSourceB("Structural analysis for coverage".into())
                    }
                }
            }
        }
    }

    /// Check if two claims are compatible (can be merged)
    fn claims_are_compatible(claim_a: &str, claim_b: &str) -> bool {
        claim_a.eq_ignore_ascii_case(claim_b)
    }

    /// Get all unresolved conflicts
    pub fn unresolved_conflicts(&self) -> Vec<&AnalysisConflict> {
        self.validation
            .conflicts
            .iter()
            .filter(|c| matches!(c.resolution, ConflictResolution::Unresolved))
            .collect()
    }

    /// Check if all conflicts are resolved
    pub fn all_conflicts_resolved(&self) -> bool {
        self.validation
            .conflicts
            .iter()
            .all(|c| !matches!(c.resolution, ConflictResolution::Unresolved))
    }
}

/// Cross-validation between analysis sources
#[derive(Debug, Clone, Default)]
pub struct CrossValidation {
    /// Findings confirmed by multiple sources
    pub confirmed_findings: Vec<ConfirmedFinding>,
    /// Conflicts between sources
    pub conflicts: Vec<AnalysisConflict>,
    /// Coverage gaps (areas not analyzed)
    pub gaps: Vec<CoverageGap>,
}

#[derive(Debug, Clone)]
pub struct ConfirmedFinding {
    pub category: FindingCategory,
    pub description: String,
    pub sources: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingCategory {
    Architecture,
    Pattern,
    Constraint,
    Dependency,
    ModuleStructure,
}

#[derive(Debug, Clone)]
pub struct AnalysisConflict {
    pub category: FindingCategory,
    pub source_a: String,
    pub claim_a: String,
    pub source_b: String,
    pub claim_b: String,
    pub resolution: ConflictResolution,
}

#[derive(Debug, Clone)]
pub enum ConflictResolution {
    PreferSourceA(String),
    PreferSourceB(String),
    Merge(String),
    Unresolved,
}

#[derive(Debug, Clone)]
pub struct CoverageGap {
    pub area: String,
    pub reason: String,
    pub impact: GapImpact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapImpact {
    Critical,
    High,
    Medium,
    Low,
}

/// Merged module from multiple analysis sources.
#[derive(Debug, Clone)]
pub struct MergedModule {
    pub name: String,
    pub path: String,
    pub responsibility: String,
    pub patterns: Vec<String>,
    pub constraints: Vec<String>,
    /// Number of artifact references - let LLM interpret significance
    pub reference_count: usize,
    pub public_items: Vec<String>,
    pub internal_deps: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ConfidenceScores {
    pub architecture: f32,
    pub patterns: f32,
    pub constraints: f32,
    pub dependencies: f32,
    pub coverage: f32,
    pub overall: f32,
}

pub struct AnalysisSynthesizer;

impl AnalysisSynthesizer {
    pub fn new(_config: AnalysisConfig) -> Self {
        Self
    }

    /// Synthesize analysis from multiple sources
    pub fn synthesize(
        &self,
        deep: DeepAnalysisResult,
        structural: Option<StructuralValidationResult>,
        _detection: &ProjectDetection,
        file_registry: &VerifiedFileRegistry,
    ) -> SynthesizedAnalysis {
        let validation = self.cross_validate(&deep, structural.as_ref(), file_registry);
        let modules = self.merge_modules(&deep, structural.as_ref());
        let confidence = self.calculate_confidence(&deep, structural.as_ref(), &validation);

        // Validate all file references
        let reference_validation = self.validate_all_references(&deep, file_registry);

        tracing::info!(
            confirmed = validation.confirmed_findings.len(),
            conflicts = validation.conflicts.len(),
            gaps = validation.gaps.len(),
            overall_confidence = format!("{:.1}%", confidence.overall * 100.0),
            valid_refs = reference_validation.valid_references,
            filtered = reference_validation.filtered_hallucinations,
            "Analysis synthesis complete"
        );

        SynthesizedAnalysis {
            deep,
            structural,
            validation,
            modules,
            confidence,
            reference_validation,
        }
    }

    /// Validate all file references in the analysis
    fn validate_all_references(
        &self,
        deep: &DeepAnalysisResult,
        file_registry: &VerifiedFileRegistry,
    ) -> ReferenceValidationResult {
        let mut total = 0;
        let mut valid = 0;
        let mut invalid_refs = Vec::new();

        // Check pattern locations
        for pattern in &deep.patterns {
            for loc in &pattern.locations {
                total += 1;
                if file_registry.contains(&loc.file) {
                    valid += 1;
                } else {
                    invalid_refs.push(InvalidReference {
                        reference: loc.file.clone(),
                        source: format!("pattern: {}", pattern.name),
                        reason: "File not found in project".into(),
                    });
                }
            }
        }

        // Check constraint evidence
        for constraint in &deep.constraints {
            for evidence in &constraint.evidence {
                total += 1;
                if file_registry.contains(&evidence.file) {
                    valid += 1;
                } else {
                    invalid_refs.push(InvalidReference {
                        reference: evidence.file.clone(),
                        source: format!("constraint: {}", constraint.title),
                        reason: "File not found in project".into(),
                    });
                }
            }
        }

        // Check insight files
        for insight in &deep.insights {
            total += 1;
            if file_registry.contains(&insight.file) {
                valid += 1;
            } else {
                invalid_refs.push(InvalidReference {
                    reference: insight.file.clone(),
                    source: "insight".into(),
                    reason: "File not found in project".into(),
                });
            }
        }

        // Check key abstraction files
        for abstraction in &deep.key_abstractions {
            total += 1;
            if file_registry.contains(&abstraction.file) {
                valid += 1;
            } else {
                invalid_refs.push(InvalidReference {
                    reference: abstraction.file.clone(),
                    source: format!("abstraction: {}", abstraction.name),
                    reason: "File not found in project".into(),
                });
            }
        }

        let filtered = total - valid;
        let ratio = if total > 0 {
            valid as f32 / total as f32
        } else {
            1.0
        };

        ReferenceValidationResult {
            total_references: total,
            valid_references: valid,
            filtered_hallucinations: filtered,
            validation_ratio: ratio,
            invalid_refs,
        }
    }

    fn cross_validate(
        &self,
        deep: &DeepAnalysisResult,
        structural: Option<&StructuralValidationResult>,
        file_registry: &VerifiedFileRegistry,
    ) -> CrossValidation {
        let mut validation = CrossValidation::default();

        // Validate pattern file references
        for pattern in &deep.patterns {
            let valid_locations: Vec<_> = pattern
                .locations
                .iter()
                .filter(|loc| file_registry.contains(&loc.file))
                .collect();

            if valid_locations.len() == pattern.locations.len() && !pattern.locations.is_empty() {
                validation.confirmed_findings.push(ConfirmedFinding {
                    category: FindingCategory::Pattern,
                    description: format!(
                        "Pattern '{}' confirmed with valid file references",
                        pattern.name
                    ),
                    sources: vec!["deep_analysis".into(), "file_registry".into()],
                    confidence: 0.9,
                });
            } else if valid_locations.is_empty() && !pattern.locations.is_empty() {
                validation.gaps.push(CoverageGap {
                    area: format!("Pattern: {}", pattern.name),
                    reason: "All file references invalid".into(),
                    impact: GapImpact::Medium,
                });
            }
        }

        // Validate constraint evidence
        for constraint in &deep.constraints {
            let valid_evidence: Vec<_> = constraint
                .evidence
                .iter()
                .filter(|e| file_registry.contains(&e.file))
                .collect();

            if valid_evidence.len() == constraint.evidence.len() && !constraint.evidence.is_empty()
            {
                validation.confirmed_findings.push(ConfirmedFinding {
                    category: FindingCategory::Constraint,
                    description: format!(
                        "Constraint '{}' confirmed with evidence",
                        constraint.title
                    ),
                    sources: vec!["deep_analysis".into(), "file_registry".into()],
                    confidence: 0.85,
                });
            }
        }

        // Cross-validate with structural analysis
        if let Some(structural) = structural {
            self.validate_structural_alignment(deep, structural, &mut validation);
        }

        // Identify gaps
        self.identify_gaps(deep, structural, file_registry, &mut validation);

        validation
    }

    fn validate_structural_alignment(
        &self,
        deep: &DeepAnalysisResult,
        structural: &StructuralValidationResult,
        validation: &mut CrossValidation,
    ) {
        // Check module alignment between deep analysis and structural analysis
        let deep_modules: HashSet<_> = deep
            .structure
            .core_modules
            .iter()
            .map(|m| m.name.as_str())
            .collect();

        let structural_modules: HashSet<_> = structural
            .coverage_report
            .missing_modules
            .iter()
            .chain(structural.coverage_report.partially_covered.iter())
            .chain(structural.coverage_report.fully_covered.iter())
            .map(|m| m.name.as_str())
            .collect();

        // Find confirmed modules (in both)
        for module in deep_modules.intersection(&structural_modules) {
            validation.confirmed_findings.push(ConfirmedFinding {
                category: FindingCategory::ModuleStructure,
                description: format!("Module '{}' confirmed by both analysis sources", module),
                sources: vec!["deep_analysis".into(), "structural_analysis".into()],
                confidence: 0.95,
            });
        }

        // Find conflicts (in deep but not structural, or vice versa)
        for module in deep_modules.difference(&structural_modules) {
            validation.conflicts.push(AnalysisConflict {
                category: FindingCategory::ModuleStructure,
                source_a: "deep_analysis".into(),
                claim_a: format!("Module '{}' exists", module),
                source_b: "structural_analysis".into(),
                claim_b: format!("Module '{}' not detected", module),
                resolution: ConflictResolution::PreferSourceA(
                    "Deep analysis reads actual code".into(),
                ),
            });
        }
    }

    fn identify_gaps(
        &self,
        deep: &DeepAnalysisResult,
        structural: Option<&StructuralValidationResult>,
        file_registry: &VerifiedFileRegistry,
        validation: &mut CrossValidation,
    ) {
        // Check for files not covered by any analysis
        let analyzed_files: HashSet<_> = deep.insights.iter().map(|i| i.file.as_str()).collect();

        let total_files = file_registry.file_count();
        let analyzed_count = analyzed_files.len();

        if analyzed_count < total_files / 2 {
            validation.gaps.push(CoverageGap {
                area: "File coverage".into(),
                reason: format!(
                    "Only {}/{} files analyzed ({:.0}%)",
                    analyzed_count,
                    total_files,
                    (analyzed_count as f32 / total_files as f32) * 100.0
                ),
                impact: GapImpact::High,
            });
        }

        // Check for missing constraint categories (using string names since ConstraintKind doesn't impl Hash)
        let constraint_kind_names: HashSet<String> = deep
            .constraints
            .iter()
            .map(|c| format!("{:?}", c.kind))
            .collect();

        let expected_kind_names = ["AntiPattern", "HiddenDependency", "WorkflowRequirement"];

        for kind_name in expected_kind_names {
            if !constraint_kind_names.iter().any(|k| k.contains(kind_name)) {
                validation.gaps.push(CoverageGap {
                    area: format!("Constraint: {}", kind_name),
                    reason: "No constraints of this type discovered".into(),
                    impact: GapImpact::Medium,
                });
            }
        }

        // Check structural coverage
        if let Some(structural) = structural
            && !structural.coverage_report.missing_modules.is_empty()
        {
            validation.gaps.push(CoverageGap {
                area: "Module documentation".into(),
                reason: format!(
                    "{} core modules not documented",
                    structural.coverage_report.missing_modules.len()
                ),
                impact: GapImpact::High,
            });
        }
    }

    fn merge_modules(
        &self,
        deep: &DeepAnalysisResult,
        structural: Option<&StructuralValidationResult>,
    ) -> Vec<MergedModule> {
        let mut modules: HashMap<String, MergedModule> = HashMap::new();

        for core in &deep.structure.core_modules {
            modules.insert(
                core.name.clone(),
                MergedModule {
                    name: core.name.clone(),
                    path: core.path.clone(),
                    responsibility: core.responsibility.clone(),
                    patterns: Vec::new(),
                    constraints: Vec::new(),
                    reference_count: 0,
                    public_items: core.public_items.clone(),
                    internal_deps: core.internal_deps.clone(),
                },
            );
        }

        if let Some(structural) = structural {
            let all_coverage = structural
                .coverage_report
                .missing_modules
                .iter()
                .chain(structural.coverage_report.partially_covered.iter())
                .chain(structural.coverage_report.fully_covered.iter());

            for mc in all_coverage {
                if let Some(merged) = modules.get_mut(&mc.name) {
                    merged.reference_count = mc.reference_count;
                } else {
                    modules.insert(
                        mc.name.clone(),
                        MergedModule {
                            name: mc.name.clone(),
                            path: mc.path.clone(),
                            responsibility: mc.responsibility.clone(),
                            patterns: Vec::new(),
                            constraints: Vec::new(),
                            reference_count: mc.reference_count,
                            public_items: Vec::new(),
                            internal_deps: Vec::new(),
                        },
                    );
                }
            }
        }

        // Match patterns to modules by exact path match or path prefix
        for pattern in &deep.patterns {
            for location in &pattern.locations {
                for module in modules.values_mut() {
                    // Use exact path match or check if file is under module path
                    let matches = location.file == module.path
                        || location
                            .file
                            .starts_with(&format!("{}/", module.path.trim_end_matches('/')));
                    if matches && !module.patterns.contains(&pattern.name) {
                        module.patterns.push(pattern.name.clone());
                        break;
                    }
                }
            }
        }

        // Match constraints to modules by exact path match or path prefix
        for constraint in &deep.constraints {
            for evidence in &constraint.evidence {
                for module in modules.values_mut() {
                    let matches = evidence.file == module.path
                        || evidence
                            .file
                            .starts_with(&format!("{}/", module.path.trim_end_matches('/')));
                    if matches && !module.constraints.contains(&constraint.title) {
                        module.constraints.push(constraint.title.clone());
                        break;
                    }
                }
            }
        }

        modules.into_values().collect()
    }

    fn calculate_confidence(
        &self,
        deep: &DeepAnalysisResult,
        structural: Option<&StructuralValidationResult>,
        validation: &CrossValidation,
    ) -> ConfidenceScores {
        let confirmed = validation.confirmed_findings.len();
        let conflicts = validation.conflicts.len();

        let base = if confirmed + conflicts > 0 {
            confirmed as f32 / (confirmed + conflicts) as f32
        } else {
            0.5
        };

        let architecture = if deep.structure.entry_points.is_empty()
            && deep.structure.core_modules.is_empty()
        {
            0.3
        } else {
            base
        };

        let patterns = if deep.patterns.is_empty() {
            0.3
        } else {
            base
        };

        let constraints = if deep.constraints.is_empty() {
            0.3
        } else {
            base
        };

        let dependencies = if deep.dependencies.is_empty() { 0.5 } else { 0.7 };

        let coverage = structural
            .map(|s| s.coverage_report.coverage)
            .unwrap_or(0.5);

        let scores = [architecture, patterns, constraints, dependencies, coverage];
        let overall = scores.iter().sum::<f32>() / scores.len() as f32;

        ConfidenceScores {
            architecture,
            patterns,
            constraints,
            dependencies,
            coverage,
            overall,
        }
    }

    /// Determine what areas need re-analysis based on synthesis results
    pub fn get_reanalysis_targets(
        &self,
        synthesis: &SynthesizedAnalysis,
        min_confidence: f32,
    ) -> ReanalysisTargets {
        let mut targets = ReanalysisTargets::default();

        // Check overall confidence
        if synthesis.confidence.overall < min_confidence {
            // Identify specific dimensions that need improvement
            if synthesis.confidence.architecture < min_confidence {
                targets.reanalyze_structure = true;
                targets
                    .reasons
                    .push("Low architecture confidence".to_string());
            }
            if synthesis.confidence.patterns < min_confidence {
                targets.reanalyze_patterns = true;
                targets.reasons.push("Low pattern confidence".to_string());
            }
            if synthesis.confidence.constraints < min_confidence {
                targets.reanalyze_constraints = true;
                targets
                    .reasons
                    .push("Low constraint confidence".to_string());
            }
        }

        // Check for critical gaps
        for gap in &synthesis.validation.gaps {
            match gap.impact {
                GapImpact::Critical | GapImpact::High => {
                    targets.critical_gaps.push(gap.area.clone());
                    // Determine which analysis to rerun based on gap area
                    if gap.area.contains("Pattern") {
                        targets.reanalyze_patterns = true;
                    } else if gap.area.contains("Constraint") {
                        targets.reanalyze_constraints = true;
                    } else if gap.area.contains("File coverage") || gap.area.contains("Module") {
                        targets.reanalyze_structure = true;
                    }
                }
                _ => {}
            }
        }

        // Check for unresolved conflicts
        for conflict in &synthesis.validation.conflicts {
            if matches!(conflict.resolution, ConflictResolution::Unresolved) {
                targets.unresolved_conflicts.push(format!(
                    "{:?}: {} vs {}",
                    conflict.category, conflict.claim_a, conflict.claim_b
                ));
            }
        }

        targets
    }

    /// Check if synthesis results meet quality requirements
    pub fn meets_requirements(&self, synthesis: &SynthesizedAnalysis, min_confidence: f32) -> bool {
        // Overall confidence must meet threshold
        if synthesis.confidence.overall < min_confidence {
            return false;
        }

        // No critical gaps
        let has_critical_gaps = synthesis
            .validation
            .gaps
            .iter()
            .any(|g| matches!(g.impact, GapImpact::Critical));
        if has_critical_gaps {
            return false;
        }

        // No unresolved conflicts
        let has_unresolved = synthesis
            .validation
            .conflicts
            .iter()
            .any(|c| matches!(c.resolution, ConflictResolution::Unresolved));
        if has_unresolved {
            return false;
        }

        true
    }

    /// Enhance synthesis with AST-based validation
    /// This uses ground-truth facts from tree-sitter parsing to validate LLM claims
    pub fn enhance_with_ast(
        &self,
        synthesis: &mut SynthesizedAnalysis,
        ast_facts: &super::ast_enrichment::AstFacts,
    ) {
        use super::ast_enrichment::AstValidation;

        let mut validated_count = 0;
        let mut corrected_count = 0;
        let mut invalidated_count = 0;

        // Validate key abstraction locations
        for abstraction in &synthesis.deep.key_abstractions {
            let result = match abstraction.kind {
                super::deep_analyzer::AbstractionKind::Function => ast_facts
                    .validate_function_reference(
                        &abstraction.name,
                        &abstraction.file,
                        abstraction.line,
                    ),
                super::deep_analyzer::AbstractionKind::Struct
                | super::deep_analyzer::AbstractionKind::Class
                | super::deep_analyzer::AbstractionKind::Enum
                | super::deep_analyzer::AbstractionKind::Type => ast_facts.validate_type_reference(
                    &abstraction.name,
                    &abstraction.file,
                    abstraction.line,
                ),
                _ => AstValidation::Exact,
            };

            match result {
                AstValidation::Exact => validated_count += 1,
                AstValidation::Close { .. } => {
                    validated_count += 1;
                    corrected_count += 1;
                }
                AstValidation::WrongLine { .. } | AstValidation::WrongFile { .. } => {
                    corrected_count += 1;
                }
                AstValidation::NotFound => {
                    invalidated_count += 1;
                }
            }
        }

        // Enrich modules with AST-derived public items
        for module in &mut synthesis.modules {
            let module_path = &module.path;
            let public_funcs = ast_facts.public_functions_in(module_path);
            let public_types = ast_facts.public_types_in(module_path);

            // Add AST-verified public items
            for func in public_funcs {
                let item = format!("fn {}()", func.name);
                if !module.public_items.contains(&item) {
                    module.public_items.push(item);
                }
            }
            for t in public_types {
                let item = format!("{:?} {}", t.kind, t.name);
                if !module.public_items.contains(&item) {
                    module.public_items.push(item);
                }
            }
        }

        // Boost confidence based on AST validation
        let ast_validation_ratio = if validated_count + invalidated_count > 0 {
            validated_count as f32 / (validated_count + invalidated_count) as f32
        } else {
            1.0
        };

        // Adjust confidence based on AST validation
        synthesis.confidence.overall = (synthesis.confidence.overall + ast_validation_ratio) / 2.0;

        tracing::info!(
            validated = validated_count,
            corrected = corrected_count,
            invalidated = invalidated_count,
            ast_validation_ratio = format!("{:.1}%", ast_validation_ratio * 100.0),
            new_confidence = format!("{:.1}%", synthesis.confidence.overall * 100.0),
            "AST enhancement complete"
        );
    }
}

/// Identifies what needs to be re-analyzed
#[derive(Debug, Clone, Default)]
pub struct ReanalysisTargets {
    pub reanalyze_structure: bool,
    pub reanalyze_patterns: bool,
    pub reanalyze_constraints: bool,
    pub critical_gaps: Vec<String>,
    pub unresolved_conflicts: Vec<String>,
    pub reasons: Vec<String>,
}

impl ReanalysisTargets {
    /// Check if any re-analysis is needed
    pub fn needs_reanalysis(&self) -> bool {
        self.reanalyze_structure
            || self.reanalyze_patterns
            || self.reanalyze_constraints
            || !self.critical_gaps.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_calculation_empty() {
        let synthesizer = AnalysisSynthesizer::new(AnalysisConfig::default());
        let deep = DeepAnalysisResult::default();
        let validation = CrossValidation::default();

        let confidence = synthesizer.calculate_confidence(&deep, None, &validation);
        assert!(confidence.overall > 0.0);
        assert!(confidence.overall < 1.0);
    }

    #[test]
    fn test_gap_impact_ordering() {
        assert!(matches!(GapImpact::Critical, GapImpact::Critical));
        assert!(matches!(GapImpact::High, GapImpact::High));
    }

    #[test]
    fn test_resolve_conflicts() {
        let mut synthesis = SynthesizedAnalysis::default();

        // Add unresolved conflict
        synthesis.validation.conflicts.push(AnalysisConflict {
            category: FindingCategory::ModuleStructure,
            source_a: "deep_analysis".into(),
            claim_a: "Module 'api' exists".into(),
            source_b: "structural_analysis".into(),
            claim_b: "Module 'api' not detected".into(),
            resolution: ConflictResolution::Unresolved,
        });

        // Resolve conflicts
        let resolved = synthesis.resolve_conflicts();

        assert_eq!(resolved, 1);
        assert!(synthesis.all_conflicts_resolved());
        assert!(matches!(
            synthesis.validation.conflicts[0].resolution,
            ConflictResolution::PreferSourceA(_)
        ));
    }

    #[test]
    fn test_claims_are_compatible() {
        assert!(SynthesizedAnalysis::claims_are_compatible(
            "Module uses async/await",
            "module uses async/await"
        ));
        assert!(!SynthesizedAnalysis::claims_are_compatible(
            "Uses synchronous IO",
            "Implements HTTP server"
        ));
    }
}
