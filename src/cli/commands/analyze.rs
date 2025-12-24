//! Analyze Command
//!
//! Builds knowledge graph from source code.
//! Extracts structural information without language-specific pattern matching.

use std::fs;
use std::path::{Path, PathBuf};

use crate::analyzer::StructureAnalyzer;
use crate::analyzer::parser::{
    BashParser, CLangParser, CppLangParser, GoParser, JavaParser, KotlinParser, Language,
    ParseResult, Parser, PythonParser, RubyParser, RustParser, TypeScriptParser,
};
use crate::analyzer::scanner::FileScanner;
use crate::config::{Config, ConfigLoader};
use crate::storage::{Database, GraphStore};
use crate::types::{Result, WeaveError};

pub fn run(full: bool, path: Option<PathBuf>, skip_docs: bool) -> Result<()> {
    let root = path.unwrap_or_else(|| PathBuf::from("."));
    let weavewiki_dir = root.join(".weavewiki");

    if !weavewiki_dir.exists() {
        return Err(WeaveError::NotInitialized);
    }

    let config = load_config()?;
    let db = Database::open(weavewiki_dir.join("graph/graph.db"))?;
    let graph_store = GraphStore::new(&db);

    println!("Starting analysis...");

    // Clear existing data if full rebuild requested
    if full {
        graph_store.clear()?;
        println!("  Cleared existing graph data");
    }

    // Step 1: Scan files
    let scanner = FileScanner::new(&root)
        .with_exclude(config.analysis.exclude.clone())
        .with_max_file_size(config.analysis.max_file_size as u64);
    let files = scanner.scan()?;
    println!("Found {} files to analyze", files.len());

    // Step 2: Parse files and build graph
    let mut total_nodes = 0;
    let mut total_edges = 0;
    let mut processed = 0;
    let mut language_counts: std::collections::HashMap<&str, u32> =
        std::collections::HashMap::new();

    for file in &files {
        let lang = Language::from_path(&file.path);

        if let Some(result) = parse_file(&file.path, lang)? {
            for node in &result.nodes {
                graph_store.insert_node(node)?;
            }
            for edge in &result.edges {
                graph_store.insert_edge(edge)?;
            }
            total_nodes += result.nodes.len();
            total_edges += result.edges.len();

            let lang_name = match lang {
                Language::TypeScript | Language::JavaScript => "TypeScript/JavaScript",
                Language::Python => "Python",
                Language::Rust => "Rust",
                Language::Go => "Go",
                Language::Java => "Java",
                Language::Kotlin => "Kotlin",
                Language::Ruby => "Ruby",
                Language::C | Language::Cpp => "C/C++",
                Language::Bash => "Bash",
                _ => "Other",
            };
            *language_counts.entry(lang_name).or_insert(0) += 1;
        }

        processed += 1;
        if processed % 100 == 0 {
            println!("  Processed {} files...", processed);
        }
    }

    println!("Parsed {} nodes and {} edges", total_nodes, total_edges);

    // Step 3: Structure analysis (universal, no pattern matching)
    println!("Analyzing code structure...");
    let analyzer = StructureAnalyzer::new(&db);
    let structure = analyzer.analyze()?;

    println!(
        "  Found {} directories, {} entry points, {} hotspots",
        structure.directories.len(),
        structure.entry_points.len(),
        structure.hotspots.len()
    );

    // Print language summary
    if !language_counts.is_empty() {
        println!("\nLanguages detected:");
        let mut sorted: Vec<_> = language_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (lang, count) in sorted {
            println!("  {}: {} files", lang, count);
        }
    }

    if !skip_docs {
        println!("\nTo generate AI-driven documentation, run: weavewiki generate");
    }

    println!("Analysis complete!");

    Ok(())
}

fn load_config() -> Result<Config> {
    ConfigLoader::load()
}

fn parse_file(path: &Path, lang: Language) -> Result<Option<ParseResult>> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let path_str = path.to_string_lossy();

    let result = match lang {
        Language::TypeScript | Language::JavaScript => {
            TypeScriptParser::new()?.parse(&path_str, &content)?
        }
        Language::Python => PythonParser::new()?.parse(&path_str, &content)?,
        Language::Rust => RustParser::new()?.parse(&path_str, &content)?,
        Language::Go => GoParser::new()?.parse(&path_str, &content)?,
        Language::Java => JavaParser::new()?.parse(&path_str, &content)?,
        Language::Kotlin => KotlinParser::new()?.parse(&path_str, &content)?,
        Language::Ruby => RubyParser::new()?.parse(&path_str, &content)?,
        Language::C => CLangParser::new()?.parse(&path_str, &content)?,
        Language::Cpp => CppLangParser::new()?.parse(&path_str, &content)?,
        Language::Bash => BashParser::new()?.parse(&path_str, &content)?,
        _ => return Ok(None),
    };

    Ok(Some(result))
}
