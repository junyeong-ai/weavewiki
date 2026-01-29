//! Configuration Loader (Figment-based)
//!
//! Loads and merges configuration from multiple sources using Figment:
//! 1. Built-in defaults (Serialized)
//! 2. Global config (~/.config/claudegen/config.toml)
//! 3. Project config (.claudegen/config.toml)
//! 4. Environment variables (CLAUDEGEN_* prefix)

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
use crate::types::{ClaudegenError, Result};

/// Configuration loader
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration with full resolution chain using Figment:
    /// defaults → global → project → env vars
    pub fn load() -> Result<Config> {
        Self::load_with_path(None)
    }

    /// Load configuration with optional custom config path
    /// If path is provided, it replaces project config in the resolution chain
    ///
    /// Resolution order: defaults → global → project → env vars
    pub fn load_with_path(custom_path: Option<&Path>) -> Result<Config> {
        // Step 1: Start with defaults (single high-quality configuration)
        let base_config = Config::default();

        // Step 2: Build Figment with defaults as base
        let mut figment = Figment::new().merge(Serialized::defaults(base_config));

        // Step 3: Merge global config (overrides defaults)
        if let Some(global_path) = Self::global_config_path()
            && global_path.exists()
        {
            debug!("Loading global config from: {}", global_path.display());
            figment = figment.merge(Toml::file(&global_path));
        }

        // Step 4: Merge project/custom config (overrides global)
        match custom_path {
            Some(explicit_path) => {
                if !explicit_path.exists() {
                    return Err(ClaudegenError::Config(format!(
                        "Config file not found: {}",
                        explicit_path.display()
                    )));
                }
                debug!("Loading config from: {}", explicit_path.display());
                figment = figment.merge(Toml::file(explicit_path));
            }
            None => {
                let project_config = Self::project_config_path();
                if project_config.exists() {
                    debug!("Loading config from: {}", project_config.display());
                    figment = figment.merge(Toml::file(&project_config));
                }
            }
        }

        // Step 5: Merge environment variables (overrides everything)
        figment = figment.merge(Env::prefixed("CLAUDEGEN_").split('_').lowercase(true));

        let config: Config = figment
            .extract()
            .map_err(|e| ClaudegenError::Config(format!("Configuration error: {e}")))?;

        // Validate configuration
        config.validate()?;

        info!(
            quality_loop_timeout = config.timeout.quality_loop_timeout_secs,
            checkpoint_interval = config.timeout.effective_checkpoint_interval_secs(),
            session_timeout = config.timeout.session_timeout_secs,
            "Configuration loaded"
        );

        Ok(config)
    }

    /// Load configuration for a specific project
    ///
    /// Resolution: defaults → global → target_project → env vars
    ///
    /// This method ensures config is loaded from the correct project when
    /// operating on a different project than CWD (e.g., `claudegen validate --path /other/project`)
    ///
    /// # Arguments
    /// * `project_root` - The target project's root directory
    /// * `config_path` - Optional explicit config file path (overrides project config)
    pub fn load_for_project(project_root: &Path, config_path: Option<&Path>) -> Result<Config> {
        match config_path {
            Some(explicit_path) => {
                // User explicitly provided --config: must exist
                Self::load_with_path(Some(explicit_path))
            }
            None => {
                // Use target project's config if exists, otherwise global + defaults only
                let target_config = project_root.join(".claudegen/config.toml");
                if target_config.exists() {
                    Self::load_with_path(Some(&target_config))
                } else {
                    // Never fall back to CWD's project config
                    Self::load_global_only()
                }
            }
        }
    }

    /// Load configuration without project config (defaults + global + env only)
    pub fn load_global_only() -> Result<Config> {
        let base_config = Config::default();
        let mut figment = Figment::new().merge(Serialized::defaults(base_config));

        if let Some(global_path) = Self::global_config_path()
            && global_path.exists()
        {
            figment = figment.merge(Toml::file(&global_path));
        }

        figment = figment.merge(Env::prefixed("CLAUDEGEN_").split('_').lowercase(true));

        let config: Config = figment
            .extract()
            .map_err(|e| ClaudegenError::Config(format!("Configuration error: {e}")))?;

        config.validate()?;

        Ok(config)
    }

    /// Load configuration from a specific file only
    pub fn load_from_file(path: &Path) -> Result<Config> {
        let base_config = Config::default();

        let config: Config = Figment::new()
            .merge(Serialized::defaults(base_config))
            .merge(Toml::file(path))
            .extract()
            .map_err(|e| ClaudegenError::Config(format!("Configuration error: {e}")))?;

        config.validate()?;

        Ok(config)
    }

    // =========================================================================
    // Path Management
    // =========================================================================

    /// Get path to global config directory (~/.config/claudegen/)
    pub fn global_dir() -> Option<PathBuf> {
        env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                env::var("HOME")
                    .ok()
                    .map(|home| PathBuf::from(home).join(".config"))
            })
            .map(|p| p.join("claudegen"))
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
            .map(|p| p.join("claudegen"))
    }

    /// Get path to project config file
    pub fn project_config_path() -> PathBuf {
        PathBuf::from(".claudegen/config.toml")
    }

    /// Get project data directory
    pub fn project_dir() -> PathBuf {
        PathBuf::from(".claudegen")
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
                toml::to_string_pretty(&config)
                    .map_err(|e| ClaudegenError::Config(e.to_string()))?
            );
        }

        Ok(())
    }

    /// Edit config file with default editor
    pub fn edit_config(global: bool) -> Result<()> {
        let path = if global {
            Self::global_config_path().ok_or_else(|| {
                ClaudegenError::Config("Cannot determine global config path".to_string())
            })?
        } else {
            Self::project_config_path()
        };

        if !path.exists() {
            println!("Config file does not exist: {}", path.display());
            println!(
                "Run: claudegen config init {}",
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
            ClaudegenError::Config(format!("Failed to launch editor {editor}: {e}"))
        })?;

        if !status.success() {
            return Err(ClaudegenError::Config(
                "Editor exited with error".to_string(),
            ));
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
            ClaudegenError::Config("Cannot determine global config directory".to_string())
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
        r#"# claudegen Global Configuration
# User-wide defaults. Project settings in .claudegen/config.toml override these.

version = "2.0"

# LLM settings (for documentation generation)
[llm]
provider = "claude-agent"
model = "claude-sonnet-4-5-20250929"
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
            r#"# claudegen Project Configuration
# Project-specific settings that override global defaults.

version = "2.0"

[project]
name = "{project_name}"
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
"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_default_config() {
        // Use global_only to avoid dependency on project config
        let config = ConfigLoader::load_global_only().unwrap();
        assert_eq!(config.version, "2.0");
    }

    #[test]
    fn test_init_project() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        ConfigLoader::init_project(Some("test-project")).unwrap();

        assert!(PathBuf::from(".claudegen").exists());
        assert!(PathBuf::from(".claudegen/config.toml").exists());
        assert!(PathBuf::from(".claudegen/cache").exists());
        assert!(PathBuf::from(".claudegen/checkpoints").exists());
    }

    #[test]
    fn test_config_load_default() {
        // Use global_only to avoid dependency on project config
        let config = ConfigLoader::load_global_only().unwrap();

        // Verify default values are sensible
        assert!(config.llm.timeout_secs > 0);
        assert!(!config.llm.default_model.is_empty());
        assert!(config.convergence.max_iterations > 0);
        assert!(config.value.min_overall >= 0.0 && config.value.min_overall <= 1.0);
    }

    #[test]
    fn test_timeout_config_defaults() {
        // Use global_only to avoid dependency on project config
        let config = ConfigLoader::load_global_only().unwrap();

        // Verify TimeoutConfig default values (high-quality configuration)
        assert_eq!(config.timeout.quality_loop_timeout_secs, 3600); // 1 hour
        assert_eq!(config.timeout.session_timeout_secs, 7200); // 2 hours
        assert_eq!(config.timeout.analysis_phase_timeout_secs, 1800); // 30 minutes
        assert_eq!(config.timeout.specialist_timeout_secs, 300); // 5 minutes
    }

    #[test]
    fn test_checkpoint_interval_dynamic_calculation() {
        let config = ConfigLoader::load_global_only().unwrap();

        // Checkpoint interval = quality_loop_timeout / 4 (min 60s)
        let expected = (config.timeout.quality_loop_timeout_secs / 4).max(60);
        assert_eq!(
            config.timeout.effective_checkpoint_interval_secs(),
            expected
        );

        // Default: 3600 / 4 = 900 seconds
        assert_eq!(config.timeout.effective_checkpoint_interval_secs(), 900);
    }

    #[test]
    fn test_checkpoint_interval_never_exceeds_timeout() {
        use crate::config::TimeoutConfig;

        // Even with very short timeout, checkpoint interval is at least 60s
        let timeout = TimeoutConfig {
            quality_loop_timeout_secs: 120,
            ..Default::default()
        };
        assert_eq!(timeout.effective_checkpoint_interval_secs(), 60);

        // With normal timeout, interval is 1/4
        let timeout = TimeoutConfig {
            quality_loop_timeout_secs: 1200,
            ..Default::default()
        };
        assert_eq!(timeout.effective_checkpoint_interval_secs(), 300);

        // Interval is always < timeout (1/4)
        assert!(timeout.effective_checkpoint_interval_secs() < timeout.quality_loop_timeout_secs);
    }

    #[test]
    fn test_project_config_timeout_override() {
        use std::io::Write;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Create config with ALL timeout fields
        let mut config_file = std::fs::File::create(&config_path).unwrap();
        writeln!(
            config_file,
            r#"
version = "2.0"

[timeout]
session_timeout_secs = 7200
quality_loop_timeout_secs = 3600
analysis_phase_timeout_secs = 600
generation_phase_timeout_secs = 300
specialist_timeout_secs = 120
llm_call_timeout_secs = 300
"#
        )
        .unwrap();

        let config = ConfigLoader::load_from_file(&config_path).unwrap();

        // Config should have overridden values
        assert_eq!(config.timeout.quality_loop_timeout_secs, 3600);
        assert_eq!(config.timeout.session_timeout_secs, 7200);

        // Checkpoint interval recalculated: 3600 / 4 = 900
        assert_eq!(config.timeout.effective_checkpoint_interval_secs(), 900);
    }

    #[test]
    fn test_project_config_partial_timeout_override() {
        use std::io::Write;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Test partial override - only specify one field
        let mut config_file = std::fs::File::create(&config_path).unwrap();
        writeln!(
            config_file,
            r#"
version = "2.0"

[timeout]
quality_loop_timeout_secs = 3600
"#
        )
        .unwrap();

        let config = ConfigLoader::load_from_file(&config_path).unwrap();

        // Partial field should be overridden
        assert_eq!(config.timeout.quality_loop_timeout_secs, 3600);

        // Other fields should retain defaults (#[serde(default)] behavior)
        assert_eq!(config.timeout.session_timeout_secs, 7200); // default
        assert_eq!(config.timeout.analysis_phase_timeout_secs, 1800); // default
    }
}
