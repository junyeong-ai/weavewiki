//! CLAUDE.md Generator
//!
//! Generates the project-level CLAUDE.md file with:
//! - Project overview and architecture
//! - Standards and conventions
//! - Domain knowledge
//! - Gotchas and anti-patterns
//!
//! ## Differential Updates (Task 8-C)
//!
//! Supports section-level caching to avoid regenerating unchanged sections:
//! - Each section tracks its input hash and content hash
//! - Cache stored in `.claudegen/cache/claude_md_sections.json`
//! - On regeneration, only stale sections are regenerated
//!
//! ## Priority-based Import Ordering (Task 4-C)
//!
//! Imports are ordered by priority:
//! - Framework/Tech rules first (highest priority, always included)
//! - Module rules sorted by file count (more files = higher priority)
//! - Group rules last (lowest priority, first to drop when limit approached)
//! - Configurable max_imports limit with graceful degradation

mod cache;
mod imports;
pub mod nested;
mod sections;

pub use cache::{
    compute_hash, ClaudeMdCache, SectionContent, SectionManifest, SectionSource,
};
pub use imports::{
    ImportPriority, ImportPriorityManager, ImportSelectionResult, PrioritizedImport,
    DEFAULT_MAX_IMPORTS,
};
pub use nested::{NestedClaudeMd, NestedClaudeMdGenerator};

use crate::constants::artifact_dirs::{AGENTS_DIR, RULES_DIR, SKILLS_DIR};
use crate::pipeline::analysis::{SynthesizedAnalysis, SynthesizedInsights};
use crate::pipeline::phases::constraint_extraction::ExtractedConstraints;
use crate::pipeline::phases::convention_inference::InferredConventions;
use crate::pipeline::phases::output_router::OutputPlan;
use crate::pipeline::phases::project_detection::ProjectDetection;
use crate::types::{ClaudeMdContent, Result};
use std::path::Path;

/// Context for CLAUDE.md generation
pub struct ClaudeMdContext<'a> {
    pub plan: &'a OutputPlan,
    pub detection: &'a ProjectDetection,
    pub conventions: &'a InferredConventions,
    pub constraints: &'a ExtractedConstraints,
    pub project_name: &'a str,
    pub synthesis: Option<&'a SynthesizedAnalysis>,
    pub domain_analysis: Option<&'a crate::types::domain::DomainAnalysisResult>,
    pub cross_insights: Option<&'a SynthesizedInsights>,
    /// Project root for cache location (optional, enables differential updates)
    pub project_root: Option<&'a Path>,
}

impl<'a> ClaudeMdContext<'a> {
    /// Compute hash for Overview section inputs (project structure)
    pub fn overview_input_hash(&self) -> String {
        let data = format!(
            "{:?}:{:?}:{:?}:{}",
            self.detection.primary_type,
            self.detection.languages,
            self.detection.is_monorepo,
            self.project_name
        );
        compute_hash(&data)
    }

    /// Compute hash for Architecture section inputs (module structure)
    pub fn architecture_input_hash(&self) -> String {
        let mut data = format!(
            "{:?}:{:?}",
            self.conventions.architecture.pattern_name,
            self.conventions.architecture.layers
        );
        if let Some(synth) = self.synthesis {
            data.push_str(&format!(":{:?}", synth.modules));
        }
        compute_hash(&data)
    }

    /// Compute hash for Standards section inputs (constraints)
    pub fn standards_input_hash(&self) -> String {
        let data = format!(
            "{:?}:{:?}:{:?}:{:?}:{:?}",
            self.constraints.anti_patterns,
            self.constraints.hidden_dependencies,
            self.constraints.gotchas,
            self.synthesis.map(|s| &s.deep.patterns),
            self.cross_insights.map(|i| &i.tier2_insights)
        );
        compute_hash(&data)
    }

    /// Compute hash for Domain section inputs (domain analysis)
    pub fn domain_input_hash(&self) -> String {
        let data = format!("{:?}", self.domain_analysis);
        compute_hash(&data)
    }

    /// Compute hash for Gotchas section inputs
    pub fn gotchas_input_hash(&self) -> String {
        let data = format!("{:?}:{:?}", self.constraints.gotchas, self.cross_insights);
        compute_hash(&data)
    }
}

