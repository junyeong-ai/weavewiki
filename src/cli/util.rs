//! CLI Common Utilities
//!
//! Shared initialization and context management for CLI commands.
//! Eliminates duplicate code across command handlers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{Config, ConfigLoader};
use crate::storage::{Database, SharedDatabase};
use crate::types::{ClaudegenError, Result};

/// claudegen directory name
pub const CLAUDEGEN_DIR: &str = ".claudegen";

/// Graph database relative path
pub const GRAPH_DB_PATH: &str = "graph/graph.db";

/// Plugin output relative path
pub const PLUGIN_PATH: &str = ".claude-plugin";

/// Config file relative path
pub const CONFIG_PATH: &str = "config.toml";

/// Command execution context
///
/// Provides unified access to common resources needed by CLI commands.
/// Created via `CommandContext::load()` for commands that need full context,
/// or via helper functions for simpler needs.
#[derive(Clone)]
pub struct CommandContext {
    /// claudegen directory path (.claudegen)
    pub claudegen_dir: PathBuf,
    /// Shared database handle
    pub db: SharedDatabase,
    /// Loaded configuration
    pub config: Config,
    /// Project root directory
    pub project_root: PathBuf,
}

impl CommandContext {
    /// Load full command context
    ///
    /// Validates initialization, loads config, and opens database.
    /// Use this for commands that need all resources.
    pub fn load() -> Result<Self> {
        let claudegen_dir = require_initialized()?;
        let db = open_graph_db(&claudegen_dir)?;
        let config = ConfigLoader::load()?;
        let project_root = std::env::current_dir().map_err(ClaudegenError::Io)?;

        Ok(Self {
            claudegen_dir,
            db: Arc::new(db),
            config,
            project_root,
        })
    }

    /// Load context without database
    ///
    /// For commands that only need config and paths.
    pub fn load_without_db() -> Result<Self> {
        let claudegen_dir = require_initialized()?;
        let config = ConfigLoader::load()?;
        let project_root = std::env::current_dir().map_err(ClaudegenError::Io)?;

        // Create in-memory db as placeholder
        let db = Database::open_in_memory()?;

        Ok(Self {
            claudegen_dir,
            db: Arc::new(db),
            config,
            project_root,
        })
    }

    /// Get plugin output directory path
    pub fn plugin_dir(&self) -> PathBuf {
        self.claudegen_dir.join(PLUGIN_PATH)
    }

    /// Get graph database path
    pub fn db_path(&self) -> PathBuf {
        self.claudegen_dir.join(GRAPH_DB_PATH)
    }

    /// Check if plugin has been generated
    pub fn plugin_exists(&self) -> bool {
        self.plugin_dir().join("plugin.json").exists()
    }
}

/// Require claudegen to be initialized
///
/// Returns the .claudegen directory path if initialized,
/// or `ClaudegenError::NotInitialized` if not.
pub fn require_initialized() -> Result<PathBuf> {
    let claudegen_dir = Path::new(CLAUDEGEN_DIR);

    if !claudegen_dir.exists() {
        return Err(ClaudegenError::NotInitialized);
    }

    Ok(claudegen_dir.to_path_buf())
}

/// Require graph database to exist
///
/// Returns the database path if it exists,
/// or `ClaudegenError::NotInitialized` if not.
pub fn require_graph_db_path() -> Result<PathBuf> {
    let claudegen_dir = require_initialized()?;
    let db_path = claudegen_dir.join(GRAPH_DB_PATH);

    if !db_path.exists() {
        return Err(ClaudegenError::NotInitialized);
    }

    Ok(db_path)
}

/// Open the graph database
///
/// Opens an existing database or returns an error if it doesn't exist.
pub fn open_graph_db(claudegen_dir: &Path) -> Result<Database> {
    let db_path = claudegen_dir.join(GRAPH_DB_PATH);

    if !db_path.exists() {
        return Err(ClaudegenError::NotInitialized);
    }

    Database::open(&db_path)
}

/// Create and initialize graph database
///
/// Creates the database directory if needed and initializes the schema.
pub fn create_graph_db(claudegen_dir: &Path) -> Result<Database> {
    let db_path = claudegen_dir.join(GRAPH_DB_PATH);

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db = Database::open(&db_path)?;
    db.initialize()?;

    Ok(db)
}

/// Get claudegen directory path (without validation)
///
/// Returns the path even if it doesn't exist.
/// Use `require_initialized()` if you need validation.
pub fn claudegen_dir() -> PathBuf {
    PathBuf::from(CLAUDEGEN_DIR)
}

/// Check if claudegen is initialized
pub fn is_initialized() -> bool {
    Path::new(CLAUDEGEN_DIR).exists()
}

/// Check if graph database exists
pub fn graph_db_exists() -> bool {
    Path::new(CLAUDEGEN_DIR).join(GRAPH_DB_PATH).exists()
}

// Tests disabled: Changing current directory in tests causes race conditions
// when running tests in parallel. The functionality is tested through
// integration tests instead.
