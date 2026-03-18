//! Context Enricher Module
//!
//! Provides 100% information preservation through aggregation (not truncation).
//! All analysis data reaches LLM via summarization with full statistics.

use std::collections::HashMap;

// Confidence level thresholds
const HIGH_CONFIDENCE_MIN_PATTERNS: usize = 10;
const HIGH_CONFIDENCE_MIN_CONSTRAINTS: usize = 5;
const MEDIUM_CONFIDENCE_MIN_PATTERNS: usize = 3;
const MEDIUM_CONFIDENCE_MIN_CONSTRAINTS: usize = 2;

// Module size classification (file count)
const MODULE_SIZE_SMALL_MAX: usize = 5;
const MODULE_SIZE_MEDIUM_MAX: usize = 20;


use crate::pipeline::analysis::ast_enrichment::{Visibility, TypeKind};
use crate::pipeline::analysis::{AstFacts, DeepAnalysisResult};
use crate::pipeline::context::VerifiedFileRegistry;

#[derive(Debug, Clone, Default)]
pub struct StructuralContext {
    pub entry_points: AggregatedEntryPoints,
    pub modules: AggregatedModules,
    pub file_count: usize,
    pub language_distribution: HashMap<String, usize>,
    pub key_directories: Vec<String>,
    pub primary_language: String,
}

#[derive(Debug, Clone, Default)]
pub struct AggregatedEntryPoints {
    pub total: usize,
    pub by_kind: HashMap<String, usize>,
    pub items: Vec<EntryPointInfo>,
}

impl AggregatedEntryPoints {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EntryPointInfo> {
        self.items.iter()
    }

    pub fn format(&self) -> String {
        if self.total == 0 {
            return "No entry points detected.".into();
        }

        let mut lines = vec![format!("Entry Points ({} total):", self.total)];

        if !self.by_kind.is_empty() {
            let kinds: Vec<_> = self
                .by_kind
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect();
            lines.push(format!("  Distribution: {}", kinds.join(", ")));
        }

        for ep in &self.items {
            lines.push(format!("  - @{} [{}] {}", ep.path, ep.kind, ep.description));
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone, Default)]
pub struct AggregatedModules {
    pub total: usize,
    pub core_count: usize,
    pub items: Vec<ModuleOverview>,
    pub by_size: HashMap<String, usize>,
}

impl AggregatedModules {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ModuleOverview> {
        self.items.iter()
    }

