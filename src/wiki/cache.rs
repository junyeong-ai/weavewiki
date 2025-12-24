//! Wiki Generation Cache
//!
//! Multi-level caching system for generated wiki content.
//! Based on deepwiki-open's cache pattern.
//!
//! ## Cache Levels
//!
//! 1. **File Cache**: JSON files for quick page retrieval
//! 2. **Database Cache**: Metadata and references in SQLite
//!
//! ## Cache Keys
//!
//! - Project path + commit hash + model name
//! - Supports partial invalidation by file/module

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::types::Result;

/// Cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Cache directory path
    pub cache_dir: PathBuf,
    /// Maximum cache age in hours (default: 168 = 1 week)
    pub max_age_hours: u64,
    /// Whether to use commit-based cache keys
    pub use_commit_key: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from(".weavewiki/cache"),
            max_age_hours: 168, // 1 week
            use_commit_key: true,
        }
    }
}

/// Cache entry metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetadata {
    /// Cache key
    pub key: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last accessed timestamp
    pub accessed_at: DateTime<Utc>,
    /// Git commit ID at cache time
    pub commit_id: Option<String>,
    /// Model used for generation
    pub model: String,
    /// Number of pages cached
    pub page_count: usize,
    /// Total cache size in bytes
    pub size_bytes: usize,
}

/// Cached wiki content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiCacheEntry {
    /// Metadata about the cache
    pub metadata: CacheMetadata,
    /// Cached page content (path -> content)
    pub pages: std::collections::HashMap<String, CachedPage>,
}

/// A single cached page
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPage {
    /// Page path relative to wiki root
    pub path: String,
    /// Page content (markdown)
    pub content: String,
    /// Last modified timestamp
    pub modified_at: DateTime<Utc>,
    /// Content hash for change detection
    pub content_hash: String,
}

/// Wiki cache manager
pub struct WikiCache {
    config: CacheConfig,
}

impl WikiCache {
    pub fn new(config: CacheConfig) -> Self {
        Self { config }
    }

    pub fn with_default_config() -> Self {
        Self::new(CacheConfig::default())
    }

    /// Generate cache key from project parameters
    pub fn cache_key(&self, project_path: &Path, commit_id: Option<&str>) -> String {
        let project_name = project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        if self.config.use_commit_key
            && let Some(commit) = commit_id
        {
            return format!("{}_{}", project_name, &commit[..7.min(commit.len())]);
        }

        project_name.to_string()
    }

    /// Get cache file path for a key
    fn cache_path(&self, key: &str) -> PathBuf {
        self.config.cache_dir.join(format!("{}.json", key))
    }

