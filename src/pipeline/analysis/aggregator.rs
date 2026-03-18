//! Analysis Aggregator Module
//!
//! Map-Reduce aggregation for distributed chunk analysis results.
//! Combines patterns, conventions, constraints, and dependencies from all chunks.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::deep_analyzer::{DiscoveredConstraint, Gotcha, PatternInstance};
use super::distributed::{AsyncStyle, ChunkAnalysisResult, ErrorStyle, NamingCase};

// =============================================================================
// AGGREGATED TYPES
// =============================================================================

/// Complete aggregated analysis from all chunks
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AggregatedAnalysis {
    pub conventions: ProjectConventions,
    pub patterns: Vec<AggregatedPattern>,
    pub constraints: Vec<AggregatedConstraint>,
    pub gotchas: Vec<Gotcha>,
    pub dependency_graph: DependencyGraph,
    pub coverage: Coverage,
}

impl AggregatedAnalysis {
    /// Convert to DeepAnalysisResult for compatibility with existing pipeline
    pub fn to_deep_analysis_result(&self) -> super::DeepAnalysisResult {
        use super::deep_analyzer::{
            AnalysisQuality, DeepAnalysisResult, DependencyType, ModuleDependency,
            SemanticStructure,
        };

        let patterns = self.patterns.iter().map(|p| p.pattern.clone()).collect();

        let constraints = self
            .constraints
            .iter()
            .map(|c| c.constraint.clone())
            .collect();

        let dependencies: Vec<ModuleDependency> = self
            .dependency_graph
            .edges
            .iter()
            .map(|e| ModuleDependency {
                from_module: e.from.clone(),
                to_module: e.to.clone(),
                dependency_type: DependencyType::Import,
                is_public: true,
            })
            .collect();

        let analysis_quality = AnalysisQuality {
            files_analyzed: self.coverage.analyzed_files,
            lines_analyzed: self.coverage.analyzed_lines,
            coverage_ratio: self.coverage.coverage_ratio,
            evidence_count: self.patterns.len() + self.constraints.len(),
            validated_refs: 0,
            filtered_hallucinations: 0,
            confidence_score: self.coverage.coverage_ratio,
        };

        DeepAnalysisResult {
            structure: SemanticStructure::default(),
            patterns,
            constraints,
            dependencies,
            insights: Vec::new(),
            key_abstractions: Vec::new(),
            analysis_quality,
        }
    }
}

/// Project-wide conventions derived from all files
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConventions {
    pub primary_naming: Option<NamingCase>,
    pub naming_distribution: HashMap<NamingCase, f32>,
    pub primary_error_handling: Option<ErrorStyle>,
    pub error_distribution: HashMap<ErrorStyle, f32>,
    pub primary_async: Option<AsyncStyle>,
    pub async_distribution: HashMap<AsyncStyle, f32>,
    pub common_import_patterns: Vec<String>,
}

/// Pattern with aggregated occurrence data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedPattern {
    pub pattern: PatternInstance,
    pub occurrence_count: usize,
    pub modules: Vec<String>,
    pub frequency: f32,
}

/// Constraint with aggregated evidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedConstraint {
    pub constraint: DiscoveredConstraint,
    pub occurrence_count: usize,
    pub modules: Vec<String>,
    pub cross_module: bool,
}

/// Dependency graph built from all modules
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub edges: Vec<DependencyEdge>,
    pub modules: HashSet<String>,
    pub hub_modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub edge_type: String,
    pub weight: usize,
}

/// Coverage statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Coverage {
    pub total_files: usize,
    pub analyzed_files: usize,
    pub total_lines: usize,
    pub analyzed_lines: usize,
    pub coverage_ratio: f32,
    pub modules_covered: usize,
}

// =============================================================================
// AGGREGATOR
// =============================================================================

pub struct AnalysisAggregator;

