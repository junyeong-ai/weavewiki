//! Status Command
//!
//! Display claudegen project status.

use std::path::{Path, PathBuf};

use crate::cli::util::{
    CLAUDEGEN_DIR, CONFIG_PATH, GRAPH_DB_PATH, ProjectState, claudegen_dir, is_initialized,
};
use crate::config::ConfigLoader;
use crate::storage::Database;
use crate::types::Result;

pub fn run(format: &str, detailed: bool, config_path: Option<&Path>) -> Result<()> {
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

    // status operates on CWD, but use load_for_project for consistency
    let project_root = std::env::current_dir()?;
    let config = ConfigLoader::load_for_project(&project_root, config_path)?;
    let (node_count, edge_count) = get_graph_stats(&claudegen_dir)?;
    let plugin_manifest = find_plugin_with_state(&project_root);

    if json_output {
        let status = serde_json::json!({
            "status": "initialized",
            "project": config.project.name,
            "type": config.project.project_type,
            "graph": {
                "nodes": node_count,
                "edges": edge_count
            },
            "plugin_generated": plugin_manifest.is_some(),
            "plugin_path": plugin_manifest.as_ref().and_then(|p| p.parent()).and_then(|p| p.parent()).map(|p| p.display().to_string())
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

        if let Some(ref manifest) = plugin_manifest {
            if let Some(plugin_dir) = manifest.parent().and_then(|p| p.parent()) {
                println!("Plugin: Generated ({})", plugin_dir.display());
            } else {
                println!("Plugin: Generated");
            }
        } else {
            println!("Plugin: Not generated");
        }

        if detailed {
            println!();
            println!("Paths:");
            println!("  Graph DB: {}/{}", CLAUDEGEN_DIR, GRAPH_DB_PATH);
            if let Some(ref manifest) = plugin_manifest {
                println!("  Plugin: {}", manifest.display());
            }
            println!("  Config: {}/{}", CLAUDEGEN_DIR, CONFIG_PATH);
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
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to count nodes");
            0
        });

    let edge_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to count edges");
            0
        });

    Ok((node_count, edge_count))
}

/// Find plugin manifest, checking saved output directory first, then project root
fn find_plugin_with_state(project_root: &Path) -> Option<PathBuf> {
    // First, check saved output directory from state
    match ProjectState::load() {
        Ok(state) => {
            if let Some(ref output_dir) = state.last_output_dir
                && let Some(manifest) = find_plugin_manifest(output_dir)
            {
                return Some(manifest);
            }
        }
        Err(e) => {
            // Warn about corrupted state file - user may have hidden output paths
            tracing::warn!(error = %e, "Failed to load project state, plugin location may be inaccurate");
        }
    }

    // Fall back to searching in project root
    find_plugin_manifest(project_root)
}

/// Find plugin manifest in directory (looks for *-plugin/.claude-plugin/plugin.json)
fn find_plugin_manifest(search_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(search_dir).ok()?;

    entries
        .filter_map(|e| e.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if name.ends_with("-plugin") {
                let manifest = path.join(".claude-plugin/plugin.json");
                manifest.exists().then_some(manifest)
            } else {
                None
            }
        })
        .next()
}
