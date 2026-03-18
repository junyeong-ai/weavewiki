//! CLAUDE.md Import Priority Management
//!
//! Handles priority-based import ordering for CLAUDE.md:
//! - Framework/Tech rules first (highest priority, always included)
//! - Module rules sorted by file count (more files = higher priority)
//! - Group rules last (lowest priority, first to drop when limit approached)
//! - Configurable max_imports limit with graceful degradation

use crate::types::module_map::DetectedModule;
use crate::types::rule::RuleCategory;
use crate::types::Rule;
use serde::{Deserialize, Serialize};

/// Default maximum number of imports to include in CLAUDE.md
pub const DEFAULT_MAX_IMPORTS: usize = 20;

/// Claude Code's maximum @import chain depth (spec limit: 5 hops)
pub const MAX_IMPORT_DEPTH: usize = 5;

/// Priority levels for import ordering
///
/// Higher priority items are included first and dropped last when
/// the import limit is approached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ImportPriority {
    /// Group rules - lowest priority, first to drop
    Group = 1,
    /// Module rules - medium priority, sorted by file count
    Module = 2,
    /// Tech/language rules - high priority, included early
    Tech = 3,
    /// Framework rules - highest priority, always included first
    Framework = 4,
}

impl ImportPriority {
    /// Get priority from rule category
    pub fn from_rule_category(category: &RuleCategory) -> Self {
        match category {
            RuleCategory::Framework => Self::Framework,
            RuleCategory::Tech => Self::Tech,
            RuleCategory::Module => Self::Module,
            RuleCategory::Group => Self::Group,
            RuleCategory::Project => Self::Framework, // Project rules treated as high priority
            RuleCategory::CrossCutting => Self::Tech,
            RuleCategory::Domain => Self::Module,
            RuleCategory::Service => Self::Module,
            RuleCategory::Custom => Self::Group,
        }
    }

    /// Get priority value for sorting (higher = more important)
    pub fn value(&self) -> u8 {
        *self as u8
    }
}

/// A rule import with its priority and metadata for sorting
#[derive(Debug, Clone)]
pub struct PrioritizedImport {
    /// The import path (e.g., ".claude/rules/tech/rust.md")
    pub path: String,
    /// Priority level for this import
    pub priority: ImportPriority,
    /// File count for module rules (used for secondary sorting)
    pub file_count: usize,
    /// Import chain depth (0 = root CLAUDE.md, 1 = directly imported, etc.)
    pub depth: usize,
    /// Reason for including this import
    pub reason: String,
}

impl PrioritizedImport {
    pub fn new(path: impl Into<String>, priority: ImportPriority, reason: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            priority,
            file_count: 0,
            depth: 1,
            reason: reason.into(),
        }
    }

    pub fn file_count(mut self, count: usize) -> Self {
        self.file_count = count;
        self
    }

    /// Create import from a rule
    pub fn from_rule(rule: &Rule) -> Self {
        let priority = ImportPriority::from_rule_category(&rule.category);
        Self {
            path: format!(".claude/rules/{}", rule.output_path()),
            priority,
            file_count: 0,
            depth: 1,
            reason: format!("{} rule", rule.category),
        }
    }

    /// Create import from a rule with module file count
    pub fn from_rule_with_module(rule: &Rule, modules: &[DetectedModule]) -> Self {
        let priority = ImportPriority::from_rule_category(&rule.category);

        // Calculate file count from matching modules
        let file_count = if matches!(rule.category, RuleCategory::Module) {
            modules
                .iter()
                .filter(|m| m.module_id == rule.name)
                .map(|m| m.key_files.len() + m.paths.len())
                .sum()
        } else {
            0
        };

        Self {
            path: format!(".claude/rules/{}", rule.output_path()),
            priority,
            file_count,
            depth: 1,
            reason: format!("{} rule", rule.category),
        }
    }
}