pub struct ClaudeMdGenerator;

/// Helper function to get cached content or regenerate
///
/// Reduces DRY violation by extracting the common cache-check-or-generate pattern.
fn get_or_generate<T, F>(
    cache: &Option<ClaudeMdCache>,
    manifest: Option<&SectionManifest>,
    section_name: &str,
    input_hash: &str,
    extract: impl Fn(&SectionContent) -> Option<T>,
    generate: F,
) -> T
where
    F: FnOnce() -> T,
{
    if let Some(c) = cache
        && let Some(content) = c.get_cached_content(manifest, section_name, input_hash)
        && let Some(value) = extract(&content)
    {
        tracing::debug!("Using cached {} section", section_name);
        return value;
    }
    tracing::debug!("Regenerating {} section", section_name);
    generate()
}

impl ClaudeMdGenerator {
    /// Generate CLAUDE.md with optional differential updates
    ///
    /// When `project_root` is provided in the context, this method will:
    /// 1. Load the section manifest from cache
    /// 2. Check which sections have stale inputs
    /// 3. Only regenerate stale sections
    /// 4. Combine cached and new sections
    /// 5. Save the updated manifest
    pub fn generate(ctx: &ClaudeMdContext<'_>) -> Result<GeneratedClaudeMd> {
        // Load cache if project root is available
        let (cache, manifest) = if let Some(root) = ctx.project_root {
            let cache = ClaudeMdCache::new(root);
            let manifest = cache.load_manifest();
            (Some(cache), manifest)
        } else {
            (None, None)
        };

        // Compute input hashes for each section
        let overview_hash = ctx.overview_input_hash();
        let architecture_hash = ctx.architecture_input_hash();
        let standards_hash = ctx.standards_input_hash();
        let domain_hash = ctx.domain_input_hash();
        let gotchas_hash = ctx.gotchas_input_hash();

        // Check staleness and generate sections, using cache when possible
        let overview = get_or_generate(
            &cache,
            manifest.as_ref(),
            "overview",
            &overview_hash,
            |c| c.as_string().cloned(),
            || sections::generate_overview(ctx.detection, ctx.project_name),
        );

        let architecture = if ctx.plan.claude_md_plan.include_architecture {
            get_or_generate(
                &cache,
                manifest.as_ref(),
                "architecture",
                &architecture_hash,
                |c| Some(c.as_optional()),
                || sections::generate_architecture(ctx.conventions, ctx.synthesis),
            )
        } else {
            None
        };

        let standards = if ctx.plan.claude_md_plan.include_conventions {
            get_or_generate(
                &cache,
                manifest.as_ref(),
                "standards",
                &standards_hash,
                |c| c.as_list().cloned(),
                || {
                    sections::generate_standards(
                        ctx.conventions,
                        ctx.constraints,
                        ctx.synthesis,
                        ctx.cross_insights,
                    )
                },
            )
        } else {
            Vec::new()
        };

        let domain_knowledge = get_or_generate(
            &cache,
            manifest.as_ref(),
            "domain",
            &domain_hash,
            |c| Some(c.as_optional()),
            || sections::generate_domain_knowledge(ctx.domain_analysis),
        );

        let gotchas = get_or_generate(
            &cache,
            manifest.as_ref(),
            "gotchas",
            &gotchas_hash,
            |c| c.as_list().cloned(),
            || sections::generate_gotchas(ctx.constraints, ctx.cross_insights),
        );

        // Save updated manifest with content
        if let Some(cache) = cache {
            let mut new_manifest = SectionManifest::new();

            new_manifest.sections.insert(
                "overview".to_string(),
                SectionSource {
                    section_name: "overview".to_string(),
                    input_hash: overview_hash,
                    content_hash: compute_hash(&overview),
                    content: SectionContent::Single(overview.clone()),
                },
            );

            new_manifest.sections.insert(
                "architecture".to_string(),
                SectionSource {
                    section_name: "architecture".to_string(),
                    input_hash: architecture_hash,
                    content_hash: compute_hash(&format!("{:?}", architecture)),
                    content: SectionContent::Optional(architecture.clone()),
                },
            );

            new_manifest.sections.insert(
                "standards".to_string(),
                SectionSource {
                    section_name: "standards".to_string(),
                    input_hash: standards_hash,
                    content_hash: compute_hash(&format!("{:?}", standards)),
                    content: SectionContent::List(standards.clone()),
                },
            );

            new_manifest.sections.insert(
                "domain".to_string(),
                SectionSource {
                    section_name: "domain".to_string(),
                    input_hash: domain_hash,
                    content_hash: compute_hash(&format!("{:?}", domain_knowledge)),
                    content: SectionContent::Optional(domain_knowledge.clone()),
                },
            );

            new_manifest.sections.insert(
                "gotchas".to_string(),
                SectionSource {
                    section_name: "gotchas".to_string(),
                    input_hash: gotchas_hash,
                    content_hash: compute_hash(&format!("{:?}", gotchas)),
                    content: SectionContent::List(gotchas.clone()),
                },
            );

            if let Err(e) = cache.save_manifest(&new_manifest) {
                tracing::warn!("Failed to save CLAUDE.md section manifest: {}", e);
            }
        }

        let mut extracted_docs = Vec::new();
        let architecture = Self::maybe_extract_section(
            architecture,
            "architecture",
            ARCHITECTURE_MAX_LINES,
            &mut extracted_docs,
        );
        let standards = Self::maybe_extract_list_section(
            standards,
            "standards",
            STANDARDS_MAX_LINES,
            STANDARDS_INLINE_COUNT,
            &mut extracted_docs,
        );
        let domain_knowledge = Self::maybe_extract_section(
            domain_knowledge,
            "domain",
            DOMAIN_MAX_LINES,
            &mut extracted_docs,
        );

        let mut imports: Vec<String> = Vec::new();
        for (doc_path, _) in &extracted_docs {
            imports.push(format!("@{}", doc_path));
        }

        imports.push(format!("@{RULES_DIR}/"));
        imports.push(format!("@{SKILLS_DIR}/"));
        imports.push(format!("@{AGENTS_DIR}/"));

        Ok(GeneratedClaudeMd {
            memory: ClaudeMdContent {
                overview,
                architecture,
                commands: Vec::new(),
                standards,
                imports,
                domain_knowledge,
                gotchas,
                navigation: None,
            },
            extracted_docs,
        })
    }

