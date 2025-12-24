//! Reference Rule
//!
//! Verifies file existence, module exports, and dependency relations.

use std::path::Path;

use crate::types::{
    Claim, ClaimType, IssueSeverity, Result, VerificationIssue, VerificationStatus,
};

pub struct ReferenceRule;

impl ReferenceRule {
    pub fn verify(
        claim: &Claim,
        root_path: &Path,
    ) -> Result<(VerificationStatus, Option<VerificationIssue>)> {
        match claim.claim_type {
            ClaimType::FileExists => Self::verify_file_exists(claim, root_path),
            ClaimType::ModuleExports => Self::verify_module_exports(claim, root_path),
            ClaimType::DependencyRelation => Self::verify_dependency(claim, root_path),
            _ => Ok((VerificationStatus::Pending, None)),
        }
    }

    fn verify_file_exists(
        claim: &Claim,
        root_path: &Path,
    ) -> Result<(VerificationStatus, Option<VerificationIssue>)> {
        let file_path = root_path.join(&claim.evidence.file);

        if file_path.exists() {
            Ok((VerificationStatus::Verified, None))
        } else {
            Ok((
                VerificationStatus::Invalid,
                Some(
                    VerificationIssue::new(
                        &claim.id,
                        IssueSeverity::Error,
                        format!("File no longer exists: {}", claim.evidence.file),
                    )
                    .with_suggestion("Remove references to this file from knowledge base")
                    .auto_fixable(),
                ),
            ))
        }
    }

    fn verify_module_exports(
        claim: &Claim,
        root_path: &Path,
    ) -> Result<(VerificationStatus, Option<VerificationIssue>)> {
        let file_path = root_path.join(&claim.evidence.file);

        if !file_path.exists() {
            return Ok((
                VerificationStatus::Invalid,
                Some(VerificationIssue::new(
                    &claim.id,
                    IssueSeverity::Error,
                    format!("Module file not found: {}", claim.evidence.file),
                )),
            ));
        }

        // Read file with proper error handling
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                return Ok((
                    VerificationStatus::Invalid,
                    Some(VerificationIssue::new(
                        &claim.id,
                        IssueSeverity::Error,
                        format!("Cannot read file '{}': {}", file_path.display(), e),
                    )),
                ));
            }
        };

        let expected_export = &claim.statement;

        let has_export = content.contains(&format!("export {}", expected_export))
            || content.contains(&format!("export default {}", expected_export))
            || content.contains(&format!("pub fn {}", expected_export))
            || content.contains(&format!("pub struct {}", expected_export))
            || content.contains(&format!("pub enum {}", expected_export));

        if has_export {
            Ok((VerificationStatus::Verified, None))
        } else {
            Ok((
                VerificationStatus::Stale,
                Some(
                    VerificationIssue::new(
                        &claim.id,
                        IssueSeverity::Warning,
                        format!(
                            "Export '{}' not found in {}",
                            expected_export, claim.evidence.file
                        ),
                    )
                    .with_suggestion("Update exports in knowledge base"),
                ),
            ))
        }
    }

    fn verify_dependency(
        claim: &Claim,
        root_path: &Path,
    ) -> Result<(VerificationStatus, Option<VerificationIssue>)> {
        let source_file = root_path.join(&claim.evidence.file);
        let target = &claim.statement;

        if !source_file.exists() {
            return Ok((
                VerificationStatus::Invalid,
                Some(VerificationIssue::new(
                    &claim.id,
                    IssueSeverity::Error,
                    format!("Source file not found: {}", claim.evidence.file),
                )),
            ));
        }

        // Read file with proper error handling
        let content = match std::fs::read_to_string(&source_file) {
            Ok(c) => c,
            Err(e) => {
                return Ok((
                    VerificationStatus::Invalid,
                    Some(VerificationIssue::new(
                        &claim.id,
                        IssueSeverity::Error,
                        format!("Cannot read file '{}': {}", source_file.display(), e),
                    )),
                ));
            }
        };

        let has_import = content.contains(&format!("from '{}'", target))
            || content.contains(&format!("from \"{}\"", target))
            || content.contains(&format!("require('{}')", target))
            || content.contains(&format!("require(\"{}\")", target))
            || content.contains(&format!("import \"{}\"", target))
            || content.contains(&format!("use {};", target));

        if has_import {
            Ok((VerificationStatus::Verified, None))
        } else {
            Ok((
                VerificationStatus::Stale,
                Some(
                    VerificationIssue::new(
                        &claim.id,
                        IssueSeverity::Info,
                        format!("Dependency '{}' no longer imported", target),
                    )
                    .with_suggestion("Remove dependency edge from knowledge graph"),
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ClaimEvidence;
    use tempfile::TempDir;

    fn create_claim(claim_type: ClaimType, file: &str, statement: &str) -> Claim {
        Claim {
            id: "test-claim".to_string(),
            claim_type,
            subject_id: "test".to_string(),
            statement: statement.to_string(),
            evidence: ClaimEvidence::new(file),
            tier: crate::types::InformationTier::Fact,
            confidence: 1.0,
            verification: VerificationStatus::Pending,
            created_at: chrono::Utc::now(),
            verified_at: None,
        }
    }

    #[test]
    fn test_file_exists_verified() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("test.rs"), "fn main() {}").unwrap();

        let claim = create_claim(ClaimType::FileExists, "test.rs", "");
        let (status, _) = ReferenceRule::verify(&claim, temp.path()).unwrap();
        assert_eq!(status, VerificationStatus::Verified);
    }

    #[test]
    fn test_file_not_exists_invalid() {
        let temp = TempDir::new().unwrap();

        let claim = create_claim(ClaimType::FileExists, "nonexistent.rs", "");
        let (status, issue) = ReferenceRule::verify(&claim, temp.path()).unwrap();
        assert_eq!(status, VerificationStatus::Invalid);
        assert!(issue.is_some());
    }

    #[test]
    fn test_module_exports_verified() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("lib.rs"), "pub fn hello() {}").unwrap();

        let claim = create_claim(ClaimType::ModuleExports, "lib.rs", "hello");
        let (status, _) = ReferenceRule::verify(&claim, temp.path()).unwrap();
        assert_eq!(status, VerificationStatus::Verified);
    }
}
