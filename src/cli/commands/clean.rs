//! Clean Command
//!
//! Clears generated data, caches, and checkpoints.
//!
//! TIER 16: Integrated with WikiCache for cache statistics

use std::path::Path;

use crate::types::Result;
use crate::wiki::cache::{CacheConfig, WikiCache};

pub async fn run(all: bool, cache: bool, checkpoints: bool, sessions: bool) -> Result<()> {
    let weavewiki_dir = Path::new(".weavewiki");

    if all {
        if weavewiki_dir.exists() {
            // Show what will be deleted
            let cache_config = CacheConfig {
                cache_dir: weavewiki_dir.join("cache"),
                ..Default::default()
            };
            let wiki_cache = WikiCache::new(cache_config);
            if let Ok(stats) = wiki_cache.stats().await
                && stats.entry_count > 0
            {
                println!(
                    "  Cache: {} entries, {} pages, {} bytes",
                    stats.entry_count, stats.total_pages, stats.total_size_bytes
                );
            }

            tokio::fs::remove_dir_all(weavewiki_dir).await?;
            println!("✓ Removed .weavewiki/");
        }
        return Ok(());
    }

    if checkpoints {
        let checkpoints_dir = weavewiki_dir.join("checkpoints");
        if checkpoints_dir.exists() {
            tokio::fs::remove_dir_all(&checkpoints_dir).await?;
            tokio::fs::create_dir_all(&checkpoints_dir).await?;
            println!("✓ Cleared checkpoints");
        }
    }

    if sessions {
        let db_path = weavewiki_dir.join("graph/graph.db");
        if db_path.exists() {
            // Clear sessions from database
            let db = crate::storage::Database::open(&db_path)?;
            db.execute(
                "DELETE FROM doc_sessions WHERE status IN ('active', 'paused', 'failed')",
                &[],
            )?;
            println!("✓ Cleared incomplete sessions");
        }
    }

    if cache {
        let cache_config = CacheConfig {
            cache_dir: weavewiki_dir.join("cache"),
            ..Default::default()
        };
        let wiki_cache = WikiCache::new(cache_config);

        // Show stats before clearing
        if let Ok(stats) = wiki_cache.stats().await
            && stats.entry_count > 0
        {
            println!(
                "  Clearing {} cache entries ({} pages, {} bytes)...",
                stats.entry_count, stats.total_pages, stats.total_size_bytes
            );
        }

        let cleared = wiki_cache.clear_all().await?;
        if cleared > 0 {
            println!("✓ Cleared {} wiki cache entries", cleared);
        } else {
            println!("  No cache entries to clear");
        }
    }

    Ok(())
}

/// List cache entries (for status display)
pub async fn list_cache() -> Result<()> {
    let weavewiki_dir = Path::new(".weavewiki");
    let cache_config = CacheConfig {
        cache_dir: weavewiki_dir.join("cache"),
        ..Default::default()
    };
    let wiki_cache = WikiCache::new(cache_config);

    let entries = wiki_cache.list_entries().await?;

    if entries.is_empty() {
        println!("  No cache entries");
        return Ok(());
    }

    println!("\n📦 Wiki Cache Entries\n");
    for entry in entries {
        println!(
            "  {} ({} pages, {} bytes)",
            entry.key, entry.page_count, entry.size_bytes
        );
        println!(
            "    Created: {}",
            entry.created_at.format("%Y-%m-%d %H:%M UTC")
        );
        if let Some(ref commit) = entry.commit_id {
            println!("    Commit: {}", commit);
        }
        println!("    Model: {}", entry.model);
        println!();
    }

    let stats = wiki_cache.stats().await?;
    println!(
        "  Total: {} entries, {} pages, {} bytes",
        stats.entry_count, stats.total_pages, stats.total_size_bytes
    );

    Ok(())
}
