//! CLAUDE.md Section-level Cache
//!
//! Provides caching for CLAUDE.md sections to avoid regenerating unchanged content.
//! Cache stored in `.claudegen/cache/claude_md_sections.json`.

use crate::types::{ClaudegenError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Tracks the source inputs and content for a single CLAUDE.md section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionSource {
    /// Name of the section (overview, architecture, standards, domain, gotchas)
    pub section_name: String,
    /// Hash of the input data that feeds this section
    pub input_hash: String,
    /// Hash of the generated content
    pub content_hash: String,
    /// Cached content (actual generated text)
    pub content: SectionContent,
}

/// Cached content for different section types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SectionContent {
    /// Single string content (overview, architecture, domain_knowledge)
    Single(String),
    /// Optional string content
    Optional(Option<String>),
    /// List of strings (standards, gotchas)
    List(Vec<String>),
}

impl SectionContent {
    pub fn as_string(&self) -> Option<&String> {
        match self {
            SectionContent::Single(s) => Some(s),
            SectionContent::Optional(Some(s)) => Some(s),
            _ => None,
        }
    }

    pub fn as_optional(&self) -> Option<String> {
        match self {
            SectionContent::Single(s) => Some(s.clone()),
            SectionContent::Optional(o) => o.clone(),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&Vec<String>> {
        match self {
            SectionContent::List(v) => Some(v),
            _ => None,
        }
    }
}

/// Section manifest storing hashes and content for all CLAUDE.md sections
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SectionManifest {
    /// Version of the manifest format
    pub version: u32,
    /// Section sources indexed by section name
    pub sections: HashMap<String, SectionSource>,
    /// Timestamp of last update
    pub last_updated: u64,
}

impl SectionManifest {
    pub fn new() -> Self {
        Self {
            version: 2, // Version 2 includes content caching
            sections: HashMap::new(),
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}

/// Computes a hash for input data using blake3
pub fn compute_hash(data: &str) -> String {
    crate::utils::hash::content_hash(data)
}

/// Cache for CLAUDE.md section-level differential updates
pub struct ClaudeMdCache {
    cache_path: std::path::PathBuf,
}

impl ClaudeMdCache {
    /// Create a new cache instance for the given project root
    pub fn new(project_root: &Path) -> Self {
        let cache_path = project_root
            .join(".claudegen")
            .join("cache")
            .join("claude_md_sections.json");
        Self { cache_path }
    }

    /// Load the section manifest from cache
    pub fn load_manifest(&self) -> Option<SectionManifest> {
        if !self.cache_path.exists() {
            return None;
        }

        match fs::read_to_string(&self.cache_path) {
            Ok(content) => {
                let manifest: Option<SectionManifest> = serde_json::from_str(&content).ok();
                // Check version compatibility
                manifest.filter(|m| m.version >= 2)
            }
            Err(_) => None,
        }
    }

    /// Get cached content for a section (if not stale)
    pub fn get_cached_content(
        &self,
        manifest: Option<&SectionManifest>,
        name: &str,
        new_hash: &str,
    ) -> Option<SectionContent> {
        let _ = self; // Use self to keep it as a method
        let m = manifest?;
        let section = m.sections.get(name)?;
        if section.input_hash == new_hash {
            Some(section.content.clone())
        } else {
            None
        }
    }

    /// Save the section manifest to cache
    pub fn save_manifest(&self, manifest: &SectionManifest) -> Result<()> {
        if let Some(parent) = self.cache_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                ClaudegenError::Io(std::io::Error::other(format!(
                    "Failed to create cache directory: {e}"
                )))
            })?;
        }

        let content = serde_json::to_string_pretty(manifest).map_err(|e| {
            ClaudegenError::Io(std::io::Error::other(format!(
                "Failed to serialize manifest: {e}"
            )))
        })?;

        // Atomic write: temp file + rename (prevents corruption on crash)
        let parent = self.cache_path.parent().unwrap_or(std::path::Path::new("."));
        let temp_path = parent.join(format!(".tmp_claude_md_{}", std::process::id()));

        fs::write(&temp_path, &content).map_err(|e| {
            ClaudegenError::Io(std::io::Error::other(format!(
                "Failed to write temp cache file: {e}"
            )))
        })?;

        // fsync to ensure data is on disk before rename
        let file = fs::File::open(&temp_path).map_err(|e| {
            ClaudegenError::Io(std::io::Error::other(format!(
                "Failed to open temp cache file for sync: {e}"
            )))
        })?;
        file.sync_all().map_err(|e| {
            ClaudegenError::Io(std::io::Error::other(format!(
                "Failed to sync temp cache file: {e}"
            )))
        })?;

        fs::rename(&temp_path, &self.cache_path).map_err(|e| {
            let _ = fs::remove_file(&temp_path);
            ClaudegenError::Io(std::io::Error::other(format!(
                "Failed to rename cache file: {e}"
            )))
        })?;

        Ok(())
    }
}