    pub fn format(&self) -> String {
        if self.total == 0 {
            return "No modules detected.".into();
        }

        let mut lines = vec![format!(
            "Modules ({} total, {} core):",
            self.total, self.core_count
        )];

        for m in &self.items {
            let core_marker = if m.is_core { " (core)" } else { "" };
            lines.push(format!(
                "  - {} ({} files){}",
                m.name, m.file_count, core_marker
            ));
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone)]
pub struct EntryPointInfo {
    pub path: String,
    pub kind: StructuralEntryPoint,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructuralEntryPoint {
    Main,
    LibRoot,
    ApiHandler,
    CliCommand,
}

impl std::fmt::Display for StructuralEntryPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Main => write!(f, "main"),
            Self::LibRoot => write!(f, "lib"),
            Self::ApiHandler => write!(f, "api"),
            Self::CliCommand => write!(f, "cli"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleOverview {
    pub name: String,
    pub path: String,
    pub file_count: usize,
    pub is_core: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AstContext {
    pub stats: AstStats,
    pub dominant_patterns: Vec<String>,
    pub key_types: Vec<KeyTypeInfo>,
    pub key_functions: Vec<KeyFunctionInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct AstStats {
    pub total_functions: usize,
    pub public_functions: usize,
    pub async_functions: usize,
    pub structs: usize,
    pub enums: usize,
    pub traits: usize,
    pub total_types: usize,
}

impl AstStats {
    pub fn format(&self) -> String {
        format!(
            "Functions: {} total ({} public, {} async)\nTypes: {} structs, {} enums, {} traits",
            self.total_functions,
            self.public_functions,
            self.async_functions,
            self.structs,
            self.enums,
            self.traits
        )
    }
}

#[derive(Debug, Clone)]
pub struct KeyTypeInfo {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub kind: String,
    pub field_count: usize,
}

#[derive(Debug, Clone)]
pub struct KeyFunctionInfo {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub is_async: bool,
    pub param_count: usize,
}

#[derive(Debug, Clone)]
pub enum ConfidenceLevel {
    High { score: f32, patterns: usize, message: String },
    Medium { score: f32, patterns: usize, message: String },
    Low { score: f32, patterns: usize, message: String },
    StructureOnly { message: String },
}

impl ConfidenceLevel {
    pub fn score(&self) -> f32 {
        match self {
            Self::High { score, .. } => *score,
            Self::Medium { score, .. } => *score,
            Self::Low { score, .. } => *score,
            Self::StructureOnly { .. } => 0.0,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::High { .. } => "HIGH",
            Self::Medium { .. } => "MEDIUM",
            Self::Low { .. } => "LOW",
            Self::StructureOnly { .. } => "STRUCTURE_ONLY",
        }
    }

    pub fn guidance(&self) -> &str {
        match self {
            Self::High { message, .. }
            | Self::Medium { message, .. }
            | Self::Low { message, .. }
            | Self::StructureOnly { message } => message,
        }
    }
}

pub struct ContextEnricher<'a> {
    file_registry: &'a VerifiedFileRegistry,
    ast_facts: Option<&'a AstFacts>,
}

impl<'a> ContextEnricher<'a> {
    pub fn new(file_registry: &'a VerifiedFileRegistry) -> Self {
        Self {
            file_registry,
            ast_facts: None,
        }
    }

    pub fn ast(mut self, ast: &'a AstFacts) -> Self {
        self.ast_facts = Some(ast);
        self
    }

    pub fn build_structural_context(&self) -> StructuralContext {
        let entry_points = self.aggregate_entry_points();
        let modules = self.aggregate_modules();
        let language_distribution = self.compute_language_stats();
        let key_directories = self.detect_key_directories();
        let primary_language = language_distribution
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(lang, _)| lang.clone())
            .unwrap_or_else(|| "unknown".to_string());

        StructuralContext {
            entry_points,
            modules,
            file_count: self.file_registry.file_count(),
            language_distribution,
            key_directories,
            primary_language,
        }
    }

    fn compute_language_stats(&self) -> HashMap<String, usize> {
        use crate::analyzer::parser::Language;
        let mut stats = HashMap::new();
        for file in self.file_registry.all_files() {
            let lang = Language::from_path(file);
            if lang.is_known() {
                *stats.entry(lang.as_str().to_string()).or_default() += 1;
            }
        }
        stats
    }

    pub fn build_ast_context(&self) -> Option<AstContext> {
        let ast = self.ast_facts?;

        let total_functions = ast.functions.len();
        let public_functions = ast
            .functions
            .values()
            .filter(|f| matches!(f.visibility, Visibility::Public))
            .count();
        let async_functions = ast.functions.values().filter(|f| f.is_async).count();

        let structs = ast
            .types
            .values()
            .filter(|t| matches!(t.kind, TypeKind::Struct))
            .count();
        let enums = ast
            .types
            .values()
            .filter(|t| matches!(t.kind, TypeKind::Enum))
            .count();
        let traits = ast.traits.len();
        let total_types = ast.types.len();

        let stats = AstStats {
            total_functions,
            public_functions,
            async_functions,
            structs,
            enums,
            traits,
            total_types,
        };

        let dominant_patterns = self.extract_dominant_patterns(ast);
        let key_types = self.extract_key_types(ast);
        let key_functions = self.extract_key_functions(ast);

        Some(AstContext {
            stats,
            dominant_patterns,
            key_types,
            key_functions,
        })
    }

    pub fn compute_confidence(&self, analysis: Option<&DeepAnalysisResult>) -> ConfidenceLevel {
        match analysis {
            Some(a) if a.patterns.len() >= HIGH_CONFIDENCE_MIN_PATTERNS
                && a.constraints.len() >= HIGH_CONFIDENCE_MIN_CONSTRAINTS =>
            {
                ConfidenceLevel::High {
                    score: 0.9,
                    patterns: a.patterns.len(),
                    message: "Rich analysis available. Ground all guidance in detected patterns and constraints.".into(),
                }
            }
            Some(a) if a.patterns.len() >= MEDIUM_CONFIDENCE_MIN_PATTERNS
                || a.constraints.len() >= MEDIUM_CONFIDENCE_MIN_CONSTRAINTS =>
            {
                ConfidenceLevel::Medium {
                    score: 0.6,
                    patterns: a.patterns.len(),
                    message: "Moderate analysis. Use detected patterns and supplement with domain expertise.".into(),
                }
            }
            Some(a) => ConfidenceLevel::Low {
                score: 0.3,
                patterns: a.patterns.len(),
                message: "Limited patterns detected. Apply domain expertise to infer high-value guidance from file structure and naming conventions. Generate specific, actionable content—not generic advice.".into(),
            },
            None => ConfidenceLevel::StructureOnly {
                message: "Structure-only mode. Leverage file organization and naming patterns to generate domain-specific guidance. Focus on concrete, project-specific insights.".into(),
            },
        }
    }

    fn aggregate_entry_points(&self) -> AggregatedEntryPoints {
        let mut items = Vec::new();
        let mut by_kind: HashMap<String, usize> = HashMap::new();

        for file in self.file_registry.all_files() {
            let path_lower = file.to_lowercase();
            let entry = if path_lower.ends_with("main.rs")
                || path_lower.ends_with("main.py")
                || path_lower.ends_with("main.go")
                || path_lower.ends_with("main.ts")
            {
                Some((StructuralEntryPoint::Main, "Application entry point"))
            } else if path_lower.ends_with("lib.rs")
                || path_lower.ends_with("index.ts")
                || path_lower.ends_with("__init__.py")
            {
                Some((StructuralEntryPoint::LibRoot, "Library root"))
            } else if (path_lower.contains("/api/")
                || path_lower.contains("/routes/")
                || path_lower.contains("/handlers/"))
                && (path_lower.ends_with("mod.rs")
                    || path_lower.ends_with("index.ts")
                    || path_lower.ends_with("__init__.py"))
            {
                Some((StructuralEntryPoint::ApiHandler, "API handler entry"))
            } else if (path_lower.contains("/cli/") || path_lower.contains("/commands/"))
                && (path_lower.ends_with("mod.rs") || path_lower.ends_with("index.ts"))
            {
                Some((StructuralEntryPoint::CliCommand, "CLI command entry"))
            } else {
                None
            };

            if let Some((kind, desc)) = entry {
                *by_kind.entry(kind.to_string()).or_default() += 1;
                items.push(EntryPointInfo {
                    path: file.to_string(),
                    kind,
                    description: desc.into(),
                });
            }
        }

        items.sort_by(|a, b| {
            let priority = |k: &StructuralEntryPoint| match k {
                StructuralEntryPoint::Main => 0,
                StructuralEntryPoint::LibRoot => 1,
                StructuralEntryPoint::CliCommand => 2,
                StructuralEntryPoint::ApiHandler => 3,
            };
            priority(&a.kind).cmp(&priority(&b.kind))
        });

        let total = items.len();
        AggregatedEntryPoints {
            total,
            by_kind,
            items,
        }
    }

    fn aggregate_modules(&self) -> AggregatedModules {
        let files_by_module = self.file_registry.files_by_module();
        let mut items: Vec<_> = files_by_module
            .into_iter()
            .map(|(name, files)| {
                let path = files
                    .first()
                    .map(|f| {
                        f.path
                            .rsplit_once('/')
                            .map(|(dir, _)| dir.to_string())
                            .unwrap_or_else(|| f.path.clone())
                    })
                    .unwrap_or_default();

                let is_core = name == "core"
                    || name == "lib"
                    || name == "src"
                    || path.contains("/core/")
                    || path.contains("/domain/");

                ModuleOverview {
                    name,
                    path,
                    file_count: files.len(),
                    is_core,
                }
            })
            .collect();

        items.sort_by(|a, b| {
            b.is_core
                .cmp(&a.is_core)
                .then_with(|| b.file_count.cmp(&a.file_count))
        });

        let total = items.len();
        let core_count = items.iter().filter(|m| m.is_core).count();
        let mut by_size = HashMap::new();
        for m in &items {
            let size_bucket = if m.file_count <= MODULE_SIZE_SMALL_MAX {
                "small"
            } else if m.file_count <= MODULE_SIZE_MEDIUM_MAX {
                "medium"
            } else {
                "large"
            };
            *by_size.entry(size_bucket.to_string()).or_default() += 1;
        }

        AggregatedModules {
            total,
            core_count,
            items,
            by_size,
        }
    }

    fn detect_key_directories(&self) -> Vec<String> {
        let mut dirs: HashMap<String, usize> = HashMap::new();

        for file in self.file_registry.all_files() {
            if let Some((dir, _)) = file.rsplit_once('/') {
                let top_level = dir.split('/').take(2).collect::<Vec<_>>().join("/");
                *dirs.entry(top_level).or_default() += 1;
            }
        }

        let mut sorted: Vec<_> = dirs.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.into_iter().map(|(d, _)| d).collect()
    }

    fn extract_dominant_patterns(&self, ast: &AstFacts) -> Vec<String> {
        let mut patterns = Vec::new();

        let total_funcs = ast.functions.len();
        if total_funcs > 0 {
            let async_count = ast.functions.values().filter(|f| f.is_async).count();
            let async_ratio = async_count as f32 / total_funcs as f32;

            if async_ratio > 0.3 {
                patterns.push(format!(
                    "async/await ({:.0}% of functions)",
                    async_ratio * 100.0
                ));
            }
        }

        let has_result_pattern = ast
            .types
            .values()
            .any(|t| t.name.contains("Result") || t.name.contains("Error"));
        if has_result_pattern {
            patterns.push("Result<T, E> error handling".into());
        }

        if !ast.traits.is_empty() {
            patterns.push(format!(
                "Trait-based abstraction ({} traits)",
                ast.traits.len()
            ));
        }

        patterns
    }

    fn extract_key_types(&self, ast: &AstFacts) -> Vec<KeyTypeInfo> {
        let mut types: Vec<_> = ast
            .types
            .values()
            .filter(|t| matches!(t.visibility, Visibility::Public))
            .map(|t| KeyTypeInfo {
                name: t.name.clone(),
                file: t.file.clone(),
                line: t.line,
                kind: t.kind.to_string(),
                field_count: t.field_count,
            })
            .collect();

        types.sort_by(|a, b| b.field_count.cmp(&a.field_count));
        types
    }

    fn extract_key_functions(&self, ast: &AstFacts) -> Vec<KeyFunctionInfo> {
        let mut funcs: Vec<_> = ast
            .functions
            .values()
            .filter(|f| matches!(f.visibility, Visibility::Public))
            .map(|f| KeyFunctionInfo {
                name: f.name.clone(),
                file: f.file.clone(),
                line: f.line,
                is_async: f.is_async,
                param_count: f.parameter_count,
            })
            .collect();

        funcs.sort_by(|a, b| b.param_count.cmp(&a.param_count));
        funcs
    }
}

#[derive(Debug, Clone)]
pub struct EnrichedContext {
    pub structural: StructuralContext,
    pub ast: Option<AstContext>,
    pub confidence: ConfidenceLevel,
}

impl EnrichedContext {
    pub fn format_structural_section(&self) -> String {
        let mut lines = Vec::new();

        lines.push(format!("Total Files: {}", self.structural.file_count));
        lines.push(format!(
            "Primary Language: {}",
            self.structural.primary_language
        ));

        lines.push(String::new());
        lines.push(self.structural.entry_points.format());

        lines.push(String::new());
        lines.push(self.structural.modules.format());

        if !self.structural.key_directories.is_empty() {
            lines.push(format!(
                "\nKey Directories: {}",
                self.structural.key_directories.join(", ")
            ));
        }

        lines.join("\n")
    }

    pub fn format_ast_section(&self) -> String {
        match &self.ast {
            Some(ast) => {
                let mut lines = Vec::new();

                lines.push(ast.stats.format());

                if !ast.dominant_patterns.is_empty() {
                    lines.push(format!(
                        "\nDominant Patterns: {}",
                        ast.dominant_patterns.join(", ")
                    ));
                }

                if !ast.key_types.is_empty() {
                    lines.push(format!("\nKey Types ({} total):", ast.key_types.len()));
                    for t in &ast.key_types {
                        lines.push(format!(
                            "  - {} @{}:{} ({}, {} fields)",
                            t.name, t.file, t.line, t.kind, t.field_count
                        ));
                    }
                }

                if !ast.key_functions.is_empty() {
                    lines.push(format!("\nKey Functions ({} total):", ast.key_functions.len()));
                    for f in &ast.key_functions {
                        let async_marker = if f.is_async { "async " } else { "" };
                        lines.push(format!(
                            "  - {}{}() @{}:{}",
                            async_marker, f.name, f.file, f.line
                        ));
                    }
                }

                lines.join("\n")
            }
            None => "AST analysis not available. Using structural inference only.".into(),
        }
    }

    pub fn format_confidence_section(&self) -> String {
        format!(
            "[{}] {}\n{}",
            self.confidence.label(),
            if self.confidence.score() > 0.0 {
                format!("Score: {:.0}%", self.confidence.score() * 100.0)
            } else {
                "No score".into()
            },
            self.confidence.guidance()
        )
    }
}

pub fn enrich_context<'a>(
    file_registry: &'a VerifiedFileRegistry,
    ast_facts: Option<&'a AstFacts>,
    analysis: Option<&DeepAnalysisResult>,
) -> EnrichedContext {
    let enricher = match ast_facts {
        Some(ast) => ContextEnricher::new(file_registry).ast(ast),
        None => ContextEnricher::new(file_registry),
    };

    EnrichedContext {
        structural: enricher.build_structural_context(),
        ast: enricher.build_ast_context(),
        confidence: enricher.compute_confidence(analysis),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_levels() {
        let registry = VerifiedFileRegistry::empty();
        let enricher = ContextEnricher::new(&registry);

        let none_conf = enricher.compute_confidence(None);
        assert!(matches!(none_conf, ConfidenceLevel::StructureOnly { .. }));
    }

    #[test]
    fn test_entry_point_display() {
        assert_eq!(format!("{}", StructuralEntryPoint::Main), "main");
        assert_eq!(format!("{}", StructuralEntryPoint::LibRoot), "lib");
    }

    #[test]
    fn test_aggregated_entry_points_format() {
        let agg = AggregatedEntryPoints {
            total: 3,
            by_kind: HashMap::from([("main".into(), 1), ("lib".into(), 2)]),
            items: vec![],
        };
        let formatted = agg.format();
        assert!(formatted.contains("3 total"));
    }
}
