use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::distributed::ChunkAnalysisResult;
use crate::types::Result;
use crate::utils::safe_join;

pub const PROMPT_VERSION: &str = "v1";

const CACHE_SUBDIR: &str = "cache/analysis";

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    created_at: u64,
    prompt_version: String,
    result: ChunkAnalysisResult,
}

pub struct ChunkCache {
    cache_dir: PathBuf,
}

impl ChunkCache {
    pub fn new(project_root: &Path) -> Self {
        Self {
            cache_dir: project_root.join(".claudegen").join(CACHE_SUBDIR),
        }
    }

    pub fn cache_key(chunk_content: &str, prompt_version: &str) -> String {
        Self::cache_key_with_imports(chunk_content, prompt_version, &[])
    }

    /// Compute cache key incorporating import paths.
    /// When file A imports file B, changes to B should invalidate A's cache.
    /// Import paths are sorted before hashing to avoid order-dependent cache misses.
    pub fn cache_key_with_imports(chunk_content: &str, prompt_version: &str, import_paths: &[&str]) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(chunk_content.as_bytes());
        hasher.update(prompt_version.as_bytes());
        let mut sorted_paths: Vec<&str> = import_paths.to_vec();
        sorted_paths.sort_unstable();
        for path in sorted_paths {
            hasher.update(path.as_bytes());
        }
        hasher.finalize().to_hex().to_string()
    }

    /// Compute cache key incorporating content hashes of cross-referenced chunks.
    /// When a referenced chunk's content changes (detected via its content_hash),
    /// this chunk's cache is invalidated.
    pub fn cache_key_with_cross_refs(
        chunk_content: &str,
        prompt_version: &str,
        cross_ref_hashes: &std::collections::HashMap<String, String>,
    ) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(chunk_content.as_bytes());
        hasher.update(prompt_version.as_bytes());
        let mut sorted: Vec<(&String, &String)> = cross_ref_hashes.iter().collect();
        sorted.sort_by_key(|(id, _)| *id);
        for (chunk_id, hash) in sorted {
            hasher.update(chunk_id.as_bytes());
            hasher.update(hash.as_bytes());
        }
        hasher.finalize().to_hex().to_string()
    }

    pub async fn get(&self, key: &str) -> Option<ChunkAnalysisResult> {
        let path = self.entry_path(key);
        let data = tokio::fs::read_to_string(&path).await.ok()?;
        let entry: CacheEntry = serde_json::from_str(&data).ok()?;
        if entry.prompt_version != PROMPT_VERSION {
            let _ = tokio::fs::remove_file(&path).await;
            return None;
        }
        Some(entry.result)
    }

    pub async fn put(&self, key: &str, result: &ChunkAnalysisResult) -> Result<()> {
        tokio::fs::create_dir_all(&self.cache_dir).await?;

        let entry = CacheEntry {
            created_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            prompt_version: PROMPT_VERSION.to_string(),
            result: result.clone(),
        };

        let json = serde_json::to_string(&entry)?;

        // Atomic write: write to temp file, then rename
        let final_path = self.entry_path(key);
        let tmp_path = self.entry_path(&format!("{}.tmp", key));
        tokio::fs::write(&tmp_path, json).await?;
        tokio::fs::rename(&tmp_path, &final_path).await?;

        Ok(())
    }

    pub async fn clear(&self) -> Result<()> {
        if self.cache_dir.exists() {
            tokio::fs::remove_dir_all(&self.cache_dir).await?;
        }
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        Ok(())
    }

    pub async fn evict_older_than(&self, max_age: Duration) -> Result<usize> {
        let cutoff = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(max_age.as_secs());

        let mut evicted = 0usize;
        let mut entries = match tokio::fs::read_dir(&self.cache_dir).await {
            Ok(entries) => entries,
            Err(_) => return Ok(0),
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(data) = tokio::fs::read_to_string(&path).await
                && let Ok(cached) = serde_json::from_str::<CacheEntry>(&data)
                && cached.created_at < cutoff
            {
                let _ = tokio::fs::remove_file(&path).await;
                evicted += 1;
            }
        }

        Ok(evicted)
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        // Sanitize key to prevent path traversal (defensive, as keys are typically hashes)
        let filename = format!("{key}.json");
        safe_join(&self.cache_dir, &filename)
            .unwrap_or_else(|| self.cache_dir.join("_invalid_key.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_key_deterministic() {
        let k1 = ChunkCache::cache_key("hello world", "v1");
        let k2 = ChunkCache::cache_key("hello world", "v1");
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_cache_key_differs_on_content() {
        let k1 = ChunkCache::cache_key("hello", "v1");
        let k2 = ChunkCache::cache_key("world", "v1");
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_cache_key_differs_on_version() {
        let k1 = ChunkCache::cache_key("same content", "v1");
        let k2 = ChunkCache::cache_key("same content", "v2");
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_cache_key_is_hex() {
        let key = ChunkCache::cache_key("test", "v1");
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(key.len(), 64);
    }

    #[test]
    fn test_cache_key_with_imports_differs() {
        let k1 = ChunkCache::cache_key("content", "v1");
        let k2 = ChunkCache::cache_key_with_imports("content", "v1", &["src/utils.rs"]);
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_cache_key_with_imports_order_independent() {
        let k1 = ChunkCache::cache_key_with_imports("content", "v1", &["a.rs", "b.rs"]);
        let k2 = ChunkCache::cache_key_with_imports("content", "v1", &["b.rs", "a.rs"]);
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_cache_key_no_imports_matches_base() {
        let k1 = ChunkCache::cache_key("content", "v1");
        let k2 = ChunkCache::cache_key_with_imports("content", "v1", &[]);
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_cache_key_with_cross_refs_differs() {
        use std::collections::HashMap;
        let k1 = ChunkCache::cache_key("content", "v1");
        let mut refs = HashMap::new();
        refs.insert("chunk-2".to_string(), "abc123".to_string());
        let k2 = ChunkCache::cache_key_with_cross_refs("content", "v1", &refs);
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_cache_key_with_cross_refs_order_independent() {
        use std::collections::HashMap;
        let mut refs1 = HashMap::new();
        refs1.insert("chunk-a".to_string(), "hash1".to_string());
        refs1.insert("chunk-b".to_string(), "hash2".to_string());
        let mut refs2 = HashMap::new();
        refs2.insert("chunk-b".to_string(), "hash2".to_string());
        refs2.insert("chunk-a".to_string(), "hash1".to_string());
        let k1 = ChunkCache::cache_key_with_cross_refs("content", "v1", &refs1);
        let k2 = ChunkCache::cache_key_with_cross_refs("content", "v1", &refs2);
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_cache_key_with_cross_refs_invalidates_on_hash_change() {
        use std::collections::HashMap;
        let mut refs1 = HashMap::new();
        refs1.insert("chunk-2".to_string(), "old_hash".to_string());
        let mut refs2 = HashMap::new();
        refs2.insert("chunk-2".to_string(), "new_hash".to_string());
        let k1 = ChunkCache::cache_key_with_cross_refs("content", "v1", &refs1);
        let k2 = ChunkCache::cache_key_with_cross_refs("content", "v1", &refs2);
        assert_ne!(k1, k2, "Cache key should change when a referenced chunk's content hash changes");
    }

    #[tokio::test]
    async fn test_get_put_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cache = ChunkCache::new(tmp.path());

        let result = ChunkAnalysisResult {
            chunk_id: "test-chunk".into(),
            module_path: "src/lib".into(),
            confidence: 0.85,
            lines_analyzed: 42,
            ..Default::default()
        };

        let key = ChunkCache::cache_key("some content", PROMPT_VERSION);
        cache.put(&key, &result).await.unwrap();

        let cached = cache.get(&key).await.unwrap();
        assert_eq!(cached.chunk_id, "test-chunk");
        assert_eq!(cached.module_path, "src/lib");
        assert!((cached.confidence - 0.85).abs() < f32::EPSILON);
        assert_eq!(cached.lines_analyzed, 42);
    }

    #[tokio::test]
    async fn test_get_missing_key() {
        let tmp = TempDir::new().unwrap();
        let cache = ChunkCache::new(tmp.path());
        assert!(cache.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_clear() {
        let tmp = TempDir::new().unwrap();
        let cache = ChunkCache::new(tmp.path());

        let result = ChunkAnalysisResult::default();
        cache.put("a", &result).await.unwrap();
        cache.put("b", &result).await.unwrap();

        cache.clear().await.unwrap();
        assert!(cache.get("a").await.is_none());
        assert!(cache.get("b").await.is_none());
    }

    #[tokio::test]
    async fn test_evict_older_than() {
        let tmp = TempDir::new().unwrap();
        let cache = ChunkCache::new(tmp.path());

        let old_entry = CacheEntry {
            created_at: 1000,
            prompt_version: PROMPT_VERSION.to_string(),
            result: ChunkAnalysisResult::default(),
        };
        tokio::fs::create_dir_all(&cache.cache_dir).await.unwrap();
        let json = serde_json::to_string(&old_entry).unwrap();
        tokio::fs::write(cache.entry_path("old_key"), json)
            .await
            .unwrap();

        let recent = ChunkAnalysisResult::default();
        cache.put("recent_key", &recent).await.unwrap();

        let evicted = cache
            .evict_older_than(Duration::from_secs(3600))
            .await
            .unwrap();
        assert_eq!(evicted, 1);

        assert!(cache.get("old_key").await.is_none());
        assert!(cache.get("recent_key").await.is_some());
    }

    #[tokio::test]
    async fn test_stale_prompt_version_rejected() {
        let tmp = TempDir::new().unwrap();
        let cache = ChunkCache::new(tmp.path());

        let entry = CacheEntry {
            created_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            prompt_version: "v_old".to_string(),
            result: ChunkAnalysisResult::default(),
        };
        tokio::fs::create_dir_all(&cache.cache_dir).await.unwrap();
        let json = serde_json::to_string(&entry).unwrap();
        tokio::fs::write(cache.entry_path("stale"), json)
            .await
            .unwrap();

        assert!(cache.get("stale").await.is_none());
    }
}
