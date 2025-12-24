//! Batch Prioritizer for Bottom-Up Analysis
//!
//! Implements leaf-first hierarchical processing order:
//! 1. Low importance files first (utilities, helpers)
//! 2. Medium importance files
//! 3. High importance files (can reference lower-level docs)
//! 4. Critical files last (entry points, core architecture)
//!
//! This ensures parent/core modules can link to already-documented child modules.

use crate::wiki::exhaustive::characterization::profile::{KeyArea, ProjectProfile};
use crate::wiki::exhaustive::types::Importance;

use super::types::ProcessingTier;

/// File with its processing metadata
#[derive(Debug, Clone)]
pub struct PrioritizedFile {
    pub path: String,
    pub tier: ProcessingTier,
    pub is_entry_point: bool,
    pub depth: usize,
}

pub struct BatchPrioritizer {
    key_areas: Vec<KeyArea>,
}

impl BatchPrioritizer {
    pub fn new(profile: &ProjectProfile) -> Self {
        Self {
            key_areas: profile.key_areas.clone(),
        }
    }

    /// Prioritize files with full metadata for processing
    pub fn prioritize_with_metadata(&self, files: Vec<String>) -> Vec<PrioritizedFile> {
        let mut prioritized: Vec<PrioritizedFile> = files
            .into_iter()
            .map(|path| {
                let tier = self.get_tier(&path);
                let is_entry_point = self.is_entry_point(&path);
                let depth = path.matches('/').count();
                PrioritizedFile {
                    path,
                    tier,
                    is_entry_point,
                    depth,
                }
            })
            .collect();

        // Leaf-first ordering: lower tier value = processed first
        // Entry points always processed last within their tier
        // Deeper files first within same tier (more specific modules first)
        prioritized.sort_by(|a, b| {
            (a.tier as u8)
                .cmp(&(b.tier as u8))
                .then_with(|| a.is_entry_point.cmp(&b.is_entry_point))
                .then_with(|| b.depth.cmp(&a.depth))
        });

        prioritized
    }

    /// Simple prioritization returning just paths (backward compatible)
    pub fn prioritize(&self, files: Vec<String>) -> Vec<String> {
        self.prioritize_with_metadata(files)
            .into_iter()
            .map(|pf| pf.path)
            .collect()
    }

    /// Get processing tier for a file
    pub fn get_tier(&self, file: &str) -> ProcessingTier {
        // Entry points are always Core tier
        if self.is_entry_point(file) {
            return ProcessingTier::Core;
        }

        // Check against key areas
        for area in &self.key_areas {
            if file.starts_with(&area.path) || file.contains(&area.path) {
                return match area.importance {
                    Importance::Critical => ProcessingTier::Core,
                    Importance::High => ProcessingTier::Important,
                    Importance::Medium => ProcessingTier::Standard,
                    Importance::Low => ProcessingTier::Leaf,
                };
            }
        }

        // Default tier based on path heuristics
        self.infer_tier_from_path(file)
    }

    /// Check if file is an entry point (should be processed last)
    fn is_entry_point(&self, file: &str) -> bool {
        let filename = file.rsplit('/').next().unwrap_or(file);
        let name_lower = filename.to_lowercase();

        matches!(
            name_lower.as_str(),
            "main.rs"
                | "lib.rs"
                | "mod.rs"
                | "index.ts"
                | "index.js"
                | "index.tsx"
                | "index.jsx"
                | "__init__.py"
                | "main.py"
                | "main.go"
                | "main.java"
                | "app.py"
                | "app.ts"
                | "app.js"
                | "server.ts"
                | "server.js"
                | "main.c"
                | "main.cpp"
        )
    }

    /// Infer tier from file path patterns
    fn infer_tier_from_path(&self, file: &str) -> ProcessingTier {
        let file_lower = file.to_lowercase();

        // Utilities - Leaf tier
        if file_lower.contains("/util")
            || file_lower.contains("/helper")
            || file_lower.contains("/common")
            || file_lower.contains("/shared")
            || file_lower.contains("/constants")
        {
            return ProcessingTier::Leaf;
        }

        // Type definitions - Leaf/Standard
        if file_lower.contains("/types")
            || file_lower.contains("/models")
            || file_lower.contains("/schema")
            || file_lower.contains("/dto")
            || file_lower.contains("/entities")
        {
            return ProcessingTier::Leaf;
        }

        // Core business logic - Important
        if file_lower.contains("/core")
            || file_lower.contains("/service")
            || file_lower.contains("/domain")
            || file_lower.contains("/engine")
        {
            return ProcessingTier::Important;
        }

        // API/CLI/UI - Important
        if file_lower.contains("/api")
            || file_lower.contains("/cli")
            || file_lower.contains("/handler")
            || file_lower.contains("/controller")
            || file_lower.contains("/route")
        {
            return ProcessingTier::Important;
        }

        ProcessingTier::Standard
    }

