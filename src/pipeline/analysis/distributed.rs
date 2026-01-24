//! Distributed Analysis
//!
//! Handles large-scale codebase analysis through:
//! - Module-based partitioning
//! - Parallel chunk analysis with semaphore control
//! - Result merging with deduplication

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

use crate::ai::LlmProvider;
use crate::types::Result;

/// Chunk of files to analyze together
#[derive(Debug, Clone)]
pub struct AnalysisChunk {
    pub id: String,
    pub module_path: PathBuf,
    pub files: Vec<PathBuf>,
    pub priority: u8,
}

impl AnalysisChunk {
    pub fn new(module_path: PathBuf, files: Vec<PathBuf>) -> Self {
        let id = format!("chunk_{}", Self::path_hash(&module_path));
        let priority = Self::calculate_priority(&files);
        Self {
            id,
            module_path,
            files,
            priority,
        }
    }

    fn path_hash(path: &Path) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        format!("{:x}", hasher.finish())[..8].to_string()
    }

    fn calculate_priority(files: &[PathBuf]) -> u8 {
        // Higher priority for more files
        let count_priority = (files.len().min(10) * 5) as u8;

        // Higher priority for core paths
        let has_core = files.iter().any(|f| {
            let s = f.to_string_lossy();
            s.contains("/core/") || s.contains("/main") || s.contains("/lib")
        });

        count_priority + if has_core { 20 } else { 0 }
    }
}

/// Result of analyzing a single chunk
#[derive(Debug, Clone)]
pub struct ChunkAnalysisResult {
    pub chunk_id: String,
    pub module_path: PathBuf,
    pub insights: Vec<ChunkInsight>,
    pub patterns: Vec<ChunkPattern>,
    pub constraints: Vec<ChunkConstraint>,
    pub confidence: f32,
}

/// Insight extracted from a chunk
#[derive(Debug, Clone)]
pub struct ChunkInsight {
    pub title: String,
    pub description: String,
    pub evidence: Vec<String>,
    pub tier: u8, // 1-3
}

/// Pattern detected in a chunk
#[derive(Debug, Clone)]
pub struct ChunkPattern {
    pub name: String,
    pub description: String,
    pub locations: Vec<String>,
}

/// Constraint found in a chunk
#[derive(Debug, Clone)]
pub struct ChunkConstraint {
    pub name: String,
    pub description: String,
    pub evidence: Vec<String>,
    pub severity: ConstraintSeverity,
}

#[derive(Debug, Clone, Copy)]
pub enum ConstraintSeverity {
    Critical,
    Important,
    Minor,
}

/// Partitions a codebase into analysis chunks
pub struct ModulePartitioner {
    max_files_per_chunk: usize,
    module_depth: usize,
}

impl ModulePartitioner {
    pub fn new() -> Self {
        Self {
            max_files_per_chunk: 50,
            module_depth: 2,
        }
    }

    pub fn with_max_files(mut self, max: usize) -> Self {
        self.max_files_per_chunk = max;
        self
    }

    pub fn with_module_depth(mut self, depth: usize) -> Self {
        self.module_depth = depth;
        self
    }

    /// Partition files into chunks based on module structure
    pub fn partition(&self, files: &[PathBuf]) -> Vec<AnalysisChunk> {
        let mut module_files: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

        // Group files by module path
        for file in files {
            let module_path = self.extract_module_path(file);
            module_files
                .entry(module_path)
                .or_default()
                .push(file.clone());
        }

        // Create chunks, splitting large modules
        let mut chunks = Vec::new();
        for (module_path, files) in module_files {
            if files.len() <= self.max_files_per_chunk {
                chunks.push(AnalysisChunk::new(module_path, files));
            } else {
                // Split large module into sub-chunks
                for (i, sub_files) in files.chunks(self.max_files_per_chunk).enumerate() {
                    let sub_path = module_path.join(format!("_part{}", i + 1));
                    chunks.push(AnalysisChunk::new(sub_path, sub_files.to_vec()));
                }
            }
        }

        // Sort by priority (descending)
        chunks.sort_by(|a, b| b.priority.cmp(&a.priority));

        info!(
            chunks = chunks.len(),
            total_files = files.len(),
            "Partitioned codebase into chunks"
        );

        chunks
    }

