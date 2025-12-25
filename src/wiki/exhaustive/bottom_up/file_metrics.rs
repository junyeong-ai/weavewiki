//! File metrics extraction for intelligent tier classification
//!
//! Uses AST and graph data to compute importance metrics based on:
//! - Structural complexity (functions, classes)
//! - Dependency graph position (imports, dependents)
//! - Interface implementations
//!
//! This provides a data-driven approach to tier classification that
//! complements the heuristic-based prioritizer.
//!
//! ## Usage
//!
//! ```no_run
//! use weavewiki::storage::Database;
//! use weavewiki::wiki::exhaustive::bottom_up::FileMetrics;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let db = Database::open(".weavewiki/weavewiki.db")?;
//! let metrics = FileMetrics::from_database(&db, "src/main.rs")?;
//!
//! println!("File: {}", metrics.path);
//! println!("Functions: {}", metrics.function_count);
//! println!("Dependents: {}", metrics.dependent_count);
//! println!("Complexity: {:.2}", metrics.complexity_score);
//! println!("Suggested tier: {}", metrics.suggested_tier());
//! # Ok(())
//! # }
//! ```
//!
//! ## Integration with BatchPrioritizer
//!
//! ```no_run
//! use weavewiki::storage::Database;
//! use weavewiki::wiki::exhaustive::bottom_up::BatchPrioritizer;
//! use weavewiki::wiki::exhaustive::characterization::profile::ProjectProfile;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let db = Database::open(".weavewiki/weavewiki.db")?;
//! let profile = ProjectProfile::default();
//! let prioritizer = BatchPrioritizer::new(&profile);
//!
//! let files = vec!["src/main.rs".to_string(), "src/utils.rs".to_string()];
//!
//! // Use graph metrics for better tier classification
//! let prioritized = prioritizer.prioritize_with_metrics(files, Some(&db));
//!
//! for file in prioritized {
//!     println!("{}: {:?}", file.path, file.tier);
//! }
//! # Ok(())
//! # }
//! ```

use crate::storage::Database;
use crate::types::Result;

/// Enriched metrics for a file, computed from AST and graph data
#[derive(Debug, Clone)]
pub struct FileMetrics {
    pub path: String,
    pub lines_of_code: usize,
    pub function_count: usize,
    pub class_count: usize,
    pub import_count: usize,     // Files this imports
    pub dependent_count: usize,  // Files that import this
    pub implements_count: usize, // Trait/interface implementations
    pub is_entry_point: bool,
    pub complexity_score: f32, // Computed from above metrics (0-1)
}

impl FileMetrics {
    /// Extract metrics for a file from the database
    ///
    /// Queries the knowledge graph for structural nodes and dependency edges
    /// to build a comprehensive picture of the file's importance.
    pub fn from_database(db: &Database, file_path: &str) -> Result<Self> {
        // Query structural nodes (parser-extracted)
        let nodes = db.get_file_structural_nodes(file_path)?;

        // Count by node type
        let function_count = nodes
            .iter()
            .filter(|n| {
                matches!(
                    n.node_type,
                    crate::types::node::NodeType::Function | crate::types::node::NodeType::Method
                )
            })
            .count();

        let class_count = nodes
            .iter()
            .filter(|n| {
                matches!(
                    n.node_type,
                    crate::types::node::NodeType::Class
                        | crate::types::node::NodeType::Type
                        | crate::types::node::NodeType::Interface
                )
            })
            .count();

        // Query dependencies (DependsOn edges outgoing from this file)
        let dependencies = db.get_file_dependencies(file_path)?;
        let import_count = dependencies.len();

        // Query dependents (files that depend on this file)
        let dependents = db.get_file_dependents(file_path)?;
        let dependent_count = dependents.len();

        // Query implements edges
        let implements = db.get_file_implements(file_path)?;
        let implements_count = implements.len();

        // Detect entry points: files with functions but no dependents
        let is_entry_point = dependent_count == 0 && function_count > 0 && class_count == 0;

        // Compute complexity score (normalized 0-1)
        let complexity_score = Self::compute_complexity(
            function_count,
            class_count,
            import_count,
            dependent_count,
            implements_count,
        );

        Ok(Self {
            path: file_path.to_string(),
            lines_of_code: 0, // Will be filled from file scan if needed
            function_count,
            class_count,
            import_count,
            dependent_count,
            implements_count,
            is_entry_point,
            complexity_score,
        })
    }

