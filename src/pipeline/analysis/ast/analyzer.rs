//! AST Analyzer Agent
//!
//! Extracts ground-truth facts from source code using tree-sitter.
//! Results are used to validate LLM-generated references.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::dependencies::{DependencyGraph, DependencyType};
use super::structure::{
    AstProjectStructure, ComplexityMetrics, EntryPointInfo, EntryPointKind, ExportInfo, ExportKind,
    FunctionInfo, ImportInfo, ModuleInfo, ParameterInfo, PublicApiSurface, TypeInfo, TypeKind,
    Visibility,
};
use crate::Result;
use crate::analyzer::parser::{Language, Parser, create_parser};
use crate::analyzer::scanner::FileScanner;
use crate::config::AnalysisConfig;
use crate::pipeline::storage::DurableStore;
use crate::types::{NodeType, Visibility as NodeVisibility};

pub struct AstAnalyzerAgent {
    project_root: PathBuf,
    analysis_config: AnalysisConfig,
    parsers: HashMap<Language, Box<dyn Parser>>,
    store: Option<Arc<DurableStore>>,
}

impl AstAnalyzerAgent {
    pub fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            analysis_config: AnalysisConfig::default(),
            parsers: HashMap::new(),
            store: None,
        }
    }

    pub fn with_config(mut self, config: AnalysisConfig) -> Self {
        self.analysis_config = config;
        self
    }

    pub fn with_store(mut self, store: Arc<DurableStore>) -> Self {
        self.store = Some(store);
        self
    }

    fn get_parser(&mut self, language: Language) -> Option<&dyn Parser> {
        if let std::collections::hash_map::Entry::Vacant(e) = self.parsers.entry(language)
            && let Ok(parser) = create_parser(language)
        {
            e.insert(parser);
        }
        self.parsers.get(&language).map(|p| p.as_ref())
    }

    pub async fn analyze(&mut self) -> Result<AstAnalysisResult> {
        info!("Starting AST analysis for {:?}", self.project_root);

        let scanner = FileScanner::new(&self.project_root, &self.analysis_config);
        let files = scanner.scan()?;

        info!("Found {} files to analyze", files.len());

        let mut file_analyses = Vec::new();
        let mut dependency_graph = DependencyGraph::new();

        for scanned_file in &files {
            let rel_path = scanned_file
                .path
                .strip_prefix(&self.project_root)
                .unwrap_or(&scanned_file.path)
                .to_string_lossy()
                .to_string();

            let language = Language::from_path(&rel_path);
            if !language.has_parser_support() {
                continue;
            }

            match self
                .analyze_file(&scanned_file.path, &rel_path, language)
                .await
            {
                Ok(analysis) => {
                    for import in &analysis.imports {
                        dependency_graph.add_edge(
                            rel_path.clone(),
                            import.path.clone(),
                            DependencyType::Import,
                        );
                    }

                    if let Some(store) = &self.store
                        && let Err(e) = store.save_file_ast(&rel_path, &analysis).await
                    {
                        warn!("Failed to save AST for {}: {}", rel_path, e);
                    }

                    file_analyses.push(analysis);
                }
                Err(e) => {
                    debug!("Failed to parse {}: {}", rel_path, e);
                }
            }
        }

        let project_structure = self.build_project_structure(&file_analyses);
        let public_api = self.extract_public_api(&file_analyses);

        let result = AstAnalysisResult {
            files: file_analyses,
            project_structure,
            dependency_graph,
            public_api,
            analyzed_at: Utc::now(),
        };

        if let Some(store) = &self.store {
            store.save_ast_result(&result).await?;
        }

        info!(
            "AST analysis complete: {} files processed",
            result.files.len()
        );
        Ok(result)
    }

    async fn analyze_file(
        &mut self,
        path: &Path,
        rel_path: &str,
        language: Language,
    ) -> Result<FileAstAnalysis> {
        let content = std::fs::read_to_string(path)?;

        let parser = self.get_parser(language).ok_or_else(|| {
            crate::ClaudegenError::Config(format!("No parser for {:?}", language))
        })?;

        let parse_result = parser.parse(rel_path, &content)?;

        let mut functions = Vec::new();
        let mut types = Vec::new();
        let mut imports = Vec::new();
        let mut exports = Vec::new();

        for node in &parse_result.nodes {
            match node.node_type {
                NodeType::Function | NodeType::Method => {
                    let is_public = node
                        .metadata
                        .visibility
                        .as_ref()
                        .map(|v| matches!(v, NodeVisibility::Public))
                        .unwrap_or(false);

                    let (is_async, params, return_type) = node
                        .metadata
                        .signature
                        .as_ref()
                        .map(|sig| {
                            (
                                sig.is_async,
                                sig.parameters
                                    .iter()
                                    .map(|p| ParameterInfo {
                                        name: p.name.clone(),
                                        type_annotation: p.param_type.clone(),
                                    })
                                    .collect::<Vec<_>>(),
                                sig.return_type.clone(),
                            )
                        })
                        .unwrap_or((false, Vec::new(), None));

                    functions.push(FunctionInfo {
                        name: node.name.clone(),
                        line_start: node.evidence.start_line as usize,
                        line_end: node.evidence.end_line as usize,
                        visibility: if is_public {
                            Visibility::Public
                        } else {
                            Visibility::Private
                        },
                        is_async,
                        parameters: params,
                        return_type,
                        doc_comment: node.metadata.description.clone(),
                    });

                    if is_public {
                        exports.push(ExportInfo {
                            name: node.name.clone(),
                            kind: ExportKind::Function,
                            line: node.evidence.start_line as usize,
                        });
                    }
                }
                NodeType::Class | NodeType::Interface | NodeType::Enum | NodeType::Type => {
                    let is_public = node
                        .metadata
                        .visibility
                        .as_ref()
                        .map(|v| matches!(v, NodeVisibility::Public))
                        .unwrap_or(false);

                    let kind = match node.node_type {
                        NodeType::Class => TypeKind::Class,
                        NodeType::Interface => TypeKind::Interface,
                        NodeType::Enum => TypeKind::Enum,
                        NodeType::Type => TypeKind::Struct,
                        _ => TypeKind::TypeAlias,
                    };

                    types.push(TypeInfo {
                        name: node.name.clone(),
                        kind,
                        line_start: node.evidence.start_line as usize,
                        line_end: node.evidence.end_line as usize,
                        visibility: if is_public {
                            Visibility::Public
                        } else {
                            Visibility::Private
                        },
                        fields: Vec::new(),
                        methods: Vec::new(),
                        doc_comment: node.metadata.description.clone(),
                    });

                    if is_public {
                        exports.push(ExportInfo {
                            name: node.name.clone(),
                            kind: ExportKind::Type,
                            line: node.evidence.start_line as usize,
                        });
                    }
                }
                NodeType::Module => {
                    if node.name != rel_path {
                        imports.push(ImportInfo {
                            path: node.name.clone(),
                            items: Vec::new(),
                            line: node.evidence.start_line as usize,
                            is_external: !node.name.starts_with('.')
                                && !node.name.starts_with("crate"),
                        });
                    }
                }
                _ => {}
            }
        }

        for edge in &parse_result.edges {
            if edge.edge_type == crate::types::EdgeType::DependsOn {
                let target = &edge.target_id;
                if !imports.iter().any(|i| &i.path == target) {
                    imports.push(ImportInfo {
                        path: target.clone(),
                        items: Vec::new(),
                        line: edge.evidence.start_line as usize,
                        is_external: !target.starts_with('.') && !target.starts_with("crate"),
                    });
                }
            }
        }

        let complexity = self.compute_complexity(&content);

        Ok(FileAstAnalysis {
            path: rel_path.to_string(),
            language,
            functions,
            types,
            imports,
            exports,
            complexity_metrics: complexity,
            line_count: content.lines().count(),
            analyzed_at: Utc::now(),
        })
    }

    fn compute_complexity(&self, content: &str) -> ComplexityMetrics {
        let lines: Vec<&str> = content.lines().collect();
        let lines_of_code = lines.iter().filter(|l| !l.trim().is_empty()).count();
        let comment_lines = lines
            .iter()
            .filter(|l| {
                let trimmed = l.trim();
                trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*")
            })
            .count();

        let mut cyclomatic: usize = 1;
        let mut nesting: usize = 0;
        let mut max_nesting: usize = 0;

        for line in &lines {
            let trimmed = line.trim();

            if trimmed.contains("if ")
                || trimmed.contains("else if")
                || trimmed.contains("match ")
                || trimmed.contains("while ")
                || trimmed.contains("for ")
                || trimmed.contains("loop ")
            {
                cyclomatic += 1;
            }

            if trimmed.contains("&&") || trimmed.contains("||") {
                cyclomatic += 1;
            }

            let opens = trimmed.matches('{').count();
            let closes = trimmed.matches('}').count();
            nesting = nesting.saturating_add(opens).saturating_sub(closes);
            max_nesting = max_nesting.max(nesting);
        }

        ComplexityMetrics {
            cyclomatic,
            cognitive: cyclomatic + max_nesting,
            lines_of_code,
            comment_lines,
            nesting_depth: max_nesting,
        }
    }

    fn build_project_structure(&self, analyses: &[FileAstAnalysis]) -> AstProjectStructure {
        let mut modules: HashMap<String, Vec<String>> = HashMap::new();

        for analysis in analyses {
            let module = self.get_module_name(&analysis.path);
            modules
                .entry(module)
                .or_default()
                .push(analysis.path.clone());
        }

        let module_infos: Vec<ModuleInfo> = modules
            .into_iter()
            .map(|(name, files)| {
                let public_items: Vec<String> = analyses
                    .iter()
                    .filter(|a| files.contains(&a.path))
                    .flat_map(|a| a.exports.iter().map(|e| e.name.clone()))
                    .collect();

                ModuleInfo {
                    name: name.clone(),
                    path: name,
                    files,
                    public_items,
                    dependencies: Vec::new(),
                }
            })
            .collect();

        let entry_points = self.find_entry_points(analyses);

        AstProjectStructure {
            root: self.project_root.to_string_lossy().to_string(),
            total_files: analyses.len(),
            total_functions: analyses.iter().map(|a| a.functions.len()).sum(),
            total_types: analyses.iter().map(|a| a.types.len()).sum(),
            modules: module_infos,
            layers: Vec::new(),
            entry_points,
        }
    }

    fn get_module_name(&self, path: &str) -> String {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() > 1 {
            parts[..parts.len() - 1].join("/")
        } else {
            ".".to_string()
        }
    }

    fn find_entry_points(&self, analyses: &[FileAstAnalysis]) -> Vec<EntryPointInfo> {
        let mut entry_points = Vec::new();

        for analysis in analyses {
            let path_lower = analysis.path.to_lowercase();

            if path_lower.contains("main.rs")
                || path_lower.contains("main.ts")
                || path_lower.contains("main.py")
                || path_lower.contains("main.go")
            {
                entry_points.push(EntryPointInfo {
                    path: analysis.path.clone(),
                    name: "main".to_string(),
                    kind: EntryPointKind::MainFunction,
                });
            } else if path_lower.contains("lib.rs")
                || path_lower.contains("index.ts")
                || path_lower.contains("__init__.py")
            {
                entry_points.push(EntryPointInfo {
                    path: analysis.path.clone(),
                    name: "lib".to_string(),
                    kind: EntryPointKind::LibraryRoot,
                });
            }
        }

        entry_points
    }

    fn extract_public_api(&self, analyses: &[FileAstAnalysis]) -> PublicApiSurface {
        let mut api = PublicApiSurface::default();

        for analysis in analyses {
            for func in &analysis.functions {
                if func.visibility == Visibility::Public {
                    api.functions.push(super::structure::ApiFunctionInfo {
                        name: func.name.clone(),
                        path: analysis.path.clone(),
                        signature: format!(
                            "fn {}({}) -> {}",
                            func.name,
                            func.parameters
                                .iter()
                                .map(|p| p.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            func.return_type.as_deref().unwrap_or("()")
                        ),
                        doc: func.doc_comment.clone(),
                    });
                }
            }

            for type_info in &analysis.types {
                if type_info.visibility == Visibility::Public {
                    api.types.push(super::structure::ApiTypeInfo {
                        name: type_info.name.clone(),
                        path: analysis.path.clone(),
                        kind: type_info.kind,
                        doc: type_info.doc_comment.clone(),
                    });
                }
            }
        }

        api
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstAnalysisResult {
    pub files: Vec<FileAstAnalysis>,
    pub project_structure: AstProjectStructure,
    pub dependency_graph: DependencyGraph,
    pub public_api: PublicApiSurface,
    pub analyzed_at: DateTime<Utc>,
}

impl AstAnalysisResult {
    pub fn get_file(&self, path: &str) -> Option<&FileAstAnalysis> {
        self.files.iter().find(|f| f.path == path)
    }

    pub fn file_exists(&self, path: &str) -> bool {
        self.files.iter().any(|f| f.path == path)
    }

    pub fn function_exists(&self, path: &str, name: &str) -> bool {
        self.get_file(path)
            .map(|f| f.functions.iter().any(|func| func.name == name))
            .unwrap_or(false)
    }

    pub fn type_exists(&self, path: &str, name: &str) -> bool {
        self.get_file(path)
            .map(|f| f.types.iter().any(|t| t.name == name))
            .unwrap_or(false)
    }

    pub fn line_in_range(&self, path: &str, line: usize) -> bool {
        self.get_file(path)
            .map(|f| line <= f.line_count)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAstAnalysis {
    pub path: String,
    pub language: Language,
    pub functions: Vec<FunctionInfo>,
    pub types: Vec<TypeInfo>,
    pub imports: Vec<ImportInfo>,
    pub exports: Vec<ExportInfo>,
    pub complexity_metrics: ComplexityMetrics,
    pub line_count: usize,
    pub analyzed_at: DateTime<Utc>,
}

impl FileAstAnalysis {
    pub fn is_complex(&self) -> bool {
        self.complexity_metrics.is_complex()
    }

    pub fn has_public_api(&self) -> bool {
        !self.exports.is_empty()
    }
}
