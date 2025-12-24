//! Init Command
//!
//! Initialize WeaveWiki in the current directory.

use crate::config::ConfigLoader;
use crate::storage::Database;
use crate::types::{Result, WeaveError};

pub fn run(force: bool) -> Result<()> {
    let root = std::env::current_dir()?;
    let weavewiki_dir = root.join(".weavewiki");

    if weavewiki_dir.exists() && !force {
        return Err(WeaveError::Config(
            "Already initialized. Use --force to overwrite.".to_string(),
        ));
    }

    // Get project name from directory
    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();

    // Initialize project directory structure and config
    ConfigLoader::init_project(Some(&project_name))?;

    // Initialize global config if not exists (don't force overwrite)
    if let Err(e) = ConfigLoader::init_global(false) {
        tracing::debug!("Global config init skipped: {}", e);
    }

    // Initialize database
    let db = Database::open(weavewiki_dir.join("graph/graph.db"))?;
    db.initialize()?;

    println!("✓ Initialized WeaveWiki in .weavewiki/");
    println!("  Project: {}", project_name);
    println!();
    println!("Next steps:");
    println!("  1. Run 'weavewiki wiki' to generate AI-driven documentation");
    println!("     (Project type, architecture, and frameworks are auto-detected)");

    Ok(())
}