/// Result of import selection with dropped imports logged
#[derive(Debug, Clone, Default)]
pub struct ImportSelectionResult {
    /// Selected imports (within limit, sorted by priority)
    pub selected: Vec<PrioritizedImport>,
    /// Dropped imports (exceeded limit, for knowledge map reference)
    pub dropped: Vec<PrioritizedImport>,
}

/// Manages priority-based import ordering and selection
pub struct ImportPriorityManager {
    max_imports: usize,
}

impl ImportPriorityManager {
    pub fn new(max_imports: usize) -> Self {
        Self { max_imports }
    }

    /// Select imports based on priority, respecting the max_imports limit and
    /// Claude Code's import chain depth limit (5 hops).
    ///
    /// Returns selected imports and dropped imports for logging.
    pub fn select_imports(&self, mut imports: Vec<PrioritizedImport>) -> ImportSelectionResult {
        // Filter out imports that exceed the depth limit
        let (within_depth, over_depth): (Vec<_>, Vec<_>) = imports
            .into_iter()
            .partition(|i| i.depth <= MAX_IMPORT_DEPTH);
        imports = within_depth;

        // Sort by priority (descending), then by file count (descending) for same priority
        imports.sort_by(|a, b| {
            match b.priority.value().cmp(&a.priority.value()) {
                std::cmp::Ordering::Equal => b.file_count.cmp(&a.file_count),
                other => other,
            }
        });

        if !over_depth.is_empty() {
            tracing::debug!(
                count = over_depth.len(),
                "Dropped imports exceeding depth limit {}",
                MAX_IMPORT_DEPTH,
            );
        }

        if imports.len() <= self.max_imports {
            return ImportSelectionResult {
                selected: imports,
                dropped: over_depth,
            };
        }

        // Split at limit
        let (selected, dropped) = imports.split_at(self.max_imports);
        let mut all_dropped = dropped.to_vec();
        all_dropped.extend(over_depth);

        ImportSelectionResult {
            selected: selected.to_vec(),
            dropped: all_dropped,
        }
    }

    /// Generate imports from rules with priority ordering
    pub fn generate_imports_from_rules(
        &self,
        rules: &[Rule],
        modules: &[DetectedModule],
    ) -> ImportSelectionResult {
        let imports: Vec<PrioritizedImport> = rules
            .iter()
            .map(|r| PrioritizedImport::from_rule_with_module(r, modules))
            .collect();

        self.select_imports(imports)
    }

    /// Log dropped imports to the knowledge map
    pub fn log_dropped_imports(dropped: &[PrioritizedImport]) {
        if dropped.is_empty() {
            return;
        }

        tracing::info!(
            count = dropped.len(),
            "Dropped {} imports due to max_imports limit",
            dropped.len()
        );

        for import in dropped {
            tracing::debug!(
                path = %import.path,
                priority = ?import.priority,
                file_count = import.file_count,
                "Dropped import"
            );
        }
    }

    /// Generate import strings for CLAUDE.md References section
    pub fn format_imports(imports: &[PrioritizedImport]) -> Vec<String> {
        imports.iter().map(|i| i.path.clone()).collect()
    }

    /// Build a hierarchical import chain for nested CLAUDE.md files,
    /// respecting the 5-hop depth limit.
    ///
    /// Given a nesting depth (e.g. `packages/api` = depth 2), computes
    /// how many import hops remain for child imports.
    pub fn remaining_depth_for_nesting(nesting_depth: usize) -> usize {
        // Nesting itself consumes 1 hop (child imports parent).
        // So remaining = MAX_IMPORT_DEPTH - 1 (parent hop) - nesting_depth adjustments
        MAX_IMPORT_DEPTH.saturating_sub(nesting_depth.min(MAX_IMPORT_DEPTH))
    }

