//! Analysis Archive
//!
//! Preserves individual chunk analysis results for later reference.
//! Stored as JSON files under `.claudegen/analysis/` for drill-down access
//! during generation and by Claude Code during usage.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::types::Result;
use crate::utils::safe_join;

/// Index entry for a single archived chunk analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkIndexEntry {
    pub chunk_id: String,
    pub module: String,
    pub files: Vec<String>,
    pub line_range: Option<(u32, u32)>,
    pub pattern_count: usize,
    pub constraint_count: usize,
    pub gotcha_count: usize,
    pub archive_path: String,
}

/// Index of all archived chunk analyses
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisArchive {
    pub chunks: Vec<ChunkIndexEntry>,
    pub total_patterns: usize,
    pub total_constraints: usize,
    pub total_gotchas: usize,
    #[serde(default)]
    pub file_to_chunks: HashMap<String, Vec<String>>,
}

impl AnalysisArchive {
    /// Archive directory path
    fn archive_dir(project_root: &Path) -> PathBuf {
        project_root.join(".claudegen").join("analysis")
    }

    /// Save a chunk analysis result to the archive.
    ///
    /// Returns the index entry for this chunk.
    pub async fn save_chunk(
        project_root: &Path,
        chunk_id: &str,
        module: &str,
        files: &[String],
        line_range: Option<(u32, u32)>,
        result: &serde_json::Value,
    ) -> Result<ChunkIndexEntry> {
        let archive_dir = Self::archive_dir(project_root);
        tokio::fs::create_dir_all(&archive_dir).await?;

        // Sanitize chunk_id to prevent path traversal
        let filename = format!("chunk_{}.json", chunk_id);
        let archive_path = safe_join(&archive_dir, &filename)
            .unwrap_or_else(|| archive_dir.join("_invalid_chunk.json"));

        let content = serde_json::to_string_pretty(result)?;
        tokio::fs::write(&archive_path, content).await?;

        // Count items in the result for the index
        let pattern_count = result
            .get("patterns")
            .and_then(|p| p.as_array())
            .map_or(0, |a| a.len());
        let constraint_count = result
            .get("constraints")
            .and_then(|c| c.as_array())
            .map_or(0, |a| a.len());
        let gotcha_count = result
            .get("gotchas")
            .and_then(|g| g.as_array())
            .map_or(0, |a| a.len());

        Ok(ChunkIndexEntry {
            chunk_id: chunk_id.to_string(),
            module: module.to_string(),
            files: files.to_vec(),
            line_range,
            pattern_count,
            constraint_count,
            gotcha_count,
            archive_path: format!(".claudegen/analysis/{}", filename),
        })
    }

    /// Save the complete archive index.
    pub async fn save_index(&self, project_root: &Path) -> Result<()> {
        let archive_dir = Self::archive_dir(project_root);
        tokio::fs::create_dir_all(&archive_dir).await?;

        let index_path = archive_dir.join("index.json");
        let content = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&index_path, content).await?;

        tracing::info!(
            chunks = self.chunks.len(),
            patterns = self.total_patterns,
            constraints = self.total_constraints,
            gotchas = self.total_gotchas,
            "Saved analysis archive index"
        );

