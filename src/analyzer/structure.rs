//! Universal Structure Analyzer
//!
//! Extracts language-agnostic structural information from codebases.
//! Provides rich context for AI-based analysis without pattern matching.
//!
//! Key principles:
//! - No language-specific patterns (no "Controller", "Service" suffix matching)
//! - No framework-specific paths (no "/adapter/", "/domain/" detection)
//! - Focus on universal structural information that AI can interpret

use crate::storage::Database;
use crate::types::{Result, log_filter_error};

/// Universal structure analysis results
#[derive(Debug, Clone)]
pub struct StructureAnalysis {
    /// Directory tree with metrics
    pub directories: Vec<DirectoryInfo>,
    /// Entry points (files with no internal dependents)
    pub entry_points: Vec<EntryPoint>,
    /// Hotspots (most referenced code)
    pub hotspots: Vec<Hotspot>,
    /// Dependency clusters (groups of tightly coupled code)
    pub clusters: Vec<CodeCluster>,
    /// Build/config files found (language-agnostic detection)
    pub build_markers: Vec<BuildMarker>,
}

/// Directory information with universal metrics
#[derive(Debug, Clone)]
pub struct DirectoryInfo {
    pub path: String,
    pub depth: usize,
    pub file_count: i64,
    pub class_count: i64,
    pub function_count: i64,
    /// Files that depend on code in this directory
    pub dependents_count: i64,
    /// Files this directory depends on
    pub dependencies_count: i64,
    /// Ratio of external connections (indicates module boundary)
    pub boundary_score: f64,
}

/// Entry point detection (API endpoints, main files, exports)
#[derive(Debug, Clone)]
pub struct EntryPoint {
    pub node_id: String,
    pub name: String,
    pub path: String,
    pub node_type: String,
    /// Number of internal callers (0 = true entry point)
    pub internal_callers: i64,
    /// Number of things this calls
    pub outgoing_calls: i64,
    /// Reason for entry point classification
    pub reason: EntryPointReason,
}

#[derive(Debug, Clone)]
pub enum EntryPointReason {
    /// No internal code calls this
    NoInternalCallers,
    /// Named as main/index/entry
    MainFile,
    /// Exports to external (public API)
    PublicExport,
    /// HTTP/RPC endpoint indicator
    EndpointPattern,
}

/// Hotspot - highly referenced code
#[derive(Debug, Clone)]
pub struct Hotspot {
    pub node_id: String,
    pub name: String,
    pub path: String,
    pub node_type: String,
    /// How many other nodes reference this
    pub reference_count: i64,
    /// How many other nodes this references
    pub dependency_count: i64,
    /// Centrality score (importance in the graph)
    pub centrality: f64,
}

/// Code cluster - tightly coupled group
#[derive(Debug, Clone)]
pub struct CodeCluster {
    pub id: String,
    pub root_directory: String,
    pub node_count: i64,
    pub internal_edges: i64,
    pub external_edges: i64,
    /// Cohesion = internal_edges / (internal + external)
    pub cohesion: f64,
}

/// Build/config file markers
#[derive(Debug, Clone)]
pub struct BuildMarker {
    pub path: String,
    pub marker_type: BuildMarkerType,
}

#[derive(Debug, Clone)]
pub enum BuildMarkerType {
    /// package.json, Cargo.toml, build.gradle, etc.
    PackageDefinition,
    /// Main entry file (main.rs, index.ts, etc.)
    MainEntry,
    /// Configuration file
    Config,
}

/// Analyzes codebase structure without language-specific patterns
pub struct StructureAnalyzer<'a> {
    db: &'a Database,
}

