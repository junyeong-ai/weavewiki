//! CLI Common Utilities
//!
//! Shared initialization and context management for CLI commands.

use std::path::{Path, PathBuf};

use crate::storage::Database;
use crate::types::{ClaudegenError, Result};

/// claudegen directory name
pub const CLAUDEGEN_DIR: &str = ".claudegen";

/// Graph database relative path
pub const GRAPH_DB_PATH: &str = "graph/graph.db";

/// Config file relative path
pub const CONFIG_PATH: &str = "config.toml";

/// State file relative path (tracks last output directory)
pub const STATE_PATH: &str = "state.toml";

/// Default validation report filename
pub const DEFAULT_REPORT_FILENAME: &str = "validation-report.json";

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

/// Project state for tracking runtime info
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ProjectState {
    /// Last plugin output directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_output_dir: Option<PathBuf>,
    /// Last generation timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_generated_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ProjectState {
    /// Load project state from CWD's .claudegen/state.toml
    pub fn load() -> Result<Self> {
        Self::load_from(Path::new("."))
    }

    /// Load project state from specified root's .claudegen/state.toml
    pub fn load_from(root: &Path) -> Result<Self> {
        let state_path = root.join(CLAUDEGEN_DIR).join(STATE_PATH);
        if !state_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&state_path)?;
        toml::from_str(&content)
            .map_err(|e| ClaudegenError::Config(format!("Invalid state file: {e}")))
    }

    /// Save project state to .claudegen/state.toml
    pub fn save(&self) -> Result<()> {
        let claudegen_dir = Path::new(CLAUDEGEN_DIR);
        if !claudegen_dir.exists() {
            std::fs::create_dir_all(claudegen_dir)?;
        }

        let state_path = claudegen_dir.join(STATE_PATH);
        let content = toml::to_string_pretty(self)
            .map_err(|e| ClaudegenError::Config(format!("Failed to serialize state: {e}")))?;
        std::fs::write(&state_path, content)?;
        Ok(())
    }

    /// Update the last output directory (converts to absolute path)
    pub fn set_output_dir(&mut self, path: PathBuf) {
        // Canonicalize to absolute path for cross-CWD reliability
        let absolute_path = if path.is_absolute() {
            path
        } else {
            match std::env::current_dir() {
                Ok(cwd) => cwd.join(&path),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %path.display(),
                        "Failed to get current directory, saving relative path (may cause issues)"
                    );
                    path
                }
            }
        };
        self.last_output_dir = Some(absolute_path);
        self.last_generated_at = Some(chrono::Utc::now());
    }
}