    /// Get files that should provide context for a given file
    pub fn get_child_files<'a>(
        &self,
        target_file: &str,
        all_files: &'a [PrioritizedFile],
    ) -> Vec<&'a PrioritizedFile> {
        let target_tier = self.get_tier(target_file);

        // Only Important and Core tiers get child context
        if !target_tier.uses_child_context() {
            return Vec::new();
        }

        let target_dir = target_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");

        all_files
            .iter()
            .filter(|pf| {
                // Must be lower tier (already processed)
                (pf.tier as u8) < (target_tier as u8)
                    // Must be in same directory or subdirectory
                    && (pf.path.starts_with(target_dir) || self.is_related_path(&pf.path, target_file))
            })
            .collect()
    }

    /// Check if two paths are related (same module hierarchy)
    fn is_related_path(&self, path1: &str, path2: &str) -> bool {
        // Same parent directory
        let parent1 = path1.rsplit_once('/').map(|(d, _)| d);
        let parent2 = path2.rsplit_once('/').map(|(d, _)| d);
        parent1 == parent2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_profile() -> ProjectProfile {
        ProjectProfile {
            key_areas: vec![
                KeyArea {
                    path: "src/core".to_string(),
                    importance: Importance::Critical,
                    focus_reasons: vec!["Core module".to_string()],
                },
                KeyArea {
                    path: "src/api".to_string(),
                    importance: Importance::High,
                    focus_reasons: vec!["API module".to_string()],
                },
                KeyArea {
                    path: "src/utils".to_string(),
                    importance: Importance::Low,
                    focus_reasons: vec!["Utilities".to_string()],
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_leaf_first_ordering() {
        let profile = make_profile();
        let prioritizer = BatchPrioritizer::new(&profile);

        let files = vec![
            "src/core/engine.rs".to_string(),
            "src/api/handler.rs".to_string(),
            "src/utils/helper.rs".to_string(),
            "src/types/model.rs".to_string(),
        ];

        let prioritized = prioritizer.prioritize(files);

        // Low importance (utils, types) should come first
        assert!(
            prioritized[0].contains("utils") || prioritized[0].contains("types"),
            "Expected utils/types first, got: {:?}",
            prioritized
        );
        // Core should come last
        let last = prioritized.last().expect("prioritized list is non-empty");
        assert!(
            last.contains("core"),
            "Expected core last, got: {:?}",
            prioritized
        );
    }

    #[test]
    fn test_entry_points_last() {
        let profile = make_profile();
        let prioritizer = BatchPrioritizer::new(&profile);

        let files = vec![
            "src/lib.rs".to_string(),
            "src/main.rs".to_string(),
            "src/utils/helper.rs".to_string(),
        ];

        let prioritized = prioritizer.prioritize(files);

        // Entry points should be at the end
        let last = prioritized.last().expect("prioritized list is non-empty");
        assert!(
            last == "src/main.rs" || last == "src/lib.rs",
            "Expected entry point last, got: {}",
            last
        );
    }

    #[test]
    fn test_tier_assignment() {
        let profile = make_profile();
        let prioritizer = BatchPrioritizer::new(&profile);

        assert_eq!(
            prioritizer.get_tier("src/core/engine.rs"),
            ProcessingTier::Core
        );
        assert_eq!(
            prioritizer.get_tier("src/api/handler.rs"),
            ProcessingTier::Important
        );
        assert_eq!(
            prioritizer.get_tier("src/utils/helper.rs"),
            ProcessingTier::Leaf
        );
        assert_eq!(prioritizer.get_tier("src/main.rs"), ProcessingTier::Core);
    }

    #[test]
    fn test_prioritize_with_metadata() {
        let profile = make_profile();
        let prioritizer = BatchPrioritizer::new(&profile);

        let files = vec!["src/main.rs".to_string(), "src/utils/helper.rs".to_string()];

        let prioritized = prioritizer.prioritize_with_metadata(files);

        assert_eq!(prioritized.len(), 2);
        assert_eq!(prioritized[0].path, "src/utils/helper.rs");
        assert_eq!(prioritized[0].tier, ProcessingTier::Leaf);
        assert!(!prioritized[0].is_entry_point);

        assert_eq!(prioritized[1].path, "src/main.rs");
        assert_eq!(prioritized[1].tier, ProcessingTier::Core);
        assert!(prioritized[1].is_entry_point);
    }
}