    /// Select imports for a nested CLAUDE.md file, accounting for the
    /// nesting depth consuming part of the chain budget.
    pub fn select_nested_imports(
        &self,
        imports: Vec<PrioritizedImport>,
        nesting_depth: usize,
    ) -> ImportSelectionResult {
        let remaining = Self::remaining_depth_for_nesting(nesting_depth);
        if remaining == 0 {
            return ImportSelectionResult {
                selected: Vec::new(),
                dropped: imports,
            };
        }

        // Adjust depth of all imports relative to nesting
        let adjusted: Vec<PrioritizedImport> = imports
            .into_iter()
            .map(|mut i| {
                i.depth += nesting_depth;
                i
            })
            .collect();

        self.select_imports(adjusted)
    }
}

impl Default for ImportPriorityManager {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_IMPORTS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_priority_ordering() {
        assert!(ImportPriority::Framework > ImportPriority::Tech);
        assert!(ImportPriority::Tech > ImportPriority::Module);
        assert!(ImportPriority::Module > ImportPriority::Group);
    }

    #[test]
    fn test_priority_from_rule_category() {
        assert_eq!(
            ImportPriority::from_rule_category(&RuleCategory::Framework),
            ImportPriority::Framework
        );
        assert_eq!(
            ImportPriority::from_rule_category(&RuleCategory::Tech),
            ImportPriority::Tech
        );
        assert_eq!(
            ImportPriority::from_rule_category(&RuleCategory::Module),
            ImportPriority::Module
        );
        assert_eq!(
            ImportPriority::from_rule_category(&RuleCategory::Group),
            ImportPriority::Group
        );
    }

    #[test]
    fn test_import_selection_within_limit() {
        let manager = ImportPriorityManager::new(10);
        let imports = vec![
            PrioritizedImport::new("rule1.md", ImportPriority::Tech, "test"),
            PrioritizedImport::new("rule2.md", ImportPriority::Module, "test"),
        ];

        let result = manager.select_imports(imports);

        assert_eq!(result.selected.len(), 2);
        assert!(result.dropped.is_empty());
        // Tech should come before Module
        assert_eq!(result.selected[0].priority, ImportPriority::Tech);
        assert_eq!(result.selected[1].priority, ImportPriority::Module);
    }