impl AnalysisAggregator {
    /// Map-Reduce: aggregate all chunk results
    pub fn aggregate(
        chunk_results: Vec<ChunkAnalysisResult>,
        total_file_count: usize,
        total_line_count: usize,
    ) -> AggregatedAnalysis {
        if chunk_results.is_empty() {
            return AggregatedAnalysis::default();
        }

        let conventions = Self::reduce_conventions(&chunk_results, total_file_count);
        let patterns = Self::merge_patterns(&chunk_results);
        let constraints = Self::combine_constraints(&chunk_results);
        let gotchas = Self::collect_gotchas(&chunk_results);
        let dependency_graph = Self::build_dependency_graph(&chunk_results);

        let analyzed_files: usize = chunk_results.iter().map(|c| c.file_count).sum();
        let analyzed_lines: usize = chunk_results.iter().map(|c| c.lines_analyzed).sum();
        let modules_covered = chunk_results
            .iter()
            .map(|c| c.module_path.clone())
            .collect::<HashSet<_>>()
            .len();

        let coverage = Coverage {
            total_files: total_file_count,
            analyzed_files,
            total_lines: total_line_count,
            analyzed_lines,
            coverage_ratio: if total_file_count > 0 {
                analyzed_files as f32 / total_file_count as f32
            } else {
                0.0
            },
            modules_covered,
        };

        AggregatedAnalysis {
            conventions,
            patterns,
            constraints,
            gotchas,
            dependency_graph,
            coverage,
        }
    }

    /// Reduce conventions using majority voting across all files
    fn reduce_conventions(
        results: &[ChunkAnalysisResult],
        _total_files: usize,
    ) -> ProjectConventions {
        let mut naming_totals: HashMap<NamingCase, usize> = HashMap::new();
        let mut error_totals: HashMap<ErrorStyle, usize> = HashMap::new();
        let mut async_totals: HashMap<AsyncStyle, usize> = HashMap::new();
        let mut import_patterns: Vec<String> = Vec::new();

        for result in results {
            for (case, count) in &result.conventions.naming_patterns {
                *naming_totals.entry(*case).or_default() += count;
            }
            for (style, count) in &result.conventions.error_handling {
                *error_totals.entry(*style).or_default() += count;
            }
            for (style, count) in &result.conventions.async_patterns {
                *async_totals.entry(*style).or_default() += count;
            }
            import_patterns.extend(result.conventions.import_patterns.clone());
        }

        let total_naming: usize = naming_totals.values().sum();
        let total_error: usize = error_totals.values().sum();
        let total_async: usize = async_totals.values().sum();

        let naming_distribution: HashMap<NamingCase, f32> = naming_totals
            .iter()
            .map(|(k, v)| {
                (
                    *k,
                    if total_naming > 0 {
                        *v as f32 / total_naming as f32
                    } else {
                        0.0
                    },
                )
            })
            .collect();

        let error_distribution: HashMap<ErrorStyle, f32> = error_totals
            .iter()
            .map(|(k, v)| {
                (
                    *k,
                    if total_error > 0 {
                        *v as f32 / total_error as f32
                    } else {
                        0.0
                    },
                )
            })
            .collect();

        let async_distribution: HashMap<AsyncStyle, f32> = async_totals
            .iter()
            .map(|(k, v)| {
                (
                    *k,
                    if total_async > 0 {
                        *v as f32 / total_async as f32
                    } else {
                        0.0
                    },
                )
            })
            .collect();

        let primary_naming = naming_totals
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(case, _)| case);

