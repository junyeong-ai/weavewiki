//! Reference Validation
//!
//! Single source of truth for @file:line reference validation.
//! Validates file existence and line ranges against VerifiedFileRegistry.
//!
//! Provides two APIs:
//! - `validate_single_ref()` → `RefValidationResult` enum (detailed, for new code)
//! - `validate_content_references()` → `RefValidationCounts` struct (backward compatibility)

use crate::pipeline::context::VerifiedFileRegistry;
use crate::utils::patterns::{FileRef, extract_file_refs};

/// Reference validation result (detailed, for single-ref validation)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefValidationResult {
    /// Reference is valid
    Valid,
    /// File not found in registry
    FileNotFound,
    /// Line number is 0 (invalid, 1-indexed)
    LineZero,
    /// Line number exceeds file length
    LineOutOfRange { line: u32, max_lines: usize },
}

impl RefValidationResult {
    pub fn is_valid(&self) -> bool {
        matches!(self, RefValidationResult::Valid)
    }

    pub fn to_error_message(&self, file_ref: &FileRef) -> Option<String> {
        match self {
            RefValidationResult::Valid => None,
            RefValidationResult::FileNotFound => {
                Some(format!("File not found: {}", file_ref.path))
            }
            RefValidationResult::LineZero => {
                Some(format!("Line 0 is invalid (1-indexed): {}", file_ref.path))
            }
            RefValidationResult::LineOutOfRange { line, max_lines } => Some(format!(
                "Line {} out of range (max {}): {}",
                line, max_lines, file_ref.path
            )),
        }
    }
}

/// Validate a single file reference against the registry
///
/// Checks:
/// 1. File existence in registry
/// 2. Line 0 rejection (lines are 1-indexed)
/// 3. Line range bounds (validates both line_start and line_end if present)
///
/// File-only references (no line info) are valid if file exists.
pub fn validate_single_ref(
    file_ref: &FileRef,
    registry: &VerifiedFileRegistry,
) -> RefValidationResult {
    // 1. Check file existence
    if !registry.contains(&file_ref.path) {
        return RefValidationResult::FileNotFound;
    }

    // 2. If no line specified, file-only ref is valid
    let Some(line_start) = file_ref.line_start else {
        return RefValidationResult::Valid;
    };

    // 3. Line 0 is invalid (1-indexed)
    if line_start == 0 {
        return RefValidationResult::LineZero;
    }

    // 4. Check line range (validate end_line if present)
    let max_lines = registry.line_count(&file_ref.path).unwrap_or(0);
    let end_line = file_ref.line_end.unwrap_or(line_start);

    // FIX: Check line_end, not just line_start
    if end_line > max_lines as u32 {
        return RefValidationResult::LineOutOfRange {
            line: end_line,
            max_lines,
        };
    }

    RefValidationResult::Valid
}

/// Detailed counts for reference validation failures (backward compatibility API)
#[derive(Debug, Clone, Default)]
pub struct RefValidationCounts {
    pub valid: usize,
    /// File path does not exist at all (hallucination).
    pub file_not_found: usize,
    /// File exists but line number is out of range (stale but not fabricated).
    pub line_out_of_range: usize,
}

impl RefValidationCounts {
    /// Total invalid references (both kinds).
    pub fn invalid_total(&self) -> usize {
        self.file_not_found + self.line_out_of_range
    }

    /// Total references checked.
    pub fn total(&self) -> usize {
        self.valid + self.invalid_total()
    }

    pub fn merge(&mut self, other: &RefValidationCounts) {
        self.valid += other.valid;
        self.file_not_found += other.file_not_found;
        self.line_out_of_range += other.line_out_of_range;
    }
}

/// Validate all `@file:line` references in `content` against the registry.
///
/// Extracts references via `extract_file_refs`, then for each:
/// - File not in registry -> `file_not_found`
/// - File exists, line present but out of range -> `line_out_of_range`
/// - Otherwise -> `valid`
pub fn validate_content_references(
    content: &str,
    registry: &VerifiedFileRegistry,
) -> RefValidationCounts {
    let refs = extract_file_refs(content);
    validate_refs(&refs, registry)
}

