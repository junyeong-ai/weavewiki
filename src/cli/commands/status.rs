//! Status Command
//!
//! Display WeaveWiki project status.

use std::path::Path;

use crate::cli::util::{GRAPH_DB_PATH, is_initialized, weavewiki_dir};
use crate::config::ConfigLoader;
use crate::storage::Database;
use crate::types::Result;

pub fn run(format: &str, detailed: bool) -> Result<()> {
    let weavewiki_dir = weavewiki_dir();
    let json_output = format == "json";

    if !is_initialized() {
        if json_output {
            println!("{{\"status\": \"not_initialized\"}}");
        } else {
            println!("WeaveWiki Status");
            println!("══════════════════════════════════════");
            println!("Not initialized. Run 'weavewiki init' first.");
        }
        // Return Ok for status command - it's informational
        return Ok(());
    }

    let config = ConfigLoader::load()?;
    let (node_count, edge_count) = get_graph_stats(&weavewiki_dir)?;
    let wiki_exists = weavewiki_dir.join("wiki/README.md").exists();

    if json_output {
        let status = serde_json::json!({
            "status": "initialized",
            "project": config.project.name,
            "type": config.project.project_type,
            "graph": {
                "nodes": node_count,
                "edges": edge_count
            },
            "wiki_generated": wiki_exists
        });

        let json = serde_json::to_string_pretty(&status).map_err(crate::types::WeaveError::Json)?;
        println!("{}", json);
    } else {
        println!("WeaveWiki Status");
        println!("══════════════════════════════════════");

        if let Some(name) = &config.project.name {
            println!("Project: {}", name);
        }
        println!("Type: {:?}", config.project.project_type);
        println!();

        println!("Knowledge Graph:");
        println!("  Nodes: {}", node_count);
        println!("  Edges: {}", edge_count);
        println!();

        println!(
            "Wiki: {}",
            if wiki_exists {
                "Generated"
            } else {
                "Not generated"
            }
        );

        if detailed {
            println!();
            println!("Paths:");
            println!("  Graph DB: .weavewiki/graph/graph.db");
            println!("  Wiki: .weavewiki/wiki/");
            println!("  Config: .weavewiki/config.toml");
        }
    }

    Ok(())
}

fn get_graph_stats(weavewiki_dir: &Path) -> Result<(i64, i64)> {
    let db_path = weavewiki_dir.join(GRAPH_DB_PATH);
    if !db_path.exists() {
        return Ok((0, 0));
    }

    let db = Database::open(&db_path)?;
    let conn = db.connection()?;

    let node_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))
        .unwrap_or(0);

    let edge_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))
        .unwrap_or(0);

    Ok((node_count, edge_count))
}
