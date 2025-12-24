//! File Content Cache
//!
//! LRU cache for file contents to avoid repeated disk reads during verification.
//! Thread-safe with automatic invalidation on file modification.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Instant, SystemTime};

/// Maximum number of files to cache
const DEFAULT_MAX_ENTRIES: usize = 100;

/// Maximum file size to cache (1MB)
const MAX_FILE_SIZE: usize = 1024 * 1024;

/// File content cache with LRU eviction
pub struct FileContentCache {
    cache: RwLock<HashMap<PathBuf, CachedFile>>,
    max_entries: usize,
    stats: RwLock<CacheStats>,
}

/// Cached file entry
struct CachedFile {
    content: String,
    modified: SystemTime,
    last_accessed: Instant,
    size: usize,
}

/// Cache statistics
#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub invalidations: u64,
}

impl CacheStats {
    /// Cache hit rate (0.0 - 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

impl Default for FileContentCache {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ENTRIES)
    }
}

impl FileContentCache {
    /// Create a new cache with specified maximum entries
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_entries,
            stats: RwLock::new(CacheStats::default()),
        }
    }

    /// Get file content, loading from disk if not cached or stale
    pub fn get_or_load(&self, path: &Path) -> std::io::Result<String> {
        // Try cache first
        if let Some(content) = self.get_cached(path) {
            return Ok(content);
        }

        // Load from disk
        let content = std::fs::read_to_string(path)?;

        // Update stats
        if let Ok(mut stats) = self.stats.write() {
            stats.misses += 1;
        }

        // Cache if not too large
        if content.len() <= MAX_FILE_SIZE {
            self.store(path.to_path_buf(), content.clone())?;
        }

        Ok(content)
    }

    /// Get cached content if valid
    fn get_cached(&self, path: &Path) -> Option<String> {
        let mut cache = self.cache.write().ok()?;

        if let Some(entry) = cache.get_mut(path) {
            // Validate freshness
            if let Ok(meta) = std::fs::metadata(path)
                && let Ok(modified) = meta.modified()
                && modified != entry.modified
            {
                // File changed, invalidate
                cache.remove(path);
                if let Ok(mut stats) = self.stats.write() {
                    stats.invalidations += 1;
                }
                return None;
            }

            // Update access time for LRU
            entry.last_accessed = Instant::now();

            // Update stats
            if let Ok(mut stats) = self.stats.write() {
                stats.hits += 1;
            }

            return Some(entry.content.clone());
        }

        None
    }

    /// Store content in cache
    fn store(&self, path: PathBuf, content: String) -> std::io::Result<()> {
        let mut cache = self
            .cache
            .write()
            .map_err(|_| std::io::Error::other("Cache lock poisoned"))?;

        // LRU eviction if at capacity
        if cache.len() >= self.max_entries {
            self.evict_oldest(&mut cache);
        }

        // Get file modification time
        let modified = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::now());

        let size = content.len();
        cache.insert(
            path,
            CachedFile {
                content,
                modified,
                last_accessed: Instant::now(),
                size,
            },
        );

        Ok(())
    }

    /// Evict oldest entry (LRU)
    fn evict_oldest(&self, cache: &mut HashMap<PathBuf, CachedFile>) {
        if let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, v)| v.last_accessed)
            .map(|(k, _)| k.clone())
        {
            cache.remove(&oldest_key);
            if let Ok(mut stats) = self.stats.write() {
                stats.evictions += 1;
            }
        }
    }

    /// Clear all cached entries
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        self.stats
            .read()
            .map(|s| CacheStats {
                hits: s.hits,
                misses: s.misses,
                evictions: s.evictions,
                invalidations: s.invalidations,
            })
            .unwrap_or_default()
    }

    /// Current number of cached entries
    pub fn len(&self) -> usize {
        self.cache.read().map(|c| c.len()).unwrap_or(0)
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total size of cached content in bytes
    pub fn total_size(&self) -> usize {
        self.cache
            .read()
            .map(|c| c.values().map(|v| v.size).sum())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_hit() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("test.txt");
        std::fs::write(&file, "hello world").unwrap();

        let cache = FileContentCache::new(10);

        // First read - miss
        let content1 = cache.get_or_load(&file).unwrap();
        assert_eq!(content1, "hello world");

        // Second read - hit
        let content2 = cache.get_or_load(&file).unwrap();
        assert_eq!(content2, "hello world");

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_cache_invalidation() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("test.txt");
        std::fs::write(&file, "version 1").unwrap();

        let cache = FileContentCache::new(10);

        // First read
        let _ = cache.get_or_load(&file).unwrap();

        // Wait a bit and modify
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&file, "version 2").unwrap();

        // Should get new content
        let content = cache.get_or_load(&file).unwrap();
        assert_eq!(content, "version 2");
    }

    #[test]
    fn test_lru_eviction() {
        let temp = TempDir::new().unwrap();
        let cache = FileContentCache::new(2);

        // Create and cache 3 files
        for i in 0..3 {
            let file = temp.path().join(format!("file{}.txt", i));
            std::fs::write(&file, format!("content {}", i)).unwrap();
            let _ = cache.get_or_load(&file).unwrap();
        }

        // Should have evicted the oldest
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.stats().evictions, 1);
    }
}
