//! Sync Result Types
//!
//! Result types for incremental sync operations.

use super::ArtifactRef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncResult {
    pub regenerated: Vec<RegeneratedArtifact>,
    pub skipped: Vec<SkippedArtifact>,
    pub errors: Vec<SyncError>,
    pub files_scanned: usize,
    pub files_changed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegeneratedArtifact {
    pub artifact: ArtifactRef,
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedArtifact {
    pub artifact: ArtifactRef,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncError {
    pub artifact: Option<ArtifactRef>,
    pub error: String,
}

impl SyncResult {
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} regenerated, {} skipped, {} errors",
            self.regenerated.len(),
            self.skipped.len(),
            self.errors.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_result_is_success() {
        let result = SyncResult::default();
        assert!(result.is_success());
    }

    #[test]
    fn test_result_with_errors() {
        let result = SyncResult {
            errors: vec![SyncError {
                artifact: None,
                error: "test".into(),
            }],
            ..Default::default()
        };
        assert!(!result.is_success());
    }

    #[test]
    fn test_summary() {
        let result = SyncResult {
            regenerated: vec![RegeneratedArtifact {
                artifact: ArtifactRef::ProjectRule,
                path: "test".into(),
                reason: "changed".into(),
            }],
            ..Default::default()
        };
        assert_eq!(result.summary(), "1 regenerated, 0 skipped, 0 errors");
    }
}