        let primary_error_handling = error_totals
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(style, _)| style);

        let primary_async = async_totals
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(style, _)| style);

        let common_import_patterns = Self::dedupe_and_rank_imports(import_patterns, 10);

        ProjectConventions {
            primary_naming,
            naming_distribution,
            primary_error_handling,
            error_distribution,
            primary_async,
            async_distribution,
            common_import_patterns,
        }
    }

    fn dedupe_and_rank_imports(patterns: Vec<String>, max_patterns: usize) -> Vec<String> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for pattern in patterns {
            *counts.entry(pattern).or_default() += 1;
        }

        let mut ranked: Vec<_> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        ranked
            .into_iter()
            .take(max_patterns)
            .map(|(p, _)| p)
            .collect()
    }

    /// Merge patterns from all chunks, deduplicating by normalized name.
    ///
    /// Normalizes pattern names so LLM variations like "Builder Pattern",
    /// "builder-pattern", "Builder Design Pattern" all merge into one entry.
    fn merge_patterns(results: &[ChunkAnalysisResult]) -> Vec<AggregatedPattern> {
        let mut pattern_map: HashMap<String, AggregatedPattern> = HashMap::new();

        for result in results {
            for pattern in &result.patterns {
                let key = normalize_pattern_key(&pattern.name);
                pattern_map
                    .entry(key)
                    .and_modify(|agg| {
                        agg.occurrence_count += 1;
                        if !agg.modules.contains(&result.module_path) {
                            agg.modules.push(result.module_path.clone());
                        }
                        agg.pattern
                            .locations
                            .extend(pattern.locations.iter().cloned());
                    })
                    .or_insert_with(|| AggregatedPattern {
                        pattern: pattern.clone(),
                        occurrence_count: 1,
                        modules: vec![result.module_path.clone()],
                        frequency: 0.0,
                    });
            }
        }

        let total_chunks = results.len();
        let mut patterns: Vec<_> = pattern_map.into_values().collect();

        for pattern in &mut patterns {
            pattern.frequency = pattern.occurrence_count as f32 / total_chunks as f32;
        }

        patterns.sort_by(|a, b| b.occurrence_count.cmp(&a.occurrence_count));
        patterns
    }

    /// Combine constraints from all chunks
    fn combine_constraints(results: &[ChunkAnalysisResult]) -> Vec<AggregatedConstraint> {
        let mut constraint_map: HashMap<String, AggregatedConstraint> = HashMap::new();

        for result in results {
            for constraint in &result.constraints {
                let key = format!("{:?}:{}", constraint.kind, constraint.title);
                constraint_map
                    .entry(key)
                    .and_modify(|agg| {
                        agg.occurrence_count += 1;
                        if !agg.modules.contains(&result.module_path) {
                            agg.modules.push(result.module_path.clone());
                            agg.cross_module = true;
                        }
                    })
                    .or_insert_with(|| AggregatedConstraint {
                        constraint: constraint.clone(),
                        occurrence_count: 1,
                        modules: vec![result.module_path.clone()],
                        cross_module: false,
                    });
            }
        }

        let mut constraints: Vec<_> = constraint_map.into_values().collect();
        constraints.sort_by(|a, b| {
            b.cross_module
                .cmp(&a.cross_module)
                .then_with(|| b.occurrence_count.cmp(&a.occurrence_count))
        });
        constraints
    }

    /// Collect all gotchas from chunks
    fn collect_gotchas(results: &[ChunkAnalysisResult]) -> Vec<Gotcha> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut gotchas = Vec::new();

        for result in results {
            for gotcha in &result.gotchas {
                let key = gotcha.description.clone();
                if seen.insert(key) {
                    gotchas.push(gotcha.clone());
                }
            }
        }

        gotchas
    }

    /// Build dependency graph from all modules
    fn build_dependency_graph(results: &[ChunkAnalysisResult]) -> DependencyGraph {
        let mut edges: HashMap<(String, String), DependencyEdge> = HashMap::new();
        let mut modules: HashSet<String> = HashSet::new();
        let mut incoming_counts: HashMap<String, usize> = HashMap::new();

        for result in results {
            modules.insert(result.module_path.clone());

            for dep in &result.dependencies {
                let key = (dep.from_module.clone(), dep.to_module.clone());
                edges
                    .entry(key.clone())
                    .and_modify(|e| e.weight += 1)
                    .or_insert_with(|| DependencyEdge {
                        from: dep.from_module.clone(),
                        to: dep.to_module.clone(),
                        edge_type: format!("{:?}", dep.dependency_type),
                        weight: 1,
                    });

                modules.insert(dep.from_module.clone());
                modules.insert(dep.to_module.clone());
                *incoming_counts.entry(dep.to_module.clone()).or_default() += 1;
            }
        }

        // Hub threshold: modules with incoming edges >= 1/3 of total modules (min 2)
        // Rationale: In a typical dependency graph, hub modules (utils, common, core)
        // are imported by many modules. 1/3 threshold identifies frequently-imported
        // modules. Min of 2 ensures small projects don't flag every module as a hub.
        // LLM uses this to understand central architectural components.
        let hub_threshold = (modules.len() / 3).max(2);
        let mut hub_modules: Vec<_> = incoming_counts
            .into_iter()
            .filter(|(_, count)| *count >= hub_threshold)
            .map(|(module, _)| module)
            .collect();
        hub_modules.sort();

        DependencyGraph {
            edges: edges.into_values().collect(),
            modules,
            hub_modules,
        }
    }
}

