//! CLI Common Utilities

use std::path::{Path, PathBuf};

use crate::types::{ClaudegenError, Result};

pub const CLAUDEGEN_DIR: &str = ".claudegen";
pub const CONFIG_PATH: &str = "config.toml";
pub const STATE_PATH: &str = "state.toml";
pub const DEFAULT_REPORT_FILENAME: &str = "validation-report.json";

pub fn require_initialized() -> Result<PathBuf> {
    let claudegen_dir = Path::new(CLAUDEGEN_DIR);
    if !claudegen_dir.exists() {
        return Err(ClaudegenError::NotInitialized);
    }
    Ok(claudegen_dir.to_path_buf())
}

pub fn claudegen_dir() -> PathBuf {
    PathBuf::from(CLAUDEGEN_DIR)
}

pub fn is_initialized() -> bool {
    Path::new(CLAUDEGEN_DIR).exists()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ProjectState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_output_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_generated_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ProjectState {
    pub fn load() -> Result<Self> {
        Self::load_from(Path::new("."))
    }

    pub fn load_from(root: &Path) -> Result<Self> {
        let state_path = root.join(CLAUDEGEN_DIR).join(STATE_PATH);
        if !state_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&state_path)?;
        toml::from_str(&content)
            .map_err(|e| ClaudegenError::Config(format!("Invalid state file: {e}")))
    }

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

    pub fn set_output_dir(&mut self, path: PathBuf) {
        let absolute_path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&path))
                .unwrap_or(path)
        };
        self.last_output_dir = Some(absolute_path);
        self.last_generated_at = Some(chrono::Utc::now());
    }
}
