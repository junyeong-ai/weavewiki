//! Query Command
//!
//! Query the knowledge graph for nodes and their relationships.

use crate::cli::util::require_graph_db_path;
use crate::storage::GraphStore;
use crate::types::Result;

pub fn run(query: &str, depth: u32, format: &str) -> Result<()> {
    let db_path = require_graph_db_path()?;
    let db = crate::storage::Database::open(&db_path)?;
    let store = GraphStore::new(&db);

    let node_id = if query.contains(':') {
        query.to_string()
    } else {
        format!("file:{}", query)
    };

    let node = store.get_node(&node_id)?;

    match format {
        "json" => {
            if let Some(n) = node {
                let deps = store.get_dependencies(&node_id)?;
                let dependents = store.get_dependents(&node_id)?;

                let output = serde_json::json!({
                    "node": {
                        "id": n.id,
                        "name": n.name,
                        "type": format!("{:?}", n.node_type),
                        "path": n.path
                    },
                    "dependencies": deps,
                    "dependents": dependents
                });

                let json = serde_json::to_string_pretty(&output)
                    .map_err(crate::types::WeaveError::Json)?;
                println!("{}", json);
            } else {
                println!("{{\"error\": \"Node not found\"}}");
            }
        }
        _ => {
            if let Some(n) = node {
                println!("Node: {}", n.id);
                println!("  Name: {}", n.name);
                println!("  Type: {:?}", n.node_type);
                println!("  Path: {}", n.path);
                println!();

                let deps = store.get_dependencies(&node_id)?;
                if !deps.is_empty() {
                    println!("Dependencies ({}):", deps.len());
                    for dep in deps.iter().take(depth as usize) {
                        println!("  → {}", dep);
                    }
                    if deps.len() > depth as usize {
                        println!("  ... and {} more", deps.len() - depth as usize);
                    }
                }

                let dependents = store.get_dependents(&node_id)?;
                if !dependents.is_empty() {
                    println!();
                    println!("Dependents ({}):", dependents.len());
                    for dep in dependents.iter().take(depth as usize) {
                        println!("  ← {}", dep);
                    }
                    if dependents.len() > depth as usize {
                        println!("  ... and {} more", dependents.len() - depth as usize);
                    }
                }
            } else {
                println!("Node not found: {}", query);
            }
        }
    }

    Ok(())
}