impl<'a> StructureAnalyzer<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Perform complete structure analysis
    pub fn analyze(&self) -> Result<StructureAnalysis> {
        Ok(StructureAnalysis {
            directories: self.analyze_directories()?,
            entry_points: self.find_entry_points()?,
            hotspots: self.find_hotspots()?,
            clusters: self.find_clusters()?,
            build_markers: self.find_build_markers()?,
        })
    }

    /// Analyze directory structure with metrics
    fn analyze_directories(&self) -> Result<Vec<DirectoryInfo>> {
        let conn = self.db.connection()?;

        // Get top-level directories (first 2 levels)
        // SQLite doesn't have reverse(), so we use a simpler approach
        let mut stmt = conn.prepare(
            "WITH top_dirs AS (
                SELECT DISTINCT
                    CASE
                        WHEN instr(substr(path, 3), '/') > 0
                        THEN substr(path, 1, instr(substr(path, 3), '/') + 2)
                        ELSE substr(path, 1, instr(path || '/', '/'))
                    END as dir_path
                FROM nodes
                WHERE node_type = 'file'
                AND path NOT LIKE '%/test%'
            )
            SELECT
                d.dir_path,
                (SELECT COUNT(*) FROM nodes WHERE node_type = 'file' AND path LIKE d.dir_path || '%') as file_count,
                (SELECT COUNT(*) FROM nodes WHERE node_type = 'class' AND path LIKE d.dir_path || '%') as class_count,
                (SELECT COUNT(*) FROM nodes WHERE node_type IN ('function', 'method') AND path LIKE d.dir_path || '%') as func_count
            FROM top_dirs d
            WHERE d.dir_path != '' AND length(d.dir_path) > 1
            GROUP BY d.dir_path
            HAVING file_count >= 3
            ORDER BY file_count DESC
            LIMIT 100"
        )?;

        let directories: Vec<DirectoryInfo> = stmt
            .query_map([], |row| {
                let path: String = row.get(0)?;
                let depth = path.matches('/').count();
                Ok(DirectoryInfo {
                    path,
                    depth,
                    file_count: row.get(1)?,
                    class_count: row.get(2)?,
                    function_count: row.get(3)?,
                    dependents_count: 0, // Calculated separately
                    dependencies_count: 0,
                    boundary_score: 0.0,
                })
            })?
            .filter_map(|r| log_filter_error(r, "reading directory info"))
            .collect();

        Ok(directories)
    }

    /// Find entry points (nodes with no internal callers)
    fn find_entry_points(&self) -> Result<Vec<EntryPoint>> {
        let conn = self.db.connection()?;

        // Find nodes that are not targets of any edge (no one calls them internally)
        let mut stmt = conn.prepare(
            "SELECT n.id, n.name, n.path, n.node_type,
                (SELECT COUNT(*) FROM edges WHERE target_id = n.id) as internal_callers,
                (SELECT COUNT(*) FROM edges WHERE source_id = n.id) as outgoing_calls
             FROM nodes n
             WHERE n.node_type IN ('class', 'function', 'interface')
             AND NOT EXISTS (SELECT 1 FROM edges WHERE target_id = n.id AND edge_type = 'depends_on')
             ORDER BY outgoing_calls DESC
             LIMIT 50"
        )?;

        let entries: Vec<EntryPoint> = stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let path: String = row.get(2)?;

                // Determine reason
                let reason = if name.to_lowercase().contains("main")
                    || name.to_lowercase().contains("index")
                    || name.to_lowercase().contains("app")
                {
                    EntryPointReason::MainFile
                } else {
                    EntryPointReason::NoInternalCallers
                };

                Ok(EntryPoint {
                    node_id: row.get(0)?,
                    name,
                    path,
                    node_type: row.get(3)?,
                    internal_callers: row.get(4)?,
                    outgoing_calls: row.get(5)?,
                    reason,
                })
            })?
            .filter_map(|r| log_filter_error(r, "reading entry point"))
            .collect();

        Ok(entries)
    }

    /// Find hotspots (most referenced code)
    fn find_hotspots(&self) -> Result<Vec<Hotspot>> {
        let conn = self.db.connection()?;

        let mut stmt = conn.prepare(
            "SELECT
                n.id, n.name, n.path, n.node_type,
                (SELECT COUNT(*) FROM edges WHERE target_id = n.id) as ref_count,
                (SELECT COUNT(*) FROM edges WHERE source_id = n.id) as dep_count
             FROM nodes n
             WHERE n.node_type IN ('class', 'function', 'interface', 'module')
             ORDER BY ref_count DESC
             LIMIT 50",
        )?;

        let hotspots: Vec<Hotspot> = stmt
            .query_map([], |row| {
                let ref_count: i64 = row.get(4)?;
                let dep_count: i64 = row.get(5)?;
                let total = ref_count + dep_count;
                let centrality = if total > 0 {
                    ref_count as f64 / total as f64
                } else {
                    0.0
                };

                Ok(Hotspot {
                    node_id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    node_type: row.get(3)?,
                    reference_count: ref_count,
                    dependency_count: dep_count,
                    centrality,
                })
            })?
            .filter_map(|r| log_filter_error(r, "reading hotspot"))
            .collect();

        Ok(hotspots)
    }

    /// Find code clusters (tightly coupled groups)
    fn find_clusters(&self) -> Result<Vec<CodeCluster>> {
        let conn = self.db.connection()?;

        // Simple clustering by top-level directory
        let mut stmt = conn.prepare(
            "WITH dir_edges AS (
                SELECT
                    CASE WHEN instr(n1.path, '/') > 0
                         THEN substr(n1.path, 1, instr(substr(n1.path, 3), '/') + 1)
                         ELSE '.' END as src_dir,
                    CASE WHEN instr(n2.path, '/') > 0
                         THEN substr(n2.path, 1, instr(substr(n2.path, 3), '/') + 1)
                         ELSE '.' END as tgt_dir
                FROM edges e
                JOIN nodes n1 ON e.source_id = n1.id
                JOIN nodes n2 ON e.target_id = n2.id
            )
            SELECT
                src_dir,
                COUNT(*) as total_edges,
                SUM(CASE WHEN src_dir = tgt_dir THEN 1 ELSE 0 END) as internal_edges,
                SUM(CASE WHEN src_dir != tgt_dir THEN 1 ELSE 0 END) as external_edges
            FROM dir_edges
            WHERE src_dir != ''
            GROUP BY src_dir
            HAVING total_edges > 10
            ORDER BY internal_edges DESC
            LIMIT 30",
        )?;

        let clusters: Vec<CodeCluster> = stmt
            .query_map([], |row| {
                let root: String = row.get(0)?;
                let internal: i64 = row.get(2)?;
                let external: i64 = row.get(3)?;
                let total = internal + external;
                let cohesion = if total > 0 {
                    internal as f64 / total as f64
                } else {
                    0.0
                };

                Ok(CodeCluster {
                    id: format!("cluster:{}", root),
                    root_directory: root,
                    node_count: 0, // Would need separate query
                    internal_edges: internal,
                    external_edges: external,
                    cohesion,
                })
            })?
            .filter_map(|r| log_filter_error(r, "reading code cluster"))
            .collect();

        Ok(clusters)
    }

    /// Find build/config markers
    fn find_build_markers(&self) -> Result<Vec<BuildMarker>> {
        let conn = self.db.connection()?;

        let build_file_patterns = [
            // Package definitions
            ("package.json", BuildMarkerType::PackageDefinition),
            ("Cargo.toml", BuildMarkerType::PackageDefinition),
            ("build.gradle", BuildMarkerType::PackageDefinition),
            ("build.gradle.kts", BuildMarkerType::PackageDefinition),
            ("pom.xml", BuildMarkerType::PackageDefinition),
            ("pyproject.toml", BuildMarkerType::PackageDefinition),
            ("setup.py", BuildMarkerType::PackageDefinition),
            ("go.mod", BuildMarkerType::PackageDefinition),
            ("Gemfile", BuildMarkerType::PackageDefinition),
            // Main entries
            ("main.rs", BuildMarkerType::MainEntry),
            ("main.ts", BuildMarkerType::MainEntry),
            ("main.py", BuildMarkerType::MainEntry),
            ("main.go", BuildMarkerType::MainEntry),
            ("main.kt", BuildMarkerType::MainEntry),
            ("main.java", BuildMarkerType::MainEntry),
            ("index.ts", BuildMarkerType::MainEntry),
            ("index.js", BuildMarkerType::MainEntry),
            ("app.ts", BuildMarkerType::MainEntry),
            ("app.py", BuildMarkerType::MainEntry),
        ];

        let mut markers = Vec::new();

        for (pattern, marker_type) in &build_file_patterns {
            let mut stmt =
                conn.prepare("SELECT path FROM nodes WHERE node_type = 'file' AND path LIKE ?")?;

            let paths: Vec<String> = stmt
                .query_map([format!("%{}", pattern)], |row| row.get(0))?
                .filter_map(|r| log_filter_error(r, "reading build marker path"))
                .collect();

            for path in paths {
                markers.push(BuildMarker {
                    path,
                    marker_type: marker_type.clone(),
                });
            }
        }

        Ok(markers)
    }
}

/// Code sample extractor for AI context
pub struct CodeSampleExtractor;

impl CodeSampleExtractor {
    /// Extract code signature/sample for a node
    /// This provides actual code context for AI to understand what the code does
    pub fn extract_signature(metadata: Option<&str>) -> Option<String> {
        let meta: serde_json::Value = metadata
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or_default();

        // Try to get signature from metadata
        if let Some(sig) = meta.get("signature").and_then(|v| v.as_str())
            && !sig.is_empty()
        {
            return Some(sig.to_string());
        }

        // Try description
        if let Some(desc) = meta.get("description").and_then(|v| v.as_str())
            && !desc.is_empty()
        {
            return Some(desc.to_string());
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_point_reason() {
        let reason = EntryPointReason::NoInternalCallers;
        assert!(matches!(reason, EntryPointReason::NoInternalCallers));
    }

    #[test]
    fn test_code_sample_extractor() {
        let meta = r#"{"signature": "fn main() -> Result<()>"}"#;
        let sig = CodeSampleExtractor::extract_signature(Some(meta));
        assert_eq!(sig, Some("fn main() -> Result<()>".to_string()));
    }
}