        Ok(())
    }

    /// Load an existing archive index.
    pub async fn load(project_root: &Path) -> Result<Option<Self>> {
        let index_path = Self::archive_dir(project_root).join("index.json");

        if !index_path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&index_path).await?;
        let archive: Self = serde_json::from_str(&content)?;
        Ok(Some(archive))
    }

    /// Get chunks related to a specific module.
    pub fn chunks_for_module(&self, module: &str) -> Vec<&ChunkIndexEntry> {
        self.chunks
            .iter()
            .filter(|c| c.module == module)
            .collect()
    }

    /// Get chunks containing a specific file.
    pub fn chunks_for_file(&self, file: &str) -> Vec<&ChunkIndexEntry> {
        self.chunks
            .iter()
            .filter(|c| c.files.iter().any(|f| f == file))
            .collect()
    }

    /// Build from a list of chunk index entries.
    ///
    /// Automatically builds the file-to-chunk reverse mapping index.
    pub fn from_entries(entries: Vec<ChunkIndexEntry>) -> Self {
        let total_patterns = entries.iter().map(|e| e.pattern_count).sum();
        let total_constraints = entries.iter().map(|e| e.constraint_count).sum();
        let total_gotchas = entries.iter().map(|e| e.gotcha_count).sum();
        let file_to_chunks = Self::build_file_index(&entries);

        Self {
            chunks: entries,
            total_patterns,
            total_constraints,
            total_gotchas,
            file_to_chunks,
        }
    }

    fn build_file_index(entries: &[ChunkIndexEntry]) -> HashMap<String, Vec<String>> {
        let mut index: HashMap<String, Vec<String>> = HashMap::new();
        for entry in entries {
            for file in &entry.files {
                index
                    .entry(file.clone())
                    .or_default()
                    .push(entry.chunk_id.clone());
            }
        }
        index
    }

    /// Look up chunk IDs for a file using the pre-built index (O(1) lookup).
    pub fn chunk_ids_for_file(&self, file: &str) -> &[String] {
        self.file_to_chunks
            .get(file)
            .map(|v| v.as_slice())
            .unwrap_or_default()
    }

    /// Clear the archive directory.
    pub async fn clear(project_root: &Path) -> Result<()> {
        let archive_dir = Self::archive_dir(project_root);
        if archive_dir.exists() {
            tokio::fs::remove_dir_all(&archive_dir).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_entries() {
        let entries = vec![
            ChunkIndexEntry {
                chunk_id: "auth_001".into(),
                module: "auth".into(),
                files: vec!["src/auth/token.rs".into()],
                line_range: Some((1, 100)),
                pattern_count: 3,
                constraint_count: 1,
                gotcha_count: 2,
                archive_path: ".claudegen/analysis/chunk_auth_001.json".into(),
            },
            ChunkIndexEntry {
                chunk_id: "api_001".into(),
                module: "api".into(),
                files: vec!["src/api/routes.rs".into()],
                line_range: None,
                pattern_count: 5,
                constraint_count: 2,
                gotcha_count: 0,
                archive_path: ".claudegen/analysis/chunk_api_001.json".into(),
            },
        ];

        let archive = AnalysisArchive::from_entries(entries);
        assert_eq!(archive.chunks.len(), 2);
        assert_eq!(archive.total_patterns, 8);
        assert_eq!(archive.total_constraints, 3);
        assert_eq!(archive.total_gotchas, 2);
    }

    #[test]
    fn test_chunks_for_module() {
        let entries = vec![
            ChunkIndexEntry {
                chunk_id: "auth_001".into(),
                module: "auth".into(),
                files: vec!["src/auth/token.rs".into()],
                line_range: None,
                pattern_count: 3,
                constraint_count: 1,
                gotcha_count: 2,
                archive_path: ".claudegen/analysis/chunk_auth_001.json".into(),
            },
            ChunkIndexEntry {
                chunk_id: "api_001".into(),
                module: "api".into(),
                files: vec!["src/api/routes.rs".into()],
                line_range: None,
                pattern_count: 5,
                constraint_count: 0,
                gotcha_count: 0,
                archive_path: ".claudegen/analysis/chunk_api_001.json".into(),
            },
        ];

        let archive = AnalysisArchive::from_entries(entries);
        assert_eq!(archive.chunks_for_module("auth").len(), 1);
        assert_eq!(archive.chunks_for_module("api").len(), 1);
        assert_eq!(archive.chunks_for_module("unknown").len(), 0);
    }

    #[test]
    fn test_chunks_for_file() {
        let entries = vec![
            ChunkIndexEntry {
                chunk_id: "auth_001".into(),
                module: "auth".into(),
                files: vec!["src/auth/token.rs".into(), "src/auth/session.rs".into()],
                line_range: None,
                pattern_count: 3,
                constraint_count: 1,
                gotcha_count: 2,
                archive_path: ".claudegen/analysis/chunk_auth_001.json".into(),
            },
        ];

        let archive = AnalysisArchive::from_entries(entries);
        assert_eq!(archive.chunks_for_file("src/auth/token.rs").len(), 1);
        assert_eq!(archive.chunks_for_file("src/auth/other.rs").len(), 0);
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let result = serde_json::json!({
            "patterns": [{"name": "test"}],
            "constraints": [],
            "gotchas": [{"title": "gotcha1"}]
        });

        let entry = AnalysisArchive::save_chunk(
            tmp.path(),
            "test_001",
            "test",
            &["src/test.rs".into()],
            Some((1, 50)),
            &result,
        )
        .await
        .unwrap();

        assert_eq!(entry.pattern_count, 1);
        assert_eq!(entry.gotcha_count, 1);

        let archive = AnalysisArchive::from_entries(vec![entry]);
        archive.save_index(tmp.path()).await.unwrap();

        let loaded = AnalysisArchive::load(tmp.path()).await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.chunks.len(), 1);
        assert_eq!(loaded.total_patterns, 1);
        assert_eq!(loaded.file_to_chunks.len(), 1);
        assert_eq!(
            loaded.chunk_ids_for_file("src/test.rs"),
            &["test_001".to_string()]
        );
    }

    #[test]
    fn test_file_to_chunk_index() {
        let entries = vec![
            ChunkIndexEntry {
                chunk_id: "c1".into(),
                module: "auth".into(),
                files: vec!["src/auth/token.rs".into(), "src/auth/session.rs".into()],
                line_range: None,
                pattern_count: 0,
                constraint_count: 0,
                gotcha_count: 0,
                archive_path: "".into(),
            },
            ChunkIndexEntry {
                chunk_id: "c2".into(),
                module: "auth".into(),
                files: vec!["src/auth/session.rs".into()],
                line_range: Some((100, 200)),
                pattern_count: 0,
                constraint_count: 0,
                gotcha_count: 0,
                archive_path: "".into(),
            },
        ];

        let archive = AnalysisArchive::from_entries(entries);
        assert_eq!(archive.chunk_ids_for_file("src/auth/token.rs"), &["c1"]);
        let session_chunks = archive.chunk_ids_for_file("src/auth/session.rs");
        assert_eq!(session_chunks.len(), 2);
        assert!(session_chunks.contains(&"c1".to_string()));
        assert!(session_chunks.contains(&"c2".to_string()));
        assert!(archive.chunk_ids_for_file("nonexistent.rs").is_empty());
    }
}