    /// Compute normalized complexity score
    ///
    /// Weighted formula prioritizing:
    /// - High weight for dependent_count (files that import this)
    /// - Medium weight for classes (complex types)
    /// - Medium weight for implements (interface implementations)
    /// - Lower weight for functions and imports
    ///
    /// Returns a score in range [0.0, 1.0]
    fn compute_complexity(
        functions: usize,
        classes: usize,
        imports: usize,
        dependents: usize,
        implements: usize,
    ) -> f32 {
        // Weighted formula
        let raw = (functions as f32 * 1.0)
            + (classes as f32 * 2.0)
            + (imports as f32 * 0.5)
            + (dependents as f32 * 3.0) // High weight for files depended upon
            + (implements as f32 * 1.5);

        // Normalize to 0-1 range (cap at 100)
        (raw / 100.0).min(1.0)
    }

    /// Suggest processing tier based on metrics
    ///
    /// Classification rules:
    /// - Core: Entry points or highly depended-upon files (10+ dependents)
    /// - Important: Moderately depended-upon (5+ dependents) or high complexity (>0.6)
    /// - Standard: Some structure (3+ functions or 1+ class)
    /// - Leaf: Everything else (utilities, simple helpers)
    pub fn suggested_tier(&self) -> &'static str {
        if self.is_entry_point || self.dependent_count > 10 {
            "core"
        } else if self.dependent_count > 5 || self.complexity_score > 0.6 {
            "important"
        } else if self.function_count > 3 || self.class_count > 1 {
            "standard"
        } else {
            "leaf"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_complexity() {
        // Minimal file: 1 function
        let score = FileMetrics::compute_complexity(1, 0, 0, 0, 0);
        assert!(score > 0.0 && score < 0.1);

        // Simple file: 5 functions
        let score = FileMetrics::compute_complexity(5, 0, 0, 0, 0);
        assert!(score > 0.0 && score < 0.1);

        // Complex file: 10 functions, 2 classes
        let score = FileMetrics::compute_complexity(10, 2, 5, 0, 0);
        assert!(score > 0.1 && score < 0.3);

        // Important file: 5 dependents
        let score = FileMetrics::compute_complexity(5, 1, 3, 5, 0);
        assert!(score > 0.2);

        // Core file: 15 dependents
        let score = FileMetrics::compute_complexity(10, 2, 5, 15, 2);
        assert!(score > 0.5);
    }

    #[test]
    fn test_suggested_tier() {
        // Leaf tier: minimal structure
        let metrics = FileMetrics {
            path: "utils/helper.rs".to_string(),
            lines_of_code: 50,
            function_count: 2,
            class_count: 0,
            import_count: 1,
            dependent_count: 0,
            implements_count: 0,
            is_entry_point: false,
            complexity_score: 0.05,
        };
        assert_eq!(metrics.suggested_tier(), "leaf");

        // Standard tier: moderate structure
        let metrics = FileMetrics {
            path: "services/processor.rs".to_string(),
            lines_of_code: 200,
            function_count: 8,
            class_count: 1,
            import_count: 5,
            dependent_count: 2,
            implements_count: 0,
            is_entry_point: false,
            complexity_score: 0.25,
        };
        assert_eq!(metrics.suggested_tier(), "standard");

        // Important tier: high complexity
        let metrics = FileMetrics {
            path: "core/engine.rs".to_string(),
            lines_of_code: 500,
            function_count: 15,
            class_count: 3,
            import_count: 10,
            dependent_count: 7,
            implements_count: 2,
            is_entry_point: false,
            complexity_score: 0.65,
        };
        assert_eq!(metrics.suggested_tier(), "important");

        // Core tier: many dependents
        let metrics = FileMetrics {
            path: "types/common.rs".to_string(),
            lines_of_code: 300,
            function_count: 10,
            class_count: 5,
            import_count: 2,
            dependent_count: 15,
            implements_count: 3,
            is_entry_point: false,
            complexity_score: 0.55,
        };
        assert_eq!(metrics.suggested_tier(), "core");

        // Core tier: entry point
        let metrics = FileMetrics {
            path: "main.rs".to_string(),
            lines_of_code: 100,
            function_count: 3,
            class_count: 0,
            import_count: 5,
            dependent_count: 0,
            implements_count: 0,
            is_entry_point: true,
            complexity_score: 0.15,
        };
        assert_eq!(metrics.suggested_tier(), "core");
    }
}
