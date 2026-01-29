//! Architectural Analyzer Module
//!
//! Validates structural coverage of generated documentation against
//! LLM-identified core modules. Works with any language/framework
//! because module identification is done by LLM, not hardcoded patterns.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::StructuralValidationConfig;
use crate::types::{Agent, ProjectMemory, Result, Rule, Severity, Skill};

use super::deep_analyzer::{CoreModule, EntryPoint, LayerBoundary};

// =============================================================================
// ARCHITECTURAL ANALYSIS RESULT
// =============================================================================

/// Result of top-down architectural analysis
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArchitecturalAnalysis {
    /// Core modules with their roles and responsibilities
    pub modules: Vec<CoreModule>,
    /// Identified entry points (main, lib roots, API endpoints)
    pub entry_points: Vec<EntryPoint>,
    /// Architecture layers (e.g., domain, application, infrastructure)
    pub layers: Vec<String>,
    /// Layer boundary rules
    pub layer_boundaries: Vec<LayerBoundary>,
    /// Detected architecture pattern (e.g., "Hexagonal", "Layered", "Modular")
    pub architecture_pattern: Option<String>,
}

impl ArchitecturalAnalysis {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_modules(mut self, modules: Vec<CoreModule>) -> Self {
        self.modules = modules;
        self
    }

    pub fn with_entry_points(mut self, entry_points: Vec<EntryPoint>) -> Self {
        self.entry_points = entry_points;
        self
    }

    pub fn with_layers(mut self, layers: Vec<String>) -> Self {
        self.layers = layers;
        self
    }

    pub fn module_names(&self) -> Vec<String> {
        self.modules.iter().map(|m| m.name.clone()).collect()
    }

    pub fn module_paths(&self) -> Vec<String> {
        self.modules.iter().map(|m| m.path.clone()).collect()
    }
}

/// Pattern to extract module names from documentation content.
/// Matches paths like @src/module, backend/src/..., `module_name`, etc.
static MODULE_PATH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"@?(?:[\w-]+/)*(?:src/)?([a-zA-Z_][a-zA-Z0-9_-]*)(?:/|\.(?:rs|kt|ts|tsx|js|jsx|py|go|java|rb|php|swift|scala|cs|cpp|c)|:|`|$)"
    )
    .expect("module path regex")
});

