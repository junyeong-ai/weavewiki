//! Status Command

use std::path::{Path, PathBuf};

use crate::cli::util::{CLAUDEGEN_DIR, CONFIG_PATH, ProjectState, is_initialized};
use crate::config::ConfigLoader;
use crate::types::Result;

pub fn run(format: &str, detailed: bool, config_path: Option<&Path>) -> Result<()> {
    let json_output = format == "json";

    if !is_initialized() {
        if json_output {
            println!("{{\"status\": \"not_initialized\"}}");
        } else {
            println!("claudegen Status");
            println!("══════════════════════════════════════");
            println!("Not initialized. Run 'claudegen init' first.");
        }
        return Ok(());
    }

    let project_root = std::env::current_dir()?;
    let config = ConfigLoader::load_for_project(&project_root, config_path)?;
    let plugin_manifest = find_plugin_with_state(&project_root);

    if json_output {
        let status = serde_json::json!({
            "status": "initialized",
            "project": config.project.name,
            "type": config.project.project_type,
            "plugin_generated": plugin_manifest.is_some(),
            "plugin_path": plugin_manifest.as_ref()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.display().to_string())
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
            if let Some(ref manifest) = plugin_manifest {
                println!("  Plugin: {}", manifest.display());
            }
            println!("  Config: {}/{}", CLAUDEGEN_DIR, CONFIG_PATH);
        }
    }

    Ok(())
}

fn find_plugin_with_state(project_root: &Path) -> Option<PathBuf> {
    match ProjectState::load() {
        Ok(state) => {
            if let Some(ref output_dir) = state.last_output_dir
                && let Some(manifest) = find_plugin_manifest(output_dir)
            {
                return Some(manifest);
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load project state");
        }
    }
    find_plugin_manifest(project_root)
}

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
