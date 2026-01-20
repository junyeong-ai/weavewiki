//! Analyze Command - Builds knowledge graph from source code

use std::fs;
use std::path::{Path, PathBuf};

use crate::analyzer::StructureAnalyzer;
use crate::analyzer::parser::{
    BashParser, GoParser, Language, ParseResult, Parser, PythonParser, RustParser, TypeScriptParser,
};
use crate::analyzer::scanner::FileScanner;
use crate::config::{Config, ConfigLoader};
use crate::constants::cli as cli_constants;
use crate::storage::{Database, GraphStore};
use crate::types::Result;

pub fn run(full: bool, path: Option<PathBuf>, skip_docs: bool) -> Result<()> {
    let root = path.unwrap_or_else(|| PathBuf::from("."));
    let claudegen_dir = root.join(".claudegen");

    if !claudegen_dir.exists() {
        std::fs::create_dir_all(&claudegen_dir)?;
    }

    let config = load_config()?;
    let db = Database::open(claudegen_dir.join("claudegen.db"))?;
    let graph_store = GraphStore::new(&db);

    println!("Starting analysis...");

    if full {
        graph_store.clear()?;
        println!("  Cleared existing graph data");
    }

    let scanner = FileScanner::new(&root)
        .with_exclude(config.analysis.exclude.clone())
        .with_max_file_size(config.analysis.max_file_size as u64);
    let files = scanner.scan()?;
    println!("Found {} files to analyze", files.len());

    let mut total_nodes = 0;
    let mut total_edges = 0;
    let mut processed = 0;
    let mut language_counts: std::collections::HashMap<&str, u32> =
        std::collections::HashMap::new();

    let mut parse_errors = 0;
    let mut store_errors = 0;

    for file in &files {
        let lang = Language::from_path(&file.path);

        let result = match parse_file(&file.path, lang) {
            Ok(Some(r)) => r,
            Ok(None) => {
                processed += 1;
                continue;
            }
            Err(e) => {
                tracing::debug!(path = %file.path.display(), error = %e, "Parse failed");
                parse_errors += 1;
                processed += 1;
                continue;
            }
        };

        let mut file_stored = true;
        for node in &result.nodes {
            if let Err(e) = graph_store.insert_node(node) {
                tracing::debug!(node = ?node.id, error = %e, "Failed to store node");
                file_stored = false;
                store_errors += 1;
                break;
            }
        }

        if file_stored {
            for edge in &result.edges {
                if let Err(e) = graph_store.insert_edge(edge) {
                    tracing::debug!(error = %e, "Failed to store edge");
                    store_errors += 1;
                }
            }
            total_nodes += result.nodes.len();
            total_edges += result.edges.len();

            let lang_name = match lang {
                Language::TypeScript | Language::JavaScript => "TypeScript/JavaScript",
                Language::Python => "Python",
                Language::Rust => "Rust",
                Language::Go => "Go",
                Language::Bash => "Bash",
                _ => "Other",
            };
            *language_counts.entry(lang_name).or_insert(0) += 1;
        }

        processed += 1;
        if processed % cli_constants::PROGRESS_REPORT_INTERVAL == 0 {
            println!("  Processed {processed} files...");
        }
    }

    if parse_errors > 0 || store_errors > 0 {
        println!(
            "  Completed with {} parse errors, {} storage errors",
            parse_errors, store_errors
        );
    }

    println!("Parsed {total_nodes} nodes and {total_edges} edges");

    println!("Analyzing code structure...");
    let analyzer = StructureAnalyzer::new(&db);
    let structure = analyzer.analyze()?;

    println!(
        "  Found {} directories, {} entry points, {} hotspots",
        structure.directories.len(),
        structure.entry_points.len(),
        structure.hotspots.len()
    );

    if !language_counts.is_empty() {
        println!("\nLanguages detected:");
        let mut sorted: Vec<_> = language_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (lang, count) in sorted {
            println!("  {lang}: {count} files");
        }
    }

    if !skip_docs {
        println!("\nTo generate Claude Code plugin, run: claudegen generate");
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
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "Failed to read file");
            return Ok(None);
        }
    };

    let path_str = path.to_string_lossy();

    let result = match lang {
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            TypeScriptParser::new()?.parse(&path_str, &content)?
        }
        Language::Python => PythonParser::new()?.parse(&path_str, &content)?,
        Language::Rust => RustParser::new()?.parse(&path_str, &content)?,
        Language::Go => GoParser::new()?.parse(&path_str, &content)?,
        Language::Bash => BashParser::new()?.parse(&path_str, &content)?,
        _ => return Ok(None),
    };

    Ok(Some(result))
}
