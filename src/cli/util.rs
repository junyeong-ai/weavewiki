//! CLI Common Utilities
//!
//! Shared initialization and context management for CLI commands.
//! Eliminates duplicate code across command handlers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{Config, ConfigLoader};
use crate::storage::{Database, SharedDatabase};
use crate::types::{Result, WeaveError};

/// WeaveWiki directory name
pub const WEAVEWIKI_DIR: &str = ".weavewiki";

/// Graph database relative path
pub const GRAPH_DB_PATH: &str = "graph/graph.db";

/// Wiki output relative path
pub const WIKI_PATH: &str = "wiki";

/// Config file relative path
pub const CONFIG_PATH: &str = "config.toml";

/// Command execution context
///
/// Provides unified access to common resources needed by CLI commands.
/// Created via `CommandContext::load()` for commands that need full context,
/// or via helper functions for simpler needs.
#[derive(Clone)]
pub struct CommandContext {
    /// WeaveWiki directory path (.weavewiki)
    pub weavewiki_dir: PathBuf,
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
        let weavewiki_dir = require_initialized()?;
        let db = open_graph_db(&weavewiki_dir)?;
        let config = ConfigLoader::load()?;
        let project_root = std::env::current_dir().map_err(WeaveError::Io)?;

        Ok(Self {
            weavewiki_dir,
            db: Arc::new(db),
            config,
            project_root,
        })
    }

    /// Load context without database
    ///
    /// For commands that only need config and paths.
    pub fn load_without_db() -> Result<Self> {
        let weavewiki_dir = require_initialized()?;
        let config = ConfigLoader::load()?;
        let project_root = std::env::current_dir().map_err(WeaveError::Io)?;

        // Create in-memory db as placeholder
        let db = Database::open_in_memory()?;

        Ok(Self {
            weavewiki_dir,
            db: Arc::new(db),
            config,
            project_root,
        })
    }

    /// Get wiki output directory path
    pub fn wiki_dir(&self) -> PathBuf {
        self.weavewiki_dir.join(WIKI_PATH)
    }

    /// Get graph database path
    pub fn db_path(&self) -> PathBuf {
        self.weavewiki_dir.join(GRAPH_DB_PATH)
    }

    /// Check if wiki has been generated
    pub fn wiki_exists(&self) -> bool {
        self.wiki_dir().join("README.md").exists()
    }
}

/// Require WeaveWiki to be initialized
///
/// Returns the .weavewiki directory path if initialized,
/// or `WeaveError::NotInitialized` if not.
pub fn require_initialized() -> Result<PathBuf> {
    let weavewiki_dir = Path::new(WEAVEWIKI_DIR);

    if !weavewiki_dir.exists() {
        return Err(WeaveError::NotInitialized);
    }

    Ok(weavewiki_dir.to_path_buf())
}

/// Require graph database to exist
///
/// Returns the database path if it exists,
/// or `WeaveError::NotInitialized` if not.
pub fn require_graph_db_path() -> Result<PathBuf> {
    let weavewiki_dir = require_initialized()?;
    let db_path = weavewiki_dir.join(GRAPH_DB_PATH);

    if !db_path.exists() {
        return Err(WeaveError::NotInitialized);
    }

    Ok(db_path)
}

/// Open the graph database
///
/// Opens an existing database or returns an error if it doesn't exist.
pub fn open_graph_db(weavewiki_dir: &Path) -> Result<Database> {
    let db_path = weavewiki_dir.join(GRAPH_DB_PATH);

    if !db_path.exists() {
        return Err(WeaveError::NotInitialized);
    }

    Database::open(&db_path)
}

/// Create and initialize graph database
///
/// Creates the database directory if needed and initializes the schema.
pub fn create_graph_db(weavewiki_dir: &Path) -> Result<Database> {
    let db_path = weavewiki_dir.join(GRAPH_DB_PATH);

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db = Database::open(&db_path)?;
    db.initialize()?;

    Ok(db)
}

/// Get WeaveWiki directory path (without validation)
///
/// Returns the path even if it doesn't exist.
/// Use `require_initialized()` if you need validation.
pub fn weavewiki_dir() -> PathBuf {
    PathBuf::from(WEAVEWIKI_DIR)
}

/// Check if WeaveWiki is initialized
pub fn is_initialized() -> bool {
    Path::new(WEAVEWIKI_DIR).exists()
}

/// Check if graph database exists
pub fn graph_db_exists() -> bool {
    Path::new(WEAVEWIKI_DIR).join(GRAPH_DB_PATH).exists()
}

// Tests disabled: Changing current directory in tests causes race conditions
// when running tests in parallel. The functionality is tested through
// integration tests instead.