    fn extract_module_path(&self, file: &Path) -> PathBuf {
        let components: Vec<_> = file.components().take(self.module_depth + 1).collect();

        if components.is_empty() {
            PathBuf::from("root")
        } else {
            components.iter().collect()
        }
    }
}

impl Default for ModulePartitioner {
    fn default() -> Self {
        Self::new()
    }
}

/// Merged analysis result from all chunks
#[derive(Debug, Clone, Default)]
pub struct MergedAnalysis {
    pub insights: Vec<ChunkInsight>,
    pub patterns: Vec<ChunkPattern>,
    pub constraints: Vec<ChunkConstraint>,
    pub module_dependencies: Vec<ModuleDependency>,
    pub overall_confidence: f32,
    pub chunks_analyzed: usize,
    pub chunks_failed: usize,
}

#[derive(Debug, Clone)]
pub struct ModuleDependency {
    pub from_module: PathBuf,
    pub to_module: PathBuf,
    pub dependency_type: DependencyType,
}

#[derive(Debug, Clone, Copy)]
pub enum DependencyType {
    Import,
    Api,
    Data,
}

/// Merges results from parallel chunk analysis
pub struct AnalysisMerger;

impl AnalysisMerger {
    /// Merge chunk results into a unified analysis
    pub fn merge(results: Vec<ChunkAnalysisResult>) -> MergedAnalysis {
        let mut all_insights = Vec::new();
        let mut all_patterns = Vec::new();
        let mut all_constraints = Vec::new();
        let mut total_confidence = 0.0;
        let chunks_analyzed = results.len();

        for result in &results {
            all_insights.extend(result.insights.clone());
            all_patterns.extend(result.patterns.clone());
            all_constraints.extend(result.constraints.clone());
            total_confidence += result.confidence;
        }

        // Deduplicate insights by title
        let insights = Self::deduplicate_insights(all_insights);

        // Deduplicate patterns by name
        let patterns = Self::deduplicate_patterns(all_patterns);

        // Deduplicate and prioritize constraints
        let constraints = Self::deduplicate_constraints(all_constraints);

        // Analyze cross-module dependencies
        let module_dependencies = Self::analyze_dependencies(&results);

        let overall_confidence = if chunks_analyzed > 0 {
            total_confidence / chunks_analyzed as f32
        } else {
            0.0
        };

        info!(
            insights = insights.len(),
            patterns = patterns.len(),
            constraints = constraints.len(),
            dependencies = module_dependencies.len(),
            "Merged chunk analysis results"
        );

        MergedAnalysis {
            insights,
            patterns,
            constraints,
            module_dependencies,
            overall_confidence,
            chunks_analyzed,
            chunks_failed: 0,
        }
    }

    fn deduplicate_insights(insights: Vec<ChunkInsight>) -> Vec<ChunkInsight> {
        let mut seen = HashMap::new();

        for insight in insights {
            seen.entry(insight.title.clone()).or_insert(insight);
        }

        seen.into_values().collect()
    }

    fn deduplicate_patterns(patterns: Vec<ChunkPattern>) -> Vec<ChunkPattern> {
        let mut seen: HashMap<String, ChunkPattern> = HashMap::new();

        for pattern in patterns {
            seen.entry(pattern.name.clone())
                .and_modify(|p| p.locations.extend(pattern.locations.clone()))
                .or_insert(pattern);
        }

        seen.into_values().collect()
    }

    fn deduplicate_constraints(constraints: Vec<ChunkConstraint>) -> Vec<ChunkConstraint> {
        let mut seen = HashMap::new();

        for constraint in constraints {
            seen.entry(constraint.name.clone())
                .and_modify(|c: &mut ChunkConstraint| {
                    c.evidence.extend(constraint.evidence.clone());
                })
                .or_insert(constraint);
        }

        // Sort by severity (critical first)
        let mut result: Vec<_> = seen.into_values().collect();
        result.sort_by_key(|c| match c.severity {
            ConstraintSeverity::Critical => 0,
            ConstraintSeverity::Important => 1,
            ConstraintSeverity::Minor => 2,
        });

        result
    }

