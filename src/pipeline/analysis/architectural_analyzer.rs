//! Architectural Analyzer Module

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::StructuralValidationConfig;
use crate::pipeline::context::VerifiedFileRegistry;
use crate::types::{Agent, ProjectMemory, Result, Rule, Skill};

static MODULE_PATH_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@?src/([a-zA-Z_][a-zA-Z0-9_]*)").unwrap());

static BACKTICK_MODULE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([a-zA-Z_][a-zA-Z0-9_-]*)`").unwrap());

/// Semantic file reference matching.
/// Checks if a source reference matches a target file path semantically,
/// not just via substring matching.
fn matches_file_reference(source: &str, target_file: &str) -> bool {
    // Normalize paths: remove leading ./ or @ prefix
    fn normalize(s: &str) -> &str {
        s.trim_start_matches("./")
            .trim_start_matches('@')
            .trim_start_matches("./")
    }

    let source_normalized = normalize(source);
    let target_normalized = normalize(target_file);

    // Exact match
    if source_normalized == target_normalized {
        return true;
    }

    // Check if source contains the full file path (with path boundaries)
    // This prevents "ai" from matching "main"
    let target_parts: Vec<&str> = target_normalized.split('/').collect();
    let source_parts: Vec<&str> = source_normalized.split('/').collect();

    // Check if target is a suffix of source path components
    if source_parts.ends_with(&target_parts) {
        return true;
    }

    // Check if source matches target directory or file
    // e.g., "src/ai" matches "src/ai/mod.rs"
    if target_normalized.starts_with(source_normalized)
        && target_normalized[source_normalized.len()..].starts_with('/')
    {
        return true;
    }

    if let Some(before_idx) = source_normalized.find(target_normalized) {
        let after_idx = before_idx + target_normalized.len();

        let valid_before = before_idx == 0
            || source_normalized.chars().nth(before_idx - 1).map(|c| c == '/').unwrap_or(true);
        let valid_after = after_idx >= source_normalized.len()
            || source_normalized
                .chars()
                .nth(after_idx)
                .map(|c| c == ':' || c == '/' || c == '`')
                .unwrap_or(true);

        if valid_before && valid_after {
            return true;
        }
    }

    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    pub path: String,
    pub file_count: usize,
    pub total_lines: usize,
    pub is_public_api: bool,
    pub key_files: Vec<String>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleCoverage {
    pub module: Module,
    pub coverage_score: f32,
    pub referenced_in: Vec<String>,
    pub missing_key_files: Vec<String>,
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
    pub severity: StructuralSeverity,
    pub category: StructuralCategory,
    pub description: String,
    pub affected_module: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StructuralSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StructuralCategory {
    MissingCoreModule,
    PartialCoverage,
    NoKeyFileReferences,
    UnbalancedCoverage,
}

pub struct ArchitecturalAnalyzer {
    config: StructuralValidationConfig,
}

impl ArchitecturalAnalyzer {
    pub fn new(config: StructuralValidationConfig) -> Self {
        Self { config }
    }

    pub fn discover_modules(&self, file_registry: &VerifiedFileRegistry) -> Vec<Module> {
        let mut module_map: HashMap<String, ModuleBuilder> = HashMap::new();

        for file in file_registry.all_files() {
            if !file.ends_with(".rs") {
                continue;
            }

            let relative_path = file.strip_prefix("src/").unwrap_or(file.as_str());
            let parts: Vec<&str> = relative_path.split('/').collect();
            if parts.is_empty() {
                continue;
            }

            let module_path = if parts.len() == 1 {
                "core".to_string()
            } else {
                parts[0].to_string()
            };

            let module_name = module_path.replace('_', "-");
            let entry = module_map.entry(module_name.clone()).or_insert_with(|| {
                ModuleBuilder {
                    name: module_name,
                    path: format!("src/{}", module_path),
                    files: Vec::new(),
                    total_lines: 0,
                }
            });

            entry.files.push(file.clone());
            if let Some(lines) = file_registry.line_count(file) {
                entry.total_lines += lines;
            }
        }

        module_map
            .into_values()
            .map(|builder| {
                let is_public_api = builder.name == "core"
                    || builder.files.iter().any(|f| f.ends_with("lib.rs") || f.ends_with("mod.rs"));

                let key_files = self.identify_key_files(&builder.files, file_registry);

                Module {
                    name: builder.name,
                    path: builder.path,
                    file_count: builder.files.len(),
                    total_lines: builder.total_lines,
                    is_public_api,
                    key_files,
                }
            })
            .collect()
    }

    fn identify_key_files(&self, files: &[String], registry: &VerifiedFileRegistry) -> Vec<String> {
        let mut file_scores: Vec<(&String, usize)> = files
            .iter()
            .map(|f| {
                let lines = registry.line_count(f).unwrap_or(0);
                let importance_bonus = if f.ends_with("mod.rs") || f.ends_with("lib.rs") {
                    100
                } else if f.contains("main") {
                    80
                } else {
                    0
                };
                (f, lines + importance_bonus)
            })
            .collect();

        file_scores.sort_by(|a, b| b.1.cmp(&a.1));

        file_scores
            .into_iter()
            .take(5)
            .map(|(f, _)| f.clone())
            .collect()
    }

    pub fn identify_core_modules<'a>(&self, modules: &'a [Module]) -> Vec<&'a Module> {
        let threshold = self.config.core_module_threshold as usize;

        let required: HashSet<String> = self
            .config
            .required_modules
            .iter()
            .cloned()
            .collect();

        modules
            .iter()
            .filter(|m| {
                required.contains(&m.name)
                    || m.file_count >= threshold
                    || (m.is_public_api && m.file_count >= 2)
                    || m.total_lines >= 500
            })
            .collect()
    }

    pub fn extract_documented_modules(
        &self,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> HashMap<String, Vec<String>> {
        let mut module_refs: HashMap<String, Vec<String>> = HashMap::new();

        let claude_md_content = claude_md.to_markdown();
        for module in extract_module_refs(&claude_md_content) {
            module_refs
                .entry(module)
                .or_default()
                .push("CLAUDE.md".to_string());
        }

        for skill in skills {
            let content = skill.to_markdown();
            for module in extract_module_refs(&content) {
                module_refs
                    .entry(module)
                    .or_default()
                    .push(format!("Skill:{}", skill.name));
            }
        }

        for agent in agents {
            let content = agent.to_markdown();
            for module in extract_module_refs(&content) {
                module_refs
                    .entry(module)
                    .or_default()
                    .push(format!("Agent:{}", agent.name));
            }
        }

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

    pub fn calculate_coverage(
        &self,
        modules: &[Module],
        core_modules: &[&Module],
        documented: &HashMap<String, Vec<String>>,
    ) -> CoverageReport {
        let mut missing = Vec::new();
        let mut partially_covered = Vec::new();
        let mut fully_covered = Vec::new();

        for core_module in core_modules {
            let refs = documented.get(&core_module.name);
            let ref_count = refs.map(|v| v.len()).unwrap_or(0);
            let referenced_in = refs.cloned().unwrap_or_default();

            let key_file_refs: Vec<_> = core_module
                .key_files
                .iter()
                .filter(|f| {
                    documented.iter().any(|(_, sources)| {
                        sources.iter().any(|s| matches_file_reference(s, f))
                    })
                })
                .cloned()
                .collect();

            let missing_key_files: Vec<_> = core_module
                .key_files
                .iter()
                .filter(|f| !key_file_refs.contains(f))
                .cloned()
                .collect();

            // Coverage calculation:
            // - Module mentioned: 40% of score (if module name appears in documentation)
            // - Key file coverage: 60% of score (if key files are documented)
            // When key_files is empty, we scale up module_mentioned to be the full score
            // to avoid artificially penalizing modules without identified key files
            let (module_mentioned, key_file_coverage) = if !core_module.key_files.is_empty() {
                let mentioned = if ref_count > 0 { 0.4 } else { 0.0 };
                let file_cov = (key_file_refs.len() as f32 / core_module.key_files.len() as f32) * 0.6;
                (mentioned, file_cov)
            } else {
                // No key files identified - base coverage entirely on module mentions
                // Scale up the module mention weight to compensate
                let mentioned = if ref_count > 0 { 1.0 } else { 0.0 };
                (mentioned, 0.0)
            };
            let coverage_score = (module_mentioned + key_file_coverage).min(1.0);

            let module_coverage = ModuleCoverage {
                module: (*core_module).clone(),
                coverage_score,
                referenced_in,
                missing_key_files,
            };

            if coverage_score < 0.3 {
                missing.push(module_coverage);
            } else if coverage_score < 0.8 {
                partially_covered.push(module_coverage);
            } else {
                fully_covered.push(module_coverage);
            }
        }

        let documented_count = fully_covered.len() + partially_covered.len();
        let coverage = if core_modules.is_empty() {
            1.0
        } else {
            documented_count as f32 / core_modules.len() as f32
        };

        CoverageReport {
            total_modules: modules.len(),
            core_modules: core_modules.len(),
            documented_modules: documented_count,
            coverage,
            missing_modules: missing,
            partially_covered,
            fully_covered,
        }
    }

    pub async fn validate(
        &self,
        file_registry: &VerifiedFileRegistry,
        skills: &[Skill],
        agents: &[Agent],
        rules: &[Rule],
        claude_md: &ProjectMemory,
    ) -> Result<StructuralValidationResult> {
        let modules = self.discover_modules(file_registry);
        let core_modules = self.identify_core_modules(&modules);
        let documented = self.extract_documented_modules(skills, agents, rules, claude_md);
        let coverage_report = self.calculate_coverage(&modules, &core_modules, &documented);

        let mut issues = Vec::new();
        let mut suggestions = Vec::new();

        for missing in &coverage_report.missing_modules {
            issues.push(StructuralIssue {
                severity: StructuralSeverity::Critical,
                category: StructuralCategory::MissingCoreModule,
                description: format!(
                    "Core module '{}' ({} files, {} lines) is not documented",
                    missing.module.name, missing.module.file_count, missing.module.total_lines
                ),
                affected_module: Some(missing.module.name.clone()),
            });

            suggestions.push(format!(
                "Add documentation for '{}' module. Key files: {}",
                missing.module.name,
                missing.module.key_files.join(", ")
            ));
        }

        for partial in &coverage_report.partially_covered {
            if partial.coverage_score < 0.5 {
                issues.push(StructuralIssue {
                    severity: StructuralSeverity::High,
                    category: StructuralCategory::PartialCoverage,
                    description: format!(
                        "Module '{}' has only {:.0}% coverage. Missing key files: {}",
                        partial.module.name,
                        partial.coverage_score * 100.0,
                        partial.missing_key_files.join(", ")
                    ),
                    affected_module: Some(partial.module.name.clone()),
                });
            }
        }

        let avg_refs = documented.values().map(|v| v.len()).sum::<usize>() as f32
            / documented.len().max(1) as f32;

        for (module, refs) in &documented {
            if refs.len() as f32 > avg_refs * 3.0 && !coverage_report.missing_modules.is_empty() {
                issues.push(StructuralIssue {
                    severity: StructuralSeverity::Medium,
                    category: StructuralCategory::UnbalancedCoverage,
                    description: format!(
                        "Module '{}' is referenced {} times while other modules are missing",
                        module,
                        refs.len()
                    ),
                    affected_module: Some(module.clone()),
                });
            }
        }

        // Pass if coverage meets threshold - missing_modules being non-empty is okay
        // as long as overall coverage is sufficient
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

fn extract_module_refs(content: &str) -> HashSet<String> {
    let mut modules = HashSet::new();

    for cap in MODULE_PATH_PATTERN.captures_iter(content) {
        if let Some(module) = cap.get(1) {
            modules.insert(module.as_str().replace('_', "-"));
        }
    }

    for cap in BACKTICK_MODULE_PATTERN.captures_iter(content) {
        if let Some(module) = cap.get(1) {
            let name = module.as_str().replace('_', "-");
            if !name.contains("()") && !name.starts_with("is-") && !name.starts_with("get-") {
                modules.insert(name);
            }
        }
    }

    modules
}

struct ModuleBuilder {
    name: String,
    path: String,
    files: Vec<String>,
    total_lines: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StructuralValidationConfig;

    fn test_config() -> StructuralValidationConfig {
        StructuralValidationConfig {
            enabled: true,
            min_module_coverage: 0.8,
            core_module_threshold: 3.0,
            required_modules: Vec::new(),
            ..Default::default()
        }
    }

    #[test]
    fn test_extract_module_refs() {
        let content = r#"
            See @src/pipeline/refinement.rs:10 for details.
            The `ai` module handles provider abstraction.
            Check src/types/error.rs for error types.
        "#;

        let modules = extract_module_refs(content);

        assert!(modules.contains("pipeline"));
        assert!(modules.contains("ai"));
        assert!(modules.contains("types"));
    }

    #[test]
    fn test_coverage_calculation() {
        let analyzer = ArchitecturalAnalyzer::new(test_config());

        let modules = vec![
            Module {
                name: "pipeline".to_string(),
                path: "src/pipeline".to_string(),
                file_count: 10,
                total_lines: 1000,
                is_public_api: true,
                key_files: vec!["src/pipeline/mod.rs".to_string()],
            },
            Module {
                name: "ai".to_string(),
                path: "src/ai".to_string(),
                file_count: 5,
                total_lines: 500,
                is_public_api: true,
                key_files: vec!["src/ai/mod.rs".to_string()],
            },
        ];

        let core_modules: Vec<_> = modules.iter().collect();

        let mut documented = HashMap::new();
        documented.insert("pipeline".to_string(), vec!["CLAUDE.md".to_string()]);

        let report = analyzer.calculate_coverage(&modules, &core_modules, &documented);

        assert_eq!(report.core_modules, 2);
        assert_eq!(report.documented_modules, 1);
        assert!(report.coverage < 1.0);
        assert_eq!(report.missing_modules.len(), 1);
        assert_eq!(report.missing_modules[0].module.name, "ai");
    }
}
