use std::path::Path;

use crate::constants::verification::STALE_FILE_THRESHOLD_SECS;
use crate::types::{
    Claim, ClaimType, InformationTier, IssueSeverity, Result, VerificationIssue,
    VerificationReport, VerificationStatus,
};

use super::cache::FileContentCache;
use super::rules::{ReferenceRule, SignatureRule};

/// Verification engine with file content caching for I/O optimization
pub struct VerificationEngine {
    root_path: std::path::PathBuf,
    /// LRU cache for file contents to avoid repeated disk reads
    cache: FileContentCache,
}

impl VerificationEngine {
    /// Create a new verification engine with default cache settings
    pub fn new(root_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            root_path: root_path.into(),
            cache: FileContentCache::default(),
        }
    }

    /// Create with custom cache size
    pub fn with_cache_size(root_path: impl Into<std::path::PathBuf>, max_entries: usize) -> Self {
        Self {
            root_path: root_path.into(),
            cache: FileContentCache::new(max_entries),
        }
    }

    /// Get cache statistics for monitoring
    pub fn cache_stats(&self) -> super::cache::CacheStats {
        self.cache.stats()
    }

    /// Clear the file content cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    pub fn verify_all(&self, claims: &[Claim]) -> Result<VerificationReport> {
        let mut report = VerificationReport::new();
        report.total_claims = claims.len() as u32;

        for claim in claims {
            let (status, issue) = self.verify_claim(claim)?;

            match status {
                VerificationStatus::Verified => report.verified += 1,
                VerificationStatus::Stale => report.stale += 1,
                VerificationStatus::Invalid | VerificationStatus::Conflict => report.invalid += 1,
                VerificationStatus::Pending => {}
            }

            if let Some(i) = issue {
                report.add_issue(i);
            }
        }

        Ok(report)
    }

    pub fn verify_claim(
        &self,
        claim: &Claim,
    ) -> Result<(VerificationStatus, Option<VerificationIssue>)> {
        if claim.tier == InformationTier::Fact {
            return self.verify_fact(claim);
        }

        Ok((VerificationStatus::Pending, None))
    }

    fn verify_fact(
        &self,
        claim: &Claim,
    ) -> Result<(VerificationStatus, Option<VerificationIssue>)> {
        let file_path = self.root_path.join(&claim.evidence.file);

        if !file_path.exists() {
            return Ok((
                VerificationStatus::Invalid,
                Some(VerificationIssue::new(
                    &claim.id,
                    IssueSeverity::Error,
                    format!("Evidence file not found: {}", claim.evidence.file),
                )),
            ));
        }

        match claim.claim_type {
            ClaimType::FunctionSignature => {
                let content = self.cache.get_or_load(&file_path)?;
                SignatureRule::verify(claim, &content)
            }
            ClaimType::FileExists | ClaimType::ModuleExports | ClaimType::DependencyRelation => {
                ReferenceRule::verify(claim, &self.root_path)
            }
            ClaimType::ClassStructure | ClaimType::TypeDefinition => {
                self.verify_type_structure(claim, &file_path)
            }
            ClaimType::ApiEndpoint => self.verify_api_endpoint(claim, &file_path),
        }
    }

    fn verify_type_structure(
        &self,
        claim: &Claim,
        file_path: &Path,
    ) -> Result<(VerificationStatus, Option<VerificationIssue>)> {
        let content = self.cache.get_or_load(file_path)?;
        let expected = &claim.statement;

        let patterns = [
            format!("class {}", expected),
            format!("interface {}", expected),
            format!("type {}", expected),
            format!("struct {}", expected),
            format!("enum {}", expected),
        ];

        for pattern in &patterns {
            if content.contains(pattern) {
                return Ok((VerificationStatus::Verified, None));
            }
        }

        Ok((
            VerificationStatus::Stale,
            Some(
                VerificationIssue::new(
                    &claim.id,
                    IssueSeverity::Warning,
                    format!("Type '{}' not found in {}", expected, claim.evidence.file),
                )
                .with_suggestion("Update type definition in knowledge base"),
            ),
        ))
    }

    fn verify_api_endpoint(
        &self,
        claim: &Claim,
        file_path: &Path,
    ) -> Result<(VerificationStatus, Option<VerificationIssue>)> {
        let content = self.cache.get_or_load(file_path)?;
        let endpoint = &claim.statement;

        let patterns = [
            format!("@Get('{}')", endpoint),
            format!("@Post('{}')", endpoint),
            format!("@Put('{}')", endpoint),
            format!("@Delete('{}')", endpoint),
            format!("@Patch('{}')", endpoint),
            format!(".get('{}'", endpoint),
            format!(".post('{}'", endpoint),
            format!(".put('{}'", endpoint),
            format!(".delete('{}'", endpoint),
            format!("\"{}\"", endpoint),
            format!("'{}'", endpoint),
        ];

        for pattern in &patterns {
            if content.contains(pattern) {
                return Ok((VerificationStatus::Verified, None));
            }
        }

        Ok((
            VerificationStatus::Stale,
            Some(
                VerificationIssue::new(
                    &claim.id,
                    IssueSeverity::Warning,
                    format!("API endpoint '{}' not found", endpoint),
                )
                .with_suggestion("Update API catalog with current endpoints"),
            ),
        ))
    }

    pub fn detect_stale_files(&self, tracked_files: &[String]) -> Result<Vec<VerificationIssue>> {
        let mut issues = Vec::new();

        for file in tracked_files {
            let file_path = self.root_path.join(file);

            if !file_path.exists() {
                issues.push(
                    VerificationIssue::new(
                        format!("file:{}", file),
                        IssueSeverity::Error,
                        format!("Tracked file no longer exists: {}", file),
                    )
                    .with_suggestion("Remove all claims referencing this file")
                    .auto_fixable(),
                );
                continue;
            }

            if let Ok(metadata) = std::fs::metadata(&file_path)
                && let Ok(modified) = metadata.modified()
                && let Ok(age) = std::time::SystemTime::now().duration_since(modified)
                && age.as_secs() < STALE_FILE_THRESHOLD_SECS
            {
                issues.push(VerificationIssue::new(
                    format!("file:{}", file),
                    IssueSeverity::Info,
                    format!("File recently modified: {}", file),
                ));
            }
        }

        Ok(issues)
    }
}
