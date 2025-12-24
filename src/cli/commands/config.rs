//! Config Command
//!
//! Manage WeaveWiki configuration.
//!
//! Usage:
//!   weavewiki config show [-g] [-f json]
//!   weavewiki config path
//!   weavewiki config edit [-g]
//!   weavewiki config init [-g] [--force]

use crate::config::ConfigLoader;
use crate::types::Result;

/// Show configuration
pub fn show(global: bool, format: &str) -> Result<()> {
    let as_json = format == "json";

    if global {
        if let Some(global_path) = ConfigLoader::global_config_path() {
            if global_path.exists() {
                let content = std::fs::read_to_string(&global_path)?;
                if format == "yaml" {
                    // Raw YAML output
                    println!("{}", content);
                } else {
                    println!("# Global Config: {}\n", global_path.display());
                    println!("{}", content);
                }
            } else {
                println!("No global config found.");
                println!("Run 'weavewiki config init --global' to create one.");
            }
        } else {
            println!("Cannot determine global config directory.");
        }
    } else {
        // Show merged effective config
        ConfigLoader::show_config(as_json)?;
    }
    Ok(())
}

/// Show configuration paths
pub fn path() -> Result<()> {
    ConfigLoader::show_path();
    Ok(())
}

/// Edit configuration file
pub fn edit(global: bool) -> Result<()> {
    ConfigLoader::edit_config(global)
}

/// Initialize global configuration
pub fn init_global(force: bool) -> Result<()> {
    let dir = ConfigLoader::init_global(force)?;
    println!("✓ Initialized global configuration");
    println!("  Directory: {}", dir.display());
    if let Some(config_path) = ConfigLoader::global_config_path() {
        println!("  Config:    {}", config_path.display());
    }
    Ok(())
}

/// Initialize project configuration
pub fn init_project() -> Result<()> {
    let root = std::env::current_dir()?;
    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let dir = ConfigLoader::init_project(Some(project_name))?;
    println!("✓ Initialized project configuration");
    println!("  Directory: {}", dir.display());
    println!(
        "  Config:    {}",
        ConfigLoader::project_config_path().display()
    );
    Ok(())
}