    /// Check if cache exists and is valid
    pub async fn is_valid(&self, key: &str) -> bool {
        let path = self.cache_path(key);

        match tokio::fs::metadata(&path).await {
            Ok(metadata) => {
                if let Ok(modified) = metadata.modified() {
                    let age = std::time::SystemTime::now()
                        .duration_since(modified)
                        .unwrap_or_default();
                    let max_age = std::time::Duration::from_secs(self.config.max_age_hours * 3600);
                    age < max_age
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    /// Load cached wiki
    pub async fn load(&self, key: &str) -> Result<Option<WikiCacheEntry>> {
        let path = self.cache_path(key);

        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let mut entry: WikiCacheEntry = serde_json::from_str(&content).map_err(|e| {
                    crate::types::WeaveError::Config(format!("Cache parse error: {}", e))
                })?;

                // Update accessed time
                entry.metadata.accessed_at = Utc::now();

                debug!("Loaded cache '{}' with {} pages", key, entry.pages.len());
                Ok(Some(entry))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Save wiki to cache
    pub async fn save(&self, entry: &WikiCacheEntry) -> Result<()> {
        tokio::fs::create_dir_all(&self.config.cache_dir).await?;

        let path = self.cache_path(&entry.metadata.key);
        let content = serde_json::to_string_pretty(entry)?;
        tokio::fs::write(&path, &content).await?;

        info!(
            "Saved cache '{}' ({} pages, {} bytes)",
            entry.metadata.key,
            entry.pages.len(),
            content.len()
        );

        Ok(())
    }

    /// Create a new cache entry
    pub fn create_entry(
        &self,
        key: &str,
        commit_id: Option<String>,
        model: &str,
    ) -> WikiCacheEntry {
        WikiCacheEntry {
            metadata: CacheMetadata {
                key: key.to_string(),
                created_at: Utc::now(),
                accessed_at: Utc::now(),
                commit_id,
                model: model.to_string(),
                page_count: 0,
                size_bytes: 0,
            },
            pages: std::collections::HashMap::new(),
        }
    }

    /// Add a page to cache entry
    pub fn add_page(entry: &mut WikiCacheEntry, path: &str, content: &str) {
        let hash = format!("{:x}", md5_hash(content));
        entry.pages.insert(
            path.to_string(),
            CachedPage {
                path: path.to_string(),
                content: content.to_string(),
                modified_at: Utc::now(),
                content_hash: hash,
            },
        );
        entry.metadata.page_count = entry.pages.len();
        entry.metadata.size_bytes = entry.pages.values().map(|p| p.content.len()).sum();
    }

    /// Invalidate cache for a key
    pub async fn invalidate(&self, key: &str) -> Result<bool> {
        let path = self.cache_path(key);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                info!("Invalidated cache '{}'", key);
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Clear all caches
    pub async fn clear_all(&self) -> Result<usize> {
        let mut count = 0;

        let mut entries = match tokio::fs::read_dir(&self.config.cache_dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                tokio::fs::remove_file(&path).await?;
                count += 1;
            }
        }

        info!("Cleared {} cache entries", count);
        Ok(count)
    }

    /// List all cache entries
    pub async fn list_entries(&self) -> Result<Vec<CacheMetadata>> {
        let mut entries = Vec::new();

        let mut dir_entries = match tokio::fs::read_dir(&self.config.cache_dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
            Err(e) => return Err(e.into()),
        };

        while let Some(entry) = dir_entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(content) = tokio::fs::read_to_string(&path).await
                && let Ok(cache_entry) = serde_json::from_str::<WikiCacheEntry>(&content)
            {
                entries.push(cache_entry.metadata);
            }
        }

        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(entries)
    }

    /// Get cache statistics
    pub async fn stats(&self) -> Result<CacheStats> {
        let entries = self.list_entries().await?;

        Ok(CacheStats {
            entry_count: entries.len(),
            total_pages: entries.iter().map(|e| e.page_count).sum(),
            total_size_bytes: entries.iter().map(|e| e.size_bytes).sum(),
            oldest_entry: entries.last().map(|e| e.created_at),
            newest_entry: entries.first().map(|e| e.created_at),
        })
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entry_count: usize,
    pub total_pages: usize,
    pub total_size_bytes: usize,
    pub oldest_entry: Option<DateTime<Utc>>,
    pub newest_entry: Option<DateTime<Utc>>,
}

/// Simple MD5 hash for content deduplication (not for security)
fn md5_hash(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_key_generation() {
        let config = CacheConfig::default();
        let cache = WikiCache::new(config);

        let key = cache.cache_key(Path::new("/project/my-app"), Some("abc123def"));
        assert!(key.contains("my-app"));
        assert!(key.contains("abc123"));
    }

    #[test]
    fn test_cache_entry_creation() {
        let cache = WikiCache::with_default_config();
        let mut entry = cache.create_entry("test", Some("abc123".to_string()), "claude-sonnet-4");

        WikiCache::add_page(&mut entry, "index.md", "# Hello World");

        assert_eq!(entry.pages.len(), 1);
        assert_eq!(entry.metadata.page_count, 1);
    }

    #[tokio::test]
    async fn test_cache_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config = CacheConfig {
            cache_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let cache = WikiCache::new(config);

        let mut entry = cache.create_entry("test-project", None, "claude-sonnet-4");
        WikiCache::add_page(&mut entry, "index.md", "# Test Content");

        cache.save(&entry).await.unwrap();

        let loaded = cache.load("test-project").await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().pages.len(), 1);
    }

    #[tokio::test]
    async fn test_cache_invalidation() {
        let temp_dir = TempDir::new().unwrap();
        let config = CacheConfig {
            cache_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let cache = WikiCache::new(config);

        let entry = cache.create_entry("to-invalidate", None, "model");
        cache.save(&entry).await.unwrap();

        assert!(cache.is_valid("to-invalidate").await);
        assert!(cache.invalidate("to-invalidate").await.unwrap());
        assert!(!cache.is_valid("to-invalidate").await);
    }
}