/// Validate pre-extracted references against the registry.
///
/// Uses `validate_single_ref()` internally - fixes line 0 and line_end bugs.
pub fn validate_refs(refs: &[FileRef], registry: &VerifiedFileRegistry) -> RefValidationCounts {
    let mut counts = RefValidationCounts::default();

    for file_ref in refs {
        match validate_single_ref(file_ref, registry) {
            RefValidationResult::Valid => counts.valid += 1,
            RefValidationResult::FileNotFound => counts.file_not_found += 1,
            RefValidationResult::LineZero => {
                // Treat line 0 as hallucination (file_not_found semantically)
                counts.file_not_found += 1;
            }
            RefValidationResult::LineOutOfRange { .. } => counts.line_out_of_range += 1,
        }
    }

    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_content() {
        let registry = VerifiedFileRegistry::default();
        let counts = validate_content_references("", &registry);
        assert_eq!(counts.total(), 0);
        assert_eq!(counts.valid, 0);
    }

    #[test]
    fn test_valid_file_ref() {
        let mut registry = VerifiedFileRegistry::default();
        registry.register_test_file("src/main.rs");
        let counts = validate_content_references("See @src/main.rs for details", &registry);
        assert_eq!(counts.valid, 1);
        assert_eq!(counts.file_not_found, 0);
    }

    #[test]
    fn test_missing_file_ref() {
        let registry = VerifiedFileRegistry::default();
        let counts = validate_content_references("See @src/missing.rs for details", &registry);
        assert_eq!(counts.file_not_found, 1);
        assert_eq!(counts.valid, 0);
    }

    #[test]
    fn test_line_in_range() {
        let mut registry = VerifiedFileRegistry::default();
        registry.register_test_file("src/main.rs");
        let counts = validate_content_references("See @src/main.rs:50", &registry);
        assert_eq!(counts.valid, 1);
        assert_eq!(counts.line_out_of_range, 0);
    }

    #[test]
    fn test_line_out_of_range() {
        let mut registry = VerifiedFileRegistry::default();
        registry.register_test_file("src/main.rs");
        // register_test_file defaults to 100 lines
        let counts = validate_content_references("See @src/main.rs:200", &registry);
        assert_eq!(counts.line_out_of_range, 1);
        assert_eq!(counts.valid, 0);
    }

    #[test]
    fn test_merge() {
        let mut a = RefValidationCounts {
            valid: 3,
            file_not_found: 1,
            line_out_of_range: 0,
        };
        let b = RefValidationCounts {
            valid: 2,
            file_not_found: 0,
            line_out_of_range: 1,
        };
        a.merge(&b);
        assert_eq!(a.valid, 5);
        assert_eq!(a.file_not_found, 1);
        assert_eq!(a.line_out_of_range, 1);
        assert_eq!(a.total(), 7);
        assert_eq!(a.invalid_total(), 2);
    }

    #[test]
    fn test_mixed_refs() {
        let mut registry = VerifiedFileRegistry::default();
        registry.register_test_file("src/a.rs");
        registry.register_test_file("src/b.rs");
        let counts = validate_content_references(
            "Valid @src/a.rs:10, missing @src/missing.rs, out of range @src/b.rs:999",
            &registry,
        );
        assert_eq!(counts.valid, 1);
        assert_eq!(counts.file_not_found, 1);
        assert_eq!(counts.line_out_of_range, 1);
    }

    // NEW TESTS: Line 0 and line_end validation

    #[test]
    fn test_line_zero_rejection() {
        let mut registry = VerifiedFileRegistry::default();
        registry.register_test_file("src/main.rs");

        let file_ref = FileRef::with_line("src/main.rs".to_string(), 0);
        let result = validate_single_ref(&file_ref, &registry);

        assert_eq!(result, RefValidationResult::LineZero);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_line_zero_in_content() {
        let mut registry = VerifiedFileRegistry::default();
        registry.register_test_file("src/main.rs");

        // Line 0 should be counted as invalid (treated as hallucination)
        let counts = validate_content_references("See @src/main.rs:0", &registry);
        assert_eq!(counts.valid, 0);
        assert_eq!(counts.file_not_found, 1); // Line 0 → treated as hallucination
    }

    #[test]
    fn test_range_end_validation() {
        let mut registry = VerifiedFileRegistry::default();
        registry.register_test_file("src/main.rs"); // Defaults to 100 lines

        // Range where end is out of range
        let file_ref = FileRef::with_range("src/main.rs".to_string(), 10, 150);
        let result = validate_single_ref(&file_ref, &registry);

        assert!(matches!(
            result,
            RefValidationResult::LineOutOfRange { line: 150, max_lines: 100 }
        ));
    }

    #[test]
    fn test_range_valid() {
        let mut registry = VerifiedFileRegistry::default();
        registry.register_test_file("src/main.rs");

        let file_ref = FileRef::with_range("src/main.rs".to_string(), 10, 50);
        let result = validate_single_ref(&file_ref, &registry);

        assert_eq!(result, RefValidationResult::Valid);
    }

    #[test]
    fn test_error_messages() {
        let mut registry = VerifiedFileRegistry::default();
        registry.register_test_file("src/main.rs");

        let file_not_found = FileRef::new("missing.rs".to_string());
        let result = validate_single_ref(&file_not_found, &registry);
        let msg = result.to_error_message(&file_not_found).unwrap();
        assert!(msg.contains("File not found"));

        let line_zero = FileRef::with_line("src/main.rs".to_string(), 0);
        let result = validate_single_ref(&line_zero, &registry);
        let msg = result.to_error_message(&line_zero).unwrap();
        assert!(msg.contains("Line 0 is invalid"));

        let out_of_range = FileRef::with_line("src/main.rs".to_string(), 200);
        let result = validate_single_ref(&out_of_range, &registry);
        let msg = result.to_error_message(&out_of_range).unwrap();
        assert!(msg.contains("out of range"));
    }
}