    fn analyze_dependencies(results: &[ChunkAnalysisResult]) -> Vec<ModuleDependency> {
        // Basic dependency analysis based on evidence paths
        let mut dependencies = Vec::new();
        let modules: Vec<_> = results.iter().map(|r| &r.module_path).collect();

        for result in results {
            for insight in &result.insights {
                for evidence in &insight.evidence {
                    for other_module in &modules {
                        if *other_module != &result.module_path {
                            let other_str = other_module.to_string_lossy();
                            if evidence.contains(other_str.as_ref()) {
                                dependencies.push(ModuleDependency {
                                    from_module: result.module_path.clone(),
                                    to_module: (*other_module).clone(),
                                    dependency_type: DependencyType::Import,
                                });
                            }
                        }
                    }
                }
            }
        }

        dependencies
    }
}

/// Parallel analyzer with concurrency control
pub struct ParallelAnalyzer<A: ChunkAnalyzer> {
    analyzer: Arc<A>,
    max_concurrent: usize,
}

impl<A: ChunkAnalyzer + Send + Sync + 'static> ParallelAnalyzer<A> {
    pub fn new(analyzer: A, max_concurrent: usize) -> Self {
        Self {
            analyzer: Arc::new(analyzer),
            max_concurrent,
        }
    }

    /// Analyze all chunks in parallel with semaphore control
    pub async fn analyze_all(&self, chunks: Vec<AnalysisChunk>) -> Result<MergedAnalysis> {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let mut join_set = JoinSet::new();
        let total_chunks = chunks.len();

        info!(
            chunks = total_chunks,
            max_concurrent = self.max_concurrent,
            "Starting parallel chunk analysis"
        );

        for chunk in chunks {
            let permit = semaphore.clone().acquire_owned().await.map_err(|e| {
                crate::types::ClaudegenError::Storage(format!("Semaphore closed: {}", e))
            })?;
            let analyzer = Arc::clone(&self.analyzer);

            join_set.spawn(async move {
                let result = analyzer.analyze(&chunk).await;
                drop(permit); // Release semaphore
                (chunk.id, result)
            });
        }

        let mut results = Vec::new();
        let mut failed = 0;

        while let Some(task_result) = join_set.join_next().await {
            match task_result {
                Ok((chunk_id, Ok(analysis))) => {
                    debug!(chunk_id = %chunk_id, "Chunk analysis completed");
                    results.push(analysis);
                }
                Ok((chunk_id, Err(e))) => {
                    warn!(chunk_id = %chunk_id, error = %e, "Chunk analysis failed");
                    failed += 1;
                }
                Err(e) => {
                    warn!(error = %e, "Chunk task panicked");
                    failed += 1;
                }
            }
        }

        let mut merged = AnalysisMerger::merge(results);
        merged.chunks_failed = failed;

        info!(
            analyzed = merged.chunks_analyzed,
            failed = merged.chunks_failed,
            insights = merged.insights.len(),
            "Parallel analysis completed"
        );

        Ok(merged)
    }
}

/// Trait for chunk analyzers
#[async_trait::async_trait]
pub trait ChunkAnalyzer: Send + Sync {
    async fn analyze(&self, chunk: &AnalysisChunk) -> Result<ChunkAnalysisResult>;
}

/// Simple LLM-based chunk analyzer
pub struct LlmChunkAnalyzer {
    provider: Arc<dyn LlmProvider>,
}

