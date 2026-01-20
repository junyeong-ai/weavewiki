//! Status Command
//!
//! Display claudegen project status.

use std::path::Path;

use crate::cli::util::{GRAPH_DB_PATH, claudegen_dir, is_initialized};
use crate::config::ConfigLoader;
use crate::storage::Database;
use crate::types::Result;

pub fn run(format: &str, detailed: bool) -> Result<()> {
    let claudegen_dir = claudegen_dir();
    let json_output = format == "json";

    if !is_initialized() {
        if json_output {
            println!("{{\"status\": \"not_initialized\"}}");
        } else {
            println!("claudegen Status");
            println!("══════════════════════════════════════");
            println!("Not initialized. Run 'claudegen init' first.");
        }
        // Return Ok for status command - it's informational
        return Ok(());
    }

    let config = ConfigLoader::load()?;
    let (node_count, edge_count) = get_graph_stats(&claudegen_dir)?;
    let plugin_exists = claudegen_dir.join(".claude-plugin/plugin.json").exists();

    if json_output {
        let status = serde_json::json!({
            "status": "initialized",
            "project": config.project.name,
            "type": config.project.project_type,
            "graph": {
                "nodes": node_count,
                "edges": edge_count
            },
            "plugin_generated": plugin_exists
        });

        let json =
            serde_json::to_string_pretty(&status).map_err(crate::types::ClaudegenError::Json)?;
        println!("{json}");
    } else {
        println!("claudegen Status");
        println!("══════════════════════════════════════");

        if let Some(name) = &config.project.name {
            println!("Project: {name}");
        }
        println!("Type: {:?}", config.project.project_type);
        println!();

        println!("Knowledge Graph:");
        println!("  Nodes: {node_count}");
        println!("  Edges: {edge_count}");
        println!();

        println!(
            "Plugin: {}",
            if plugin_exists {
                "Generated"
            } else {
                "Not generated"
            }
        );

        if detailed {
            println!();
            println!("Paths:");
            println!("  Graph DB: .claudegen/graph/graph.db");
            println!("  Plugin: .claudegen/.claude-plugin/");
            println!("  Config: .claudegen/config.toml");
        }
    }

    Ok(())
}

fn get_graph_stats(claudegen_dir: &Path) -> Result<(i64, i64)> {
    let db_path = claudegen_dir.join(GRAPH_DB_PATH);
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