    #[test]
    fn test_import_selection_exceeds_limit() {
        let manager = ImportPriorityManager::new(2);
        let imports = vec![
            PrioritizedImport::new("group.md", ImportPriority::Group, "test"),
            PrioritizedImport::new("framework.md", ImportPriority::Framework, "test"),
            PrioritizedImport::new("module.md", ImportPriority::Module, "test"),
            PrioritizedImport::new("tech.md", ImportPriority::Tech, "test"),
        ];

        let result = manager.select_imports(imports);

        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.dropped.len(), 2);
        // Framework and Tech should be selected (highest priority)
        assert_eq!(result.selected[0].priority, ImportPriority::Framework);
        assert_eq!(result.selected[1].priority, ImportPriority::Tech);
        // Module and Group should be dropped
        assert!(result
            .dropped
            .iter()
            .any(|i| i.priority == ImportPriority::Module));
        assert!(result
            .dropped
            .iter()
            .any(|i| i.priority == ImportPriority::Group));
    }

    #[test]
    fn test_module_imports_sorted_by_file_count() {
        let manager = ImportPriorityManager::new(3);
        let imports = vec![
            PrioritizedImport::new("small.md", ImportPriority::Module, "test").file_count(5),
            PrioritizedImport::new("large.md", ImportPriority::Module, "test").file_count(50),
            PrioritizedImport::new("medium.md", ImportPriority::Module, "test").file_count(20),
        ];

        let result = manager.select_imports(imports);

        assert_eq!(result.selected.len(), 3);
        // Should be sorted by file count (descending)
        assert_eq!(result.selected[0].path, "large.md");
        assert_eq!(result.selected[1].path, "medium.md");
        assert_eq!(result.selected[2].path, "small.md");
    }

    #[test]
    fn test_format_imports() {
        let imports = vec![
            PrioritizedImport::new(".claude/rules/tech/rust.md", ImportPriority::Tech, "test"),
            PrioritizedImport::new(
                ".claude/rules/modules/auth.md",
                ImportPriority::Module,
                "test",
            ),
        ];

        let formatted = ImportPriorityManager::format_imports(&imports);

        assert_eq!(formatted.len(), 2);
        assert_eq!(formatted[0], ".claude/rules/tech/rust.md");
        assert_eq!(formatted[1], ".claude/rules/modules/auth.md");
    }

    #[test]
    fn test_prioritized_import_from_rule() {
        let rule = Rule::tech(
            "rust",
            vec!["**/*.rs".to_string()],
            vec!["# Rust".to_string()],
        );

        let import = PrioritizedImport::from_rule(&rule);

        assert_eq!(import.priority, ImportPriority::Tech);
        assert!(import.path.contains("tech/rust.md"));
    }

    // =========================================================================
    // Import chain depth tracking tests
    // =========================================================================

    #[test]
    fn test_remaining_depth_for_nesting() {
        // Root level (depth 0) = full 5 hops available
        assert_eq!(ImportPriorityManager::remaining_depth_for_nesting(0), 5);
        // 1 level deep = 4 remaining
        assert_eq!(ImportPriorityManager::remaining_depth_for_nesting(1), 4);
        // 2 levels deep = 3 remaining
        assert_eq!(ImportPriorityManager::remaining_depth_for_nesting(2), 3);
        // At max depth = 0 remaining
        assert_eq!(ImportPriorityManager::remaining_depth_for_nesting(5), 0);
        // Beyond max depth = 0 (saturating)
        assert_eq!(ImportPriorityManager::remaining_depth_for_nesting(10), 0);
    }

    #[test]
    fn test_select_nested_imports_at_depth_zero() {
        let manager = ImportPriorityManager::new(10);
        let imports = vec![
            PrioritizedImport::new("rule1.md", ImportPriority::Tech, "test"),
            PrioritizedImport::new("rule2.md", ImportPriority::Module, "test"),
        ];

        let result = manager.select_nested_imports(imports, 0);
        assert_eq!(result.selected.len(), 2);
        assert!(result.dropped.is_empty());
    }

    #[test]
    fn test_select_nested_imports_at_max_depth() {
        let manager = ImportPriorityManager::new(10);
        let imports = vec![
            PrioritizedImport::new("rule1.md", ImportPriority::Tech, "test"),
        ];

        // At depth 5 (max), no remaining budget
        let result = manager.select_nested_imports(imports, MAX_IMPORT_DEPTH);
        assert!(result.selected.is_empty());
        assert_eq!(result.dropped.len(), 1);
    }

    #[test]
    fn test_select_nested_imports_adjusts_depth() {
        let manager = ImportPriorityManager::new(10);
        let imports = vec![
            PrioritizedImport::new("rule1.md", ImportPriority::Tech, "test"),
        ];

        // At depth 3, imports get depth adjusted to 3+1=4 (within limit)
        let result = manager.select_nested_imports(imports, 3);
        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].depth, 4); // 1 (original) + 3 (nesting)
    }

    #[test]
    fn test_depth_limit_filters_deep_imports() {
        let manager = ImportPriorityManager::new(10);
        let imports = vec![
            PrioritizedImport {
                path: "shallow.md".into(),
                priority: ImportPriority::Tech,
                file_count: 0,
                depth: 2,
                reason: "test".into(),
            },
            PrioritizedImport {
                path: "deep.md".into(),
                priority: ImportPriority::Tech,
                file_count: 0,
                depth: 6, // Exceeds MAX_IMPORT_DEPTH (5)
                reason: "test".into(),
            },
        ];

        let result = manager.select_imports(imports);
        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].path, "shallow.md");
        assert_eq!(result.dropped.len(), 1);
        assert_eq!(result.dropped[0].path, "deep.md");
    }
}
