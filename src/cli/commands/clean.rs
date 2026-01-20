//! Clean Command - Clears generated data, caches, and checkpoints

use crate::types::Result;
use std::path::Path;

pub async fn run(all: bool, cache: bool, checkpoints: bool, sessions: bool) -> Result<()> {
    let claudegen_dir = Path::new(".claudegen");

    if all {
        if claudegen_dir.exists() {
            tokio::fs::remove_dir_all(claudegen_dir).await?;
            println!("✓ Removed .claudegen/");
        }
        return Ok(());
    }

    if cache {
        let cache_dir = claudegen_dir.join("cache");
        if cache_dir.exists() {
            tokio::fs::remove_dir_all(&cache_dir).await?;
            tokio::fs::create_dir_all(&cache_dir).await?;
            println!("✓ Cleared cache");
        }
    }

    if checkpoints {
        let checkpoints_dir = claudegen_dir.join("checkpoints");
        if checkpoints_dir.exists() {
            tokio::fs::remove_dir_all(&checkpoints_dir).await?;
            tokio::fs::create_dir_all(&checkpoints_dir).await?;
            println!("✓ Cleared checkpoints");
        }
    }

    if sessions {
        let db_path = Path::new(".claudegen").join("claudegen.db");
        if db_path.exists() {
            let db = crate::storage::Database::open(&db_path)?;
            db.execute(
                "DELETE FROM sessions WHERE status IN ('active', 'paused', 'failed')",
                &[],
            )?;
            println!("✓ Cleared incomplete sessions");
        }
    }

    Ok(())
}