static BACKTICK_MODULE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([a-zA-Z_][a-zA-Z0-9_-]*)`").expect("backtick module regex"));

/// Coverage report showing how well documentation covers identified modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    pub total_modules: usize,
    pub core_modules: usize,
    pub documented_modules: usize,
    pub coverage: f32,
    pub missing_modules: Vec<ModuleCoverage>,
    pub partially_covered: Vec<ModuleCoverage>,
    pub fully_covered: Vec<ModuleCoverage>,
}

/// Coverage information for a single module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleCoverage {
    /// Module name
    pub name: String,
    /// Module path
    pub path: String,
    /// Module responsibility description
    pub responsibility: String,
    /// Number of artifacts referencing this module - let LLM interpret significance
    pub reference_count: usize,
    /// Artifacts that reference this module
    pub referenced_in: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralValidationResult {
    pub passed: bool,
    pub coverage_report: CoverageReport,
    pub issues: Vec<StructuralIssue>,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralIssue {
    pub severity: Severity,
    pub category: StructuralCategory,
    pub description: String,
    pub affected_module: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StructuralCategory {
    MissingCoreModule,
    PartialCoverage,
    UnbalancedCoverage,
}

/// Validates documentation coverage against LLM-identified modules.
pub struct ArchitecturalAnalyzer {
    config: StructuralValidationConfig,
    modules: Vec<CoreModule>,
}

impl ArchitecturalAnalyzer {
    pub fn new(config: &StructuralValidationConfig, core_modules: &[CoreModule]) -> Self {
        tracing::debug!(
            modules = core_modules.len(),
            "ArchitecturalAnalyzer initialized"
        );

        Self {
            config: config.clone(),
            modules: core_modules.to_vec(),
        }
    }

    /// Extract module references from generated artifacts.
    fn extract_documented_modules(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> HashMap<String, Vec<String>> {
        let mut module_refs: HashMap<String, Vec<String>> = HashMap::new();

        // Check CLAUDE.md
        let claude_md_content = claude_md.to_markdown();
        for module in extract_module_refs(&claude_md_content) {
            module_refs
                .entry(module)
                .or_default()
                .push("CLAUDE.md".to_string());
        }

        // Check skills
        for skill in skills {
            let content = skill.to_markdown();
            for module in extract_module_refs(&content) {
                module_refs
                    .entry(module)
                    .or_default()
                    .push(format!("Skill:{}", skill.name));
            }
        }

        // Check agents
        for agent in agents {
            let content = agent.to_markdown();
            for module in extract_module_refs(&content) {
                module_refs
                    .entry(module)
                    .or_default()
                    .push(format!("Agent:{}", agent.name));
            }
        }

        // Check rules
        for rule in rules {
            let content = rule.to_markdown();
            for module in extract_module_refs(&content) {
                module_refs
                    .entry(module)
                    .or_default()
                    .push(format!("Rule:{}", rule.name));
            }
        }

        module_refs
    }

    /// Calculate coverage statistics.
    fn calculate_coverage(&self, documented: &HashMap<String, Vec<String>>) -> CoverageReport {
        let mut missing = Vec::new();
        let mut fully_covered = Vec::new();
        // partially_covered kept empty - previous threshold-based categorization removed

        for module in &self.modules {
            // Normalize consistently with extract_module_refs()
            let module_name_normalized = normalize_module_name(&module.name);

            // Check if module is referenced in documentation
            let refs = documented.get(&module_name_normalized);
            let reference_count = refs.map(|v| v.len()).unwrap_or(0);
            let referenced_in = refs.cloned().unwrap_or_default();

            let module_coverage = ModuleCoverage {
                name: module.name.clone(),
                path: module.path.clone(),
                responsibility: module.responsibility.clone(),
                reference_count,
                referenced_in,
            };

            // Report raw reference counts - let downstream (LLM/human) interpret significance
            // Previous hardcoded thresholds (0=missing, 1-2=partial, 3+=full) were arbitrary
            if reference_count == 0 {
                missing.push(module_coverage);
            } else {
                // Non-zero references = some coverage exists
                // LLM determines if coverage is adequate for the module's importance
                fully_covered.push(module_coverage);
            }
        }

        let documented_count = fully_covered.len();
        let coverage = if self.modules.is_empty() {
            1.0
        } else {
            documented_count as f32 / self.modules.len() as f32
        };

        CoverageReport {
            total_modules: self.modules.len(),
            core_modules: self.modules.len(),
            documented_modules: documented_count,
            coverage,
            missing_modules: missing,
            partially_covered: Vec::new(), // Threshold-based categorization removed
            fully_covered,
        }
    }

    /// Validate documentation coverage.
    pub async fn validate(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> Result<StructuralValidationResult> {
        let documented = self.extract_documented_modules(skills, agents, rules, claude_md);
        let coverage_report = self.calculate_coverage(&documented);

        let mut issues = Vec::new();
        let mut suggestions = Vec::new();

        for missing in &coverage_report.missing_modules {
            issues.push(StructuralIssue {
                severity: Severity::Critical,
                category: StructuralCategory::MissingCoreModule,
                description: format!(
                    "Core module '{}' ({}) is not documented",
                    missing.name, missing.responsibility
                ),
                affected_module: Some(missing.name.clone()),
            });

            suggestions.push(format!(
                "Add documentation for '{}' module at {}",
                missing.name, missing.path
            ));
        }

        // Partial coverage and unbalanced coverage checks removed:
        // - Hardcoded thresholds (ref_count < 2, 3x average) were arbitrary
        // - Different modules have legitimately different coverage needs
        // - LLM can interpret coverage_report data contextually

        let passed = coverage_report.coverage >= self.config.min_module_coverage;

        if !passed && suggestions.is_empty() {
            suggestions.push(format!(
                "Increase module coverage from {:.0}% to {:.0}%",
                coverage_report.coverage * 100.0,
                self.config.min_module_coverage * 100.0
            ));
        }

        Ok(StructuralValidationResult {
            passed,
            coverage_report,
            issues,
            suggestions,
        })
    }
}

/// Normalize module name for consistent matching.
/// Converts underscores to hyphens and lowercases.
fn normalize_module_name(name: &str) -> String {
    name.replace('_', "-").to_lowercase()
}

/// Extract module references from text content.
fn extract_module_refs(content: &str) -> HashSet<String> {
    let mut modules = HashSet::new();

    for cap in MODULE_PATH_PATTERN.captures_iter(content) {
        if let Some(module) = cap.get(1) {
            let name = normalize_module_name(module.as_str());
            // Filter out common non-module words (allow 2-char module names like "ai", "db")
            if name.len() > 1 && !is_common_word(&name) {
                modules.insert(name);
            }
        }
    }

    for cap in BACKTICK_MODULE_PATTERN.captures_iter(content) {
        if let Some(module) = cap.get(1) {
            let name = normalize_module_name(module.as_str());
            if name.len() > 1 && !is_common_word(&name) && !name.contains("()") {
                modules.insert(name);
            }
        }
    }

    modules
}

/// Check if word is likely not a module name
/// Only filter obvious non-module patterns, not legitimate short names
fn is_common_word(word: &str) -> bool {
    // Only filter 2-char words that are clearly not module names
    // Kept minimal: "go" could be Go language module, "io" is common module name
    matches!(
        word,
        "an" | "as"
            | "at"
            | "be"
            | "by"
            | "do"
            | "he"
            | "if"
            | "in"
            | "is"
            | "it"
            | "me"
            | "my"
            | "no"
            | "of"
            | "ok"
            | "on"
            | "or"
            | "so"
            | "to"
            | "up"
            | "us"
            | "we"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_module_refs() {
        let content = r#"
            See @src/pipeline/refinement.rs:10 for details.
            The `provider` module handles provider abstraction.
            Check src/types/error.rs for error types.
        "#;

        let modules = extract_module_refs(content);

        // Regex captures the final component (module/file name without extension)
        // 2-char module names (ai, db, io) are allowed; common words filtered via is_common_word()
        assert!(modules.contains("refinement"));
        assert!(modules.contains("provider")); // From backtick pattern
        assert!(modules.contains("error"));
    }

    #[test]
    fn test_is_common_word() {
        // Only 2-char common words are filtered now
        // 3+ char words kept because they could be valid module names
        assert!(is_common_word("in"));
        assert!(is_common_word("to"));
        assert!(!is_common_word("the")); // Not filtered anymore
        assert!(!is_common_word("for")); // Not filtered anymore
        assert!(!is_common_word("go")); // Valid module name (Go language)
        assert!(!is_common_word("pipeline"));
        assert!(!is_common_word("ai"));
    }

    #[test]
    fn test_module_name_normalization() {
        // Test that underscores and hyphens are normalized consistently
        let content = r#"
            The `provider_chain` handles fallback logic.
            Check src/pipeline/quality_loop.rs for details.
        "#;

        let modules = extract_module_refs(content);

        // Underscores should be converted to hyphens
        assert!(modules.contains("provider-chain"));
        assert!(modules.contains("quality-loop"));
        // Underscored versions should NOT exist
        assert!(!modules.contains("provider_chain"));
        assert!(!modules.contains("quality_loop"));
    }

    #[test]
    fn test_two_char_module_names() {
        // 2-char module names like ai, db, io should be captured from backticks
        // Path regex captures final component (e.g., src/db.rs -> db, but src/db/mod.rs -> mod)
        let content = r#"
            The `ai` module provides LLM abstraction.
            Database logic is in src/db.rs or the `db` module.
            Common words like `to` and `if` should be filtered.
        "#;

        let modules = extract_module_refs(content);

        // Valid 2-char module names should be captured
        assert!(modules.contains("ai"), "ai module should be captured");
        assert!(modules.contains("db"), "db module should be captured");

        // Common 2-char words should be filtered
        assert!(
            !modules.contains("to"),
            "common word 'to' should be filtered"
        );
        assert!(
            !modules.contains("if"),
            "common word 'if' should be filtered"
        );
    }
}
