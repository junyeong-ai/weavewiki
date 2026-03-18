use regex::Regex;
use std::sync::LazyLock;

use crate::pipeline::context::VerifiedFileRegistry;
use crate::pipeline::generation::evidence_gate::EvidenceProfile;
use crate::utils::patterns::extract_file_refs;

static VERIFIED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[Verified(?::|\s)[^\]]*\]").unwrap());

static INFERRED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[Inferred(?::|\s)[^\]]*\]").unwrap());

static CONVENTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[Convention(?:\]|(?::|\s)[^\]]*\])").unwrap());

/// Evidence profile with validated file reference counts.
#[derive(Debug, Clone, Default)]
pub struct ValidatedEvidenceProfile {
    pub profile: EvidenceProfile,
    pub verified_valid: usize,
    pub verified_invalid: usize,
    pub invalid_refs: Vec<String>,
}

impl ValidatedEvidenceProfile {
    pub fn validity_ratio(&self) -> f32 {
        let total = self.verified_valid + self.verified_invalid;
        if total == 0 {
            0.0
        } else {
            self.verified_valid as f32 / total as f32
        }
    }
}

pub struct EvidenceLabelScanner;

impl EvidenceLabelScanner {
    pub fn scan(content: &str) -> EvidenceProfile {
        EvidenceProfile {
            verified_count: VERIFIED_RE.find_iter(content).count(),
            inferred_count: INFERRED_RE.find_iter(content).count(),
            convention_count: CONVENTION_RE.find_iter(content).count(),
        }
    }

    /// Scan content and validate file references inside [Verified:...] tags against the registry.
    pub fn scan_and_validate(
        content: &str,
        registry: &VerifiedFileRegistry,
    ) -> ValidatedEvidenceProfile {
        let profile = Self::scan(content);
        let mut verified_valid = 0;
        let mut verified_invalid = 0;
        let mut invalid_refs = Vec::new();

        for tag_match in VERIFIED_RE.find_iter(content) {
            let tag_content = tag_match.as_str();
            let refs = extract_file_refs(tag_content);
            for file_ref in refs {
                if crate::pipeline::file_reference::is_valid_file_ref(&file_ref.path, file_ref.line_start, registry) {
                    verified_valid += 1;
                } else {
                    verified_invalid += 1;
                    if let Some(line) = file_ref.line_start {
                        if let Some(max_lines) = registry.line_count(&file_ref.path) {
                            invalid_refs.push(format!("{}:{} (max {})", file_ref.path, line, max_lines));
                        } else {
                            invalid_refs.push(file_ref.path);
                        }
                    } else {
                        invalid_refs.push(file_ref.path);
                    }
                }
            }
        }

        ValidatedEvidenceProfile {
            profile,
            verified_valid,
            verified_invalid,
            invalid_refs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_verified_tags() {
        let content = "Uses repository pattern [Verified: @src/main.rs:42] and DI [Verified: @src/lib.rs:10]";
        let profile = EvidenceLabelScanner::scan(content);
        assert_eq!(profile.verified_count, 2);
        assert_eq!(profile.inferred_count, 0);
        assert_eq!(profile.convention_count, 0);
    }

    #[test]
    fn test_scan_mixed_tags() {
        let content = "\
            The auth module [Verified: @src/auth.rs:15] uses JWT tokens.\n\
            Error handling follows [Convention] patterns.\n\
            The cache layer likely uses LRU [Inferred: based on memory constraints].\n\
            Another verified ref [Verified: @src/db.rs:100].\n\
            Naming follows [Convention: Rust style].\
        ";
        let profile = EvidenceLabelScanner::scan(content);
        assert_eq!(profile.verified_count, 2);
        assert_eq!(profile.inferred_count, 1);
        assert_eq!(profile.convention_count, 2);
        assert_eq!(profile.total(), 5);
    }

    #[test]
    fn test_empty_content_zero_profile() {
        let profile = EvidenceLabelScanner::scan("");
        assert_eq!(profile.total(), 0);
        assert_eq!(profile.verification_ratio(), 0.0);
    }

    #[test]
    fn test_no_tags() {
        let content = "This is plain text without any evidence tags.";
        let profile = EvidenceLabelScanner::scan(content);
        assert_eq!(profile.total(), 0);
    }

    #[test]
    fn test_scan_and_validate_valid_refs() {
        let mut registry = VerifiedFileRegistry::empty();
        registry.register_test_file("src/main.rs");
        registry.register_test_file("src/lib.rs");

        let content = "Uses DI [Verified: @src/main.rs:42] and patterns [Verified: @src/lib.rs:10]";
        let validated = EvidenceLabelScanner::scan_and_validate(content, &registry);
        assert_eq!(validated.profile.verified_count, 2);
        assert_eq!(validated.verified_valid, 2);
        assert_eq!(validated.verified_invalid, 0);
        assert!(validated.invalid_refs.is_empty());
        assert!((validated.validity_ratio() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_scan_and_validate_invalid_refs() {
        let registry = VerifiedFileRegistry::empty();

        let content = "Uses DI [Verified: @src/nonexistent.rs:42]";
        let validated = EvidenceLabelScanner::scan_and_validate(content, &registry);
        assert_eq!(validated.profile.verified_count, 1);
        assert_eq!(validated.verified_valid, 0);
        assert_eq!(validated.verified_invalid, 1);
        assert_eq!(validated.invalid_refs.len(), 1);
        assert!((validated.validity_ratio() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_scan_and_validate_empty() {
        let registry = VerifiedFileRegistry::empty();
        let validated = EvidenceLabelScanner::scan_and_validate("no tags here", &registry);
        assert_eq!(validated.profile.total(), 0);
        assert_eq!(validated.verified_valid, 0);
        assert_eq!(validated.verified_invalid, 0);
        assert!((validated.validity_ratio() - 0.0).abs() < f32::EPSILON);
    }
}