    fn maybe_extract_section(
        content: Option<String>,
        section_name: &str,
        max_lines: usize,
        extracted: &mut Vec<(String, String)>,
    ) -> Option<String> {
        let text = content?;
        let line_count = text.lines().count();
        if line_count <= max_lines {
            return Some(text);
        }

        let doc_path = format!(".claude/docs/{}.md", section_name);
        extracted.push((doc_path.clone(), text));
        Some(format!(
            "See @{} for full {} documentation.",
            doc_path, section_name
        ))
    }

    fn maybe_extract_list_section(
        items: Vec<String>,
        section_name: &str,
        max_lines: usize,
        inline_count: usize,
        extracted: &mut Vec<(String, String)>,
    ) -> Vec<String> {
        if items.len() <= max_lines {
            return items;
        }

        let doc_path = format!(".claude/docs/{}.md", section_name);
        let full_content = items.join("\n");
        extracted.push((doc_path.clone(), full_content));

        let mut result: Vec<String> = items.into_iter().take(inline_count).collect();
        result.push(format!(
            "... see @{} for complete {} list.",
            doc_path, section_name
        ));
        result
    }
}

/// Maximum lines for architecture section before extraction to external doc.
const ARCHITECTURE_MAX_LINES: usize = 30;

/// Maximum items for standards section before partial extraction.
const STANDARDS_MAX_LINES: usize = 50;

/// Number of standards to keep inline when section is extracted.
const STANDARDS_INLINE_COUNT: usize = 5;

/// Maximum lines for domain section before extraction to external doc.
const DOMAIN_MAX_LINES: usize = 30;

/// Result of CLAUDE.md generation including extracted docs.
pub struct GeneratedClaudeMd {
    pub memory: ClaudeMdContent,
    pub extracted_docs: Vec<(String, String)>,
}

/// Minimum number of modules to include a navigation map.
const MIN_MODULES_FOR_NAVIGATION: usize = 3;

/// Generates a navigation map table mapping modules to their rules, agents, and skills.
pub struct NavigationMapGenerator;