fn normalize_pattern_key(name: &str) -> String {
    name.to_lowercase()
        .replace(['-', '_'], " ")
        .replace("design pattern", "")
        .replace("pattern", "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::distributed::ChunkConventions;
    use super::*;

    #[test]
    fn test_empty_aggregation() {
        let result = AnalysisAggregator::aggregate(vec![], 0, 0);
        assert!(result.patterns.is_empty());
        assert!(result.constraints.is_empty());
        assert_eq!(result.coverage.coverage_ratio, 0.0);
    }

    #[test]
    fn test_coverage_calculation() {
        let chunk = ChunkAnalysisResult {
            chunk_id: "chunk-1".to_string(),
            module_path: "src/test".to_string(),
            file_count: 5,
            lines_analyzed: 500,
            ..Default::default()
        };

        let result = AnalysisAggregator::aggregate(vec![chunk], 10, 1000);
        assert_eq!(result.coverage.total_files, 10);
        assert_eq!(result.coverage.analyzed_files, 5);
        assert_eq!(result.coverage.coverage_ratio, 0.5);
    }

    #[test]
    fn test_convention_reduction() {
        let mut conventions = ChunkConventions::default();
        conventions
            .naming_patterns
            .insert(NamingCase::SnakeCase, 10);
        conventions.naming_patterns.insert(NamingCase::CamelCase, 3);

        let chunk = ChunkAnalysisResult {
            chunk_id: "chunk-1".to_string(),
            module_path: "src/test".to_string(),
            conventions,
            file_count: 5,
            ..Default::default()
        };

        let result = AnalysisAggregator::aggregate(vec![chunk], 5, 500);
        assert_eq!(
            result.conventions.primary_naming,
            Some(NamingCase::SnakeCase)
        );
    }

    #[test]
    fn test_normalize_pattern_key() {
        // All LLM name variations should merge into the same key
        assert_eq!(normalize_pattern_key("Builder Pattern"), "builder");
        assert_eq!(normalize_pattern_key("builder-pattern"), "builder");
        assert_eq!(normalize_pattern_key("builder_pattern"), "builder");
        assert_eq!(normalize_pattern_key("Builder Design Pattern"), "builder");
        assert_eq!(normalize_pattern_key("Builder"), "builder");
        assert_eq!(normalize_pattern_key("BUILDER PATTERN"), "builder");
    }

    #[test]
    fn test_pattern_dedup_via_normalization() {
        use super::super::deep_analyzer::PatternInstance;

        let chunk1 = ChunkAnalysisResult {
            chunk_id: "chunk-1".to_string(),
            module_path: "src/api".to_string(),
            patterns: vec![PatternInstance {
                name: "Builder Pattern".to_string(),
                ..Default::default()
            }],
            file_count: 3,
            ..Default::default()
        };
        let chunk2 = ChunkAnalysisResult {
            chunk_id: "chunk-2".to_string(),
            module_path: "src/core".to_string(),
            patterns: vec![PatternInstance {
                name: "builder-pattern".to_string(),
                ..Default::default()
            }],
            file_count: 2,
            ..Default::default()
        };
        let chunk3 = ChunkAnalysisResult {
            chunk_id: "chunk-3".to_string(),
            module_path: "src/utils".to_string(),
            patterns: vec![PatternInstance {
                name: "Builder Design Pattern".to_string(),
                ..Default::default()
            }],
            file_count: 4,
            ..Default::default()
        };

        let result = AnalysisAggregator::aggregate(vec![chunk1, chunk2, chunk3], 9, 900);

        // All three should merge into a single pattern
        assert_eq!(result.patterns.len(), 1);
        assert_eq!(result.patterns[0].occurrence_count, 3);
        assert_eq!(result.patterns[0].modules.len(), 3);
    }
}
