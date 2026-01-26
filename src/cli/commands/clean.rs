//! Clean Command

use std::path::Path;

use crate::cli::util::CLAUDEGEN_DIR;
use crate::types::Result;

pub async fn run(all: bool, cache: bool, checkpoints: bool) -> Result<()> {
    let claudegen_dir = Path::new(CLAUDEGEN_DIR);

    if all {
        if claudegen_dir.exists() {
            tokio::fs::remove_dir_all(claudegen_dir).await?;
            println!("Removed .claudegen/");
        }
        return Ok(());
    }

    if cache {
        let cache_dir = claudegen_dir.join("cache");
        if cache_dir.exists() {
            tokio::fs::remove_dir_all(&cache_dir).await?;
            tokio::fs::create_dir_all(&cache_dir).await?;
            println!("Cleared cache");
        }
    }

    if checkpoints {
        let checkpoints_dir = claudegen_dir.join("checkpoints");
        if checkpoints_dir.exists() {
            tokio::fs::remove_dir_all(&checkpoints_dir).await?;
            tokio::fs::create_dir_all(&checkpoints_dir).await?;
            println!("Cleared checkpoints");
        }
    }

    Ok(())
}
