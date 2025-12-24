//! Configuration Loader (Figment-based)
//!
//! Loads and merges configuration from multiple sources using Figment:
//! 1. Built-in defaults (Serialized)
//! 2. Global config (~/.config/weavewiki/config.toml)
//! 3. Project config (.weavewiki/config.toml)
//! 4. Environment variables (WEAVEWIKI_* prefix)

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{debug, info};

use super::types::Config;
use crate::types::{Result, WeaveError};

/// Configuration loader
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration with full resolution chain using Figment:
    /// defaults → global → project → env vars
    pub fn load() -> Result<Config> {
        let mut figment = Figment::new().merge(Serialized::defaults(Config::default()));

        // Merge global config
        if let Some(global_path) = Self::global_config_path()
            && global_path.exists()
        {
            debug!("Loading global config from: {}", global_path.display());
            figment = figment.merge(Toml::file(&global_path));
        }

        // Merge project config
        let project_path = Self::project_config_path();
        if project_path.exists() {
            debug!("Loading project config from: {}", project_path.display());
            figment = figment.merge(Toml::file(&project_path));
        }

        // Merge environment variables (e.g., WEAVEWIKI_LLM_MODEL -> llm.model)
        figment = figment.merge(Env::prefixed("WEAVEWIKI_").split('_').lowercase(true));

        let config: Config = figment
            .extract()
            .map_err(|e| WeaveError::Config(format!("Configuration error: {}", e)))?;

        // Validate configuration after loading
        config.validate()?;

        Ok(config)
    }

    /// Load configuration from a specific file only
    pub fn load_from_file(path: &Path) -> Result<Config> {
        Figment::new()
            .merge(Serialized::defaults(Config::default()))
            .merge(Toml::file(path))
            .extract()
            .map_err(|e| WeaveError::Config(format!("Configuration error: {}", e)))
    }

    // =========================================================================
    // Path Management
    // =========================================================================

    /// Get path to global config directory (~/.config/weavewiki/)
    pub fn global_dir() -> Option<PathBuf> {
        env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                env::var("HOME")
                    .ok()
                    .map(|home| PathBuf::from(home).join(".config"))
            })
            .map(|p| p.join("weavewiki"))
    }

    /// Get path to global config file
    pub fn global_config_path() -> Option<PathBuf> {
        Self::global_dir().map(|dir| dir.join("config.toml"))
    }

    /// Get path to global cache directory
    pub fn global_cache_dir() -> Option<PathBuf> {
        env::var("XDG_CACHE_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                env::var("HOME")
                    .ok()
                    .map(|home| PathBuf::from(home).join(".cache"))
            })
            .map(|p| p.join("weavewiki"))
    }

    /// Get path to project config file
    pub fn project_config_path() -> PathBuf {
        PathBuf::from(".weavewiki/config.toml")
    }

    /// Get project data directory
    pub fn project_dir() -> PathBuf {
        PathBuf::from(".weavewiki")
    }

    // =========================================================================
    // Config Commands
    // =========================================================================

    /// Show config file path
    pub fn show_path() {
        println!("Configuration paths:");
        println!();

        // Global config
        if let Some(global) = Self::global_config_path() {
            let exists = if global.exists() { "✓" } else { "✗" };
            println!("  Global:  {} {}", exists, global.display());
        } else {
            println!("  Global:  (not available)");
        }

        // Project config
        let project = Self::project_config_path();
        let exists = if project.exists() { "✓" } else { "✗" };
        println!("  Project: {} {}", exists, project.display());

        // Cache directory
        if let Some(cache) = Self::global_cache_dir() {
            let exists = if cache.exists() { "✓" } else { "✗" };
            println!("  Cache:   {} {}", exists, cache.display());
        }
    }

    /// Show current effective configuration
    pub fn show_config(as_json: bool) -> Result<()> {
        let config = Self::load()?;

        if as_json {
            println!("{}", serde_json::to_string_pretty(&config)?);
        } else {
            // Pretty print in TOML format
            println!(
                "{}",
                toml::to_string_pretty(&config).map_err(|e| WeaveError::Config(e.to_string()))?
            );
        }

        Ok(())
    }

    /// Edit config file with default editor
    pub fn edit_config(global: bool) -> Result<()> {
        let path = if global {
            Self::global_config_path().ok_or_else(|| {
                WeaveError::Config("Cannot determine global config path".to_string())
            })?
        } else {
            Self::project_config_path()
        };

        if !path.exists() {
            println!("Config file does not exist: {}", path.display());
            println!(
                "Run: weavewiki config init {}",
                if global { "--global" } else { "" }
            );
            return Ok(());
        }

        let editor = env::var("EDITOR").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "open".to_string()
            } else if cfg!(target_os = "windows") {
                "notepad".to_string()
            } else {
                "vi".to_string()
            }
        });

        let status = Command::new(&editor).arg(&path).status().map_err(|e| {
            WeaveError::Config(format!("Failed to launch editor {}: {}", editor, e))
        })?;

        if !status.success() {
            return Err(WeaveError::Config("Editor exited with error".to_string()));
        }

        println!("Config saved: {}", path.display());
        Ok(())
    }

    // =========================================================================
    // Initialization
    // =========================================================================

    /// Initialize global configuration
    pub fn init_global(force: bool) -> Result<PathBuf> {
        let global_dir = Self::global_dir().ok_or_else(|| {
            WeaveError::Config("Cannot determine global config directory".to_string())
        })?;

        // Create directories
        fs::create_dir_all(&global_dir)?;

        if let Some(cache_dir) = Self::global_cache_dir() {
            fs::create_dir_all(&cache_dir)?;
        }

        // Create default config
        let config_path = global_dir.join("config.toml");
        if !config_path.exists() || force {
            let default_config = Self::default_global_config();
            fs::write(&config_path, default_config)?;
            info!("Created global config: {}", config_path.display());
        } else {
            info!("Global config exists: {}", config_path.display());
        }

        Ok(global_dir)
    }

    /// Initialize project configuration
    pub fn init_project(name: Option<&str>) -> Result<PathBuf> {
        let project_dir = Self::project_dir();

        // Create directories
        fs::create_dir_all(&project_dir)?;
        fs::create_dir_all(project_dir.join("graph"))?;
        fs::create_dir_all(project_dir.join("wiki"))?;
        fs::create_dir_all(project_dir.join("cache"))?;
        fs::create_dir_all(project_dir.join("checkpoints"))?;

        // Create default config if not exists
        let config_path = project_dir.join("config.toml");
        if !config_path.exists() {
            let default_config = Self::default_project_config(name);
            fs::write(&config_path, default_config)?;
            info!("Created project config: {}", config_path.display());
        }

        Ok(project_dir)
    }

    /// Check if project is initialized
    pub fn is_project_initialized() -> bool {
        Self::project_dir().exists()
    }

    // =========================================================================
    // Internal
    // =========================================================================

    /// Generate default global config content (TOML)
    fn default_global_config() -> String {
        r#"# WeaveWiki Global Configuration
# User-wide defaults. Project settings in .weavewiki/config.toml override these.

version = "1.0"

# LLM settings (for wiki generation)
[llm]
provider = "claude-code"
model = "claude-sonnet-4-20250514"
timeout_secs = 300

# Session settings
[session]
checkpoint_interval = 100
auto_resume = true
"#
        .to_string()
    }

    /// Generate default project config content (TOML)
    fn default_project_config(name: Option<&str>) -> String {
        let project_name = name.unwrap_or("project");
        format!(
            r#"# WeaveWiki Project Configuration
# Project-specific settings that override global defaults.

version = "1.0"

[project]
name = "{}"
type = "auto"

# Analysis settings
[analysis]
include = ["**/*"]
exclude = [
    "node_modules/**",
    "dist/**",
    ".git/**",
    "target/**",
    "build/**",
]

# Documentation output
[documentation]
output_dir = "wiki"
"#,
            project_name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_default_config() {
        let config = ConfigLoader::load().unwrap();
        assert_eq!(config.version, "1.0");
    }

    #[test]
    fn test_init_project() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        ConfigLoader::init_project(Some("test-project")).unwrap();

        assert!(PathBuf::from(".weavewiki").exists());
        assert!(PathBuf::from(".weavewiki/config.toml").exists());
        assert!(PathBuf::from(".weavewiki/graph").exists());
        assert!(PathBuf::from(".weavewiki/wiki").exists());
    }

    #[test]
    fn test_env_override() {
        // SAFETY: This test runs in isolation
        unsafe {
            std::env::set_var("WEAVEWIKI_LLM_MODEL", "test-model");
        }
        let config = ConfigLoader::load().unwrap();
        assert_eq!(config.llm.model, "test-model");
        unsafe {
            std::env::remove_var("WEAVEWIKI_LLM_MODEL");
        }
    }
}
