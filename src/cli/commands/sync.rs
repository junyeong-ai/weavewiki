//! Sync Command
//!
//! Incrementally updates artifacts based on file changes.
//! Detects changed files, resolves transitive dependencies,
//! marks affected artifacts as stale, and updates the manifest.

use std::path::PathBuf;

use crate::cli::util::{CLAUDEGEN_DIR, is_initialized};
use crate::pipeline::sync::{
    ChangeSet, DependencyGraph, FileTracker, SkippedArtifact, SyncResult,
};
use crate::types::Result;
use modmap::ProjectManifest;

#[derive(Default)]
pub struct SyncOptions {
    pub dry_run: bool,
    pub verbose: bool,
}

pub async fn run(options: SyncOptions) -> Result<SyncResult> {
    if !is_initialized() {
        println!("Not initialized. Run 'claudegen init' first.");
        return Ok(SyncResult::default());
    }

    let project_root = std::env::current_dir()?;
    let manifest_path = project_root.join(CLAUDEGEN_DIR).join("manifest.json");

    if !manifest_path.exists() {
        println!("No manifest found. Run 'claudegen generate' first.");
        return Ok(SyncResult::default());
    }

    let mut manifest = load_manifest(&manifest_path)?;
    let mut tracker = FileTracker::from_manifest(&manifest);

    println!("Scanning for changes...");
    let changes = tracker.detect_changes().await?;

    if changes.is_empty() {
        println!("No changes detected. All artifacts are up to date.");
        return Ok(SyncResult::default());
    }

    print_changes(&changes, options.verbose);

    let graph = DependencyGraph::build(&manifest);
    let affected = graph.affected_by(&changes);

    if affected.is_empty() {
        if !options.dry_run {
            update_manifest_hashes(&mut tracker, &mut manifest, &manifest_path).await?;
        }
        println!("No artifacts affected by changes.");
        return Ok(SyncResult {
            files_scanned: changes.total_changes(),
            files_changed: changes.total_changes(),
            ..Default::default()
        });
    }

    println!("\nAffected artifacts ({}):", affected.len());
    for artifact in &affected {
        println!("  - {}", artifact.output_path());
    }

    if options.dry_run {
        println!("\n[dry-run] Would mark {} artifacts as stale.", affected.len());
        return Ok(SyncResult {
            files_scanned: changes.total_changes(),
            files_changed: changes.total_changes(),
            skipped: affected
                .iter()
                .map(|a| SkippedArtifact {
                    artifact: a.clone(),
                    reason: "dry-run".into(),
                })
                .collect(),
            ..Default::default()
        });
    }

    update_manifest_hashes(&mut tracker, &mut manifest, &manifest_path).await?;

    let stale: Vec<_> = affected
        .iter()
        .map(|a| SkippedArtifact {
            artifact: a.clone(),
            reason: "stale - regeneration needed".into(),
        })
        .collect();

    println!(
        "\nUpdated file tracking. {} artifact(s) are stale.",
        stale.len()
    );
    println!("Run 'claudegen generate' to regenerate affected artifacts.");

    Ok(SyncResult {
        files_scanned: changes.total_changes(),
        files_changed: changes.total_changes(),
        skipped: stale,
        ..Default::default()
    })
}

async fn update_manifest_hashes(
    tracker: &mut FileTracker,
    manifest: &mut ProjectManifest,
    manifest_path: &PathBuf,
) -> Result<()> {
    let new_tracked = tracker.scan_and_track().await?;
    manifest.tracked = new_tracked;

    let json = manifest.to_json().map_err(|e| {
        crate::types::ClaudegenError::Config(format!(
            "Failed to serialize manifest: {e}"
        ))
    })?;

    tokio::fs::write(manifest_path, json.as_bytes()).await?;
    println!("Updated file tracking in manifest.");

    Ok(())
}

fn load_manifest(path: &PathBuf) -> Result<ProjectManifest> {
    let content = std::fs::read_to_string(path)?;
    let manifest: ProjectManifest =
        serde_json::from_str(&content).map_err(crate::types::ClaudegenError::Json)?;
    Ok(manifest)
}

fn print_changes(changes: &ChangeSet, verbose: bool) {
    let total = changes.total_changes();
    println!("Found {} changed file(s):", total);

    if verbose {
        for file in &changes.added {
            println!("  + {}", file);
        }
        for file in &changes.modified {
            println!("  ~ {}", file);
        }
        for file in &changes.deleted {
            println!("  - {}", file);
        }
    } else {
        println!(
            "  {} added, {} modified, {} deleted",
            changes.added.len(),
            changes.modified.len(),
            changes.deleted.len()
        );
    }
}