impl LlmChunkAnalyzer {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait::async_trait]
impl ChunkAnalyzer for LlmChunkAnalyzer {
    async fn analyze(&self, chunk: &AnalysisChunk) -> Result<ChunkAnalysisResult> {
        // Build file list for prompt
        let file_list = chunk
            .files
            .iter()
            .map(|f| format!("- {}", f.display()))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r##"Analyze this module for insights, patterns, and constraints.

MODULE: {}
FILES:
{}

Extract:
1. Key insights (title, description, evidence files)
2. Code patterns (name, description, locations)
3. Constraints (name, description, evidence, severity)

Focus on project-specific details, not generic advice.
Return JSON with insights, patterns, and constraints arrays."##,
            chunk.module_path.display(),
            file_list
        );

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "insights": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "description": {"type": "string"},
                            "evidence": {"type": "array", "items": {"type": "string"}},
                            "tier": {"type": "integer"}
                        }
                    }
                },
                "patterns": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "description": {"type": "string"},
                            "locations": {"type": "array", "items": {"type": "string"}}
                        }
                    }
                },
                "constraints": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "description": {"type": "string"},
                            "evidence": {"type": "array", "items": {"type": "string"}},
                            "severity": {"type": "string"}
                        }
                    }
                }
            }
        });

        let response = self.provider.generate(&prompt, &schema).await?;

        // Parse response
        let insights = response
            .content
            .get("insights")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| {
                        Some(ChunkInsight {
                            title: i.get("title")?.as_str()?.to_string(),
                            description: i.get("description")?.as_str()?.to_string(),
                            evidence: i
                                .get("evidence")?
                                .as_array()?
                                .iter()
                                .filter_map(|e| e.as_str().map(String::from))
                                .collect(),
                            tier: i.get("tier")?.as_u64()? as u8,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let patterns = response
            .content
            .get("patterns")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        Some(ChunkPattern {
                            name: p.get("name")?.as_str()?.to_string(),
                            description: p.get("description")?.as_str()?.to_string(),
                            locations: p
                                .get("locations")?
                                .as_array()?
                                .iter()
                                .filter_map(|l| l.as_str().map(String::from))
                                .collect(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let constraints = response
            .content
            .get("constraints")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        let severity_str = c.get("severity")?.as_str()?;
                        let severity = match severity_str.to_lowercase().as_str() {
                            "critical" => ConstraintSeverity::Critical,
                            "important" => ConstraintSeverity::Important,
                            _ => ConstraintSeverity::Minor,
                        };
                        Some(ChunkConstraint {
                            name: c.get("name")?.as_str()?.to_string(),
                            description: c.get("description")?.as_str()?.to_string(),
                            evidence: c
                                .get("evidence")?
                                .as_array()?
                                .iter()
                                .filter_map(|e| e.as_str().map(String::from))
                                .collect(),
                            severity,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(ChunkAnalysisResult {
            chunk_id: chunk.id.clone(),
            module_path: chunk.module_path.clone(),
            insights,
            patterns,
            constraints,
            confidence: 0.8, // Default confidence
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_partitioner() {
        let partitioner = ModulePartitioner::new();
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/api/mod.rs"),
            PathBuf::from("src/api/routes.rs"),
            PathBuf::from("tests/test_main.rs"),
        ];

        let chunks = partitioner.partition(&files);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_analysis_chunk_priority() {
        let core_files = vec![PathBuf::from("src/core/main.rs")];
        let test_files = vec![PathBuf::from("tests/test.rs")];

        let core_chunk = AnalysisChunk::new(PathBuf::from("src/core"), core_files);
        let test_chunk = AnalysisChunk::new(PathBuf::from("tests"), test_files);

        assert!(core_chunk.priority > test_chunk.priority);
    }

    #[test]
    fn test_analysis_merger() {
        let result1 = ChunkAnalysisResult {
            chunk_id: "chunk1".to_string(),
            module_path: PathBuf::from("src/api"),
            insights: vec![ChunkInsight {
                title: "API Pattern".to_string(),
                description: "Description".to_string(),
                evidence: vec!["src/api/mod.rs:10".to_string()],
                tier: 3,
            }],
            patterns: vec![],
            constraints: vec![],
            confidence: 0.9,
        };

        let result2 = ChunkAnalysisResult {
            chunk_id: "chunk2".to_string(),
            module_path: PathBuf::from("src/db"),
            insights: vec![ChunkInsight {
                title: "DB Pattern".to_string(),
                description: "Description".to_string(),
                evidence: vec!["src/db/mod.rs:5".to_string()],
                tier: 2,
            }],
            patterns: vec![],
            constraints: vec![],
            confidence: 0.85,
        };

        let merged = AnalysisMerger::merge(vec![result1, result2]);
        assert_eq!(merged.insights.len(), 2);
        assert_eq!(merged.chunks_analyzed, 2);
    }
}
