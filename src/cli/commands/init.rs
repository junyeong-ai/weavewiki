//! Init Command

use crate::cli::util::CLAUDEGEN_DIR;
use crate::config::ConfigLoader;
use crate::types::{ClaudegenError, Result};

pub fn run(force: bool) -> Result<()> {
    let root = std::env::current_dir()?;
    let claudegen_dir = root.join(CLAUDEGEN_DIR);

    if claudegen_dir.exists() && !force {
        return Err(ClaudegenError::Config(
            "Already initialized. Use --force to overwrite.".to_string(),
        ));
    }

    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();

    ConfigLoader::init_project(Some(&project_name))?;

    if let Err(e) = ConfigLoader::init_global(false) {
        tracing::debug!("Global config init skipped: {}", e);
    }

    println!("Initialized claudegen in .claudegen/");
    println!("  Project: {project_name}");
    println!();
    println!("Next: Run 'claudegen generate' to create Claude Code plugin");

    Ok(())
}