/// Segment-level matching: matches on hyphen-separated segments rather than
/// arbitrary substrings. "auth" matches "auth-specialist" but "api" does not
/// match "capital-allocation".
fn matches_module(artifact_name: &str, module_name: &str) -> bool {
    let module_lower = module_name.to_lowercase();
    let artifact_lower = artifact_name.to_lowercase();
    if artifact_lower == module_lower {
        return true;
    }
    artifact_lower
        .split('-')
        .any(|seg| seg == module_lower)
        || module_lower
            .split('-')
            .any(|seg| seg == artifact_lower)
}

impl NavigationMapGenerator {
    /// Generate a markdown table mapping modules to related artifacts.
    ///
    /// Returns `None` if there are fewer than `MIN_MODULES_FOR_NAVIGATION` modules.
    pub fn generate(
        modules: &[crate::types::module_map::DetectedModule],
        rules: &[crate::types::rule::Rule],
        agents: &[crate::types::agent::Agent],
        skills: &[crate::types::skill::Skill],
    ) -> Option<String> {
        if modules.len() < MIN_MODULES_FOR_NAVIGATION {
            return None;
        }

        let mut table = String::from("| Module | Rules | Agent | Skills |\n");
        table.push_str("|--------|-------|-------|--------|\n");

        for module in modules {
            let module_id = &module.module_id;

            let matching_rules: Vec<&str> = rules
                .iter()
                .filter(|r| matches_module(&r.name, module_id))
                .map(|r| r.name.as_str())
                .collect();

            let matching_agents: Vec<&str> = agents
                .iter()
                .filter(|a| matches_module(&a.name, module_id))
                .map(|a| a.name.as_str())
                .collect();

            let matching_skills: Vec<&str> = skills
                .iter()
                .filter(|s| {
                    matches_module(&s.name, module_id)
                        || s.body.to_lowercase().contains(&module_id.to_lowercase())
                })
                .map(|s| s.name.as_str())
                .collect();

            let rules_cell = if matching_rules.is_empty() {
                "-".to_string()
            } else {
                matching_rules.join(", ")
            };
            let agents_cell = if matching_agents.is_empty() {
                "-".to_string()
            } else {
                matching_agents.join(", ")
            };
            let skills_cell = if matching_skills.is_empty() {
                "-".to_string()
            } else {
                matching_skills.join(", ")
            };

            table.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                module_id, rules_cell, agents_cell, skills_cell
            ));
        }

        Some(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::agent::Agent;
    use crate::types::module_map::DetectedModule;
    use crate::types::rule::Rule;
    use crate::types::skill::Skill;

    #[test]
    fn test_navigation_map_too_few_modules() {
        let modules = vec![
            DetectedModule::new("auth", "Auth module"),
            DetectedModule::new("api", "API module"),
        ];
        let result = NavigationMapGenerator::generate(&modules, &[], &[], &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_navigation_map_generates_table() {
        let modules = vec![
            DetectedModule::new("auth", "Auth module"),
            DetectedModule::new("api", "API module"),
            DetectedModule::new("db", "Database module"),
        ];
        let rules = vec![
            Rule::module("auth", vec!["src/auth/**".into()], vec!["# Auth".into()]),
            Rule::module("api", vec!["src/api/**".into()], vec!["# API".into()]),
        ];
        let agents = vec![
            Agent::new("auth-specialist", "Auth specialist", "Auth prompt"),
        ];
        let skills = vec![
            Skill::new("auth-test", "Test auth", "# Test auth module"),
        ];

        let result = NavigationMapGenerator::generate(&modules, &rules, &agents, &skills);
        assert!(result.is_some());

        let table = result.unwrap();
        assert!(table.contains("| Module |"));
        assert!(table.contains("| auth |"));
        assert!(table.contains("auth-specialist"));
        assert!(table.contains("auth-test"));
        assert!(table.contains("| db |"));
    }

    #[test]
    fn test_navigation_map_dash_for_no_matches() {
        let modules = vec![
            DetectedModule::new("auth", "Auth"),
            DetectedModule::new("api", "API"),
            DetectedModule::new("db", "DB"),
        ];

        let result = NavigationMapGenerator::generate(&modules, &[], &[], &[]);
        let table = result.unwrap();

        // All cells should be "-" since no artifacts match
        assert!(table.contains("| auth | - | - | - |"));
    }
}
